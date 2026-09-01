//! Parameter symbol extraction shared by the ECMAScript extractors.

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use std::collections::HashMap;
use tree_sitter::Node;

/// Node kinds whose parameters this module owns; the `variable_declarator`
/// wrapper around an arrow function is excluded so the arrow node itself
/// carries the parameters exactly once.
pub(crate) fn is_parameter_owner(node_kind: &str) -> bool {
    matches!(
        node_kind,
        "function_declaration"
            | "function"
            | "function_expression"
            | "arrow_function"
            | "generator_function"
            | "generator_function_declaration"
            | "method_definition"
    )
}

/// Create one `variable` symbol per named parameter of `callable_node`, with
/// metadata `role: "parameter"` and `parent_id` = the callable's symbol id.
/// Returns each symbol with its parameter node so callers can record
/// language-specific type facts.
pub(crate) fn extract_parameter_symbols<'tree>(
    base: &mut BaseExtractor,
    callable_node: Node<'tree>,
    callable_id: &str,
) -> Vec<(Symbol, Node<'tree>)> {
    let mut parameter_nodes = Vec::new();
    if let Some(params_node) = callable_node.child_by_field_name("parameters") {
        let mut cursor = params_node.walk();
        parameter_nodes.extend(params_node.named_children(&mut cursor));
    } else if let Some(single) = callable_node.child_by_field_name("parameter") {
        parameter_nodes.push(single);
    }

    parameter_nodes
        .into_iter()
        .filter_map(|param_node| {
            let name_node = parameter_name_node(param_node)?;
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
            Some((symbol, param_node))
        })
        .collect()
}

fn parameter_name_node(param_node: Node<'_>) -> Option<Node<'_>> {
    match param_node.kind() {
        "identifier" => Some(param_node),
        "assignment_pattern" => param_node
            .child_by_field_name("left")
            .filter(|left| left.kind() == "identifier"),
        "rest_pattern" => rest_identifier(param_node),
        "required_parameter" | "optional_parameter" => {
            let pattern = param_node.child_by_field_name("pattern")?;
            match pattern.kind() {
                "identifier" => Some(pattern),
                "rest_pattern" => rest_identifier(pattern),
                _ => None,
            }
        }
        _ => None,
    }
}

fn rest_identifier(rest_node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = rest_node.walk();
    rest_node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "identifier")
}
