// C# Identifier Extraction

use crate::base::{BaseExtractor, Identifier, IdentifierKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

/// Extract all identifier usages
pub fn extract_identifiers(
    base: &mut BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let symbol_map: HashMap<String, &Symbol> = symbols.iter().map(|s| (s.id.clone(), s)).collect();
    walk_tree_for_identifiers(base, tree.root_node(), &symbol_map, 0);
    base.identifiers.clone()
}

fn walk_tree_for_identifiers(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    extract_identifier_from_node(base, node, symbol_map);
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_identifiers(base, child, symbol_map, child_depth);
    }
}

fn extract_identifier_from_node(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    match node.kind() {
        "invocation_expression" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    let name = base.get_node_text(&child);
                    let containing_symbol_id = find_containing_symbol_id(base, node, symbol_map);
                    base.create_identifier(
                        &child,
                        name,
                        IdentifierKind::Call,
                        containing_symbol_id,
                    );
                    break;
                } else if child.kind() == "member_access_expression" {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = base.get_node_text(&name_node);
                        let containing_symbol_id =
                            find_containing_symbol_id(base, node, symbol_map);
                        base.create_identifier(
                            &name_node,
                            name,
                            IdentifierKind::Call,
                            containing_symbol_id,
                        );
                    }
                    break;
                }
            }
            // Phase 3: capture string-literal call-arguments (config-free; the
            // carrier classification + gate happen in the artifact language-policy pass).
            record_csharp_call_arg_literals(base, node, symbol_map);
        }
        "object_creation_expression" => {
            if let Some(type_node) = node.child_by_field_name("type")
                && let Some((name_node, name)) = terminal_type_identifier(base, type_node)
            {
                let containing_symbol_id = find_containing_symbol_id(base, node, symbol_map);
                base.create_identifier(
                    &name_node,
                    name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                );
            }
        }
        "member_access_expression" => {
            if let Some(parent) = node.parent()
                && parent.kind() == "invocation_expression"
            {
                return;
            }

            if let Some(name_node) = node.child_by_field_name("name") {
                let name = base.get_node_text(&name_node);
                let containing_symbol_id = find_containing_symbol_id(base, node, symbol_map);
                base.create_identifier(
                    &name_node,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }
        "identifier" if is_csharp_type_usage_identifier(node) => {
            let name = base.get_node_text(&node);
            if !is_csharp_builtin_type(&name) {
                let containing_symbol_id = find_containing_symbol_id(base, node, symbol_map);
                let identifier = base.create_identifier(
                    &node,
                    name,
                    IdentifierKind::TypeUsage,
                    containing_symbol_id,
                );
                record_outermost_generic_type_arguments(base, node, &identifier);
            }
        }
        // `variable_ref` complement arm: a bare `identifier` used as a value or as
        // the object/receiver of a member access — the reads the Call/MemberAccess/
        // TypeUsage arms above do not own. Evaluated only after the TypeUsage guard,
        // so type positions never reach here (single row per node; no duplicates).
        "identifier" if is_csharp_value_read_identifier(node) => {
            let name = base.get_node_text(&node);
            // Rule 5: reuse the builtin/keyword filter. (`this`/`base`/`true`/`false`/
            // `null` are distinct grammar nodes, not `identifier`, so they never
            // reach this arm.)
            if !is_csharp_builtin_type(&name) {
                let containing_symbol_id = find_containing_symbol_id(base, node, symbol_map);
                base.create_identifier(
                    &node,
                    name,
                    IdentifierKind::VariableRef,
                    containing_symbol_id,
                );
            }
        }
        _ => {}
    }
}

/// If `name_node` is the base identifier of an *outermost* generic type use
/// (e.g. the `Dictionary` of `Dictionary<string, List<int>>`), record that
/// generic's ordered/nested applied type arguments against `identifier`.
///
/// Fires from the universal `identifier` arm so it uniformly covers member
/// types, `new T<...>()`, and generic invocations (`CreateMap<A,B>()`,
/// `AddScoped<IFoo,Foo>()`) without a method-name allowlist. Nested generics
/// are skipped here because they are captured as `children` of the enclosing
/// usage — recording them again would double-count.
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
    // A generic_name whose parent is a type_argument_list is itself nested
    // inside another generic — its args ride along under the outer usage.
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
    let arguments = crate::base::extract_type_arguments(base, arg_list, decompose_csharp_type_arg);
    base.record_type_arguments(identifier, arguments);
}

/// `TypeArgDecomposer` for C#: maps a child of a `type_argument_list` to its
/// applied argument. Skips punctuation (`<`, `,`, `>`); for a nested
/// `generic_name` returns the base name plus its inner `type_argument_list` to
/// recurse into; for every other type node returns its source text as a leaf.
fn decompose_csharp_type_arg<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None;
    }
    match node.kind() {
        "generic_name" => {
            let name = direct_identifier(base, node)
                .map(|(_, name)| name)
                .unwrap_or_else(|| base.get_node_text(&node));
            Some((name, type_argument_list_child(node)))
        }
        _ => Some((base.get_node_text(&node), None)),
    }
}

/// First `type_argument_list` child of a `generic_name` (its `<...>`), if any.
fn type_argument_list_child(generic_name: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = generic_name.walk();
    generic_name
        .children(&mut cursor)
        .find(|child| child.kind() == "type_argument_list")
}

fn terminal_type_identifier<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(Node<'a>, String)> {
    match node.kind() {
        "identifier" => Some((node, base.get_node_text(&node))),
        "generic_name" => direct_identifier(base, node).or_else(|| {
            node.child_by_field_name("name")
                .and_then(|name_node| terminal_type_identifier(base, name_node))
        }),
        "qualified_name" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                terminal_type_identifier(base, name_node)
            } else {
                rightmost_identifier(base, node)
            }
        }
        _ => rightmost_identifier(base, node),
    }
}

fn direct_identifier<'a>(base: &BaseExtractor, node: Node<'a>) -> Option<(Node<'a>, String)> {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            return Some((child, base.get_node_text(&child)));
        }
    }

    None
}

fn rightmost_identifier<'a>(base: &BaseExtractor, node: Node<'a>) -> Option<(Node<'a>, String)> {
    let mut cursor = node.walk();
    let mut found = None;

    for child in node.children(&mut cursor) {
        if let Some(identifier) = terminal_type_identifier(base, child) {
            found = Some(identifier);
        }
    }

    found
}

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
            | "compilation_unit" => {
                return false;
            }
            _ => {}
        }

        current = parent;
    }

    false
}

// ============================================================================
// variable_ref emission — LOCKED SEMANTIC CONTRACT (reference implementation)
// ============================================================================
//
// Miller's dead-code candidate reader decides name-liveness by whether any
// `identifiers` row has `name = S.name` OUTSIDE S's own definition. A bare read
// (`return VisibilityUnknown;`) or a static-access receiver (`GraphTraversal` in
// `GraphTraversal.Reach()`) previously emitted NO identifier, so live symbols were
// falsely flagged dead. `variable_ref` closes that gap. Every rollout task copies
// this arm's structure verbatim, so the six rules below are load-bearing.
//
// Emit `IdentifierKind::VariableRef` for a name node N when ALL hold:
//   1. Read in value or receiver position — N is used as an expression / operand /
//      argument / initializer / return value / collection element, OR the object
//      (receiver) of a member access (`X` in `X.Y` / `X.Y()`), OR a member-reference
//      LHS in an initializer/named-argument context (`Bar` in `new Foo { Bar = 5 }`,
//      `Bar` in `[Foo(Bar = 1)]`).
//   2. Not already emitted by another arm — not a call callee (Call), not the
//      accessed `.name` of a member access (MemberAccess/Call), not a type usage
//      (TypeUsage). This arm is the *complement*; match ordering (TypeUsage guard
//      first) guarantees a type identifier never reaches here.
//   3. Not a declaration name — not the defining identifier of a type / method /
//      constructor / property / field / enum-member / parameter / local /
//      local-function / label / using-alias LHS.
//   4. Not a write-only target — not the LHS of a PLAIN assignment (`x = 5`). A
//      COMPOUND assignment (`x += 1`) IS a read. An initializer/named-argument
//      member LHS is a read (rule 1). Liveness counts *reads*.
//   5. Not a keyword/builtin — reuse `is_csharp_builtin_type`; `this`/`base`/
//      `true`/`false`/`null` are distinct grammar nodes and never appear as
//      `identifier`, so they are structurally excluded.
//   6. `containing_symbol_id` is set via `find_containing_symbol_id`, exactly as
//      the sibling arms do.
//
// NON-GOALS: `variable_ref` is NOT made resolvable (`ReferenceKind::from_identifier_kind`
// stays call/type_usage/member_access — new rows are consumed by name-match only);
// the `identifiers` schema / `IdentifierKind` enum / `sqlite_schema_version` are
// unchanged (`variable_ref` is already a valid kind). Serialized string: `variable_ref`.

/// Rule 1/4 predicate: is this bare `identifier` a value read or a member-access
/// receiver (the complement of the Call/MemberAccess/TypeUsage arms)? Mirrors the
/// structure of `is_csharp_type_usage_identifier` but for value/receiver reads,
/// reusing `is_csharp_declaration_name` for exclusions. Node kinds and field names
/// were verified against the vendored tree-sitter-c-sharp 0.23.5 grammar.
fn is_csharp_value_read_identifier(node: Node) -> bool {
    // Rule 3: never a type/method/property/namespace/type-parameter definition name.
    if is_csharp_declaration_name(node) {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };

    // Rule 2: a `type`/`returns` field identifier is a type usage, not a value. The
    // TypeUsage arm owns the type positions its predicate recognizes, but its walk
    // misses a bare method `returns` type; excluding both fields here keeps a type
    // from being mislabeled as a read (a return type is not "value position").
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

        // Rule 3: definition names the shared declaration guard does not cover. Their
        // NON-name identifier children (a declarator initializer value, the foreach
        // collection) fall through below as reads.
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
        // foreach loop variable (`left`) is a definition; the `right` collection reads.
        "foreach_statement" => {
            parent.child_by_field_name("left").map(|l| l.id()) != Some(node.id())
        }

        // Rule 3: labels and using/extern aliases / namespace names are not variables.
        "labeled_statement"
        | "goto_statement"
        | "using_directive"
        | "extern_alias_directive"
        | "namespace_declaration"
        | "file_scoped_namespace_declaration" => false,

        // Rule 1: an argument value is a read; the `name:` label of a named argument
        // (`foo(bar: 5)`) is a parameter name, not a read.
        "argument" => !is_name_field,
        // Rule 1: attribute named-arg member / positional value is a read; the
        // attribute's own `name` is a type usage.
        "attribute" => false,
        "attribute_argument" => true,

        // Rule 4: assignment RHS is a read; the LHS is a read only for a COMPOUND
        // operator or an object/collection-initializer member. A plain `x = 5` LHS is
        // write-only.
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

        // Type positions (also removed by the TypeUsage arm via match ordering; kept
        // explicit so the predicate is correct in isolation).
        "qualified_name" | "generic_name" | "type_argument_list" | "array_type"
        | "nullable_type" | "pointer_type" | "tuple_type" | "base_list" => false,

        // Every other expression/statement value slot — return / binary / conditional /
        // interpolation / initializer element / switch value / element access / cast /
        // constant pattern / … — is a read.
        _ => true,
    }
}

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

fn contains_node(parent: Node, child: Node) -> bool {
    child.start_byte() >= parent.start_byte() && child.end_byte() <= parent.end_byte()
}

fn is_csharp_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "byte"
            | "sbyte"
            | "char"
            | "decimal"
            | "double"
            | "float"
            | "int"
            | "uint"
            | "nint"
            | "nuint"
            | "long"
            | "ulong"
            | "short"
            | "ushort"
            | "object"
            | "string"
            | "void"
            | "dynamic"
    )
}

fn find_containing_symbol_id(
    base: &BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) -> Option<String> {
    base.find_containing_symbol_from_map(&node, symbol_map)
        .map(|s| s.id.clone())
}

// ============================================================================
// String-literal call-argument capture helpers (Miller bridge Phase 3)
// ============================================================================

/// Capture string-literal arguments of a C# `invocation_expression` as `Literal`
/// records. Config-free: `carrier` is the method name (generics stripped); the
/// URL/SQL classification and the carrier gate run later in the artifact language-policy pass.
///
/// C# wraps each call argument in an `argument` node, so the value expression is
/// the argument's last named child (after any `name:` for a named argument).
/// `arg_position` is counted over the full argument list.
fn record_csharp_call_arg_literals(
    base: &mut BaseExtractor,
    node: Node,
    symbol_map: &HashMap<String, &Symbol>,
) {
    let Some(function) = node.child_by_field_name("function") else {
        return;
    };
    let Some(args) = node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = csharp_carrier(base, function);
    let containing_symbol_id = find_containing_symbol_id(base, node, symbol_map);

    let mut cursor = args.walk();
    for (pos, arg) in args.named_children(&mut cursor).enumerate() {
        let value = if arg.kind() == "argument" {
            let mut vc = arg.walk();
            arg.named_children(&mut vc).last()
        } else {
            Some(arg)
        };
        if let Some(value) = value
            && let Some(text) = base.decode_string_literal(&value)
        {
            base.record_literal(
                &value,
                text,
                carrier.clone(),
                pos as u32,
                containing_symbol_id.clone(),
            );
        }
    }
}

/// Derive a C# call's carrier: the method name with generic type arguments
/// stripped (`conn.Query<User>` -> `Query`, `Foo<T>` -> `Foo`, `Execute` ->
/// `Execute`). The receiver is intentionally dropped — Dapper/ADO carriers are
/// matched by method name, and the receiver is usually a local variable.
fn csharp_carrier(base: &BaseExtractor, function: Node) -> Option<String> {
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
