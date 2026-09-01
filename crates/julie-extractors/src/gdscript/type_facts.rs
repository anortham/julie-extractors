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
    same_file_class_names: &HashSet<String>,
) {
    if let Some(type_node) = statement.child_by_field_name("type")
        && type_node.kind() == "type"
    {
        record_declared_type_node(base, symbol_id, type_node);
        return;
    }
    if let Some(value) = statement.child_by_field_name("value") {
        record_same_file_new_fact(base, symbol_id, value, same_file_class_names);
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

/// Record `Foo.new(...)` when `Foo` is a bare identifier naming a class
/// declared in the same file.
pub(super) fn record_same_file_new_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value_node: Node,
    same_file_class_names: &HashSet<String>,
) {
    let Some(class_name) = constructor_type_name(base, value_node) else {
        return;
    };
    if !same_file_class_names.contains(&class_name) {
        return;
    }
    base.record_declared_type_fact(symbol_id, &class_name, &TYPE_NAME_RULES, true);
}

fn constructor_type_name(base: &BaseExtractor, value_node: Node) -> Option<String> {
    if value_node.kind() != "attribute" {
        return None;
    }
    let mut cursor = value_node.walk();
    let children: Vec<Node> = value_node.named_children(&mut cursor).collect();
    let [receiver, call] = children.as_slice() else {
        return None;
    };
    if receiver.kind() != "identifier" || call.kind() != "attribute_call" {
        return None;
    }
    let mut call_cursor = call.walk();
    let constructs = call.children(&mut call_cursor).any(|call_child| {
        call_child.kind() == "identifier" && base.get_node_text(&call_child) == "new"
    });
    constructs.then(|| base.get_node_text(receiver))
}

pub(super) fn collect_class_names(base: &BaseExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_class_names_into(base, root, 0, &mut names);
    names
}

fn collect_class_names_into(
    base: &BaseExtractor,
    node: Node,
    depth: u32,
    names: &mut HashSet<String>,
) {
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
        collect_class_names_into(base, child, child_depth, names);
    }
}
