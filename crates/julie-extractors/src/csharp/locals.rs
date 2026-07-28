// Local variables and parameters for C# callables.

use super::helpers;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract a local `variable_declaration` / `local_declaration_statement`.
///
/// Multiple declarators (`int a = 1, b = 2;`) produce one symbol each, all
/// sharing the declared type.
pub fn extract_local_declaration(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Vec<Symbol> {
    let declaration = if node.kind() == "local_declaration_statement" {
        find_child(node, "variable_declaration").unwrap_or(node)
    } else {
        node
    };

    let declared_type = type_name_from_declaration(base, declaration);
    let is_var = declared_type
        .as_deref()
        .is_some_and(|t| t == "var" || t == "using");

    let mut symbols = Vec::new();
    let mut cursor = declaration.walk();
    for child in declaration.children(&mut cursor) {
        if child.kind() != "variable_declarator" {
            continue;
        }
        if let Some(symbol) = extract_declarator(
            base,
            child,
            parent_id.clone(),
            declared_type.as_deref(),
            is_var,
            "local",
        ) {
            symbols.push(symbol);
        }
    }
    symbols
}

/// Extract a formal parameter (`parameter`, `parameter_array`).
pub fn extract_parameter(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = parameter_name(base, node)?;
    let declared_type = parameter_type_name(base, node);
    let is_var = declared_type.as_deref() == Some("var");

    let mut signature_parts = Vec::new();
    let modifiers = helpers::extract_modifiers(base, &node);
    if !modifiers.is_empty() {
        signature_parts.push(modifiers.join(" "));
    }
    if node.kind() == "parameter_array" {
        signature_parts.push("params".to_string());
    }
    if let Some(ref ty) = declared_type {
        signature_parts.push(ty.clone());
    }
    signature_parts.push(name.clone());

    let mut metadata = HashMap::new();
    metadata.insert("role".to_string(), serde_json::json!("parameter"));
    if let Some(ref ty) = declared_type {
        metadata.insert("variableType".to_string(), serde_json::json!(ty));
    }
    metadata.insert(
        "isInferred".to_string(),
        serde_json::json!(is_var || declared_type.is_none()),
    );

    Some(base.create_symbol(
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
    ))
}

fn extract_declarator(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    declared_type: Option<&str>,
    is_var: bool,
    role: &str,
) -> Option<Symbol> {
    let name_node = find_child(node, "identifier")?;
    let name = base.get_node_text(&name_node);

    let mut initializer = None;
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    if let Some(eq) = children.iter().position(|c| c.kind() == "=")
        && eq + 1 < children.len()
    {
        initializer = Some(base.get_node_text(&children[eq + 1]));
    }

    let mut signature_parts = Vec::new();
    if let Some(ty) = declared_type {
        signature_parts.push(ty.to_string());
    }
    signature_parts.push(name.clone());
    if let Some(ref init) = initializer {
        signature_parts.push(format!("= {init}"));
    }

    let mut metadata = HashMap::new();
    metadata.insert("role".to_string(), serde_json::json!(role));
    if let Some(ty) = declared_type {
        metadata.insert("variableType".to_string(), serde_json::json!(ty));
    }
    if let Some(init) = initializer {
        metadata.insert("initializer".to_string(), serde_json::json!(init));
    }
    metadata.insert(
        "isInferred".to_string(),
        serde_json::json!(is_var || declared_type.is_none()),
    );

    Some(base.create_symbol(
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
    ))
}

fn type_name_from_declaration(base: &BaseExtractor, declaration: Node) -> Option<String> {
    let mut cursor = declaration.walk();
    for child in declaration.children(&mut cursor) {
        match child.kind() {
            "predefined_type" | "identifier" | "generic_name" | "qualified_name"
            | "nullable_type" | "array_type" | "tuple_type" | "pointer_type" | "ref_type"
            | "implicit_type" => {
                return Some(base.get_node_text(&child));
            }
            _ => {}
        }
    }
    None
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

fn parameter_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    if let Some(ty) = node.child_by_field_name("type") {
        return Some(base.get_node_text(&ty));
    }
    type_name_from_declaration(base, node)
}

fn find_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == kind)
}
