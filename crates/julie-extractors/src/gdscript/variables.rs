//! Variable and constant extraction for GDScript

use super::helpers::{declaration_annotations, doc_comment, inside_callable, member_visibility};
use super::type_facts;
use super::types::extract_variable_type;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, normalize_annotations};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract a `var` or `const` statement over its whole span, so its
/// initializer and property accessors belong to the symbol.
pub(super) fn extract_variable(
    base: &mut BaseExtractor,
    statement: Node,
    parent_id: Option<&String>,
    same_file_types: &type_facts::SameFileTypes,
) -> Option<Symbol> {
    let is_const = statement.kind() == "const_statement";
    let name_node = statement.child_by_field_name("name")?;
    let name = base.get_node_text(&name_node);
    let signature = base.get_node_text(&statement);

    let declared_annotations = declaration_annotations(base, statement, false);
    let full_signature = declared_annotations.signature(&signature);
    let annotations = declared_annotations.all;
    let annotation_markers = normalize_annotations(&annotations, "gdscript");
    let data_type =
        extract_variable_type(base, statement, &name_node).unwrap_or_else(|| "Variant".to_string());

    let mut metadata = HashMap::new();
    metadata.insert("dataType".to_string(), Value::String(data_type));
    if !annotations.is_empty() {
        metadata.insert(
            "annotations".to_string(),
            Value::Array(annotations.iter().cloned().map(Value::String).collect()),
        );
    }

    let is_local = inside_callable(statement);
    let visibility = (!is_local).then(|| member_visibility(&name));
    let kind = if is_const {
        if is_local {
            SymbolKind::Variable
        } else {
            SymbolKind::Constant
        }
    } else {
        let is_exported = statement.kind() == "export_variable_statement"
            || annotations.iter().any(|a| a.starts_with("@export"));
        let is_onready = statement.kind() == "onready_variable_statement"
            || annotations.iter().any(|a| a.starts_with("@onready"));
        metadata.insert("isExported".to_string(), Value::Bool(is_exported));
        metadata.insert("isOnReady".to_string(), Value::Bool(is_onready));
        if is_local {
            SymbolKind::Variable
        } else {
            SymbolKind::Field
        }
    };

    let doc = doc_comment(base, statement);
    let mut symbol = base.create_symbol(
        &statement,
        name,
        kind,
        SymbolOptions {
            signature: Some(full_signature),
            visibility,
            parent_id: parent_id.cloned(),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: annotation_markers,
        },
    );
    symbol.doc_comment = doc;
    symbol.body_span = None;
    symbol.body_hash = None;
    type_facts::record_statement_type_facts(base, &symbol.id, statement, same_file_types);
    Some(symbol)
}

/// `const Helper = preload("res://scripts/helper.gd")` or
/// `var Scene = load("res://scenes/main.tscn")` outside a function also
/// imports the resource: an import named by the constant or field, over the
/// path string, with the path as `source` and the loader as `form`.
pub(super) fn extract_resource_import(
    base: &mut BaseExtractor,
    statement: Node,
    parent_id: Option<&String>,
) -> Option<Symbol> {
    if inside_callable(statement) {
        return None;
    }
    let name = base.get_node_text(&statement.child_by_field_name("name")?);
    let call = statement
        .child_by_field_name("value")
        .filter(|value| value.kind() == "call")?;
    let mut cursor = call.walk();
    let loader = call
        .named_children(&mut cursor)
        .next()
        .filter(|callee| callee.kind() == "identifier")
        .map(|callee| base.get_node_text(&callee))?;
    let form = match loader.as_str() {
        "preload" => "preload",
        "load" => "load",
        _ => return None,
    };
    let arguments = call.child_by_field_name("arguments")?;
    if arguments.named_child_count() != 1 {
        return None;
    }
    let path_node = arguments
        .named_child(0)
        .filter(|path| path.kind() == "string")?;
    let source = base
        .decode_string_literal(&path_node)
        .filter(|source| !source.is_empty())?;

    let metadata = HashMap::from([
        ("source".to_string(), Value::String(source)),
        ("form".to_string(), Value::String(form.to_string())),
    ]);
    let visibility = member_visibility(&name);
    let signature = base.get_node_text(&call);
    let mut symbol = base.create_symbol(
        &path_node,
        name,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.cloned(),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    symbol.body_span = None;
    symbol.body_hash = None;
    Some(symbol)
}
