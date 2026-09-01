use super::{PhpExtractor, namespaces::extract_variable_assignment, type_facts};
use crate::base::Symbol;
use tree_sitter::Node;

pub(super) fn extract_assignment(
    extractor: &mut PhpExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let value_node = assignment_value_node(node);
    let symbol = extract_variable_assignment(extractor, node, parent_id)?;
    if let Some(value_node) = value_node {
        type_facts::record_new_expression_type(extractor.get_base_mut(), &symbol.id, value_node);
    }
    Some(symbol)
}

fn assignment_value_node(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("right")
}
