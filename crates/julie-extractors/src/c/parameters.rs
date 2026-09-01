use crate::base::{Symbol, SymbolKind, SymbolOptions};
use crate::c::CExtractor;
use std::collections::HashMap;
use tree_sitter::Node;

use super::helpers;
use super::type_facts;

pub(super) fn extract_parameter_symbols(
    extractor: &mut CExtractor,
    function_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let Some(params_node) = parameter_list(function_node) else {
        return Vec::new();
    };
    let param_nodes: Vec<Node> = {
        let mut cursor = params_node.walk();
        params_node
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "parameter_declaration")
            .collect()
    };

    let mut symbols = Vec::new();
    for param_node in param_nodes {
        let Some(declarator) = param_node.child_by_field_name("declarator") else {
            continue;
        };
        let Some(name) = helpers::extract_variable_name(&extractor.base, declarator) else {
            continue;
        };
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
        type_facts::record_declared_from_declaration(
            &mut extractor.base,
            &symbol.id,
            param_node,
            declarator,
        );
        symbols.push(symbol);
    }
    symbols
}

fn parameter_list(function_node: Node) -> Option<Node> {
    let mut node = function_node.child_by_field_name("declarator")?;
    loop {
        match node.kind() {
            "function_declarator" => return node.child_by_field_name("parameters"),
            "pointer_declarator" | "parenthesized_declarator" | "array_declarator" => {
                node = node.child_by_field_name("declarator")?;
            }
            _ => return None,
        }
    }
}
