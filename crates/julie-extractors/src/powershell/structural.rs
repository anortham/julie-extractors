//! PowerShell structural-fact predicates and metadata, called from the shared
//! code structural-fact collector.

use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

use super::manifest::{
    entry_value, first_string, hash_entries, is_data_file_path, unquote, value_kind,
};

/// Whether a `pipeline` joins commands with the `|` operator itself, not a
/// `|` inside a string or a nested pipeline.
pub(crate) fn has_pipeline_operator(node: Node) -> bool {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "pipeline_chain")
        .any(|chain| {
            let mut cursor = chain.walk();
            chain.children(&mut cursor).any(|token| token.kind() == "|")
        })
}

pub(crate) fn is_dsc_resource(content: &str, node: Node) -> bool {
    super::commands::dsc_resource(content, node).is_some()
}

pub(crate) fn dsc_resource_metadata(
    content: &str,
    node: Node,
    metadata: &mut HashMap<String, Value>,
) {
    let Some(resource) = super::commands::dsc_resource(content, node) else {
        return;
    };
    metadata.insert(
        "resource_type".into(),
        Value::String(resource.resource_type),
    );
    metadata.insert(
        "resource_name".into(),
        Value::String(resource.resource_name),
    );
    if let Some(configuration) = resource.configuration {
        metadata.insert("configuration_name".into(), Value::String(configuration));
    }
    if let Some(node_name) = resource.node_name {
        metadata.insert("node_name".into(), Value::String(node_name));
    }
    if !resource.depends_on.is_empty() {
        metadata.insert(
            "depends_on".into(),
            Value::Array(resource.depends_on.into_iter().map(Value::String).collect()),
        );
    }
}

/// A key of a PowerShell data file (`.psd1`) hashtable.
pub(crate) fn is_data_key(file_path: &str, node: Node) -> bool {
    is_data_file_path(file_path) && node.kind() == "hash_entry"
}

pub(crate) fn data_key_metadata(content: &str, node: Node, metadata: &mut HashMap<String, Value>) {
    let Some(key) = entry_key(content, node) else {
        return;
    };
    let mut path = vec![key.clone()];
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        if current.kind() == "hash_entry"
            && let Some(parent_key) = entry_key(content, current)
        {
            path.push(parent_key);
        }
        ancestor = current.parent();
    }
    path.reverse();
    metadata.insert("key".into(), Value::String(key));
    metadata.insert("key_path".into(), Value::String(path.join(".")));
    if let Some(value) = entry_value(node) {
        metadata.insert(
            "value_kind".into(),
            Value::String(value_kind(content, value).into()),
        );
    }
}

fn entry_key(content: &str, entry: Node) -> Option<String> {
    let key = entry.named_child(0)?;
    content.get(key.byte_range()).map(unquote)
}

const MANIFEST_KEYS: [(&str, &str); 4] = [
    ("RootModule", "root_module"),
    ("ModuleVersion", "module_version"),
    ("GUID", "guid"),
    ("PowerShellVersion", "powershell_version"),
];

/// The outermost hashtable of a `.psd1` module manifest: one that sets
/// `RootModule` or `ModuleVersion`.
pub(crate) fn is_module_manifest(file_path: &str, content: &str, node: Node) -> bool {
    if !is_data_file_path(file_path) || node.kind() != "hash_literal_expression" {
        return false;
    }
    let mut ancestor = node.parent();
    while let Some(current) = ancestor {
        if current.kind() == "hash_entry" {
            return false;
        }
        ancestor = current.parent();
    }
    hash_entries(content, node).iter().any(|(key, _)| {
        key.eq_ignore_ascii_case("RootModule") || key.eq_ignore_ascii_case("ModuleVersion")
    })
}

pub(crate) fn module_manifest_metadata(
    content: &str,
    node: Node,
    metadata: &mut HashMap<String, Value>,
) {
    for (key, entry) in hash_entries(content, node) {
        let Some((_, metadata_key)) = MANIFEST_KEYS
            .iter()
            .find(|(manifest_key, _)| manifest_key.eq_ignore_ascii_case(&key))
        else {
            continue;
        };
        if let Some(value) = entry_value(entry).and_then(|value| first_string(content, value)) {
            metadata.insert((*metadata_key).into(), Value::String(value));
        }
    }
}
