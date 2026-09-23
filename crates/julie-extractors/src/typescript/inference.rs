//! Type inference from assignments and return statements
//!
//! This module handles basic type inference for variables and functions
//! based on their assignments and return statements.

use crate::base::{Symbol, SymbolKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use crate::typescript::TypeScriptExtractor;
use std::collections::HashMap;
use tree_sitter::Node;

/// Infer types from variable assignments and function returns
pub(crate) fn infer_types(
    extractor: &TypeScriptExtractor,
    symbols: &[Symbol],
) -> HashMap<String, String> {
    let mut types = HashMap::new();

    if let Ok(tree) = parse_content(extractor) {
        infer_types_from_tree(extractor, tree.root_node(), symbols, &mut types);
    }

    types
}

/// Parse content using the tree-sitter parser
fn parse_content(
    extractor: &TypeScriptExtractor,
) -> Result<tree_sitter::Tree, Box<dyn std::error::Error>> {
    let mut parser = tree_sitter::Parser::new();
    let language = if extractor.base().language == "tsx" {
        tree_sitter_typescript::LANGUAGE_TSX.into()
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    };
    parser.set_language(&language)?;
    let tree = parser
        .parse(&extractor.base().content, None)
        .ok_or("Failed to parse content")?;
    Ok(tree)
}

/// Recursively infer types from tree nodes
pub(crate) fn infer_types_from_tree(
    extractor: &TypeScriptExtractor,
    node: Node,
    symbols: &[Symbol],
    types: &mut HashMap<String, String>,
) {
    infer_types_from_tree_at_depth(extractor, node, symbols, types, 0);
}

fn infer_types_from_tree_at_depth(
    extractor: &TypeScriptExtractor,
    node: Node,
    symbols: &[Symbol],
    types: &mut HashMap<String, String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    infer_node_type(extractor, node, symbols, types);

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        infer_types_from_tree_at_depth(extractor, child, symbols, types, child_depth);
    }
}

/// Infer the type of the symbol `node` declares. The symbol is found by its
/// span, never by name, so a local never takes a same-named symbol's type.
/// Callables with a `return_type` annotation keep their declared fact, and
/// placeholders that name no type (`any`, `function`) record nothing.
fn infer_node_type(
    extractor: &TypeScriptExtractor,
    node: Node,
    symbols: &[Symbol],
    types: &mut HashMap<String, String>,
) {
    let inferred = match node.kind() {
        "variable_declarator" => {
            if node
                .child_by_field_name("name")
                .is_none_or(|name| name.kind() != "identifier")
            {
                return;
            }
            let Some(value_node) = node.child_by_field_name("value") else {
                return;
            };
            Some((
                declared_symbol(node, symbols, &[SymbolKind::Variable]),
                infer_type_from_value(extractor, &value_node),
            ))
        }
        "function_declaration"
        | "generator_function_declaration"
        | "function_expression"
        | "arrow_function"
        | "method_definition"
            if node.child_by_field_name("return_type").is_none() =>
        {
            Some((
                declared_symbol(node, symbols, &[SymbolKind::Function, SymbolKind::Method]),
                infer_function_return_type(extractor, &node),
            ))
        }
        _ => None,
    };
    if let Some((Some(symbol), inferred_type)) = inferred
        && !matches!(inferred_type.as_str(), "any" | "function" | "Promise<any>")
    {
        types.insert(symbol.id.clone(), inferred_type);
    }
}

fn declared_symbol<'a>(
    node: Node,
    symbols: &'a [Symbol],
    kinds: &[SymbolKind],
) -> Option<&'a Symbol> {
    symbols.iter().find(|symbol| {
        symbol.start_byte == node.start_byte() as u32
            && symbol.end_byte == node.end_byte() as u32
            && kinds.contains(&symbol.kind)
    })
}

/// Infer type from a value node
pub(crate) fn infer_type_from_value(extractor: &TypeScriptExtractor, value_node: &Node) -> String {
    match value_node.kind() {
        "string" => "string".to_string(),
        "number" => "number".to_string(),
        "true" | "false" => "boolean".to_string(),
        "array" => "array".to_string(),
        "object" => "object".to_string(),
        "null" => "null".to_string(),
        "undefined" => "undefined".to_string(),
        "arrow_function" | "function" | "function_expression" => "function".to_string(),
        "call_expression" => {
            // Try to infer based on common function calls
            if let Some(function_node) = value_node.child_by_field_name("function") {
                let function_name = extractor.base().get_node_text(&function_node);
                match function_name.as_str() {
                    "fetch" => "Promise<Response>".to_string(),
                    "Promise.resolve" => "Promise<any>".to_string(),
                    "JSON.parse" => "any".to_string(),
                    "JSON.stringify" => "string".to_string(),
                    _ => "any".to_string(),
                }
            } else {
                "any".to_string()
            }
        }
        _ => "any".to_string(),
    }
}

/// Infer return type of a function
pub(crate) fn infer_function_return_type(
    extractor: &TypeScriptExtractor,
    func_node: &Node,
) -> String {
    // Check for async functions
    let is_async = func_node
        .children(&mut func_node.walk())
        .any(|child| child.kind() == "async");

    if is_async {
        return "Promise<any>".to_string();
    }

    // Look for return statements in the function body
    if let Some(body_node) = func_node.child_by_field_name("body") {
        let mut return_types = Vec::new();
        collect_return_types(extractor, &body_node, &mut return_types);

        if !return_types.is_empty() {
            // If we found return statements, try to unify types
            if return_types.iter().all(|t| t == "string") {
                return "string".to_string();
            } else if return_types.iter().all(|t| t == "number") {
                return "number".to_string();
            } else if return_types.iter().all(|t| t == "boolean") {
                return "boolean".to_string();
            }
            // Mixed types or complex types
            return "any".to_string();
        }
    }

    // Default to function type
    "function".to_string()
}

/// Collect return types from a node's tree
pub(crate) fn collect_return_types(
    extractor: &TypeScriptExtractor,
    node: &Node,
    return_types: &mut Vec<String>,
) {
    collect_return_types_at_depth(extractor, node, return_types, 0);
}

fn collect_return_types_at_depth(
    extractor: &TypeScriptExtractor,
    node: &Node,
    return_types: &mut Vec<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "return_statement"
        && let Some(value_node) = node
            .named_child(0)
            .filter(|value| value.kind() != "comment")
    {
        let return_type = infer_type_from_value(extractor, &value_node);
        return_types.push(return_type);
    }

    // Recursively search children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_return_types_at_depth(extractor, &child, return_types, child_depth);
    }
}
