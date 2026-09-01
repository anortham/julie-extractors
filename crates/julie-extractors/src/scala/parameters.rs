use crate::base::{Symbol, SymbolKind, SymbolOptions};
use std::collections::HashMap;
use tree_sitter::Node;

use super::type_facts;

type Base = crate::base::BaseExtractor;

pub(super) fn extract_parameter_symbols(
    base: &mut Base,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let mut param_nodes = Vec::new();
    let mut cursor = callable_node.walk();
    for child in callable_node.children(&mut cursor) {
        if child.kind() != "parameters" {
            continue;
        }
        let mut param_cursor = child.walk();
        for param_node in child.named_children(&mut param_cursor) {
            if param_node.kind() == "parameter" {
                param_nodes.push(param_node);
            }
        }
    }

    let mut symbols = Vec::new();
    for param_node in param_nodes {
        let Some(name_node) = param_node.child_by_field_name("name") else {
            continue;
        };
        let name = base.get_node_text(&name_node);
        let signature = base.get_node_text(&param_node);
        let metadata = HashMap::from([(
            "role".to_string(),
            serde_json::Value::String("parameter".to_string()),
        )]);
        let symbol = base.create_symbol(
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
            type_facts::record_declared_type(base, &symbol.id, type_node);
        }
        symbols.push(symbol);
    }
    symbols
}
