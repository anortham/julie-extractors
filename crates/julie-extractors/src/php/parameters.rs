use super::{PhpExtractor, find_child, type_facts};
use crate::base::{Symbol, SymbolKind, SymbolOptions};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) fn extract_parameter_symbols(
    extractor: &mut PhpExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let Some(params_node) = find_child(extractor, &callable_node, "formal_parameters") else {
        return Vec::new();
    };
    let param_nodes: Vec<Node> = {
        let mut cursor = params_node.walk();
        params_node
            .named_children(&mut cursor)
            .filter(|child| {
                matches!(
                    child.kind(),
                    "simple_parameter" | "variadic_parameter" | "property_promotion_parameter"
                )
            })
            .collect()
    };

    let mut symbols = Vec::new();
    for param_node in param_nodes {
        let Some(name) = parameter_name(extractor, param_node) else {
            continue;
        };
        let signature = extractor.get_base().get_node_text(&param_node);
        let metadata = HashMap::from([(
            "role".to_string(),
            serde_json::Value::String("parameter".to_string()),
        )]);
        let symbol = extractor.get_base_mut().create_symbol(
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
            type_facts::record_declared_type(extractor.get_base_mut(), &symbol.id, type_node);
        }
        symbols.push(symbol);
    }
    symbols
}

fn parameter_name(extractor: &PhpExtractor, param_node: Node) -> Option<String> {
    let name_node = param_node.child_by_field_name("name")?;
    if name_node.kind() == "variable_name" {
        if let Some(inner) = find_child(extractor, &name_node, "name") {
            return Some(extractor.get_base().get_node_text(&inner));
        }
        return Some(
            extractor
                .get_base()
                .get_node_text(&name_node)
                .trim_start_matches('$')
                .to_string(),
        );
    }
    Some(
        extractor
            .get_base()
            .get_node_text(&name_node)
            .trim_start_matches('$')
            .to_string(),
    )
}
