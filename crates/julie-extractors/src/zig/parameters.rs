use crate::base::{Symbol, SymbolKind, SymbolOptions};
use std::collections::HashMap;
use tree_sitter::Node;

use super::type_facts;
use super::ZigExtractor;

pub(super) fn extract_parameter_symbols(
    extractor: &mut ZigExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let mut cursor = callable_node.walk();
    let Some(params_node) = callable_node
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "parameters" | "parameter_list"))
    else {
        return Vec::new();
    };

    let param_nodes: Vec<Node> = {
        let mut param_cursor = params_node.walk();
        params_node
            .named_children(&mut param_cursor)
            .filter(|child| child.kind() == "parameter")
            .collect()
    };

    let mut symbols = Vec::new();
    for param_node in param_nodes {
        let Some(name_node) = param_node
            .child_by_field_name("name")
            .or_else(|| extractor.base.find_child_by_type(&param_node, "identifier"))
        else {
            continue;
        };
        let name = extractor.base.get_node_text(&name_node);
        let signature = extractor.base.get_node_text(&param_node);
        let metadata = HashMap::from([(
            "role".to_string(),
            serde_json::Value::String("parameter".to_string()),
        )]);
        let symbol = extractor.base.create_symbol(
            &param_node,
            name,
            SymbolKind::Variable,
            SymbolOptions {
                signature: Some(signature),
                parent_id: Some(callable_id.to_string()),
                metadata: Some(metadata),
                ..Default::default()
            },
        );
        if let Some(type_node) = param_node.child_by_field_name("type") {
            type_facts::record_declared_type(&mut extractor.base, &symbol.id, type_node);
        }
        symbols.push(symbol);
    }
    symbols
}
