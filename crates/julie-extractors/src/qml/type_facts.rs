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
