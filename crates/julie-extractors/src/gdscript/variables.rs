//! Variable and constant extraction for GDScript

use super::helpers::{doc_comment, extract_variable_annotations, inside_callable};
use super::type_facts;
use super::types::extract_variable_type;
use crate::base::{
    BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility, normalize_annotations,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

/// Extract a `var` or `const` statement over its whole span, so its
/// initializer and property accessors belong to the symbol.
pub(super) fn extract_variable(
    base: &mut BaseExtractor,
    statement: Node,
    parent_id: Option<&String>,
    same_file_class_names: &HashSet<String>,
) -> Option<Symbol> {
    let is_const = statement.kind() == "const_statement";
    let name_node = statement.child_by_field_name("name")?;
    let name = base.get_node_text(&name_node);
    let signature = base.get_node_text(&statement);

    let (annotations, full_signature) = extract_variable_annotations(base, statement, &signature);
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
    let (kind, visibility) = if is_const {
        let kind = if is_local {
            SymbolKind::Variable
        } else {
            SymbolKind::Constant
        };
        (kind, Visibility::Public)
    } else {
        let is_exported = statement.kind() == "export_variable_statement"
            || annotations.iter().any(|a| a.starts_with("@export"));
        let is_onready = statement.kind() == "onready_variable_statement"
            || annotations.iter().any(|a| a.starts_with("@onready"));
        metadata.insert("isExported".to_string(), Value::Bool(is_exported));
        metadata.insert("isOnReady".to_string(), Value::Bool(is_onready));
        let kind = if is_local {
            SymbolKind::Variable
        } else {
            SymbolKind::Field
        };
        let visibility = if is_exported {
            Visibility::Public
        } else {
            Visibility::Private
        };
        (kind, visibility)
    };

    let doc = doc_comment(base, statement);
    let mut symbol = base.create_symbol(
        &statement,
        name,
        kind,
        SymbolOptions {
            signature: Some(full_signature),
            visibility: Some(visibility),
            parent_id: parent_id.cloned(),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: annotation_markers,
        },
    );
    symbol.doc_comment = doc;
    symbol.body_span = None;
    symbol.body_hash = None;
    type_facts::record_statement_type_facts(base, &symbol.id, statement, same_file_class_names);
    Some(symbol)
}
