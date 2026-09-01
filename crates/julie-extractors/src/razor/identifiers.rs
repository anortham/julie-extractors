/// LSP-quality identifier extraction for find_references support
use crate::base::{BaseExtractor, Identifier, IdentifierKind, Symbol, extract_type_arguments};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

impl super::RazorExtractor {
    /// Extract all identifier usages (function calls, member access, etc.)
    /// Following the Rust extractor reference implementation pattern
    pub fn extract_identifiers(
        &mut self,
        tree: &tree_sitter::Tree,
        symbols: &[Symbol],
    ) -> Vec<Identifier> {
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
    /// Razor-specific: handles C# code within Razor directives and code blocks
    fn extract_identifier_from_node(&mut self, node: Node, symbol_map: &HashMap<String, &Symbol>) {
        match node.kind() {
            // Function/method calls: foo(), bar.Baz()
            // These appear in C# code blocks within Razor (@code {}, @{}, etc.)
            "invocation_expression" => {
                // The name is typically a child of the invocation_expression
                // Look for identifier or member_access_expression
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "identifier" {
                        let name = self.base.get_node_text(&child);
                        let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);

                        self.base.create_identifier(
                            &child,
                            name,
                            IdentifierKind::Call,
                            containing_symbol_id,
                        );
                        break;
                    } else if child.kind() == "member_access_expression" {
                        // For member access, extract the rightmost identifier (the method name)
                        if let Some(name_node) = child.child_by_field_name("name") {
                            let name = self.base.get_node_text(&name_node);
                            let containing_symbol_id =
                                self.find_containing_symbol_id(node, symbol_map);
                            let receiver_type = self_receiver_type(&self.base, child);
                            self.base.create_identifier_with_receiver_type(
                                &name_node,
                                name,
                                IdentifierKind::Call,
                                containing_symbol_id,
                                receiver_type,
                            );
                        }
                        break;
                    }
                }
                // Phase 3: capture string-literal call-arguments (config-free; the
                // carrier classification + gate happen in the artifact language-policy pass).
                self.record_razor_call_arg_literals(node, symbol_map);
            }

            // Member access: object.field
            // These appear in C# code blocks and Razor expressions
            "member_access_expression" => {
                // Only extract if it's NOT part of an invocation_expression
                // (we handle those in the invocation_expression case above)
                if let Some(parent) = node.parent()
                    && parent.kind() == "invocation_expression"
                {
                    return; // Skip - handled by invocation_expression
                }

                // Extract the rightmost identifier (the member name)
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.base.get_node_text(&name_node);
                    let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);

                    self.base.create_identifier(
                        &name_node,
                        name,
                        IdentifierKind::MemberAccess,
                        containing_symbol_id,
                    );
                }
            }

            "member_binding_expression" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.base.get_node_text(&name_node);
                    let kind = if node
                        .parent()
                        .and_then(|parent| parent.parent())
                        .is_some_and(|grandparent| grandparent.kind() == "invocation_expression")
                    {
                        IdentifierKind::Call
                    } else {
                        IdentifierKind::MemberAccess
                    };
                    let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);
                    self.base
                        .create_identifier(&name_node, name, kind, containing_symbol_id);
                }
            }

            // Type references in C# code blocks: `List<IBrowserFile>`, generics, etc.
            // Razor embeds C# with the same `generic_name` + `type_argument_list` grammar
            // as standalone C# — reuse the same outermost-check and decomposer logic.
            //
            // `variable_ref` complement arm (else-branch): a bare `identifier` used as
            // a value or as the object/receiver of a member access — the reads the
            // Call/MemberAccess/TypeUsage arms do not own. The TypeUsage check runs
            // first, so a type position never reaches the value-read predicate
            // (single row per node; no duplicates).
            "identifier" => {
                let name = self.base.get_node_text(&node);
                // Rule 5: reuse the existing builtin/keyword filter (`this`/`true`/
                // `false`/`null` are distinct grammar nodes and never reach here).
                if is_csharp_builtin_type(&name) {
                    return;
                }
                if is_csharp_type_usage_identifier(node) {
                    let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);
                    let identifier = self.base.create_identifier(
                        &node,
                        name,
                        IdentifierKind::TypeUsage,
                        containing_symbol_id,
                    );
                    record_outermost_generic_type_arguments(&mut self.base, node, &identifier);
                } else if is_razor_value_read_identifier(node) {
                    let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);
                    self.base.create_identifier(
                        &node,
                        name,
                        IdentifierKind::VariableRef,
                        containing_symbol_id,
                    );
                }
            }

            _ => {
                // Skip other node types
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

    /// Capture string-literal arguments of a Razor/C# `invocation_expression`
    /// as `Literal` records. Config-free: `carrier` is the invoked method name
    /// (mirrors the C# leg); the URL/SQL classification and the carrier gate run
    /// later in the artifact language-policy pass. Razor embeds C#, so each argument is wrapped
    /// in an `argument` node whose value is its last named child. `arg_position`
    /// is counted over the full argument list.
    fn record_razor_call_arg_literals(
        &mut self,
        node: Node,
        symbol_map: &HashMap<String, &Symbol>,
    ) {
        let Some(function) = node.child_by_field_name("function") else {
            return;
        };
        let Some(args) = node.child_by_field_name("arguments") else {
            return;
        };
        let carrier = razor_carrier(&self.base, function);
        let containing_symbol_id = self.find_containing_symbol_id(node, symbol_map);

        let mut cursor = args.walk();
        for (pos, arg) in args.named_children(&mut cursor).enumerate() {
            let value = if arg.kind() == "argument" {
                let mut vc = arg.walk();
                arg.named_children(&mut vc).last()
            } else {
                Some(arg)
            };
            if let Some(value) = value
                && let Some(text) = self.base.decode_string_literal(&value)
            {
                self.base.record_literal(
                    &value,
                    text,
                    carrier.clone(),
                    pos as u32,
                    containing_symbol_id.clone(),
                );
            }
        }
    }
}

/// Derive a Razor/C# call's carrier: the invoked method name with generic type
/// arguments stripped (`conn.Query<User>` -> `Query`, `Execute` -> `Execute`).
/// The receiver is dropped — Dapper/ADO/HttpClient carriers are matched by bare
/// method name via the gate's last-segment rule, and the receiver is usually a
/// local variable.
fn razor_carrier(base: &BaseExtractor, function: Node) -> Option<String> {
    let text = match function.kind() {
        "identifier" | "generic_name" => base.get_node_text(&function),
        "member_access_expression" => function
            .child_by_field_name("name")
            .map(|n| base.get_node_text(&n))?,
        _ => base.get_node_text(&function),
    };
    let stripped = match text.find('<') {
        Some(i) => text[..i].to_string(),
        None => text,
    };
    if stripped.is_empty() {
        None
    } else {
        Some(stripped)
    }
}

// ── Free helpers (mirrors of csharp/identifiers.rs logic) ─────────────────────
// Razor embeds C# with the same generic_name + type_argument_list grammar.
// These functions operate on BaseExtractor / Node only — no C#-extractor coupling.

/// If `name_node` is the base identifier of an outermost `generic_name` use site
/// in Razor C# code (e.g. `List` in `List<IBrowserFile>`), record its ordered/
/// nested type arguments against `identifier`. Nested generics (whose `generic_name`
/// parent is a `type_argument_list`) are skipped — they ride along as `children`.
fn record_outermost_generic_type_arguments(
    base: &mut BaseExtractor,
    name_node: Node,
    identifier: &Identifier,
) {
    let Some(generic_name) = name_node.parent() else {
        return;
    };
    if generic_name.kind() != "generic_name" {
        return;
    }
    // A generic_name nested inside type_argument_list is itself a type argument
    // of an outer generic — skip here; it rides along as a child.
    if generic_name
        .parent()
        .map(|p| p.kind() == "type_argument_list")
        .unwrap_or(false)
    {
        return;
    }
    let Some(arg_list) = type_argument_list_child(generic_name) else {
        return;
    };
    let arguments = extract_type_arguments(base, arg_list, decompose_csharp_type_arg);
    base.record_type_arguments(identifier, arguments);
}

/// `TypeArgDecomposer` for Razor/C#: maps a child of `type_argument_list` to its
/// applied argument. Named `generic_name` children recurse (nested generics);
/// everything else (identifier, predefined_type, array_type, …) returns its
/// source text as a leaf. Unnamed punctuation is skipped.
fn decompose_csharp_type_arg<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None;
    }
    match node.kind() {
        "generic_name" => {
            // Name is the first identifier child of generic_name.
            let name = direct_identifier(base, node)
                .map(|(_, n)| n)
                .unwrap_or_else(|| base.get_node_text(&node));
            Some((name, type_argument_list_child(node)))
        }
        _ => Some((base.get_node_text(&node), None)),
    }
}

/// First `type_argument_list` child of a `generic_name` node.
fn type_argument_list_child(generic_name: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = generic_name.walk();
    generic_name
        .children(&mut cursor)
        .find(|c| c.kind() == "type_argument_list")
}

/// First `identifier` child of `node`, returned with its source text.
fn direct_identifier<'a>(base: &BaseExtractor, node: Node<'a>) -> Option<(Node<'a>, String)> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            return Some((child, base.get_node_text(&child)));
        }
    }
    None
}

/// Returns `true` when `node` is an `identifier` used in a type-annotation position
/// inside Razor/C# code (field type, parameter type, return type, generic arg, etc.).
fn is_csharp_type_usage_identifier(node: Node) -> bool {
    if is_csharp_declaration_name(node) {
        return false;
    }
    let mut current = node;
    while let Some(parent) = current.parent() {
        if let Some(type_node) = parent.child_by_field_name("type")
            && contains_node(type_node, node)
        {
            return true;
        }
        match parent.kind() {
            "generic_name" | "qualified_name" | "array_type" | "nullable_type" | "pointer_type"
            | "tuple_type" | "type_argument_list" => return true,
            "object_creation_expression" => {
                if let Some(type_node) = parent.child_by_field_name("type")
                    && contains_node(type_node, node)
                {
                    return true;
                }
            }
            "invocation_expression"
            | "member_access_expression"
            | "argument_list"
            | "assignment_expression"
            | "return_statement"
            | "block"
            | "compilation_unit" => return false,
            _ => {}
        }
        current = parent;
    }
    false
}

/// Rule 1/4 predicate: is this bare `identifier` a value read or a member-access
/// receiver — the complement of the Call/MemberAccess/TypeUsage arms? Mirror of
/// the locked reference `is_csharp_value_read_identifier` in csharp/identifiers.rs
/// (see its LOCKED SEMANTIC CONTRACT doc comment for the six rules), adapted to
/// the tree-sitter-razor grammar, which embeds C#-shaped statement/expression
/// nodes inside Razor markup nodes. Razor-specific facts verified empirically:
/// `assignment_expression` carries the same `operator` field as C#;
/// `razor_foreach` (markup) and `foreach_statement` (@code) both declare their
/// loop variable in `left`; `razor_implicit_expression` (`@total`),
/// `razor_condition` (`@if (flag)`), and `razor_attribute_value`
/// (`@onclick="Handler"`) identifiers are value READS and fall through to the
/// inclusive default; `@inherits`/`@implements`/`@typeparam`/`@layout`
/// directives name types, not values.
fn is_razor_value_read_identifier(node: Node) -> bool {
    // Rule 3: never a type/method/property/namespace/type-parameter definition name.
    if is_csharp_declaration_name(node) {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };

    // Rule 2: a `type`/`returns` field identifier is a type usage, not a value
    // (the TypeUsage arm owns type positions; excluding both fields here keeps
    // the predicate correct in isolation, mirroring the C# reference).
    if parent.child_by_field_name("type").map(|t| t.id()) == Some(node.id())
        || parent.child_by_field_name("returns").map(|t| t.id()) == Some(node.id())
    {
        return false;
    }

    let is_name_field = parent.child_by_field_name("name").map(|n| n.id()) == Some(node.id());

    match parent.kind() {
        // Rule 2: the callee identifier of a call is owned by the Call arm.
        "invocation_expression" => false,
        // Rule 1/2: only the receiver (`expression`) of a member access is a read;
        // the accessed member `name` is owned by the MemberAccess/Call arms.
        "member_access_expression" => {
            parent.child_by_field_name("expression").map(|e| e.id()) == Some(node.id())
        }
        // `?.Prop`: the `condition` receiver is a read; the bound member name is not.
        "conditional_access_expression" => {
            parent.child_by_field_name("condition").map(|c| c.id()) == Some(node.id())
        }
        "member_binding_expression" => false,

        // Rule 3: definition names the shared declaration guard does not cover.
        // Their NON-name identifier children (a declarator initializer value, the
        // foreach collection) fall through below as reads.
        "constructor_declaration"
        | "destructor_declaration"
        | "record_declaration"
        | "record_struct_declaration"
        | "delegate_declaration"
        | "enum_member_declaration"
        | "local_function_statement"
        | "event_declaration" => !is_name_field,
        "variable_declarator"
        | "parameter"
        | "declaration_expression"
        | "catch_declaration"
        | "implicit_parameter" => !is_name_field,
        // foreach loop variable (`left`) is a definition; the `right` collection
        // reads. Covers both the @code-block statement and the markup form.
        "foreach_statement" | "razor_foreach" => {
            parent.child_by_field_name("left").map(|l| l.id()) != Some(node.id())
        }

        // Rule 3: labels and using/extern aliases / namespace names are not variables.
        "labeled_statement"
        | "goto_statement"
        | "using_directive"
        | "extern_alias_directive"
        | "namespace_declaration"
        | "file_scoped_namespace_declaration" => false,
        // Razor directives that name a type, type parameter, or layout — not values.
        "razor_inherits_directive"
        | "razor_implements_directive"
        | "razor_typeparam_directive"
        | "razor_layout_directive" => false,

        // Rule 1: an argument value is a read; the `name:` label of a named
        // argument (`foo(bar: 5)`) is a parameter name, not a read.
        "argument" => !is_name_field,
        // Rule 1: attribute named-arg member / positional value is a read; the
        // attribute's own `name` is a type usage.
        "attribute" => false,
        "attribute_argument" => true,

        // Rule 4: assignment RHS is a read; the LHS is a read only for a COMPOUND
        // operator or an object/collection-initializer member. A plain `x = 5`
        // LHS is write-only.
        "assignment_expression" => {
            let is_left = parent.child_by_field_name("left").map(|n| n.id()) == Some(node.id());
            if !is_left {
                return true;
            }
            if parent
                .parent()
                .map(|g| g.kind() == "initializer_expression")
                .unwrap_or(false)
            {
                return true;
            }
            parent
                .child_by_field_name("operator")
                .map(|op| op.kind() != "=")
                .unwrap_or(false)
        }

        // Type positions (also removed by the TypeUsage arm via ordering; kept
        // explicit so the predicate is correct in isolation).
        "qualified_name" | "generic_name" | "type_argument_list" | "array_type"
        | "nullable_type" | "pointer_type" | "tuple_type" | "base_list" => false,

        // Every other value slot — return / binary / conditional / interpolation /
        // initializer element / razor implicit or explicit expression / razor
        // condition / razor attribute value — is a read.
        _ => true,
    }
}

/// Returns `true` when `node` is the declared name of a type, method, property,
/// namespace, or generic type parameter — not a reference position.
fn is_csharp_declaration_name(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if let Some(name_node) = parent.child_by_field_name("name")
        && name_node.id() == node.id()
    {
        return matches!(
            parent.kind(),
            "class_declaration"
                | "interface_declaration"
                | "struct_declaration"
                | "enum_declaration"
                | "method_declaration"
                | "property_declaration"
                | "namespace_declaration"
                | "type_parameter"
        );
    }
    false
}

/// Returns `true` when `child` is byte-range-contained within `parent`.
fn contains_node(parent: Node, child: Node) -> bool {
    child.start_byte() >= parent.start_byte() && child.end_byte() <= parent.end_byte()
}

/// Returns `true` for C# builtin type keywords that are noise for centrality.
fn is_csharp_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "byte"
            | "char"
            | "decimal"
            | "double"
            | "float"
            | "int"
            | "long"
            | "object"
            | "sbyte"
            | "short"
            | "string"
            | "uint"
            | "ulong"
            | "ushort"
            | "var"
            | "void"
    )
}

fn self_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let member_access = match node.kind() {
        "member_access_expression" => node,
        "invocation_expression" => node
            .child_by_field_name("function")
            .filter(|function| function.kind() == "member_access_expression")?,
        _ => return None,
    };
    let receiver = member_access.child(0)?;
    match receiver.kind() {
        "this" => enclosing_type_name(base, member_access)
            .or_else(|| component_name_from_file_path(&base.file_path)),
        _ => None,
    }

}

fn enclosing_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "class_declaration"
                | "struct_declaration"
                | "record_declaration"
                | "interface_declaration"
        ) {
            return candidate
                .child_by_field_name("name")
                .map(|name_node| base.get_node_text(&name_node));
        }
        current = candidate.parent();
    }
    None
}


fn component_name_from_file_path(file_path: &str) -> Option<String> {
    let path = std::path::Path::new(file_path);
    if path.extension().and_then(|extension| extension.to_str()) != Some("razor") {
        return None;
    }
    if matches!(
        path.file_stem().and_then(|stem| stem.to_str()),
        Some("_Imports" | "_ViewImports")
    ) {
        return None;
    }
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .map(ToOwned::to_owned)
}
