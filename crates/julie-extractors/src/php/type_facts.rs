use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use tree_sitter::Node;

pub(super) const PHP_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["?", "\\"],
    generic_open: &[],
};

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

pub(super) fn record_new_expression_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value_node: Node,
) {
    if value_node.kind() != "object_creation_expression" {
        return;
    }
    let Some(type_node) = object_creation_type(value_node) else {
        return;
    };
    record_type_node(base, symbol_id, type_node, true);
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    if !names_single_base_type(type_node) {
        return;
    }
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact(symbol_id, &declared, &PHP_TYPE_NAME_RULES, is_inferred);
}

fn object_creation_type(node: Node<'_>) -> Option<Node<'_>> {
    if node
        .named_child(0)
        .is_some_and(|child| child.kind() == "anonymous_class")
    {
        return None;
    }
    node.child_by_field_name("type").or_else(|| {
        let child = node.named_child(0)?;
        match child.kind() {
            "name" | "qualified_name" | "named_type" | "primitive_type" => Some(child),
            _ => None,
        }
    })
}

fn names_single_base_type(node: Node) -> bool {
    match node.kind() {
        "name" | "qualified_name" | "named_type" | "primitive_type" => true,
        "optional_type" => node.named_child(0).is_some_and(names_single_base_type),
        "union_type" | "intersection_type" | "disjunctive_normal_form_type" => false,
        _ => false,
    }
}
