// Dart Extractor - Implementation of dart-extractor.ts (TDD GREEN phase)
//
// Direct Implementation of Dart extractor logic to idiomatic Rust.
// Preserves proven extraction strategy while leveraging Rust's safety and performance.
//
// Test parity: All test cases must pass

mod functions;
mod helpers;
mod identifiers;
mod imports;
mod locals;
mod members;
mod parameters;
mod pending_calls;
mod relationships;
mod signatures;
pub(crate) mod test_calls;
mod type_facts;
mod types;

use crate::base::{
    BaseExtractor, Identifier, PendingRelationship, Relationship, StructuredPendingRelationship,
    Symbol, SymbolKind, SymbolOptions, Visibility,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use helpers::{find_child_by_type, get_node_text};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use tree_sitter::{Node, Tree};

// Static regex compiled once for performance
static TYPE_SIGNATURE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\w+)\s+\w+").unwrap());

const SIGNATURE_MODIFIERS: &[&str] = &[
    "final",
    "var",
    "const",
    "late",
    "static",
    "async",
    "external",
    "covariant",
    "required",
];

fn is_variable_like(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Variable | SymbolKind::Constant | SymbolKind::Field | SymbolKind::Property
    )
}

fn legacy_signature_type(signature: &str, kind: &SymbolKind) -> Option<String> {
    let mut rest = signature.trim_start();
    while let Some(word) = rest.split_whitespace().next()
        && SIGNATURE_MODIFIERS.contains(&word)
    {
        rest = rest[word.len()..].trim_start();
    }
    let type_text = TYPE_SIGNATURE_RE.captures(rest)?.get(1)?.as_str();
    if SIGNATURE_MODIFIERS.contains(&type_text) || (is_variable_like(kind) && type_text == "void") {
        return None;
    }
    Some(type_text.to_string())
}

/// Dart language extractor that handles Dart-specific constructs including Flutter
pub struct DartExtractor {
    pub(crate) base: BaseExtractor,
    same_file_calls: Vec<(String, String, u32, crate::base::NormalizedSpan)>,
    same_file_type_names: HashSet<String>,
    consumed_blocks: HashSet<usize>,
}

/// tree-sitter-dart roots parsed files at `source_file`, not `program`.
fn is_dart_top_level_parent(parent: Node) -> bool {
    parent.kind() == "source_file"
}

fn is_dart_callable(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "local_function_declaration"
            | "lambda_expression"
            | "function_signature"
            | "method_signature"
            | "method_declaration"
            | "constructor_signature"
            | "factory_constructor_signature"
            | "constant_constructor_signature"
    )
}

fn wrapped_constructor_signature(node: Node) -> bool {
    node.parent()
        .is_some_and(|parent| matches!(parent.kind(), "method_signature" | "method_declaration"))
}

impl DartExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            same_file_calls: Vec::new(),
            same_file_type_names: HashSet::new(),
            consumed_blocks: HashSet::new(),
        }
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        // WORKAROUND: Set global content cache for get_node_text() helper
        helpers::set_dart_content_cache(&self.base.content);

        self.same_file_type_names = type_facts::collect_type_names(&self.base, tree.root_node());
        let mut symbols = Vec::new();
        self.visit_node(tree.root_node(), &mut symbols, None, 0);
        symbols
    }

    fn visit_node(
        &mut self,
        node: Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<&str>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        if node.kind().is_empty() {
            return;
        }

        // Skip nodes already consumed as Dart 3 modifier class bodies or generic class ERROR content
        if self.consumed_blocks.contains(&node.start_byte()) {
            return;
        }

        let mut symbol: Option<Symbol> = None;
        let current_parent_id = parent_id.map(|id| id.to_string());

        match node.kind() {
            "class_definition" | "class_declaration" => {
                symbol =
                    functions::extract_class(&mut self.base, &node, current_parent_id.as_deref());
            }
            "function_declaration" | "lambda_expression" => {
                symbol = functions::extract_function(
                    &mut self.base,
                    &node,
                    current_parent_id.as_deref(),
                );
            }
            "local_function_declaration" => {
                symbol = functions::extract_local_function(
                    &mut self.base,
                    &node,
                    current_parent_id.as_deref(),
                );
            }
            "function_signature" => {
                if let Some(parent) = node.parent()
                    && !matches!(
                        parent.kind(),
                        "method_signature" | "function_declaration" | "local_function_declaration"
                    )
                {
                    symbol = if current_parent_id.is_some() {
                        functions::extract_method(
                            &mut self.base,
                            &node,
                            current_parent_id.as_deref(),
                        )
                    } else {
                        functions::extract_function(
                            &mut self.base,
                            &node,
                            current_parent_id.as_deref(),
                        )
                    };
                }
            }
            "method_declaration" => {
                symbol = if let Some(constructor) = functions::nested_constructor_signature(&node) {
                    functions::extract_constructor(
                        &mut self.base,
                        &constructor,
                        current_parent_id.as_deref(),
                    )
                } else {
                    functions::extract_method(&mut self.base, &node, current_parent_id.as_deref())
                };
            }
            "method_signature" => {
                if !node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "method_declaration")
                {
                    symbol = if functions::nested_constructor_signature(&node).is_some() {
                        None
                    } else {
                        functions::extract_method(
                            &mut self.base,
                            &node,
                            current_parent_id.as_deref(),
                        )
                    };
                }
            }
            "constructor_signature"
            | "factory_constructor_signature"
            | "constant_constructor_signature"
                if !wrapped_constructor_signature(node) =>
            {
                symbol = functions::extract_constructor(
                    &mut self.base,
                    &node,
                    current_parent_id.as_deref(),
                );
            }
            "call_expression" => {
                // package:test call-style (Miller bridge test-roles): test()/group()/
                // setUp() etc. become test symbols. Non-test calls return None and
                // fall through to normal child recursion. A returned container/test
                // symbol is set as the parent for nested test calls below.
                symbol = test_calls::extract_dart_test_call(
                    &mut self.base,
                    &node,
                    current_parent_id.as_deref(),
                );
            }
            "enum_declaration" => {
                symbol = types::extract_enum(&mut self.base, &node, current_parent_id.as_deref());
            }
            "enum_constant" => {
                symbol = types::extract_enum_constant(
                    &mut self.base,
                    &node,
                    current_parent_id.as_deref(),
                );
            }
            "type_identifier" => {
                // `sealed class AsyncValue<T>` — grammar sees the generic `<T>` as a relational
                // expression, so `sealed` ends up as a standalone type_identifier at
                // source_file level rather than inside an ERROR. Detect the pattern here
                // and recover.
                if node.parent().is_some_and(is_dart_top_level_parent)
                    && let Some((class_sym, body_opt, container_start)) =
                        recover_dart3_generic_modifier_class(
                            &mut self.base,
                            &node,
                            current_parent_id.as_deref(),
                        )
                {
                    let class_id = class_sym.id.clone();
                    // Extract inheritance from source text before pushing symbol.
                    // Skip inheritance extraction when the node has no parent
                    // (root-level construct) rather than panicking.
                    if let Some(parent) = node.parent() {
                        let source = self.base.get_node_text(&parent);
                        for (target_name, kind) in extract_inheritance_from_source(&source) {
                            self.add_pending_relationship(PendingRelationship {
                                from_symbol_id: class_id.clone(),
                                callee_name: target_name,
                                kind,
                                file_path: self.base.file_path.clone(),
                                line_number: node.start_position().row as u32 + 1,
                                confidence: 0.8,
                            });
                        }
                    }
                    symbols.push(class_sym);
                    // Prevent the expression_statement/ERROR container from being double-visited
                    self.consumed_blocks.insert(container_start);
                    if let Some(body_node) = body_opt {
                        let Some(child_depth) = child_tree_depth(depth) else {
                            return;
                        };
                        let mut cursor = body_node.walk();
                        for child in body_node.children(&mut cursor) {
                            self.visit_node(child, symbols, Some(&class_id), child_depth);
                        }
                    }
                    return;
                }
            }
            "mixin_declaration" => {
                // Dart 3 `mixin class Foo {}`: try mixin class recovery first.
                // Tree-sitter produces two different structures for this depending on
                // context; recover_mixin_class_declaration handles both.
                if let Some(class_sym) = recover_mixin_class_declaration(
                    &mut self.base,
                    &node,
                    current_parent_id.as_deref(),
                ) {
                    let class_id = class_sym.id.clone();
                    // Extract inheritance from source text of the mixin_declaration
                    let source = self.base.get_node_text(&node);
                    for (target_name, kind) in extract_inheritance_from_source(&source) {
                        self.add_pending_relationship(PendingRelationship {
                            from_symbol_id: class_id.clone(),
                            callee_name: target_name,
                            kind,
                            file_path: self.base.file_path.clone(),
                            line_number: node.start_position().row as u32 + 1,
                            confidence: 0.8,
                        });
                    }
                    symbols.push(class_sym);
                    if let Some(body_node) = find_child_by_type(&node, "class_body") {
                        let Some(child_depth) = child_tree_depth(depth) else {
                            return;
                        };
                        let mut cursor = body_node.walk();
                        for child in body_node.children(&mut cursor) {
                            self.visit_node(child, symbols, Some(&class_id), child_depth);
                        }
                    }
                    return;
                }
                symbol = types::extract_mixin(&mut self.base, &node, current_parent_id.as_deref());
            }
            "extension_declaration" => {
                symbol =
                    types::extract_extension(&mut self.base, &node, current_parent_id.as_deref());
            }
            "getter_signature" => {
                symbol =
                    members::extract_getter(&mut self.base, &node, current_parent_id.as_deref());
            }
            "setter_signature" => {
                symbol =
                    members::extract_setter(&mut self.base, &node, current_parent_id.as_deref());
            }
            "declaration" => {
                symbol =
                    members::extract_field(&mut self.base, &node, current_parent_id.as_deref());
            }
            "local_variable_declaration" => {
                symbols.extend(locals::extract_locals(
                    self,
                    node,
                    current_parent_id.as_deref(),
                ));
            }
            "top_level_variable_declaration" => {
                symbol = functions::extract_variable(
                    &mut self.base,
                    &node,
                    current_parent_id.as_deref(),
                );
            }
            "initialized_variable_definition"
                if !node
                    .parent()
                    .is_some_and(|parent| parent.kind() == "local_variable_declaration") =>
            {
                symbol = functions::extract_variable(
                    &mut self.base,
                    &node,
                    current_parent_id.as_deref(),
                );
            }
            "type_alias" => {
                symbol =
                    types::extract_typedef(&mut self.base, &node, current_parent_id.as_deref());
            }
            "import_or_export" => {
                symbol = imports::extract_import_or_export(
                    &mut self.base,
                    &node,
                    current_parent_id.as_deref(),
                );
            }
            "ERROR" | "expression_statement"
                if node.parent().is_some_and(is_dart_top_level_parent) =>
            {
                // Dart 3 class modifier recovery: some parser releases produce
                // ERROR nodes for base/sealed/final/interface classes. Recover
                // class symbols from the ERROR content.
                if let Some(class_sym) = recover_dart3_modifier_class(
                    &mut self.base,
                    &node,
                    current_parent_id.as_deref(),
                ) {
                    let class_id = class_sym.id.clone();
                    // Extract inheritance from source text of the ERROR node
                    let source = self.base.get_node_text(&node);
                    for (target_name, kind) in extract_inheritance_from_source(&source) {
                        self.add_pending_relationship(PendingRelationship {
                            from_symbol_id: class_id.clone(),
                            callee_name: target_name,
                            kind,
                            file_path: self.base.file_path.clone(),
                            line_number: node.start_position().row as u32 + 1,
                            confidence: 0.8,
                        });
                    }
                    symbols.push(class_sym);

                    // The class body is the sibling `block` node immediately after this ERROR.
                    // Recurse into it with the class as parent so members are parented correctly.
                    // Mark the block as consumed to prevent double-visiting during
                    // source_file iteration.
                    if let Some(sibling) = node.next_sibling()
                        && sibling.kind() == "block"
                    {
                        self.consumed_blocks.insert(sibling.start_byte());
                        let Some(child_depth) = child_tree_depth(depth) else {
                            return;
                        };
                        let mut cursor = sibling.walk();
                        for child in sibling.children(&mut cursor) {
                            self.visit_node(child, symbols, Some(&class_id), child_depth);
                        }
                    }
                    // Skip normal child recursion for this ERROR node; we handled it.
                    return;
                }

                // Some parser releases split enhanced enum bodies after the first
                // enum_constant into ERROR and expression_statement siblings at
                // source_file level. Recover symbols generically by detecting enum context.
                if let Some(enum_id) = find_enum_context_parent(&node, symbols) {
                    recover_enum_symbols_from_error(&mut self.base, &node, Some(&enum_id), symbols);
                }
            }
            _ => {}
        }

        // Add symbol if extracted successfully
        let next_parent_id = if let Some(sym) = &symbol {
            if matches!(sym.kind, SymbolKind::Class) {
                self.same_file_type_names.insert(sym.name.clone());
            }
            symbols.push(sym.clone());
            if is_dart_callable(node.kind()) {
                symbols.extend(parameters::extract_parameter_symbols(
                    &mut self.base,
                    node,
                    &sym.id,
                ));
            }
            Some(sym.id.as_str())
        } else {
            current_parent_id.as_deref()
        };

        // Recursively visit children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child, symbols, next_parent_id, child_depth);
        }
    }

    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let mut rels =
            relationships::extract_relationships(&mut self.base, tree.root_node(), symbols);
        self.same_file_calls.clear();
        self.extract_pending_relationships(tree, symbols);

        for (caller_id, callee_id, line_number, span) in self.same_file_calls.drain(..) {
            rels.push(crate::base::Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    caller_id,
                    callee_id,
                    crate::base::RelationshipKind::Calls,
                    line_number
                ),
                from_symbol_id: caller_id,
                to_symbol_id: callee_id,
                kind: crate::base::RelationshipKind::Calls,
                file_path: self.base.file_path.clone(),
                line_number,
                span: Some(span),
                reference_site_is_exact: false,
                confidence: 0.9,
                metadata: None,
            });
        }

        rels
    }

    fn extract_pending_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) {
        let symbol_map: HashMap<String, &Symbol> =
            crate::base::ScopedSymbolIndex::unique_symbol_map(symbols);
        self.walk_for_pending_calls(tree.root_node(), &symbol_map, 0);
    }

    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        let mut types = HashMap::new();
        for symbol in symbols {
            if self.base.type_info.contains_key(&symbol.id) {
                continue;
            }
            let Some(signature) = &symbol.signature else {
                continue;
            };
            if let Some(type_text) = legacy_signature_type(signature, &symbol.kind) {
                types.insert(symbol.id.clone(), type_text);
            }
        }
        types
    }

    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        let containing_symbols = self.base.containing_symbol_index(symbols);
        identifiers::walk_tree_for_identifiers(
            &mut self.base,
            tree.root_node(),
            &containing_symbols,
            0,
        );
        self.base.identifiers.clone()
    }

    pub fn get_pending_relationships(&self) -> Vec<PendingRelationship> {
        self.base.get_pending_relationships()
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals (Miller bridge Phase 3).
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn add_pending_relationship(&mut self, pending: PendingRelationship) {
        self.base.add_pending_relationship(pending);
    }

    pub fn add_structured_pending_relationship(&mut self, pending: StructuredPendingRelationship) {
        self.base.add_structured_pending_relationship(pending);
    }

    pub fn get_structured_pending_relationships(&self) -> Vec<StructuredPendingRelationship> {
        self.base.get_structured_pending_relationships()
    }
}

// === Dart 3 Class Modifier Recovery ===
//
// Some Dart parser releases produce ERROR nodes for Dart 3 class modifiers
// (base, sealed, final, interface). These have a recognizable internal
// structure:
//
//   ERROR[type_identifier("base"), identifier("class"), identifier("ClassName")]
//   ERROR[final_builtin("final"), type_identifier("class"), ..., identifier("ClassName")]
//
// The class body `{}` appears as a sibling `block` node.

/// Dart 3 class modifier keywords that the grammar doesn't support.
const DART3_CLASS_MODIFIERS: &[&str] = &["base", "sealed", "final", "interface"];

/// Regex for extracting extends/implements/with clauses from source text.
/// Used by Dart 3 recovery paths where the AST is too mangled to walk.
static EXTENDS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bextends\s+([A-Z]\w*)").unwrap());
static IMPLEMENTS_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bimplements\s+([A-Z]\w*(?:\s*,\s*[A-Z]\w*)*)").unwrap());
static WITH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bwith\s+([A-Z]\w*(?:\s*,\s*[A-Z]\w*)*)").unwrap());

/// Extract extends/implements/with relationships from source text around a
/// recovered Dart 3 modifier class. The AST is mangled by tree-sitter, but
/// the source is correct, so regex is the reliable approach.
///
/// Returns a Vec of (name, RelationshipKind) pairs.
fn extract_inheritance_from_source(source: &str) -> Vec<(String, crate::base::RelationshipKind)> {
    use crate::base::RelationshipKind;
    let mut result = Vec::new();

    // Only look at text before the opening brace (the class header)
    let header = source.split('{').next().unwrap_or(source);

    if let Some(caps) = EXTENDS_RE.captures(header)
        && let Some(name) = caps.get(1)
    {
        result.push((name.as_str().to_string(), RelationshipKind::Extends));
    }

    for re in [&*IMPLEMENTS_RE, &*WITH_RE] {
        if let Some(caps) = re.captures(header)
            && let Some(names) = caps.get(1)
        {
            for name in names.as_str().split(',') {
                let name = name.trim();
                if !name.is_empty() && name.chars().next().is_some_and(|c| c.is_uppercase()) {
                    result.push((name.to_string(), RelationshipKind::Implements));
                }
            }
        }
    }

    result
}

/// Attempt to recover a class symbol from an ERROR node that represents a
/// Dart 3 modifier class (base/sealed/final/interface class).
///
/// Returns `Some(Symbol)` if the ERROR node matches the pattern, `None` otherwise.
fn recover_dart3_modifier_class(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    if node.kind() != "ERROR" {
        return None;
    }

    // Collect children info: we're looking for a pattern like
    //   [modifier_node] [class_keyword_node] [class_name_node] [optional extends/implements ...]
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();

    if children.len() < 2 {
        return None;
    }

    // Find the modifier and "class" keyword.
    // Patterns observed:
    //   base:      type_identifier("base") identifier("class") identifier("Name")
    //   sealed:    type_identifier("sealed") identifier("class") identifier("Name")
    //   interface: type_identifier("interface") identifier("class") identifier("Name")
    //   final:     final_builtin("final") type_identifier("class") ERROR("Name ...") | identifier("Name")
    let mut modifier: Option<String> = None;
    let mut class_name: Option<String> = None;
    let mut class_name_node: Option<Node> = None;
    let mut saw_class_keyword = false;

    for child in &children {
        let text = get_node_text(child);

        if modifier.is_none() && DART3_CLASS_MODIFIERS.contains(&text.as_str()) {
            modifier = Some(text);
            continue;
        }

        if modifier.is_some() && !saw_class_keyword && text == "class" {
            saw_class_keyword = true;
            continue;
        }

        if saw_class_keyword && class_name.is_none() {
            // The class name. For `final class`, tree-sitter sometimes wraps
            // "Name extends/implements ..." in a nested ERROR node, so check
            // for an identifier child inside it.
            if child.kind() == "identifier" || child.kind() == "type_identifier" {
                class_name = Some(text);
                class_name_node = Some(*child);
            } else if child.kind() == "ERROR" {
                // Nested ERROR: look for first identifier-like child
                let mut inner_cursor = child.walk();
                for grandchild in child.children(&mut inner_cursor) {
                    let gtext = get_node_text(&grandchild);
                    if (grandchild.kind() == "identifier" || grandchild.kind() == "type_identifier")
                        && gtext.chars().next().is_some_and(|c| c.is_uppercase())
                    {
                        class_name = Some(gtext);
                        class_name_node = Some(grandchild);
                        break;
                    }
                }
            }
            break;
        }
    }

    let modifier = modifier?;
    let name = class_name?;
    let name_node = class_name_node?;

    // Build signature: e.g. "sealed class Sealed"
    let signature = format!("{} class {}", modifier, name);

    Some(base.create_symbol(
        &name_node,
        name,
        SymbolKind::Class,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|id| id.to_string()),
            metadata: Some(HashMap::new()),
            doc_comment: base.find_doc_comment(node),
            ..Default::default()
        },
    ))
}

// === Dart 3 Generic Modifier Class Recovery ===
//
// When a Dart 3 modifier class has generic type parameters, e.g.:
//   sealed class AsyncValue<T> { ... }
//
// Some Dart parser releases cannot parse the `<T>` as a generic and instead
// treat `AsyncValue<T>` as a relational expression. This produces a completely
// different source_file-level structure:
//
//   type_identifier("sealed")          <- modifier sits outside the ERROR
//   initialized_identifier_list        <- "class" parsed as variable name
//     initialized_identifier
//       identifier("class")
//   ;
//   ERROR
//     relational_expression            <- AsyncValue < T interpreted as comparison
//       relational_expression
//         identifier("AsyncValue")     <- actual class name is here
//         relational_operator <
//         identifier("T")
//       relational_operator >
//       set_or_map_literal             <- class body { ... }
//
// The recovery triggers on the type_identifier node and walks its siblings.

/// Recover a Dart 3 modifier class whose generic parameter caused the grammar
/// to produce a type_identifier + initialized_identifier_list + ERROR pattern.
///
/// Returns `Some((Symbol, Option<body_node>, error_start_byte))` on match.
fn recover_dart3_generic_modifier_class<'a>(
    base: &mut BaseExtractor,
    node: &Node<'a>,
    parent_id: Option<&str>,
) -> Option<(Symbol, Option<Node<'a>>, usize)> {
    // Text must be a known Dart 3 modifier
    let modifier = get_node_text(node);
    if !DART3_CLASS_MODIFIERS.contains(&modifier.as_str()) {
        return None;
    }

    // The next NAMED sibling contains the "class" keyword (parsed as
    // initialized_identifier_list or similar).
    let next = node.next_named_sibling()?;
    if get_node_text(&next).trim() != "class" {
        return None;
    }

    // Walk named siblings forward to find the expression_statement or ERROR that
    // contains the relational_expression with the class name and body.
    // In the full-file case, a class body with members causes the relational
    // expression to appear in an expression_statement (not ERROR).
    let mut sib = next.next_named_sibling();
    let body_container = loop {
        let s = sib?;
        match s.kind() {
            "ERROR" | "expression_statement" => break s,
            _ => sib = s.next_named_sibling(),
        }
    };
    let container_start = body_container.start_byte();

    // Extract class name: leftmost identifier inside relational_expression(s)
    let class_name_node = find_leftmost_identifier_in_relational(&body_container)?;
    let name = get_node_text(&class_name_node);
    if !name.chars().next().is_some_and(|c| c.is_uppercase()) {
        return None;
    }

    // Class body (partial): set_or_map_literal inside the expression_statement/ERROR
    let body_node = find_set_or_map_literal_in_node(&body_container);

    let signature = format!("{} class {}", modifier, name);
    let symbol = base.create_symbol(
        &class_name_node,
        name,
        SymbolKind::Class,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|id| id.to_string()),
            metadata: Some(HashMap::new()),
            doc_comment: base.find_doc_comment(node),
            ..Default::default()
        },
    );

    Some((symbol, body_node, container_start))
}

/// Find the leftmost identifier in a nested relational_expression tree.
/// `relational_expression > relational_expression > identifier` — the
/// deepest-left identifier is the class name (e.g. `AsyncValue` in `AsyncValue<T>`).
fn find_leftmost_identifier_in_relational<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    find_leftmost_identifier_in_relational_at_depth(node, 0)
}

fn find_leftmost_identifier_in_relational_at_depth<'a>(
    node: &Node<'a>,
    depth: u32,
) -> Option<Node<'a>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "identifier" | "type_identifier" => return Some(child),
            "relational_expression" => {
                if let Some(found) =
                    find_leftmost_identifier_in_relational_at_depth(&child, child_depth)
                {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

/// Recursively search for the first `set_or_map_literal` node in a subtree.
fn find_set_or_map_literal_in_node<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    find_set_or_map_literal_in_node_at_depth(node, 0)
}

fn find_set_or_map_literal_in_node_at_depth<'a>(node: &Node<'a>, depth: u32) -> Option<Node<'a>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "set_or_map_literal" {
            return Some(child);
        }
        if let Some(found) = find_set_or_map_literal_in_node_at_depth(&child, child_depth) {
            return Some(found);
        }
    }
    None
}

// === Dart 3 mixin class Recovery ===
//
// `mixin class Foo {}` — since `mixin` is a valid Dart keyword, the grammar
// starts a mixin_declaration. It uses "class" as the mixin name (identifier)
// and puts the actual class name "Foo" in an ERROR child:
//
//   mixin_declaration
//     mixin = "mixin"
//     identifier = "class"    <- wrong: mixin name is "class"
//     ERROR
//       identifier = "Foo"    <- actual class name
//     class_body { }

/// Recover a `mixin class Foo {}` declaration misparsed as mixin_declaration.
///
/// Tree-sitter produces two different structures depending on context:
///
/// Structure 1 (isolated or after certain nodes):
///   mixin_declaration(mixin, identifier("class"), ERROR(identifier("Foo")), class_body)
///
/// Structure 2 (after complex preceding code):
///   mixin_declaration(mixin, ERROR(identifier("class")), identifier("Foo"), class_body)
///
/// Returns None for genuine mixin declarations (e.g. `mixin Foo on Bar`).
fn recover_mixin_class_declaration<'a>(
    base: &mut BaseExtractor,
    node: &Node<'a>,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    let children: Vec<Node<'a>> = node.children(&mut cursor).collect();

    let mut saw_mixin = false;
    let mut saw_class = false;
    let mut name: Option<String> = None;
    let mut name_node: Option<Node<'a>> = None;

    for child in children {
        let kind = child.kind();

        if kind == "mixin" {
            saw_mixin = true;
            continue;
        }

        if !saw_mixin {
            continue;
        }

        if !saw_class {
            // Look for the "class" keyword — either directly or inside an ERROR child
            if kind == "identifier" && get_node_text(&child) == "class" {
                saw_class = true;
                continue;
            }
            if kind == "ERROR" {
                let mut ec = child.walk();
                if child
                    .children(&mut ec)
                    .any(|gc| gc.kind() == "identifier" && get_node_text(&gc) == "class")
                {
                    saw_class = true;
                    continue;
                }
            }
            // Something other than "class" appeared after "mixin" — not mixin class
            break;
        }

        // After the "class" keyword, find the actual class name
        if name.is_none() {
            if kind == "identifier" || kind == "type_identifier" {
                let text = get_node_text(&child);
                if !text.is_empty() {
                    name = Some(text);
                    name_node = Some(child);
                }
            } else if kind == "ERROR" {
                // Structure 1: name is inside the ERROR
                let mut ec = child.walk();
                for gc in child.children(&mut ec) {
                    if (gc.kind() == "identifier" || gc.kind() == "type_identifier")
                        && !get_node_text(&gc).is_empty()
                    {
                        name = Some(get_node_text(&gc));
                        name_node = Some(gc);
                        break;
                    }
                }
            }
        }
    }

    let name = name?;
    let name_node = name_node?;

    let signature = format!("mixin class {}", name);
    Some(base.create_symbol(
        &name_node,
        name,
        SymbolKind::Class,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|id| id.to_string()),
            metadata: Some(HashMap::new()),
            doc_comment: base.find_doc_comment(node),
            ..Default::default()
        },
    ))
}

// === Generic Enhanced Enum Error Recovery ===

/// Walk backward through previous siblings to find an enum_declaration.
/// Some parser releases split enhanced enum bodies across sibling nodes at the
/// source_file level, so ERROR / expression_statement nodes that immediately follow
/// an enum_declaration likely contain the rest of that enum's body.
fn find_enum_context_parent(node: &Node, symbols: &[Symbol]) -> Option<String> {
    let mut prev = node.prev_sibling();
    while let Some(sib) = prev {
        match sib.kind() {
            "enum_declaration" => {
                // Find the enum symbol's id
                if let Some(name_node) = find_child_by_type(&sib, "identifier") {
                    let name = get_node_text(&name_node);
                    if let Some(sym) = symbols
                        .iter()
                        .find(|s| s.name == name && s.kind == SymbolKind::Enum)
                    {
                        return Some(sym.id.clone());
                    }
                }
                return None;
            }
            // Another ERROR or expression_statement is fine -- keep walking back
            "ERROR" | "expression_statement" | "local_variable_declaration" => {
                prev = sib.prev_sibling();
            }
            // Hit a different top-level declaration -- no enum context
            _ => return None,
        }
    }
    None
}

/// Entry point for recovering symbols from an ERROR / expression_statement node
/// that belongs to an enhanced enum whose body was misparsed.
fn recover_enum_symbols_from_error(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    // Collect names already extracted so we skip duplicates
    let already_extracted: HashSet<String> = symbols.iter().map(|s| s.name.clone()).collect();
    recover_from_node_recursive(base, node, parent_id, symbols, &already_extracted, 0);
}

/// Recursively walk a subtree recovering enum members and constructors.
///
/// In the misparsed tree the relevant patterns are:
///   - `member_access` containing an `identifier` child  -->  EnumMember
///   - `const_object_expression` with a `type_identifier` child  -->  Constructor
///   - plain `identifier` that is lowercase  -->  EnumMember (fallback)
///
/// We skip noise nodes like `parenthesized_expression`, `string_literal`,
/// `argument_part`, and `arguments` to avoid extracting garbage.
fn recover_from_node_recursive(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
    symbols: &mut Vec<Symbol>,
    already_extracted: &HashSet<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "member_access" => {
            // e.g. green('Green') parses as member_access with identifier "green"
            if let Some(id_node) = find_child_by_type(node, "identifier") {
                let name = get_node_text(&id_node);
                if !already_extracted.contains(&name) && looks_like_enum_value(&name) {
                    let sym = base.create_symbol(
                        &id_node,
                        name,
                        SymbolKind::EnumMember,
                        SymbolOptions {
                            visibility: Some(Visibility::Public),
                            parent_id: parent_id.map(|id| id.to_string()),
                            metadata: Some(HashMap::new()),
                            doc_comment: base.find_doc_comment(node),
                            ..Default::default()
                        },
                    );
                    symbols.push(sym);
                }
            }
            return; // Don't recurse into member_access children
        }
        "const_object_expression" => {
            // e.g. `const Color(this.displayName)` parses as const_object_expression
            if let Some(type_node) = find_child_by_type(node, "type_identifier") {
                let name = get_node_text(&type_node);
                // Only create if we don't already have a Constructor with this name
                let has_constructor = symbols
                    .iter()
                    .any(|s| s.name == name && s.kind == SymbolKind::Constructor);
                if !has_constructor {
                    let sym = base.create_symbol(
                        &type_node,
                        name.clone(),
                        SymbolKind::Constructor,
                        SymbolOptions {
                            signature: Some(format!("const {}", name)),
                            visibility: Some(Visibility::Public),
                            parent_id: parent_id.map(|id| id.to_string()),
                            metadata: Some(HashMap::new()),
                            doc_comment: base.find_doc_comment(node),
                            ..Default::default()
                        },
                    );
                    symbols.push(sym);
                }
            }
            return; // Don't recurse into const_object_expression children
        }
        // Skip noise nodes entirely
        "parenthesized_expression" | "string_literal" | "argument_part" | "arguments" => {
            return;
        }
        _ => {}
    }

    // Recurse into children for other node types
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        recover_from_node_recursive(
            base,
            &child,
            parent_id,
            symbols,
            already_extracted,
            child_depth,
        );
    }
}

/// Check whether an identifier plausibly looks like a Dart enum value.
///
/// Filters out noise that can leak through ERROR recovery:
/// - Private identifiers (start with `_`) are never enum values
/// - Dart keywords/built-ins that appear as identifiers in misparsed trees
/// - Single-character identifiers (loop variables, type params)
fn looks_like_enum_value(name: &str) -> bool {
    // Private identifiers are never enum values
    if name.starts_with('_') {
        return false;
    }

    // Single characters are noise (loop vars, type params)
    if name.len() <= 1 {
        return false;
    }

    // Dart keywords and built-in names that can appear as identifiers in ERROR nodes
    !matches!(
        name,
        "return"
            | "switch"
            | "case"
            | "default"
            | "throw"
            | "break"
            | "continue"
            | "this"
            | "super"
            | "if"
            | "else"
            | "for"
            | "while"
            | "do"
            | "try"
            | "catch"
            | "finally"
            | "new"
            | "const"
            | "var"
            | "final"
            | "static"
            | "void"
            | "null"
            | "true"
            | "false"
            | "async"
            | "await"
            | "yield"
            | "get"
            | "set"
            | "String"
            | "int"
            | "double"
            | "bool"
            | "List"
            | "Map"
            | "Set"
            | "Future"
            | "Stream"
            | "dynamic"
            | "Object"
            | "num"
    )
}
