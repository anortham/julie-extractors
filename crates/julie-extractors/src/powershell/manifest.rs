//! PowerShell data files (`.psd1`): module manifests and settings hashtables.
//! A module manifest names the modules it loads (`RootModule`,
//! `NestedModules`, `RequiredModules`) and the members it exports
//! (`FunctionsToExport`, ...), which become import and export rows.

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

use super::helpers::module_stem;

const IMPORT_KEYS: [&str; 3] = ["RootModule", "NestedModules", "RequiredModules"];
const EXPORT_KEYS: [(&str, &str); 5] = [
    ("FunctionsToExport", "function"),
    ("CmdletsToExport", "cmdlet"),
    ("VariablesToExport", "variable"),
    ("AliasesToExport", "alias"),
    ("DscResourcesToExport", "dsc_resource"),
];

pub(crate) fn is_data_file_path(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".psd1")
}

/// Import rows for the modules a manifest loads and export rows for the
/// members it exports, each at the string that names it.
pub(super) fn extract_manifest_symbols(base: &mut BaseExtractor, root: Node) -> Vec<Symbol> {
    let Some(hash) = top_level_hash(root, 0) else {
        return Vec::new();
    };
    let content = base.content.clone();
    let mut symbols = Vec::new();
    for (key, entry) in hash_entries(&content, hash) {
        let Some(value) = entry_value(entry) else {
            continue;
        };
        let (kind, metadata_key, metadata_value) =
            if let Some(key) = IMPORT_KEYS.iter().find(|k| k.eq_ignore_ascii_case(&key)) {
                (SymbolKind::Import, "manifest_key", *key)
            } else if let Some((_, export_kind)) = EXPORT_KEYS
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&key))
            {
                (SymbolKind::Export, "export_kind", *export_kind)
            } else {
                continue;
            };
        for name_node in module_name_strings(&content, value) {
            let text = unquote(&base.get_node_text(&name_node));
            let name = if kind == SymbolKind::Import {
                module_stem(&text)
            } else {
                Some(text)
            };
            let Some(name) = name.filter(|name| !name.is_empty()) else {
                continue;
            };
            let metadata = HashMap::from([(
                metadata_key.to_string(),
                Value::String(metadata_value.to_string()),
            )]);
            symbols.push(base.create_symbol(
                &name_node,
                name,
                kind.clone(),
                SymbolOptions {
                    signature: Some(base.get_node_text(&entry).trim().to_string()),
                    visibility: Some(Visibility::Public),
                    parent_id: None,
                    metadata: Some(metadata),
                    doc_comment: None,
                    annotations: Vec::new(),
                },
            ));
        }
    }
    symbols
}

/// The outermost hashtable literal of a data file.
pub(crate) fn top_level_hash(node: Node, depth: u32) -> Option<Node> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.kind() == "hash_literal_expression" {
        return Some(node);
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| top_level_hash(child, child_depth))
}

/// The entries of a hashtable literal with their key text.
pub(crate) fn hash_entries<'a>(content: &str, hash: Node<'a>) -> Vec<(String, Node<'a>)> {
    let mut cursor = hash.walk();
    let Some(body) = hash
        .named_children(&mut cursor)
        .find(|child| child.kind() == "hash_literal_body")
    else {
        return Vec::new();
    };
    let mut cursor = body.walk();
    body.named_children(&mut cursor)
        .filter(|entry| entry.kind() == "hash_entry")
        .filter_map(|entry| {
            let key = entry.named_child(0)?;
            let text = content.get(key.byte_range())?;
            Some((unquote(text), entry))
        })
        .collect()
}

/// The value expression of a hash entry.
pub(crate) fn entry_value(entry: Node) -> Option<Node> {
    let mut cursor = entry.walk();
    entry
        .named_children(&mut cursor)
        .skip(1)
        .find(|child| child.kind() != "comment")
}

/// The string nodes naming modules or members in a manifest value: plain
/// strings, and the `ModuleName` of a module specification hashtable.
fn module_name_strings<'a>(content: &str, value: Node<'a>) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    collect_module_name_strings(content, value, &mut out, 0);
    out
}

fn collect_module_name_strings<'a>(
    content: &str,
    node: Node<'a>,
    out: &mut Vec<Node<'a>>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "string_literal" {
        out.push(node);
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    if node.kind() == "hash_literal_expression" {
        if let Some(name) = hash_entries(content, node)
            .into_iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("ModuleName"))
            .and_then(|(_, entry)| entry_value(entry))
        {
            collect_module_name_strings(content, name, out, child_depth);
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_module_name_strings(content, child, out, child_depth);
    }
}

/// The shape of a data-file value: `string`, `number`, `boolean`, `array`,
/// `hashtable`, `scriptblock`, or `expression`.
pub(crate) fn value_kind(content: &str, value: Node) -> &'static str {
    let mut current = value;
    loop {
        let named = current.named_child_count();
        match current.kind() {
            "string_literal" => return "string",
            "integer_literal" | "real_literal" => return "number",
            "hash_literal_expression" => return "hashtable",
            "array_expression" => return "array",
            "script_block_expression" => return "scriptblock",
            "variable" => {
                let text = content.get(current.byte_range()).unwrap_or_default();
                return if text.eq_ignore_ascii_case("$true") || text.eq_ignore_ascii_case("$false")
                {
                    "boolean"
                } else {
                    "expression"
                };
            }
            "array_literal_expression" if named > 1 => return "array",
            _ if named == 1 => current = current.named_child(0).unwrap_or(current),
            _ => return "expression",
        }
    }
}

pub(crate) fn unquote(text: &str) -> String {
    text.trim().trim_matches(['"', '\'']).to_string()
}

/// The first string a manifest value holds, unquoted.
pub(crate) fn first_string(content: &str, value: Node) -> Option<String> {
    let mut strings = Vec::new();
    collect_module_name_strings(content, value, &mut strings, 0);
    strings
        .first()
        .and_then(|node| content.get(node.byte_range()))
        .map(unquote)
}
