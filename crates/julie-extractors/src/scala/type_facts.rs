use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

pub(super) const SCALA_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['['],
};

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

pub(super) fn record_initializer_type(base: &mut BaseExtractor, symbol_id: &str, value: Node) {
    match value.kind() {
        "instance_expression" => {
            if let Some(type_node) = instance_type_node(value) {
                record_type_node(base, symbol_id, type_node, true);
            }
        }
        "call_expression" => {
            if let Some(class_name) = same_file_constructor_class(base, value) {
                base.record_declared_type_fact(
                    symbol_id,
                    &class_name,
                    &SCALA_TYPE_NAME_RULES,
                    true,
                );
            }
        }
        _ => {}
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
        &SCALA_TYPE_NAME_RULES,
        is_inferred,
    );
}

fn base_type_name_node(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        match node.kind() {
            "type_identifier" | "identifier" => return Some(node),
            "generic_type" => {
                node = node.child_by_field_name("type")?;
            }
            "stable_type_identifier" => {
                return last_named_type_identifier(node);
            }
            _ => return None,
        }
    }
}

fn last_named_type_identifier(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| matches!(child.kind(), "type_identifier" | "identifier"))
        .last()
}

fn instance_type_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|child| {
        matches!(
            child.kind(),
            "type_identifier" | "generic_type" | "stable_type_identifier"
        )
    })
}

fn same_file_constructor_class(base: &BaseExtractor, value: Node) -> Option<String> {
    let callee = call_callee(value)?;
    if callee.kind() != "identifier" {
        return None;
    }
    let name = base.get_node_text(&callee);
    same_file_has_class(file_root(value), &name, base, 0).then_some(name)
}

fn call_callee(value: Node) -> Option<Node> {
    let mut cursor = value.walk();
    let callee = value.children(&mut cursor).find(|child| {
        matches!(
            child.kind(),
            "identifier" | "generic_function" | "field_expression"
        )
    })?;
    if callee.kind() == "generic_function" {
        callee.child_by_field_name("function")
    } else {
        Some(callee)
    }
}

fn file_root(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

fn same_file_has_class(node: Node, name: &str, base: &BaseExtractor, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.kind() == "class_definition"
        && node
            .child_by_field_name("name")
            .is_some_and(|name_node| base.get_node_text(&name_node) == name)
    {
        return true;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if same_file_has_class(child, name, base, child_depth) {
            return true;
        }
    }
    false
}
