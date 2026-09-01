//! Parameter symbol extraction for Kotlin functions and secondary constructors.

use super::helpers;
use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) fn extract_parameter_symbols(
    base: &mut BaseExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let params_node = {
        let mut cursor = callable_node.walk();
        callable_node
            .children(&mut cursor)
            .find(|child| child.kind() == "function_value_parameters")
    };
    let Some(params_node) = params_node else {
        return Vec::new();
    };
    let param_nodes: Vec<Node> = {
        let mut cursor = params_node.walk();
        params_node
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "parameter")
            .collect()
    };

    let mut symbols = Vec::new();
    for param_node in param_nodes {
        let name_node = {
            let mut cursor = param_node.walk();
            param_node
                .children(&mut cursor)
                .find(|child| child.kind() == "identifier" || child.kind() == "simple_identifier")
        };
        let Some(name_node) = name_node else {
            continue;
        };
        let raw_name = base.get_node_text(&name_node);
        let name = helpers::strip_backticks(&raw_name).to_string();
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
        if let Some(type_node) = type_facts::declared_type_child(param_node) {
            type_facts::record_declared_type(base, &symbol.id, type_node);
        }
        symbols.push(symbol);
    }
    symbols
}
