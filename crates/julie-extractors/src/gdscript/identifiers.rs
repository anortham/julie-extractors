//! Identifier extraction for GDScript (function calls, member access, type annotations, etc.)

use crate::base::{
    BaseExtractor, ContainingSymbolIndex, Identifier, IdentifierKind, Symbol,
    extract_type_arguments,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

/// Extract all identifier usages (function calls, member access, etc.)
pub(super) fn extract_identifiers(
    base: &mut BaseExtractor,
    tree: &tree_sitter::Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let containing_symbols = base.containing_symbol_index(symbols);
    walk_tree_for_identifiers(base, tree.root_node(), &containing_symbols, 0);
    base.identifiers.clone()
}

/// Recursively walk tree extracting identifiers from each node
fn walk_tree_for_identifiers(
    base: &mut BaseExtractor,
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    extract_identifier_from_node(base, node, containing_symbols);

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_identifiers(base, child, containing_symbols, child_depth);
    }
}

/// Extract identifier from a single node based on its kind
fn extract_identifier_from_node(
    base: &mut BaseExtractor,
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    match node.kind() {
        "call" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    if let Some(parent) = node.parent()
                        && parent.kind() == "attribute"
                    {
                        continue;
                    }

                    let name = base.get_node_text(&child);
                    let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                    base.create_identifier(
                        &child,
                        name,
                        IdentifierKind::Call,
                        containing_symbol_id,
                    );
                    break;
                }

                if child.kind() == "attribute"
                    && let Some(name_node) = attribute_call_name_node(child)
                        .or_else(|| rightmost_identifier_descendant(child))
                {
                    if let Some(parent) = node.parent()
                        && parent.kind() == "attribute"
                    {
                        continue;
                    }

                    let name = base.get_node_text(&name_node);
                    let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                    let receiver_type = call_receiver_type(base, child);
                    base.create_identifier_with_receiver_type(
                        &name_node,
                        name,
                        IdentifierKind::Call,
                        containing_symbol_id,
                        receiver_type,
                    );
                    break;
                }
            }
            // Phase 3b: capture string-literal call-arguments config-free; the
            // carrier classification + bloat gate run later in the artifact language-policy pass.
            record_gdscript_call_arg_literals(base, node, containing_symbols);
        }

        // `recv.method(args)` parses as `attribute { recv, attribute_call }`, so
        // the call args live on the `attribute_call` node, not a `call` node.
        "attribute_call" => {
            record_gdscript_attribute_call_arg_literals(base, node, containing_symbols);
        }

        "get_node" => {
            let name = "get_node".to_string();
            let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
            base.create_identifier(&node, name, IdentifierKind::Call, containing_symbol_id);
        }

        "attribute" => {
            if let Some(name_node) = attribute_call_name_node(node) {
                let name = base.get_node_text(&name_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                let receiver_type = call_receiver_type(base, node);
                base.create_identifier_with_receiver_type(
                    &name_node,
                    name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                    receiver_type,
                );
                return;
            }

            if let Some(last_child) = rightmost_identifier_descendant(node) {
                let name = base.get_node_text(&last_child);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(
                    &last_child,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }

        "subscript" => {
            if let Some(parent) = node.parent()
                && parent.kind() == "call"
            {
                return;
            }

            if let Some(index_node) = node.child_by_field_name("index")
                && index_node.kind() == "identifier"
            {
                let name = base.get_node_text(&index_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(
                    &index_node,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }

        "type" => {
            // Collect children once to avoid cursor borrow conflicts.
            let children: Vec<_> = {
                let mut cursor = node.walk();
                node.children(&mut cursor).collect()
            };
            if let Some(id_child) = children.iter().find(|c| c.kind() == "identifier") {
                // Plain type reference: `var x: Foo`
                let name = base.get_node_text(id_child);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(
                    id_child,
                    name,
                    IdentifierKind::TypeUsage,
                    containing_symbol_id,
                );
            } else if let Some(subscript_child) = children.iter().find(|c| c.kind() == "subscript")
            {
                // Generic type: `var x: Array[String]`, `Dictionary[String, int]`, etc.
                record_gdscript_subscript_as_type(base, node, *subscript_child, containing_symbols);
            }
        }

        // `variable_ref` complement arm (locked contract in csharp/identifiers.rs):
        // a bare `identifier` used as a value or as the receiver of an attribute
        // access — the reads the Call/MemberAccess/TypeUsage arms above do not own.
        // The other arms match on PARENT node kinds (`call`, `attribute`, `type`,
        // `subscript`), so this arm fires once per identifier node and the predicate
        // excludes every position those arms own (no duplicate rows). Declaration
        // names in tree-sitter-gdscript 6.1.0 are `name` nodes (a distinct kind), so
        // most of rule 3 is satisfied structurally; parameters and enum members are
        // the exceptions handled in the predicate.
        "identifier" if is_gdscript_value_read_identifier(node) => {
            let name = base.get_node_text(&node);
            // Rule 5: `self`/`super` parse as plain `identifier` nodes in receiver
            // and value positions; they are keywords, not user variables.
            if !is_gdscript_keyword_identifier(&name) {
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
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

/// Rule 5 filter: GDScript keywords that the grammar tokenizes as ordinary
/// `identifier` nodes (`true`/`false`/`null` are distinct node kinds and never
/// reach the identifier arm; `self`/`super` do). `_` is the match-pattern
/// wildcard / discard convention, never a user symbol read.
fn is_gdscript_keyword_identifier(name: &str) -> bool {
    matches!(name, "self" | "super" | "_")
}

/// Rule 2 helper: is this identifier inside a TYPE annotation? Type positions are
/// owned by the TypeUsage arm (`type` nodes) and by
/// `record_gdscript_subscript_as_type` (generic annotations such as
/// `Array[String]`, including nested generics and `Outer.Inner` attribute bases),
/// so the read arm must never fire there. Climbs through the node shapes a type
/// annotation can nest (`subscript`, `subscript_arguments`, `attribute`) looking
/// for a `type` ancestor.
fn is_gdscript_type_position(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        match parent.kind() {
            "type" => return true,
            "subscript" | "subscript_arguments" | "attribute" => current = parent,
            _ => return false,
        }
    }
    false
}

/// Rule 1/4 predicate: is this bare `identifier` a value read or an attribute
/// (member-access) receiver — the complement of the Call/MemberAccess/TypeUsage
/// arms? Mirrors `is_csharp_value_read_identifier`; node kinds and field names
/// were verified empirically against tree-sitter-gdscript 6.1.0 (probe evidence
/// in the Task 6 report): declaration names are `name` nodes, plain assignment is
/// `assignment` (left/right fields) while compound assignment is a distinct
/// `augmented_assignment` kind, and `a.b.c()` parses as a flat
/// `attribute { a, b, attribute_call(c) }`.
fn is_gdscript_value_read_identifier(node: Node) -> bool {
    // Rule 2: type annotations belong to the TypeUsage arm.
    if is_gdscript_type_position(node) {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };

    match parent.kind() {
        // Rule 2: call callees are owned by the Call arm (`call` for bare calls,
        // `attribute_call` for `recv.method()` method names).
        "call" | "attribute_call" => false,
        // Rule 1/2: only the FIRST child of a flat `attribute` chain is the
        // receiver (a read). The rightmost name is owned by the MemberAccess/Call
        // arms; interior names of `a.b.c` are member accesses, not bare reads.
        "attribute" => {
            let mut cursor = parent.walk();
            parent
                .children(&mut cursor)
                .find(|c| c.is_named())
                .map(|first| first.id() == node.id())
                .unwrap_or(false)
        }
        // Rule 3: annotation names (`@export`, `@onready`) are not variables.
        "annotation" => false,
        // Rule 3: parameter names. A bare parameter is a direct `parameters` child;
        // typed parameters own only their name as a direct identifier child (the
        // type lives under a `type` node). A default parameter's `value` field is
        // an initializer expression — a read.
        "parameters" | "typed_parameter" => false,
        "default_parameter" | "typed_default_parameter" => {
            parent.child_by_field_name("value").map(|v| v.id()) == Some(node.id())
        }
        // Rule 3: enum member declaration names (`enumerator` left); a member's
        // explicit value expression (`FLAG_B = flag_seed`) reads.
        "enumerator" => parent.child_by_field_name("left").map(|l| l.id()) != Some(node.id()),
        // Rule 4: plain-assignment LHS is write-only; the RHS reads. Compound
        // assignment is the distinct `augmented_assignment` kind — both of its
        // sides read, so it falls through to the default arm.
        "assignment" => parent.child_by_field_name("left").map(|l| l.id()) != Some(node.id()),
        // Rule 3: the `for` loop variable (`left`) is a definition; the iterated
        // collection (`right`) reads.
        "for_statement" => parent.child_by_field_name("left").map(|l| l.id()) != Some(node.id()),
        // Every other expression/statement value slot — return / binary operand /
        // condition / argument / array or dictionary element / match subject or
        // pattern / augmented-assignment side / subscript base or index / … — is
        // a read.
        _ => true,
    }
}

/// Find the ID of the symbol that contains this node
fn find_containing_symbol_id(
    node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) -> Option<String> {
    containing_symbols.find(node).map(|s| s.id.clone())
}

/// Record a GDScript generic type annotation (`Array[String]`, `Dictionary[String,int]`).
///
/// Called from the `"type"` arm when the type node's child is a `subscript`
/// (e.g. `Array[String]`). Extracts the base type name from the subscript's
/// primary-expression child (an `identifier`), creates a TypeUsage identifier
/// for it, and records the ordered type arguments from `subscript_arguments`.
fn record_gdscript_subscript_as_type(
    base: &mut BaseExtractor,
    type_node: Node,
    subscript: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    // The base type name is the subscript's primary_expression child
    // (an identifier or attribute — not the subscript_arguments field).
    let mut cursor = subscript.walk();
    let Some(base_name_node) = subscript
        .named_children(&mut cursor)
        .find(|c| c.kind() == "identifier" || c.kind() == "attribute")
    else {
        return;
    };
    let name = base.get_node_text(&base_name_node);
    let containing_symbol_id = find_containing_symbol_id(type_node, containing_symbols);
    let identifier = base.create_identifier(
        &base_name_node,
        name,
        IdentifierKind::TypeUsage,
        containing_symbol_id,
    );
    // `subscript_arguments` is the `arguments` named field of the subscript node.
    let Some(arg_list) = subscript.child_by_field_name("arguments") else {
        return;
    };
    let arguments = extract_type_arguments(base, arg_list, decompose_gdscript_type_arg);
    base.record_type_arguments(&identifier, arguments);
}

/// `TypeArgDecomposer` for GDScript: maps a child of a `subscript_arguments`
/// node to its applied argument.
///
/// GDScript type arguments are `identifier` nodes for leaf types, or `subscript`
/// nodes for nested generics (`Array[Array[int]]`). Unnamed nodes (commas,
/// brackets) return `None` and are skipped.
fn decompose_gdscript_type_arg<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(String, Option<Node<'a>>)> {
    if !node.is_named() {
        return None; // skip commas and punctuation
    }
    match node.kind() {
        "identifier" => Some((base.get_node_text(&node), None)),
        "subscript" => {
            // Nested generic: `Array[Array[int]]` — extract base name + nested args.
            let mut cursor = node.walk();
            let base_node = node
                .named_children(&mut cursor)
                .find(|c| c.kind() == "identifier" || c.kind() == "attribute")?;
            let name = base.get_node_text(&base_node);
            let nested = node.child_by_field_name("arguments");
            Some((name, nested))
        }
        _ => Some((base.get_node_text(&node), None)),
    }
}

fn rightmost_identifier_descendant(node: Node) -> Option<Node> {
    rightmost_identifier_descendant_at_depth(node, 0)
}

fn rightmost_identifier_descendant_at_depth(node: Node, depth: u32) -> Option<Node> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    if node.kind() == "attribute_call" {
        return None;
    }

    if node.kind() == "identifier" {
        return Some(node);
    }

    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for child in children.into_iter().rev() {
        if let Some(found) = rightmost_identifier_descendant_at_depth(child, child_depth) {
            return Some(found);
        }
    }

    None
}

fn attribute_call_name_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();

    let attribute_call = children
        .iter()
        .find(|child| child.kind() == "attribute_call")?;

    let mut call_cursor = attribute_call.walk();
    attribute_call
        .children(&mut call_cursor)
        .find(|child| child.kind() == "identifier")
}

pub(super) fn call_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let attribute = if node.kind() == "attribute" {
        node
    } else if node.kind() == "call" {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find(|child| child.kind() == "attribute")?
    } else {
        return None;
    };
    attribute_receiver_type(base, attribute)
}

fn attribute_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    let receiver = node
        .children(&mut cursor)
        .find(|child| child.is_named() && child.kind() != "attribute_call")?;
    if receiver.kind() != "identifier" {
        return None;
    }
    match base.get_node_text(&receiver).as_str() {
        "self" => enclosing_type_name(base, node),
        "super" => declared_extends_name(base, node),
        _ => None,
    }
}

fn enclosing_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "class_definition" => {
                return candidate
                    .child_by_field_name("name")
                    .map(|name_node| base.get_node_text(&name_node));
            }
            "source" => {
                let mut cursor = candidate.walk();
                for child in candidate.children(&mut cursor) {
                    if child.kind() == "class_name_statement"
                        && let Some(name_node) = child.child_by_field_name("name")
                    {
                        return Some(base.get_node_text(&name_node));
                    }
                }
                return None;
            }
            _ => current = candidate.parent(),
        }
    }
    None
}

fn declared_extends_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "class_definition" => {
                return candidate
                    .child_by_field_name("extends")
                    .and_then(|extends| extends_type_name(base, extends));
            }
            "source" => {
                let mut cursor = candidate.walk();
                for child in candidate.children(&mut cursor) {
                    if child.kind() == "extends_statement" {
                        return extends_type_name(base, child);
                    }
                    if child.kind() == "class_name_statement"
                        && let Some(extends) = child.child_by_field_name("extends")
                    {
                        return extends_type_name(base, extends);
                    }
                }
                return None;
            }
            _ => current = candidate.parent(),
        }
    }
    None
}

fn extends_type_name(base: &BaseExtractor, extends_node: Node) -> Option<String> {
    let mut cursor = extends_node.walk();
    let type_node = extends_node
        .children(&mut cursor)
        .find(|child| child.kind() == "type")?;
    let mut type_cursor = type_node.walk();
    if let Some(identifier) = type_node
        .children(&mut type_cursor)
        .find(|child| child.kind() == "identifier")
    {
        return Some(base.get_node_text(&identifier));
    }
    Some(base.get_node_text(&type_node))
}

// ============================================================================
// String-literal call-argument capture (Miller bridge Phase 3b)
// ============================================================================

/// Capture string-literal arguments of a bare GDScript `call` (`load("res://…")`,
/// `query("SELECT …")`). Carrier is the plain `identifier` callee. `kind` stays
/// `Other`; the `src/` carrier gate sets the authoritative kind and drops
/// non-carrier literals. `arg_position` counts over the full argument list.
fn record_gdscript_call_arg_literals(
    base: &mut BaseExtractor,
    call_node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    let Some(args_node) = call_node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = gdscript_call_carrier(base, call_node);
    let containing_symbol_id = find_containing_symbol_id(call_node, containing_symbols);
    record_gdscript_string_args(base, args_node, carrier, containing_symbol_id);
}

/// Capture string-literal arguments of a GDScript `attribute_call`
/// (`http.request("https://…")`, `db.query("SELECT …")`). The method is the
/// `attribute_call`'s `identifier` child; the receiver is its previous named
/// sibling within the enclosing `attribute`, so the carrier is the
/// `receiver.method` join (`http.request`).
fn record_gdscript_attribute_call_arg_literals(
    base: &mut BaseExtractor,
    attr_call_node: Node,
    containing_symbols: &ContainingSymbolIndex<'_>,
) {
    let Some(args_node) = attr_call_node.child_by_field_name("arguments") else {
        return;
    };
    let carrier = gdscript_attribute_call_carrier(base, attr_call_node);
    let containing_symbol_id = find_containing_symbol_id(attr_call_node, containing_symbols);
    record_gdscript_string_args(base, args_node, carrier, containing_symbol_id);
}

/// Record every string-literal argument in `args_node` against `carrier`.
/// Shared by the bare-`call` and `attribute_call` arms.
fn record_gdscript_string_args(
    base: &mut BaseExtractor,
    args_node: Node,
    carrier: Option<String>,
    containing_symbol_id: Option<String>,
) {
    let mut cursor = args_node.walk();
    for (pos, arg) in args_node.named_children(&mut cursor).enumerate() {
        if let Some(text) = base.decode_string_literal(&arg) {
            base.record_literal(
                &arg,
                text,
                carrier.clone(),
                pos as u32,
                containing_symbol_id.clone(),
            );
        }
    }
}

/// Carrier for a bare `call`: the plain `identifier` callee (the named child
/// that is not the `arguments` node).
fn gdscript_call_carrier(base: &BaseExtractor, call_node: Node) -> Option<String> {
    let args_id = call_node.child_by_field_name("arguments").map(|n| n.id());
    let mut cursor = call_node.walk();
    let callee = call_node
        .named_children(&mut cursor)
        .find(|n| Some(n.id()) != args_id)?;
    let text = base.get_node_text(&callee);
    if text.is_empty() { None } else { Some(text) }
}

/// Carrier for an `attribute_call`: the `receiver.method` join, where the method
/// is the `attribute_call`'s `identifier` child and the receiver is its previous
/// named sibling within the enclosing `attribute`.
fn gdscript_attribute_call_carrier(base: &BaseExtractor, attr_call_node: Node) -> Option<String> {
    let mut cursor = attr_call_node.walk();
    let method = attr_call_node
        .named_children(&mut cursor)
        .find(|n| n.kind() == "identifier")
        .map(|n| base.get_node_text(&n));
    let receiver = attr_call_node
        .prev_named_sibling()
        .map(|n| base.get_node_text(&n));
    match (receiver, method) {
        (Some(r), Some(m)) => Some(format!("{r}.{m}")),
        (None, Some(m)) => Some(m),
        _ => None,
    }
}
