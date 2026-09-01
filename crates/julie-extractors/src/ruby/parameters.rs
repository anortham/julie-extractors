use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) fn extract_parameter_symbols(
    base: &mut BaseExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let Some(params_node) = method_parameters_node(callable_node) else {
        return Vec::new();
    };

    let mut symbols = Vec::new();
    let mut cursor = params_node.walk();
    for param_node in params_node.named_children(&mut cursor) {
        if let Some(symbol) = extract_one_parameter(base, param_node, callable_id) {
            symbols.push(symbol);
        }
    }
    symbols
}

fn method_parameters_node(callable_node: Node) -> Option<Node> {
    callable_node
        .child_by_field_name("parameters")
        .or_else(|| callable_node.child_by_field_name("method_parameters"))
        .or_else(|| {
            let mut cursor = callable_node.walk();
            callable_node.children(&mut cursor).find(|child| {
                matches!(
                    child.kind(),
                    "parameters" | "method_parameters" | "parameter_list"
                )
            })
        })
}

fn extract_one_parameter(
    base: &mut BaseExtractor,
    param_node: Node,
    callable_id: &str,
) -> Option<Symbol> {
    let name_node = parameter_name_node(param_node)?;
    let name = base.get_node_text(&name_node);
    let signature = base.get_node_text(&param_node);
    let metadata = HashMap::from([(
        "role".to_string(),
        serde_json::Value::String("parameter".to_string()),
    )]);
    Some(base.create_symbol(
        &param_node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            parent_id: Some(callable_id.to_string()),
            metadata: Some(metadata),
            ..Default::default()
        },
    ))
}

fn parameter_name_node(param_node: Node) -> Option<Node> {
    match param_node.kind() {
        "identifier" => Some(param_node),
        "optional_parameter"
        | "keyword_parameter"
        | "splat_parameter"
        | "hash_splat_parameter"
        | "block_parameter" => param_node.child_by_field_name("name").or_else(|| {
            let mut cursor = param_node.walk();
            param_node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "identifier")
        }),
        _ => None,
    }
}
