use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) fn extract_parameter_symbols(
    base: &mut BaseExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let Some(params_node) = callable_node.child_by_field_name("parameters") else {
        return Vec::new();
    };

    let mut symbols = Vec::new();
    let mut cursor = params_node.walk();
    for param_node in params_node.named_children(&mut cursor) {
        symbols.extend(extract_one_parameter(base, param_node, callable_id));
    }
    symbols
}

fn extract_one_parameter(
    base: &mut BaseExtractor,
    param_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    if param_node.kind() == "variadic_parameter" {
        let mut nested = Vec::new();
        let mut cursor = param_node.walk();
        for child in param_node.named_children(&mut cursor) {
            nested.extend(extract_one_parameter(base, child, callable_id));
        }
        return nested;
    }

    let Some(name_node) = parameter_name_node(param_node) else {
        return Vec::new();
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
    if let Some(type_node) = param_node.child_by_field_name("type")
        && type_node.kind() == "type"
    {
        type_facts::record_declared_type_node(base, &symbol.id, type_node);
    }
    vec![symbol]
}

fn parameter_name_node(param_node: Node) -> Option<Node> {
    match param_node.kind() {
        "identifier" => Some(param_node),
        "typed_parameter" | "default_parameter" | "typed_default_parameter" => {
            let mut cursor = param_node.walk();
            param_node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "identifier")
        }
        _ => None,
    }
}
