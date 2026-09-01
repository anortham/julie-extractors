use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['['],
};

pub(super) fn record_statement_type_facts(
    base: &mut BaseExtractor,
    symbol_id: &str,
    statement: Node,
) {
    if let Some(type_node) = statement.child_by_field_name("type")
        && type_node.kind() == "type"
    {
        record_declared_type_node(base, symbol_id, type_node);
        return;
    }
    if let Some(value) = statement.child_by_field_name("value") {
        record_same_file_new_fact(base, symbol_id, value);
    }
}

pub(super) fn record_declared_type_node(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
) {
    if type_node.kind() != "type" {
        return;
    }
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact(symbol_id, &declared, &TYPE_NAME_RULES, false);
}

pub(super) fn record_same_file_new_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value_node: Node,
) {
    let Some(class_name) = constructor_type_name(base, value_node) else {
        return;
    };
    if !same_file_class_names(base, value_node).contains(&class_name) {
        return;
    }
    base.record_declared_type_fact(symbol_id, &class_name, &TYPE_NAME_RULES, true);
}

fn constructor_type_name(base: &BaseExtractor, value_node: Node) -> Option<String> {
    let attribute = match value_node.kind() {
        "attribute" => value_node,
        _ => return None,
    };
    let mut cursor = attribute.walk();
    let mut class_name = None;
    let mut constructs = false;
    for child in attribute.children(&mut cursor) {
        if child.kind() == "identifier" && class_name.is_none() {
            class_name = Some(base.get_node_text(&child));
        }
        if child.kind() == "attribute_call" {
            let mut call_cursor = child.walk();
            constructs = child.children(&mut call_cursor).any(|call_child| {
                call_child.kind() == "identifier" && base.get_node_text(&call_child) == "new"
            });
        }
    }
    constructs.then_some(class_name).flatten()
}

fn same_file_class_names(base: &BaseExtractor, node: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_class_names(base, file_root(node), 0, &mut names);
    names
}

fn file_root(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

fn collect_class_names(base: &BaseExtractor, node: Node, depth: u32, names: &mut HashSet<String>) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if matches!(node.kind(), "class_name_statement" | "class_definition")
        && let Some(name_node) = node.child_by_field_name("name")
    {
        names.insert(base.get_node_text(&name_node));
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_class_names(base, child, child_depth, names);
    }
}
