use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

use super::type_facts;

pub(super) fn extract_function_locals(
    base: &mut BaseExtractor,
    function_node: Node,
    function_id: &str,
    depth: u32,
) -> Vec<Symbol> {
    let Some(body) = function_node.child_by_field_name("body") else {
        return Vec::new();
    };
    let Some(child_depth) = child_tree_depth(depth) else {
        return Vec::new();
    };
    let mut symbols = Vec::new();
    walk_for_locals(base, body, function_id, child_depth, &mut symbols);
    symbols
}

fn walk_for_locals(
    base: &mut BaseExtractor,
    node: Node,
    function_id: &str,
    depth: u32,
    symbols: &mut Vec<Symbol>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if is_nested_function(node.kind()) {
        return;
    }
    if matches!(node.kind(), "lexical_declaration" | "variable_declaration") {
        extract_declaration(base, node, function_id, symbols);
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_for_locals(base, child, function_id, child_depth, symbols);
    }
}

fn extract_declaration(
    base: &mut BaseExtractor,
    node: Node,
    function_id: &str,
    symbols: &mut Vec<Symbol>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "variable_declarator" {
            continue;
        }
        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };
        if name_node.kind() != "identifier" {
            continue;
        }
        let name = base.get_node_text(&name_node);
        let signature = base.get_node_text(&child);
        let symbol = base.create_symbol(
            &child,
            name,
            SymbolKind::Variable,
            SymbolOptions {
                signature: Some(signature),
                parent_id: Some(function_id.to_string()),
                ..Default::default()
            },
        );
        if let Some(value) = child.child_by_field_name("value") {
            type_facts::record_new_expression_fact(base, &symbol.id, value);
        }
        symbols.push(symbol);
    }
}

fn is_nested_function(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "function"
            | "function_expression"
            | "arrow_function"
            | "generator_function"
            | "generator_function_declaration"
            | "method_definition"
    )
}
