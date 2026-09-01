//! Parameter symbol extraction for Swift functions and initializers.

use crate::base::{Symbol, SymbolKind, SymbolOptions};
use crate::swift::SwiftExtractor;
use std::collections::HashMap;
use tree_sitter::Node;

use super::type_facts;

/// Create one `variable` symbol per named parameter of `callable_node`, with
/// metadata `role: "parameter"` and `parent_id` = the callable's symbol id.
/// Parameters with a recordable stated type also record a declared-type fact.
pub(super) fn extract_parameter_symbols(
    extractor: &mut SwiftExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let param_nodes = parameter_nodes(callable_node);
    let mut symbols = Vec::new();
    for param_node in param_nodes {
        let Some(name_node) = parameter_name_node(param_node) else {
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
        if let Some(type_node) = declared_parameter_type(param_node) {
            let declared = declared_parameter_text(extractor, param_node, type_node);
            type_facts::record_declared_type_text(
                &mut extractor.base,
                &symbol.id,
                type_node,
                &declared,
            );
        }
        symbols.push(symbol);
    }
    symbols
}

fn parameter_nodes(callable_node: Node) -> Vec<Node> {
    let mut cursor = callable_node.walk();
    callable_node
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "parameter")
        .collect()
}

fn parameter_name_node(param_node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = param_node.walk();
    let named = param_node
        .children_by_field_name("name", &mut cursor)
        .find(|child| child.kind() == "simple_identifier");
    if named.is_some() {
        return named;
    }
    param_node
        .named_children(&mut param_node.walk())
        .find(|child| child.kind() == "simple_identifier")
}

fn declared_parameter_type(param_node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = param_node.walk();
    if let Some(type_node) = param_node
        .children_by_field_name("type", &mut cursor)
        .find(|child| child.is_named())
    {
        return Some(type_node);
    }
    param_node
        .named_children(&mut param_node.walk())
        .find(|child| is_type_node(child.kind()))
}

fn is_type_node(kind: &str) -> bool {
    matches!(
        kind,
        "user_type"
            | "optional_type"
            | "array_type"
            | "dictionary_type"
            | "tuple_type"
            | "function_type"
            | "primitive_type"
            | "type_identifier"
            | "opaque_type"
            | "existential_type"
            | "protocol_composition_type"
            | "metatype"
    )
}

fn declared_parameter_text(
    extractor: &SwiftExtractor,
    param_node: Node,
    type_node: Node,
) -> String {
    let type_text = extractor.base.get_node_text(&type_node);
    if parameter_is_inout(extractor, param_node) {
        format!("inout {type_text}")
    } else {
        type_text
    }
}

fn parameter_is_inout(extractor: &SwiftExtractor, param_node: Node) -> bool {
    let modifiers: Vec<Node> = param_node
        .children(&mut param_node.walk())
        .filter(|child| child.kind() == "parameter_modifiers")
        .collect();
    for modifiers_node in modifiers {
        let items: Vec<Node> = modifiers_node
            .children(&mut modifiers_node.walk())
            .collect();
        if items
            .iter()
            .any(|item| extractor.base.get_node_text(item) == "inout")
        {
            return true;
        }
    }
    false
}
