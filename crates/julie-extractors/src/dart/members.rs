// Dart Extractor - Members Extraction
//
// Methods for extracting fields, properties, getters, and setters

use super::helpers::*;
use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

/// Extract every field a class-body `declaration` declares: typed or untyped,
/// `static const`/`final` lists, and each name of `int a, b;`. The first
/// field spans the declaration; later ones span their own declarator.
pub(super) fn extract_fields(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
    same_file_types: &HashSet<String>,
) -> Vec<Symbol> {
    if node.kind() != "declaration" {
        return Vec::new();
    }
    extract_declarators(base, node, parent_id, same_file_types, |_| {
        SymbolKind::Field
    })
}

/// Extract the symbols of a top-level variable declaration: `const`/`final`
/// names are constants, the rest are variables.
pub(super) fn extract_top_level_variables(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
    same_file_types: &HashSet<String>,
) -> Vec<Symbol> {
    extract_declarators(base, node, parent_id, same_file_types, |modifiers| {
        if modifiers.contains(&"const") || modifiers.contains(&"final") {
            SymbolKind::Constant
        } else {
            SymbolKind::Variable
        }
    })
}

fn extract_declarators(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
    same_file_types: &HashSet<String>,
    kind_for: impl Fn(&[&str]) -> SymbolKind,
) -> Vec<Symbol> {
    let Some(list) = find_child_by_type(node, "initialized_identifier_list")
        .or_else(|| find_child_by_type(node, "static_final_declaration_list"))
    else {
        return Vec::new();
    };
    let type_node = node.child_by_field_name("type").or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .find(|child| matches!(child.kind(), "type" | "type_identifier"))
    });
    let prefix = base.content[node.start_byte()..list.start_byte()]
        .trim()
        .to_string();
    let type_text = type_node.map(|type_node| get_node_text(&type_node));
    let modifiers: Vec<&str> = match &type_text {
        Some(type_text) => prefix.strip_suffix(type_text.as_str()).unwrap_or(&prefix),
        None => &prefix,
    }
    .split_whitespace()
    .collect();
    let kind = kind_for(&modifiers);
    let is_late = modifiers.contains(&"late");
    let is_final = modifiers.contains(&"final") || modifiers.contains(&"const");
    let is_static = modifiers.contains(&"static");
    let annotations = extract_annotation_markers(node);

    let mut cursor = list.walk();
    let declarators: Vec<Node> = list
        .named_children(&mut cursor)
        .filter(|child| {
            matches!(
                child.kind(),
                "initialized_identifier" | "static_final_declaration"
            )
        })
        .collect();
    let mut symbols = Vec::new();
    for (index, declarator) in declarators.into_iter().enumerate() {
        let Some(name_node) = declarator
            .child_by_field_name("name")
            .or_else(|| find_child_by_type(&declarator, "identifier"))
        else {
            continue;
        };
        let name = get_node_text(&name_node);
        let anchor = if index == 0 { *node } else { declarator };
        let is_private = name.starts_with('_');
        let mut symbol = base.create_symbol(
            &anchor,
            name.clone(),
            kind.clone(),
            SymbolOptions {
                signature: Some(format!("{prefix} {name}").trim().to_string()),
                visibility: Some(if is_private {
                    Visibility::Private
                } else {
                    Visibility::Public
                }),
                parent_id: parent_id.map(|id| id.to_string()),
                metadata: Some(HashMap::new()),
                doc_comment: if index == 0 {
                    base.find_doc_comment(node)
                } else {
                    None
                },
                annotations: annotations.clone(),
            },
        );
        if index > 0 {
            symbol.doc_comment = None;
        }
        symbol.body_span = None;
        symbol.body_hash = None;
        let metadata = symbol.metadata.get_or_insert_with(HashMap::new);
        if kind == SymbolKind::Field {
            metadata.insert("isLate".to_string(), serde_json::Value::Bool(is_late));
            metadata.insert("isStatic".to_string(), serde_json::Value::Bool(is_static));
        }
        metadata.insert("isFinal".to_string(), serde_json::Value::Bool(is_final));

        if let Some(type_node) = type_node {
            type_facts::record_declared_type(base, &symbol.id, type_node);
        } else if let Some(value) = declarator.child_by_field_name("value")
            && let Some(class_name) =
                type_facts::inferred_constructor_name(base, value, same_file_types)
        {
            type_facts::record_constructor_fact(base, &symbol.id, &class_name);
        }
        symbols.push(symbol);
    }
    symbols
}

/// Extract getter property
pub(super) fn extract_getter(
    base: &mut BaseExtractor,
    node: &Node,
    anchor: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = find_child_by_type(node, "identifier")?;
    let name = get_node_text(&name_node);
    let is_private = name.starts_with('_');

    let mut symbol = base.create_symbol(
        anchor,
        name.clone(),
        SymbolKind::Property,
        SymbolOptions {
            signature: Some(accessor_signature(node, &name, "get")),
            visibility: Some(if is_private {
                Visibility::Private
            } else {
                Visibility::Public
            }),
            parent_id: parent_id.map(|id| id.to_string()),
            metadata: Some(HashMap::new()),
            doc_comment: base.find_doc_comment(anchor),
            annotations: extract_annotation_markers(anchor),
        },
    );

    clear_bodyless_span(&mut symbol, anchor);
    symbol.metadata.get_or_insert_with(HashMap::new).insert(
        "accessorKind".to_string(),
        serde_json::Value::String("getter".to_string()),
    );
    if let Some(return_type) = node.child_by_field_name("return_type") {
        type_facts::record_declared_type(base, &symbol.id, return_type);
    }

    Some(symbol)
}

/// Extract setter property
pub(super) fn extract_setter(
    base: &mut BaseExtractor,
    node: &Node,
    anchor: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = find_child_by_type(node, "identifier")?;
    let name = get_node_text(&name_node);
    let is_private = name.starts_with('_');

    let mut symbol = base.create_symbol(
        anchor,
        name.clone(),
        SymbolKind::Property,
        SymbolOptions {
            signature: Some(format!("set {}", name)),
            visibility: Some(if is_private {
                Visibility::Private
            } else {
                Visibility::Public
            }),
            parent_id: parent_id.map(|id| id.to_string()),
            metadata: Some(HashMap::new()),
            doc_comment: base.find_doc_comment(anchor),
            annotations: extract_annotation_markers(anchor),
        },
    );

    clear_bodyless_span(&mut symbol, anchor);
    symbol.metadata.get_or_insert_with(HashMap::new).insert(
        "accessorKind".to_string(),
        serde_json::Value::String("setter".to_string()),
    );

    Some(symbol)
}

/// `int get count`: the getter text up to its name, which keeps the return type.
fn accessor_signature(node: &Node, name: &str, keyword: &str) -> String {
    let text = get_node_text(node);
    if text.contains(keyword) {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        format!("{keyword} {name}")
    }
}
