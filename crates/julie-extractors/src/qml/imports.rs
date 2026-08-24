use super::QmlExtractor;
use crate::base::{Symbol, SymbolKind, SymbolOptions};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) fn extract(
    extractor: &mut QmlExtractor,
    node: &Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let source_node = node.child_by_field_name("source")?;
    let source = normalize_source(&extractor.base.get_node_text(&source_node));
    if source.is_empty() {
        return None;
    }
    let source_kind = super::import_source_kind(&source_node)?;
    let import_kind = super::import_kind(source_kind, &source);

    let mut metadata = HashMap::new();
    metadata.insert("source".to_string(), Value::String(source.clone()));
    metadata.insert(
        "source_kind".to_string(),
        Value::String(source_kind.to_string()),
    );
    metadata.insert(
        "import_kind".to_string(),
        Value::String(import_kind.to_string()),
    );

    if let Some(version_node) = node.child_by_field_name("version") {
        let version = extractor.base.get_node_text(&version_node);
        if !version.is_empty() {
            metadata.insert("version".to_string(), Value::String(version));
        }
    }

    let name = if let Some(alias_node) = node.child_by_field_name("alias") {
        let alias = extractor.base.get_node_text(&alias_node);
        if !alias.is_empty() {
            metadata.insert("alias".to_string(), Value::String(alias.clone()));
            metadata.insert("local_name".to_string(), Value::String(alias));
            metadata.insert("imported_name".to_string(), Value::String(source.clone()));
            metadata.insert("is_namespace".to_string(), Value::Bool(true));
        }
        source.clone()
    } else {
        source.clone()
    };

    let options = SymbolOptions {
        parent_id,
        visibility: Some(crate::base::Visibility::Public),
        metadata: Some(metadata),
        doc_comment: super::semantics::extract_qml_doc_comment(extractor, node),
        ..Default::default()
    };
    Some(
        extractor
            .base
            .create_symbol(node, name, SymbolKind::Import, options),
    )
}

fn normalize_source(source: &str) -> String {
    let source = source.trim();
    if source.len() >= 2
        && ((source.starts_with('"') && source.ends_with('"'))
            || (source.starts_with('\'') && source.ends_with('\'')))
    {
        source[1..source.len() - 1].to_string()
    } else {
        source.to_string()
    }
}

pub(crate) fn source_kind(node: &Node<'_>) -> Option<&'static str> {
    match node.kind() {
        "identifier" | "nested_identifier" => Some("uri"),
        "string" => Some("quoted"),
        _ => None,
    }
}

/// Classifies quoted `.js` paths case-insensitively; every other quoted path is a directory.
pub(crate) fn import_kind(source_kind: &str, source: &str) -> &'static str {
    if source_kind == "uri" {
        "module"
    } else if source.to_ascii_lowercase().ends_with(".js") {
        "javascript"
    } else {
        "directory"
    }
}
