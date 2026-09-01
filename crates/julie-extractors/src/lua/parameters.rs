use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) fn extract_parameter_symbols(
    base: &mut BaseExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let params_node = callable_node.child_by_field_name("parameters");
    if let Some(owner) = type_facts::colon_method_owner_name(base, callable_node) {
        let span_node = params_node.unwrap_or(callable_node);
        let self_symbol = parameter_symbol(base, span_node, "self", callable_id);
        type_facts::record_declared_owner_fact(base, &self_symbol.id, &owner);
        symbols.push(self_symbol);
    }
    let Some(params_node) = params_node else {
        return symbols;
    };
    let mut cursor = params_node.walk();
    for name_node in params_node.children_by_field_name("name", &mut cursor) {
        if name_node.kind() != "identifier" {
            continue;
        }
        let name = base.get_node_text(&name_node);
        symbols.push(parameter_symbol(base, name_node, name, callable_id));
    }
    symbols
}

fn parameter_symbol(
    base: &mut BaseExtractor,
    node: Node,
    name: impl Into<String>,
    callable_id: &str,
) -> Symbol {
    let name = name.into();
    let signature = base.get_node_text(&node);
    let metadata = HashMap::from([(
        "role".to_string(),
        serde_json::Value::String("parameter".to_string()),
    )]);
    base.create_symbol(
        &node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            parent_id: Some(callable_id.to_string()),
            metadata: Some(metadata),
            ..Default::default()
        },
    )
}
