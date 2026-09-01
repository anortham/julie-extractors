//! Parameter symbols for methods and local functions inside Razor `@code` blocks.

use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// Create one `variable` symbol per named parameter of `callable_node`, with
/// metadata `role: "parameter"` and `parent_id` = the callable's symbol id.
pub(super) fn extract_parameter_symbols(
    base: &mut BaseExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let Some(list_node) = parameter_list(callable_node) else {
        return Vec::new();
    };
    let mut symbols = Vec::new();
    let mut cursor = list_node.walk();
    for child in list_node.children(&mut cursor) {
        if !matches!(child.kind(), "parameter" | "parameter_array") {
            continue;
        }
        if let Some(symbol) = extract_parameter(base, child, Some(callable_id.to_string())) {
            symbols.push(symbol);
        }
    }
    symbols
}

fn parameter_list(callable_node: Node) -> Option<Node> {
    if let Some(list) = callable_node.child_by_field_name("parameters")
        && list.kind() == "parameter_list"
    {
        return Some(list);
    }
    let mut cursor = callable_node.walk();
    callable_node
        .children(&mut cursor)
        .find(|child| child.kind() == "parameter_list")
}

fn extract_parameter(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = parameter_name(base, node)?;
    let type_node = node.child_by_field_name("type");
    let declared_type = type_node.map(|ty| base.get_node_text(&ty));
    let is_var = declared_type.as_deref() == Some("var")
        || type_node.is_some_and(|ty| ty.kind() == "implicit_type");

    let mut signature_parts = Vec::new();
    if node.kind() == "parameter_array" {
        signature_parts.push("params".to_string());
    }
    if let Some(ty) = &declared_type {
        signature_parts.push(ty.clone());
    }
    signature_parts.push(name.clone());

    let mut metadata = HashMap::new();
    metadata.insert("role".to_string(), serde_json::json!("parameter"));
    if let Some(ty) = &declared_type {
        metadata.insert("variableType".to_string(), serde_json::json!(ty));
    }
    metadata.insert(
        "isInferred".to_string(),
        serde_json::json!(is_var || declared_type.is_none()),
    );

    let symbol = base.create_symbol(
        &node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature_parts.join(" ")),
            visibility: Some(Visibility::Private),
            parent_id,
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    if !is_var && let Some(type_node) = type_node {
        type_facts::record_declared_type(base, &symbol.id, type_node);
    }
    Some(symbol)
}

fn parameter_name(base: &BaseExtractor, node: Node) -> Option<String> {
    if let Some(name) = node.child_by_field_name("name") {
        return Some(base.get_node_text(&name));
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for child in children.into_iter().rev() {
        if child.kind() == "identifier" {
            return Some(base.get_node_text(&child));
        }
    }
    None
}
