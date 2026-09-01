/// Function signatures and parameter extraction
/// Handles parameter lists, type hints, return types, and visibility inference
use super::PythonExtractor;
use super::type_facts;
use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract function parameters from a parameters node
pub fn extract_parameters(extractor: &PythonExtractor, parameters_node: &Node) -> Vec<String> {
    let mut params = Vec::new();
    let base = extractor.base();

    let mut cursor = parameters_node.walk();
    for child in parameters_node.children(&mut cursor) {
        match child.kind() {
            "identifier" => {
                // Simple parameter name
                params.push(base.get_node_text(&child));
            }
            "parameter" => {
                // Handle basic parameter - find identifier child
                let mut param_cursor = child.walk();
                for param_child in child.children(&mut param_cursor) {
                    if param_child.kind() == "identifier" {
                        params.push(base.get_node_text(&param_child));
                        break;
                    }
                }
            }
            "default_parameter" => {
                // parameter = default_value
                let mut parts = Vec::new();
                let mut param_cursor = child.walk();
                for param_child in child.children(&mut param_cursor) {
                    if param_child.kind() == "identifier" {
                        parts.push(base.get_node_text(&param_child));
                    } else if param_child.kind() == "=" {
                        parts.push("=".to_string());
                    } else if !["(", ")", ","].contains(&param_child.kind()) {
                        parts.push(base.get_node_text(&param_child));
                    }
                }
                if !parts.is_empty() {
                    params.push(parts.join(""));
                }
            }
            "typed_parameter" => {
                // parameter: type
                let mut name = String::new();
                let mut type_str = String::new();
                let mut param_cursor = child.walk();
                for param_child in child.children(&mut param_cursor) {
                    if param_child.kind() == "identifier" && name.is_empty() {
                        name = base.get_node_text(&param_child);
                    } else if param_child.kind() == "type" {
                        type_str = format!(": {}", base.get_node_text(&param_child));
                    }
                }
                params.push(format!("{}{}", name, type_str));
            }
            "typed_default_parameter" => {
                // parameter: type = default_value
                let text = base.get_node_text(&child);
                params.push(text);
            }
            _ => {}
        }
    }

    params
}

/// Create one `variable` symbol per parameter of a function or method, with
/// `role: "parameter"` metadata and the callable as parent. Annotated
/// parameters also record a declared-type fact; `self` and `cls` never do.
pub(super) fn extract_parameter_symbols(
    extractor: &mut PythonExtractor,
    function_node: Node,
    parent_id: &str,
) -> Vec<Symbol> {
    let Some(parameters_node) = function_node.child_by_field_name("parameters") else {
        return Vec::new();
    };

    let mut symbols = Vec::new();
    let mut cursor = parameters_node.walk();
    for parameter in parameters_node.named_children(&mut cursor) {
        let Some(name_node) = parameter_name_node(parameter) else {
            continue;
        };
        let name = extractor.base().get_node_text(&name_node);
        let signature = extractor.base().get_node_text(&parameter);
        let metadata = HashMap::from([("role".to_string(), serde_json::json!("parameter"))]);

        let symbol = extractor.base_mut().create_symbol(
            &parameter,
            name.clone(),
            SymbolKind::Variable,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(infer_visibility(&name)),
                parent_id: Some(parent_id.to_string()),
                metadata: Some(metadata),
                doc_comment: None,
                annotations: Vec::new(),
            },
        );

        if name != "self"
            && name != "cls"
            && let Some(type_node) = parameter.child_by_field_name("type")
        {
            type_facts::record_annotation_fact(extractor.base_mut(), &symbol.id, type_node);
        }

        symbols.push(symbol);
    }

    symbols
}

fn parameter_name_node(parameter: Node) -> Option<Node> {
    match parameter.kind() {
        "identifier" => Some(parameter),
        "typed_parameter" => {
            let type_id = parameter.child_by_field_name("type").map(|node| node.id());
            let mut cursor = parameter.walk();
            parameter
                .named_children(&mut cursor)
                .find(|child| Some(child.id()) != type_id)
                .and_then(binding_identifier)
        }
        "default_parameter" | "typed_default_parameter" => parameter
            .child_by_field_name("name")
            .and_then(binding_identifier),
        "list_splat_pattern" | "dictionary_splat_pattern" => binding_identifier(parameter),
        _ => None,
    }
}

fn binding_identifier(node: Node) -> Option<Node> {
    match node.kind() {
        "identifier" => Some(node),
        "list_splat_pattern" | "dictionary_splat_pattern" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .find(|child| child.kind() == "identifier")
        }
        _ => None,
    }
}

/// Infer visibility from a symbol name
/// Python uses naming conventions: _private, __dunder__, public
pub fn infer_visibility(name: &str) -> Visibility {
    if name.starts_with("__") && name.ends_with("__") {
        // Dunder methods are public
        Visibility::Public
    } else if name.starts_with("_") {
        // Single underscore indicates private/protected
        Visibility::Private
    } else {
        Visibility::Public
    }
}

/// Check if a function has an async keyword
pub(super) fn has_async_keyword(node: &Node) -> bool {
    // Check if any of the node's children is an "async" keyword
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "async" {
            return true;
        }
    }
    false
}

/// Find type annotation in an assignment node
#[allow(clippy::manual_find)] // Manual loop required for borrow checker
pub(super) fn find_type_annotation<'a>(node: &'a Node<'a>) -> Option<Node<'a>> {
    // Look for type annotation in assignment node children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "type" {
            return Some(child);
        }
    }
    None
}
