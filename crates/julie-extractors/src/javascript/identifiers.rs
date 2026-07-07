//! Identifier extraction for JavaScript
//!
//! Handles extraction of all identifier usages including function calls,
//! member access, and other references used for LSP-quality find_references.

use crate::base::{Identifier, IdentifierKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

impl super::JavaScriptExtractor {
    /// Extract all identifier usages (function calls, member access, etc.)
    /// Following the Rust extractor reference implementation pattern
    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        // Create symbol map for fast lookup
        let symbol_map: HashMap<String, &Symbol> =
            symbols.iter().map(|s| (s.id.clone(), s)).collect();

        // Walk the tree and extract identifiers
        self.walk_tree_for_identifiers(tree.root_node(), &symbol_map, 0);

        // Return the collected identifiers
        self.base.identifiers.clone()
    }

    /// Recursively walk tree extracting identifiers from each node
    fn walk_tree_for_identifiers(
        &mut self,
        node: Node,
        symbol_map: &HashMap<String, &Symbol>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        // Extract identifier from this node if applicable
        self.extract_identifier_from_node(node, symbol_map);

        // Recursively walk children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_tree_for_identifiers(child, symbol_map, child_depth);
        }
    }

    /// Extract identifier from a single node based on its kind
    fn extract_identifier_from_node(&mut self, node: Node, symbol_map: &HashMap<String, &Symbol>) {
        match node.kind() {
            // Function/method calls: foo(), bar.baz()
            "call_expression" => {
                // The function being called is in the "function" field
                if let Some(function_node) = node.child_by_field_name("function") {
                    match function_node.kind() {
                        "identifier" => {
                            // Simple function call: foo()
                            let name = self.base.get_node_text(&function_node);
                            let containing_symbol_id =
                                self.find_containing_symbol_id(node, symbol_map);

                            self.base.create_identifier(
                                &function_node,
                                name,
                                IdentifierKind::Call,
                                containing_symbol_id,
                            );
                        }
                        "member_expression" => {
                            // Member call: object.method()
                            // Extract the rightmost identifier (the method name)
                            if let Some(property_node) =
                                function_node.child_by_field_name("property")
                            {
                                let name = self.base.get_node_text(&property_node);
                                let containing_symbol_id =
                                    self.find_containing_symbol_id(node, symbol_map);

                                self.base.create_identifier(
                                    &property_node,
                                    name,
                                    IdentifierKind::Call,
                                    containing_symbol_id,
                                );
                            }
                        }
                        _ => {
                            // Other cases like computed member expressions
                            // Skip for now
                        }
                    }
                }
                // Phase 3: capture string-literal call-arguments (config-free; the
                // carrier classification + gate happen in the artifact language-policy pass).
                self.record_call_arg_literals(&node, symbol_map);
            }

            "new_expression" => {
                if let Some((name_node, name)) = self.constructor_identifier(&node) {
                    let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);
                    self.base.create_identifier(
                        &name_node,
                        name,
                        IdentifierKind::Call,
                        containing_symbol_id,
                    );
                }
            }

            // Member access: object.property
            "member_expression" => {
                // Only extract if it's NOT part of a call_expression
                // (we handle those in the call_expression case above)
                if let Some(parent) = node.parent() {
                    if parent.kind() == "call_expression" {
                        // Check if this member_expression is the function being called
                        if let Some(function_node) = parent.child_by_field_name("function")
                            && function_node.id() == node.id()
                        {
                            return; // Skip - handled by call_expression
                        }
                    }
                    if parent.kind() == "new_expression"
                        && let Some(constructor_node) = parent.child_by_field_name("constructor")
                        && constructor_node.id() == node.id()
                    {
                        return;
                    }
                }

                // Extract the rightmost identifier (the property name)
                if let Some(property_node) = node.child_by_field_name("property") {
                    let name = self.base.get_node_text(&property_node);
                    let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);

                    self.base.create_identifier(
                        &property_node,
                        name,
                        IdentifierKind::MemberAccess,
                        containing_symbol_id,
                    );
                }
            }

            // `variable_ref` complement arm: a bare `identifier` used as a value or
            // as the object/receiver of a member access — the reads the Call/
            // MemberAccess arms above do not own. Property names are a distinct
            // `property_identifier` node kind, so they can never reach this arm.
            "identifier" if is_ecmascript_value_read_identifier(node) => {
                let name = self.base.get_node_text(&node);
                let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);
                self.base.create_identifier(
                    &node,
                    name,
                    IdentifierKind::VariableRef,
                    containing_symbol_id,
                );
            }

            // `{foo}` object-literal shorthand is a READ of the binding `foo`.
            // The destructuring form (`const {foo} = o`) is a distinct node kind
            // (`shorthand_property_identifier_pattern`) and stays excluded; an
            // object-literal KEY (`{foo: 1}`) is a `property_identifier`, not this.
            "shorthand_property_identifier" => {
                let name = self.base.get_node_text(&node);
                let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);
                self.base.create_identifier(
                    &node,
                    name,
                    IdentifierKind::VariableRef,
                    containing_symbol_id,
                );
            }

            _ => {
                // Skip other node types for now
                // Future: type usage, constructor calls, etc.
            }
        }
    }

    /// Find the ID of the symbol that contains this node
    /// CRITICAL: Only search symbols from THIS FILE (file-scoped filtering)
    fn find_containing_symbol_id(
        &self,
        node: Node,
        symbol_map: &HashMap<String, &Symbol>,
    ) -> Option<String> {
        self.base
            .find_containing_symbol_from_map(&node, symbol_map)
            .map(|s| s.id.clone())
    }

    // ========================================================================
    // String-literal call-argument capture (Miller bridge Phase 3)
    // ========================================================================

    /// Capture string-literal arguments of a JS `call_expression` as `Literal`
    /// records. Config-free: `carrier` is the verbatim callee text; the URL/SQL
    /// classification and the carrier gate run later in the artifact language-policy pass.
    /// Mirrors the TypeScript leg (JS shares the same `call_expression` grammar
    /// shape: `function` callee + `arguments` list, with tagged templates
    /// arriving as a `template_string` in the `arguments` field). `arg_position`
    /// is counted over the full (named) argument list.
    fn record_call_arg_literals(
        &mut self,
        call_node: &Node,
        symbol_map: &HashMap<String, &Symbol>,
    ) {
        let Some(function_node) = call_node.child_by_field_name("function") else {
            return;
        };
        let Some(args_node) = call_node.child_by_field_name("arguments") else {
            return;
        };
        let carrier = self.callee_text(function_node);
        let containing_symbol_id = self.find_containing_symbol_id(*call_node, symbol_map);

        let mut cursor = args_node.walk();
        for (pos, arg) in args_node.named_children(&mut cursor).enumerate() {
            if let Some(text) = self.base.decode_string_literal(&arg) {
                self.base.record_literal(
                    &arg,
                    text,
                    carrier.clone(),
                    pos as u32,
                    containing_symbol_id.clone(),
                );
            }
        }
    }

    /// Derive the verbatim callee text used as a literal's `carrier`.
    ///
    /// Plain `identifier` → its text (`fetch`). `member_expression` → the
    /// `object.property` join (`axios.get`) so dotted client APIs match config.
    fn callee_text(&self, function_node: Node) -> Option<String> {
        match function_node.kind() {
            "identifier" => Some(self.base.get_node_text(&function_node)),
            "member_expression" => {
                let object = function_node
                    .child_by_field_name("object")
                    .map(|n| self.base.get_node_text(&n));
                let property = function_node
                    .child_by_field_name("property")
                    .map(|n| self.base.get_node_text(&n));
                match (object, property) {
                    (Some(o), Some(p)) => Some(format!("{o}.{p}")),
                    (None, Some(p)) => Some(p),
                    _ => None,
                }
            }
            _ => {
                let text = self.base.get_node_text(&function_node);
                if text.is_empty() { None } else { Some(text) }
            }
        }
    }

    fn constructor_identifier<'tree>(&self, node: &Node<'tree>) -> Option<(Node<'tree>, String)> {
        let constructor = node
            .child_by_field_name("constructor")
            .or_else(|| node.child_by_field_name("callee"))
            .or_else(|| {
                let mut cursor = node.walk();
                node.named_children(&mut cursor)
                    .find(|child| child.kind() != "arguments")
            })?;
        self.terminal_identifier(constructor)
    }

    fn terminal_identifier<'tree>(&self, node: Node<'tree>) -> Option<(Node<'tree>, String)> {
        match node.kind() {
            "identifier" | "property_identifier" | "private_property_identifier" => {
                Some((node, self.base.get_node_text(&node)))
            }
            "member_expression" => node
                .child_by_field_name("property")
                .and_then(|property| self.terminal_identifier(property)),
            _ => None,
        }
    }
}

// ============================================================================
// variable_ref emission — ECMAScript rule 1/4 predicate
// ============================================================================
//
// Shared by the JavaScript, TypeScript, and Vue extractors (Vue parses embedded
// <script> sections with these same tree-sitter grammars — do not fork this).
// Mirrors the STRUCTURE of the locked reference `is_csharp_value_read_identifier`
// in csharp/identifiers.rs (see its LOCKED SEMANTIC CONTRACT doc comment for the
// six rules); node kinds and field names below were verified empirically against
// the vendored tree-sitter-javascript and tree-sitter-typescript grammars.
// TS-only node kinds simply never occur under the JS grammar.
//
// Rule 5 note: `this` / `true` / `false` / `null` / `undefined` are distinct
// node kinds in both grammars (never `identifier`), so keywords are structurally
// excluded and no name-based builtin filter is needed.

/// Rule 1/4 predicate: is this bare `identifier` a value read or a member-access
/// receiver — the complement of the Call/MemberAccess/TypeUsage arms? The default
/// is inclusive (`_ => true`) with enumerated exclusions, exactly like the C#
/// reference arm.
pub(crate) fn is_ecmascript_value_read_identifier(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_field = |field: &str| {
        parent
            .child_by_field_name(field)
            .map(|f| f.id() == node.id())
            .unwrap_or(false)
    };

    match parent.kind() {
        // Rule 2: the bare callee of a call / constructor of `new` is owned by the
        // Call arm. Call arguments are wrapped in an `arguments` node and never
        // appear as direct children here.
        "call_expression" | "new_expression" => false,
        // Rule 1/2: only the receiver (`object`) of a member access is a read; the
        // accessed `property` is a `property_identifier` (MemberAccess/Call arms).
        "member_expression" => is_field("object"),
        // Rule 2 (TS): `class A extends Base` puts `Base` in an expression-context
        // `value` field owned by the TypeUsage extends_clause arm. The JS grammar
        // uses `class_heritage` instead, which no arm owns — it falls through to
        // the default below as a read.
        "extends_clause" => false,
        // Rule 2 (JSX): element names are owned by the JSX Call arm (uppercase
        // components) or are plain HTML tag names (lowercase) — never value reads.
        "jsx_opening_element" | "jsx_closing_element" | "jsx_self_closing_element" => false,

        // Rule 3: declaration names. Their NON-name identifier children (e.g. a
        // declarator's initializer value) fall through below as reads.
        "variable_declarator"
        | "function_declaration"
        | "generator_function_declaration"
        | "function_expression"
        | "generator_function"
        | "class_declaration"
        | "class"
        | "enum_declaration"
        | "module"
        | "internal_module" => !is_field("name"),
        // JS bare parameters sit directly under `formal_parameters`; TS wraps them
        // in (required|optional)_parameter with the name in `pattern` and an
        // optional default in `value` — the default IS a read.
        "formal_parameters" => false,
        "required_parameter" | "optional_parameter" => !is_field("pattern"),
        // Single-parameter arrow form `x => ...` declares `x` in the `parameter`
        // field; the body expression falls through as a read.
        "arrow_function" => !is_field("parameter"),
        "catch_clause" => !is_field("parameter"),
        // Destructuring patterns DECLARE bindings (`const {a, b: c} = o`); the
        // source object is the declarator's `value` and falls through as a read.
        "array_pattern" | "object_pattern" | "rest_pattern" | "pair_pattern" => false,
        // Pattern/parameter default value: `left` declares, `right` reads.
        "assignment_pattern" => !is_field("left"),
        // Import bindings are declarations (default, named, namespace, require).
        "import_specifier" | "import_clause" | "namespace_import" | "import_require_clause" => {
            false
        }
        // `export { local as alias }`: `name` READS the local binding (it keeps
        // the symbol alive); `alias` only names the export (rule 3).
        "export_specifier" => is_field("name"),
        // TS: index-signature parameter (`[key: string]`) and type-predicate
        // subject (`x is T`) are type-context names, not value reads.
        "index_signature" | "type_predicate" => false,
        // TS type positions that can contain a bare `identifier` (the qualifier of
        // `NS.Type`, nested jsx/module names) — type machinery, not value reads.
        "nested_type_identifier" | "nested_identifier" => false,

        // Rule 4: a PLAIN assignment LHS is write-only; the RHS reads. Compound
        // assignment (`x += 1`) is a distinct `augmented_assignment_expression`
        // node kind, so its LHS falls through below as a read.
        "assignment_expression" => !is_field("left"),
        // for-in/of: the loop binding (`left`) is a write target/declaration; the
        // iterated collection (`right`) reads.
        "for_in_statement" => !is_field("left"),

        // Every other position — argument, operand, return value, template
        // substitution, JSX expression `{x}`, collection element, spread,
        // decorator, `typeof` query, update expression — is a read (rule 1).
        _ => true,
    }
}
