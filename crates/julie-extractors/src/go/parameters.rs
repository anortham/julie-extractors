//! Parameter symbol extraction for Go callables, including method receivers.

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use std::collections::HashMap;
use tree_sitter::Node;

/// Create one `variable` symbol per named parameter of `callable_node`, with
/// metadata `role: "parameter"` and `parent_id` = the callable's symbol id.
/// A method receiver is a parameter; an anonymous parameter gets no symbol.
pub(super) fn extract_parameter_symbols(
    base: &mut BaseExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for field in ["receiver", "parameters"] {
        if let Some(list_node) = callable_node.child_by_field_name(field)
            && list_node.kind() == "parameter_list"
        {
            extract_from_parameter_list(base, list_node, callable_id, &mut symbols);
        }
    }
    symbols
}

fn extract_from_parameter_list(
    base: &mut BaseExtractor,
    list_node: Node,
    callable_id: &str,
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = list_node.walk();
    let param_nodes: Vec<Node> = list_node.named_children(&mut cursor).collect();
    for param_node in param_nodes {
        if !matches!(
            param_node.kind(),
            "parameter_declaration" | "variadic_parameter_declaration"
        ) {
            continue;
        }
        let records_facts = param_node.kind() == "parameter_declaration";
        let signature = base.get_node_text(&param_node);
        let type_node = param_node.child_by_field_name("type");
        let mut name_cursor = param_node.walk();
        let name_nodes: Vec<Node> = param_node
            .children_by_field_name("name", &mut name_cursor)
            .collect();
        for name_node in name_nodes {
            let name = base.get_node_text(&name_node);
            if name == "_" {
                continue;
            }
            let metadata = HashMap::from([(
                "role".to_string(),
                serde_json::Value::String("parameter".to_string()),
            )]);
            let symbol = base.create_symbol(
                &param_node,
                name,
                SymbolKind::Variable,
                SymbolOptions {
                    signature: Some(signature.clone()),
                    parent_id: Some(callable_id.to_string()),
                    metadata: Some(metadata),
                    ..Default::default()
                },
            );
            if records_facts && let Some(type_node) = type_node {
                super::type_facts::record_type_node_fact(base, &symbol.id, type_node, false);
            }
            symbols.push(symbol);
        }
    }
}
