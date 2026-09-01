//! Declared-type fact recording for Swift.

use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Node;

pub(super) const SWIFT_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?", "!"],
    reference_prefixes: &["inout"],
    generic_open: &['<'],
};

pub(super) fn collect_type_names(base: &BaseExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_type_names_into(base, root, 0, &mut names);
    names
}

fn collect_type_names_into(
    base: &BaseExtractor,
    node: Node,
    depth: u32,
    names: &mut HashSet<String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "class_declaration" | "actor_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                insert_type_name(base, name_node, names);
            }
        }
        "struct_declaration" | "enum_declaration" => {
            if let Some(name_node) = node
                .children(&mut node.walk())
                .find(|child| child.kind() == "type_identifier")
            {
                insert_type_name(base, name_node, names);
            }
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_type_names_into(base, child, child_depth, names);
    }
}

fn insert_type_name(base: &BaseExtractor, name_node: Node, names: &mut HashSet<String>) {
    let text = base.get_node_text(&name_node);
    let resolved = crate::base::types::strip_type_decorations(&text, &SWIFT_TYPE_NAME_RULES);
    if !resolved.is_empty() {
        names.insert(resolved);
    }
}

/// Record a syntactically stated type for a symbol (`is_inferred=false`).
pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

pub(super) fn record_declared_type_text(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    declared_text: &str,
) {
    let Some(name_node) = base_type_name_node(type_node) else {
        return;
    };
    let base_name = base.get_node_text(&name_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        declared_text,
        &SWIFT_TYPE_NAME_RULES,
        false,
    );
}

/// Record `Foo(...)` when `Foo` names a same-file class-like symbol.
pub(super) fn record_same_file_constructor(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    same_file_type_names: &HashSet<String>,
) {
    if value.kind() != "call_expression" {
        return;
    }
    let mut cursor = value.walk();
    let Some(callee) = value.children(&mut cursor).next() else {
        return;
    };
    if callee.kind() != "simple_identifier" {
        return;
    }
    let name = base.get_node_text(&callee);
    if same_file_type_names.contains(&name) {
        base.record_declared_type_fact(symbol_id, &name, &SWIFT_TYPE_NAME_RULES, true);
    }
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    let Some(name_node) = base_type_name_node(type_node) else {
        return;
    };
    let base_name = base.get_node_text(&name_node);
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &SWIFT_TYPE_NAME_RULES,
        is_inferred,
    );
}

fn base_type_name_node(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        match node.kind() {
            "type_identifier" | "simple_identifier" | "primitive_type" => return Some(node),
            "optional_type" => {
                node = node.child_by_field_name("wrapped")?;
            }
            "type_annotation" => {
                node = node
                    .child_by_field_name("name")
                    .or_else(|| named_type_field(node))?;
            }
            "user_type" => {
                let identifiers: Vec<Node> = {
                    let mut cursor = node.walk();
                    node.named_children(&mut cursor)
                        .filter(|child| child.kind() == "type_identifier")
                        .collect()
                };
                return match identifiers.as_slice() {
                    [only] => Some(*only),
                    _ => None,
                };
            }
            "array_type"
            | "dictionary_type"
            | "tuple_type"
            | "function_type"
            | "protocol_composition_type" => return None,
            _ => return None,
        }
    }
}

fn named_type_field(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children_by_field_name("type", &mut cursor)
        .find(|child| child.is_named())
}

pub(super) fn property_type_node(node: Node) -> Option<Node> {
    node.children(&mut node.walk())
        .find(|child| child.kind() == "type_annotation")
        .and_then(|annotation| {
            annotation
                .child_by_field_name("name")
                .or_else(|| named_type_field(annotation))
        })
}

pub(super) fn property_value_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children_by_field_name("value", &mut cursor)
        .find(|child| child.is_named())
}

pub(super) fn nearest_callable_ancestor(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "function_declaration"
            | "init_declaration"
            | "deinit_declaration"
            | "protocol_function_declaration" => return true,
            "class_declaration"
            | "struct_declaration"
            | "enum_declaration"
            | "extension_declaration"
            | "protocol_declaration"
            | "actor_declaration" => return false,
            _ => current = parent.parent(),
        }
    }
    false
}
