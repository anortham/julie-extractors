//! Bash Extractor - Complete Implementation of bash-extractor.ts
//!
//! Handles Bash/shell-specific constructs for DevOps tracing:
//! - Functions and their definitions
//! - Variables (local, environment, exported)
//! - External command calls (critical for cross-language tracing!)
//! - Script arguments and parameters
//! - Conditional logic and loops
//! - Source/include relationships
//! - Docker, kubectl, npm, and other DevOps tool calls
//!
//! Special focus on cross-language tracing since Bash scripts often orchestrate
//! other programs (Python, Node.js, Go binaries, Docker containers, etc.).

mod arithmetic;
mod commands;
mod functions;
pub(crate) mod helpers;
mod http;
mod invocations;
mod relationships;
mod signatures;
pub(crate) mod test_calls;
mod types;
mod variables;

pub(crate) use http::http_requests;
pub(crate) use variables::{declaration_flags, declared_names};

use crate::base::{
    BaseExtractor, ContainingSymbolIndex, Identifier, PendingRelationship, Relationship,
    ScopedSymbolIndex, StructuredPendingRelationship, Symbol, SymbolKind,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Tree;

pub struct BashExtractor {
    pub(super) base: BaseExtractor,
    associative_arrays: std::collections::HashSet<String>,
    local_functions: std::collections::HashSet<String>,
}

impl BashExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        let mut base = BaseExtractor::new(language, file_path, content, workspace_root);
        base.body_span_rule = Some(helpers::body_span);
        Self {
            base,
            associative_arrays: std::collections::HashSet::new(),
            local_functions: std::collections::HashSet::new(),
        }
    }

    /// bats, ShellSpec, shunit2, and bashunit files, where `run`, `load`,
    /// `Include`, and the ShellSpec hooks have their test-framework meaning.
    pub(super) fn test_context(&self) -> bool {
        crate::test_detection::is_test_path(&self.base.file_path)
    }

    pub(super) fn command_scope(&self) -> invocations::CommandScope<'_> {
        invocations::CommandScope {
            local_functions: &self.local_functions,
            test_context: self.test_context(),
        }
    }

    fn remember_local_functions(&mut self, symbols: &[Symbol]) {
        self.local_functions = symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Function)
            .filter(|symbol| {
                symbol
                    .signature
                    .as_deref()
                    .is_some_and(|signature| signature.starts_with("function "))
            })
            .map(|symbol| symbol.name.clone())
            .collect();
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();

        // Detect shebang line
        if let Some(first_line) = self.base.content.lines().next()
            && first_line.starts_with("#!")
        {
            let interpreter = first_line.trim_start_matches("#!").trim();
            // Handle "#!/usr/bin/env python3" -> "python3"
            // Handle "#!/bin/bash" -> "bash"
            let name = if interpreter.contains("env ") {
                interpreter
                    .rsplit_once(' ')
                    .map(|(_, cmd)| cmd)
                    .unwrap_or(interpreter)
            } else {
                interpreter.rsplit('/').next().unwrap_or(interpreter)
            };
            let root = tree.root_node();
            let shebang = root
                .child(0)
                .filter(|node| node.kind() == "comment" && node.start_byte() == 0)
                .unwrap_or(root);
            let symbol = self.base.create_symbol(
                &shebang,
                name.to_string(),
                SymbolKind::Variable,
                crate::base::SymbolOptions {
                    signature: Some(first_line.to_string()),
                    ..Default::default()
                },
            );
            symbols.push(symbol);
        }

        self.walk_tree_for_symbols(tree.root_node(), &mut symbols, None, 0);
        symbols
    }

    /// Main tree traversal for symbol extraction
    fn walk_tree_for_symbols(
        &mut self,
        node: tree_sitter::Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        if node.kind() == "declaration_command" {
            self.walk_declaration(node, symbols, parent_id, depth);
            return;
        }

        let symbol = self.extract_symbol_from_node(node, parent_id.as_deref());
        let mut current_parent_id = parent_id;

        if let Some(ref sym) = symbol {
            symbols.push(sym.clone());

            // If this is a function, extract its positional parameters
            if sym.kind == SymbolKind::Function {
                let parameters = self.extract_positional_parameters(node, &sym.id);
                symbols.extend(parameters);
            }

            current_parent_id = Some(sym.id.clone());
        }

        self.walk_symbol_children(node, symbols, current_parent_id, depth);
    }

    /// Declare the names of a declaration command, then walk each assignment
    /// value under the symbol it initializes.
    #[inline(never)]
    fn walk_declaration(
        &mut self,
        node: tree_sitter::Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        let values = self.extract_declarations(node, parent_id.as_deref(), symbols);
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        for (value, owner_id) in values {
            let value_parent = owner_id.or_else(|| parent_id.clone());
            self.walk_tree_for_symbols(value, symbols, value_parent, child_depth);
        }
    }

    /// Walk `node`'s children. A bats or ShellSpec block opener becomes a test
    /// symbol spanning to its closing `}` or `End`, and the siblings in between
    /// take it as their parent.
    fn walk_symbol_children(
        &mut self,
        node: tree_sitter::Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        let children: Vec<tree_sitter::Node> = node.children(&mut cursor).collect();
        let mut open_blocks: Vec<(String, usize)> = Vec::new();
        for (index, child) in children.iter().enumerate() {
            while open_blocks.last().is_some_and(|(_, end)| *end < index) {
                open_blocks.pop();
            }
            let child_parent = open_blocks
                .last()
                .map(|(id, _)| id.clone())
                .or_else(|| parent_id.clone());
            if let Some(end) = test_calls::block_end(&self.base, &children, index)
                && let Some(block) = test_calls::extract_bash_test_block(
                    &mut self.base,
                    &children,
                    index,
                    end,
                    child_parent.as_deref(),
                )
            {
                open_blocks.push((block.id.clone(), end));
                symbols.push(block);
                continue;
            }
            self.walk_tree_for_symbols(*child, symbols, child_parent, child_depth);
        }
    }

    /// Extract symbol from a node based on its type
    fn extract_symbol_from_node(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        match node.kind() {
            "function_definition" => self.extract_function(node, parent_id),
            "variable_assignment" => self.extract_variable(node, parent_id),
            "command" | "simple_command" => {
                // Try shellspec/bats DSL first; fall through to alias/source detection.
                if let Some(sym) =
                    test_calls::extract_bash_test_call(&mut self.base, node, parent_id)
                {
                    Some(sym)
                } else {
                    self.extract_command(node, parent_id)
                }
            }
            "for_statement" | "while_statement" | "if_statement" => None,
            _ => None,
        }
    }

    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        self.remember_local_functions(symbols);
        let mut relationships = Vec::new();
        let function_symbols = ContainingSymbolIndex::from_iter(
            symbols.iter().filter(|s| s.kind == SymbolKind::Function),
        );
        let scoped_index = ScopedSymbolIndex::new(symbols);
        self.walk_tree_for_relationships(
            tree.root_node(),
            &function_symbols,
            &scoped_index,
            &mut relationships,
            0,
        );
        relationships
    }

    /// Walk tree extracting relationships
    fn walk_tree_for_relationships<'a>(
        &mut self,
        node: tree_sitter::Node,
        function_symbols: &ContainingSymbolIndex<'a>,
        scoped_index: &ScopedSymbolIndex<'a>,
        relationships: &mut Vec<Relationship>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        match node.kind() {
            "command" | "simple_command" => {
                self.extract_command_relationships(
                    node,
                    function_symbols,
                    scoped_index,
                    relationships,
                );
            }
            _ => {}
        }

        // Recursively process child nodes
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_tree_for_relationships(
                child,
                function_symbols,
                scoped_index,
                relationships,
                child_depth,
            );
        }
    }

    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        self.remember_local_functions(symbols);
        let containing_symbols = self.base.containing_symbol_index(symbols);
        self.associative_arrays = arithmetic::associative_array_names(&self.base, tree.root_node());
        self.walk_tree_for_identifiers(tree.root_node(), &containing_symbols, 0);
        self.base.identifiers.clone()
    }

    pub fn infer_types(&self, symbols: &[Symbol]) -> std::collections::HashMap<String, String> {
        // Delegate to types module
        types::BashExtractor::infer_types(self, symbols)
    }

    // Identifier extraction helper methods
    fn walk_tree_for_identifiers(
        &mut self,
        node: tree_sitter::Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        self.extract_identifier_from_node(node, containing_symbols);
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_tree_for_identifiers(child, containing_symbols, child_depth);
        }
    }

    fn extract_identifier_from_node(
        &mut self,
        node: tree_sitter::Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) {
        match node.kind() {
            "command" => self.extract_command_identifiers(node, containing_symbols),
            "subscript" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "variable_name" || child.kind() == "simple_expansion" {
                        let name = self.base.get_node_text(&child);
                        let clean_name = name.trim_start_matches('$').to_string();
                        let containing_symbol_id =
                            self.find_containing_symbol_id(node, containing_symbols);
                        self.base.create_identifier(
                            &child,
                            clean_name,
                            crate::base::IdentifierKind::MemberAccess,
                            containing_symbol_id,
                        );
                        break;
                    }
                }
            }
            // variable_ref (Batch F): `$var` (simple_expansion) and `${var}`
            // (expansion) are bash's genuine variable reads; before this arm
            // they emitted nothing, so declared variables looked dead to
            // kind-blind name matching. Only a DIRECT `variable_name` child is
            // taken: in `${arr[$i]}` the array name sits inside the `subscript`
            // node (owned by the MemberAccess arm above — kind untouched), so
            // no double emission; the index read `$i` is its own
            // simple_expansion and is picked up here by the walker. Positional
            // parameters (`$1` parses as variable_name "1") are skipped via the
            // all-digits check; special parameters (`$@`, `$?`, …) parse as
            // `special_variable_name` and never match. Non-resolvable: no
            // target binding at extraction.
            "simple_expansion" | "expansion" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "variable_name" {
                        let name = self.base.get_node_text(&child);
                        if !name.is_empty() && !name.bytes().all(|b| b.is_ascii_digit()) {
                            let containing_symbol_id =
                                self.find_containing_symbol_id(node, containing_symbols);
                            self.base.create_identifier(
                                &child,
                                name,
                                crate::base::IdentifierKind::VariableRef,
                                containing_symbol_id,
                            );
                        }
                        break;
                    }
                }
            }
            "variable_name" | "word"
                if arithmetic::is_arithmetic_read(&self.base, node, &self.associative_arrays) =>
            {
                let containing_symbol_id = self.find_containing_symbol_id(node, containing_symbols);
                let name = self.base.get_node_text(&node);
                self.base.create_identifier(
                    &node,
                    name,
                    crate::base::IdentifierKind::VariableRef,
                    containing_symbol_id,
                );
            }
            _ => {}
        }
    }

    /// Call identifiers for a command with a static name and for every command
    /// it runs or registers (`sudo x`, `trap handler`, `complete -F fn`), plus
    /// its string-literal arguments.
    #[inline(never)]
    fn extract_command_identifiers(
        &mut self,
        node: tree_sitter::Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) {
        let Some((name_node, name)) = invocations::static_command_name(&self.base.content, node)
        else {
            return;
        };
        if matches!(name.as_str(), "}" | "End") {
            return;
        }
        let containing_symbol_id = self.find_containing_symbol_id(node, containing_symbols);
        let targets = invocations::invocations(&self.base.content, node, &self.command_scope());
        for target in &targets {
            let Some(span) = self
                .base
                .span_for_byte_range(target.range.start, target.range.end)
            else {
                continue;
            };
            self.base.create_identifier_at_span(
                span,
                target.name.clone(),
                crate::base::IdentifierKind::Call,
                containing_symbol_id.clone(),
                None,
            );
        }
        self.base.create_identifier(
            &name_node,
            name.clone(),
            crate::base::IdentifierKind::Call,
            containing_symbol_id.clone(),
        );
        let (carrier, arguments) = match targets
            .into_iter()
            .find_map(|target| Some((target.name, target.arguments?)))
        {
            Some(wrapped) => wrapped,
            None => (name, invocations::arguments(node)),
        };
        self.record_command_literals(node, &carrier, &arguments, containing_symbol_id);
    }

    /// Capture the string literals a command receives.
    ///
    /// Bash commands are a COMMAND grammar, not `call_expression`: the carrier is
    /// the command name itself (`curl`, `wget`, `psql`, `mysql`, `sqlite3`, …),
    /// or the wrapped command for `sudo curl …`. This is config-free — `kind` is
    /// `Other`, and the `[literal_carriers]` table in `languages/bash.toml`
    /// decides which carriers survive.
    ///
    /// String-bearing arguments are captured (`string`, `raw_string`,
    /// `ansi_c_string`, `translated_string`). A bare `word` such as the unquoted
    /// URL in `curl https://x` is not a string literal. For `curl` and `wget`
    /// only request arguments count, so a `-H` header value is not captured.
    /// A herestring (`<<< "..."`) and a heredoc body are captured after the
    /// arguments. A literal that is only interpolation holes (`"$DB"`) is not.
    ///
    /// `arg_position` counts over the full argument list, so the SQL in
    /// `psql -c "SELECT …"` (args `["-c", "SELECT …"]`) reports position 1.
    fn record_command_literals(
        &mut self,
        command_node: tree_sitter::Node,
        carrier: &str,
        args: &[tree_sitter::Node],
        containing_symbol_id: Option<String>,
    ) {
        let request_args = http::HTTP_CLIENTS
            .contains(&carrier)
            .then(|| http::http_arguments(&self.base.content, carrier, args).positionals);
        for (position, arg) in args.iter().enumerate() {
            if request_args
                .as_ref()
                .is_some_and(|requests| !requests.contains(arg))
            {
                continue;
            }
            if let Some(text) = self.base.decode_string_literal(arg) {
                self.record_command_literal(
                    arg,
                    text,
                    carrier,
                    position as u32,
                    containing_symbol_id.clone(),
                );
            }
        }
        let position = args.len() as u32;
        for input in redirected_inputs(command_node) {
            let text = match input.kind() {
                "heredoc_body" => heredoc_text(&self.base.content, input),
                _ => self.base.decode_string_literal(&input),
            };
            if let Some(text) = text {
                self.record_command_literal(
                    &input,
                    text,
                    carrier,
                    position,
                    containing_symbol_id.clone(),
                );
            }
        }
    }

    fn record_command_literal(
        &mut self,
        node: &tree_sitter::Node,
        text: String,
        carrier: &str,
        position: u32,
        containing_symbol_id: Option<String>,
    ) {
        if text.replace("{}", "").trim().is_empty() {
            return;
        }
        self.base.record_literal(
            node,
            text,
            Some(carrier.to_string()),
            position,
            containing_symbol_id,
        );
    }

    fn find_containing_symbol_id(
        &self,
        node: tree_sitter::Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) -> Option<String> {
        containing_symbols.find(node).map(|s| s.id.clone())
    }

    // ========================================================================
    // Pending Relationship Management
    // ========================================================================

    pub(crate) fn add_structured_pending_relationship(
        &mut self,
        pending: StructuredPendingRelationship,
    ) {
        self.base.add_structured_pending_relationship(pending);
    }

    /// Get all pending relationships collected during extraction
    pub fn get_pending_relationships(&self) -> Vec<PendingRelationship> {
        self.base.get_pending_relationships()
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals.
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn get_structured_pending_relationships(&self) -> Vec<StructuredPendingRelationship> {
        self.base.get_structured_pending_relationships()
    }
}

/// The herestring and heredoc bodies fed to `command` on standard input.
fn redirected_inputs(command: tree_sitter::Node<'_>) -> Vec<tree_sitter::Node<'_>> {
    let statement = command
        .parent()
        .filter(|parent| parent.kind() == "redirected_statement")
        .filter(|parent| parent.child_by_field_name("body") == Some(command));
    let mut inputs = Vec::new();
    for owner in std::iter::once(command).chain(statement) {
        let mut cursor = owner.walk();
        for redirect in owner.children_by_field_name("redirect", &mut cursor) {
            let mut inner = redirect.walk();
            let body = match redirect.kind() {
                "herestring_redirect" => redirect
                    .named_children(&mut inner)
                    .find(|child| child.kind().contains("string")),
                "heredoc_redirect" => redirect
                    .named_children(&mut inner)
                    .find(|child| child.kind() == "heredoc_body"),
                _ => None,
            };
            inputs.extend(body);
        }
    }
    inputs
}

/// A heredoc body with each expansion replaced by `{}`. A quoted delimiter
/// (`<<'SQL'`) turns expansion off, so its body is kept verbatim.
fn heredoc_text(content: &str, body: tree_sitter::Node<'_>) -> Option<String> {
    let quoted = body
        .prev_named_sibling()
        .filter(|start| start.kind() == "heredoc_start")
        .and_then(|start| content.get(start.byte_range()))
        .is_some_and(|start| start.contains(['\'', '"', '\\']));
    let mut text = String::new();
    let mut position = body.start_byte();
    if !quoted {
        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            if child.kind().contains("expansion") || child.kind().contains("substitution") {
                text.push_str(content.get(position..child.start_byte())?);
                text.push_str("{}");
                position = child.end_byte();
            }
        }
    }
    text.push_str(content.get(position..body.end_byte())?);
    Some(text.trim_end_matches(['\n', '\r']).to_string())
}
