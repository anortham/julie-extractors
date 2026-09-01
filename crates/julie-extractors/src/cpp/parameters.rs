//! Parameter symbol extraction for C++ function definitions.

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

use super::function_declarators::{self, GTEST_MACROS};
use super::type_facts;

pub(super) fn extract_parameter_symbols(
    base: &mut BaseExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    if callable_node.kind() != "function_definition" {
        return Vec::new();
    }
    if is_googletest_macro(base, callable_node) {
        return Vec::new();
    }
    let Some(params_node) = callable_parameter_list(callable_node) else {
        return Vec::new();
    };
    let param_nodes: Vec<Node> = {
        let mut cursor = params_node.walk();
        params_node
            .named_children(&mut cursor)
            .filter(|child| {
                matches!(
                    child.kind(),
                    "parameter_declaration" | "optional_parameter_declaration"
                )
            })
            .collect()
    };

    let mut symbols = Vec::new();
    for param_node in param_nodes {
        let Some(name_node) = parameter_name_node(param_node) else {
            continue;
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
        type_facts::record_parameter_fact(base, &symbol.id, param_node);
        symbols.push(symbol);
    }
    symbols
}

fn is_googletest_macro(base: &BaseExtractor, callable_node: Node) -> bool {
    let Some(declarator) = callable_declarator(callable_node) else {
        return false;
    };
    let Some(func) = function_declarators::unwrap_to_function_declarator(declarator) else {
        return false;
    };
    let Some(name_node) = func.child_by_field_name("declarator") else {
        return false;
    };
    let name = base.get_node_text(&name_node);
    GTEST_MACROS.contains(&name.as_str())
}

fn callable_parameter_list(callable_node: Node) -> Option<Node> {
    let declarator = callable_declarator(callable_node)?;
    let func =
        function_declarators::unwrap_to_function_declarator(declarator).unwrap_or(declarator);
    func.child_by_field_name("parameters").or_else(|| {
        func.children(&mut func.walk())
            .find(|child| child.kind() == "parameter_list")
    })
}

fn callable_declarator(callable_node: Node) -> Option<Node> {
    callable_node.child_by_field_name("declarator").or_else(|| {
        callable_node
            .children(&mut callable_node.walk())
            .find(|child| {
                matches!(
                    child.kind(),
                    "function_declarator" | "pointer_declarator" | "reference_declarator"
                )
            })
    })
}

fn parameter_name_node(param_node: Node) -> Option<Node> {
    let declarator = param_node.child_by_field_name("declarator")?;
    declarator_name(declarator, 0)
}

fn declarator_name(node: Node, depth: u32) -> Option<Node> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    match node.kind() {
        "identifier" | "field_identifier" => Some(node),
        "pointer_declarator"
        | "reference_declarator"
        | "array_declarator"
        | "parenthesized_declarator"
        | "function_declarator"
        | "init_declarator" => {
            let child_depth = child_tree_depth(depth)?;
            if let Some(inner) = node.child_by_field_name("declarator") {
                return declarator_name(inner, child_depth);
            }
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if let Some(name) = declarator_name(child, child_depth) {
                    return Some(name);
                }
            }
            None
        }
        _ => None,
    }
}
