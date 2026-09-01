use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) fn extract_parameter(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = base.get_node_text(&name_node);
    let signature = base.get_node_text(&node);
    let mut metadata = HashMap::new();
    metadata.insert("role".to_string(), serde_json::json!("parameter"));
    let symbol = base.create_symbol(
        &node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id,
            metadata: Some(metadata),
            ..Default::default()
        },
    );
    if let Some(type_node) = type_facts::declared_type_node(node) {
        type_facts::record_declared_type(
            base,
            &symbol.id,
            type_node,
            type_facts::declarator_rank_node(node),
        );
    }
    Some(symbol)
}
