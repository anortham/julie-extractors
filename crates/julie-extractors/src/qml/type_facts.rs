use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['<'],
};

pub(super) fn record_property_type(base: &mut BaseExtractor, symbol_id: &str, property_node: Node) {
    let Some(type_node) = property_node.child_by_field_name("type") else {
        return;
    };
    let declared = base.get_node_text(&type_node);
    if declared == "alias" {
        return;
    }
    let base_text = property_base_name(base, type_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_text,
        &declared,
        &TYPE_NAME_RULES,
        false,
    );
}

pub(super) fn record_new_expression_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value_node: Node,
) {
    crate::javascript::type_facts::record_new_expression_fact(
        base,
        symbol_id,
        value_node,
        &TYPE_NAME_RULES,
    );
}

fn property_base_name(base: &BaseExtractor, type_node: Node) -> String {
    if type_node.kind() == "ui_list_property_type"
        && let Some(name_node) = type_node.named_child(0)
    {
        return base.get_node_text(&name_node);
    }
    base.get_node_text(&type_node)
}

/// Record the type an annotation field (`type` on a parameter, `return_type`
/// on a function) states, when it names a single type.
pub(super) fn record_annotation_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    annotated_node: Node,
    field: &str,
) {
    let Some(annotation) = annotated_node.child_by_field_name(field) else {
        return;
    };
    let mut cursor = annotation.walk();
    let Some(type_node) = annotation.named_children(&mut cursor).last() else {
        return;
    };
    if !matches!(
        type_node.kind(),
        "type_identifier" | "nested_type_identifier" | "predefined_type" | "generic_type"
    ) {
        return;
    }
    let declared = base.get_node_text(&type_node);
    record_named_type(base, symbol_id, &declared);
}

/// Record a type the source names directly: an object's type or the component
/// an `id` names.
pub(super) fn record_named_type(base: &mut BaseExtractor, symbol_id: &str, type_name: &str) {
    base.record_declared_type_fact_with_declared(
        symbol_id,
        type_name,
        type_name,
        &TYPE_NAME_RULES,
        false,
    );
}
