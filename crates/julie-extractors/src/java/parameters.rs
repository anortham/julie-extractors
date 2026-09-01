//! Parameter symbol extraction for Java methods and constructors.

use crate::base::{Symbol, SymbolKind, SymbolOptions};
use crate::java::JavaExtractor;
use std::collections::HashMap;
use tree_sitter::Node;

use super::type_facts;

/// Create one `variable` symbol per named parameter of `callable_node`, with
/// metadata `role: "parameter"` and `parent_id` = the callable's symbol id.
/// Parameters with a recordable stated type also record a declared-type fact.
pub(super) fn extract_parameter_symbols(
    extractor: &mut JavaExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let Some(params_node) = callable_node.child_by_field_name("parameters") else {
        return Vec::new();
    };
    let param_nodes: Vec<Node> = {
        let mut cursor = params_node.walk();
        params_node
            .named_children(&mut cursor)
            .filter(|child| matches!(child.kind(), "formal_parameter" | "spread_parameter"))
            .collect()
    };

    let mut symbols = Vec::new();
    for param_node in param_nodes {
        let Some(name_node) = parameter_name_node(param_node) else {
            continue;
        };
        let name = extractor.base().get_node_text(&name_node);
        let signature = extractor.base().get_node_text(&param_node);
        let metadata = HashMap::from([(
            "role".to_string(),
            serde_json::Value::String("parameter".to_string()),
        )]);
        let symbol = extractor.base_mut().create_symbol(
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
        if let Some(type_node) = declared_parameter_type(param_node) {
            type_facts::record_declared_type(extractor.base_mut(), &symbol.id, type_node);
        }
        symbols.push(symbol);
    }
    symbols
}

fn parameter_name_node(param_node: Node<'_>) -> Option<Node<'_>> {
    match param_node.kind() {
        "formal_parameter" => param_node
            .child_by_field_name("name")
            .filter(|name| name.kind() == "identifier"),
        "spread_parameter" => {
            let declarator = {
                let mut cursor = param_node.walk();
                param_node
                    .children(&mut cursor)
                    .find(|child| child.kind() == "variable_declarator")
            }?;
            declarator
                .child_by_field_name("name")
                .filter(|name| name.kind() == "identifier")
        }
        _ => None,
    }
}

/// The type node a `formal_parameter` states. Spread (`Foo... parts`) and
/// bracket-suffixed (`Foo parts[]`) parameters name an array the grammar does
/// not spell as `array_type`, so they state no recordable type.
fn declared_parameter_type(param_node: Node<'_>) -> Option<Node<'_>> {
    if param_node.kind() != "formal_parameter"
        || param_node.child_by_field_name("dimensions").is_some()
    {
        return None;
    }
    param_node.child_by_field_name("type")
}
