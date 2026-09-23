//! Variable and constant extraction for GDScript

use super::helpers::{declaration_annotations, doc_comment, inside_callable, member_visibility};
use super::type_facts;
use super::types::extract_variable_type;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, normalize_annotations};
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
    type_facts::record_statement_type_facts(base, &symbol.id, statement, same_file_class_names);
    Some(symbol)
}
