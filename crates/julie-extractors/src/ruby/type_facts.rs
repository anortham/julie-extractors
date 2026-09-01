use super::helpers::extract_name_from_node;
use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &[],
};

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

pub(super) fn self_receiver_type(base: &BaseExtractor, call_node: Node) -> Option<String> {
    let receiver = call_node.child_by_field_name("receiver")?;
    if receiver.kind() != "self" {
        return None;
    }
    enclosing_type_name(base, call_node)
}

fn constructor_type_name(base: &BaseExtractor, value_node: Node) -> Option<String> {
    if value_node.kind() != "call" {
        return None;
    }
    let method = value_node.child_by_field_name("method")?;
    if base.get_node_text(&method) != "new" {
        return None;
    }
    let receiver = value_node.child_by_field_name("receiver")?;
    if receiver.kind() != "constant" {
        return None;
    }
    Some(base.get_node_text(&receiver))
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
    if node.kind() == "class"
        && let Some(name) = extract_name_from_node(node, |n| base.get_node_text(n), "name")
    {
        names.insert(name);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_class_names(base, child, child_depth, names);
    }
}

fn enclosing_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(candidate.kind(), "class" | "module") {
            return extract_name_from_node(candidate, |n| base.get_node_text(n), "name");
        }
        current = candidate.parent();
    }
    None
}
