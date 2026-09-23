// Dart Extractor - Import, Export, Part, and Library Directives
//
// The Dart grammar parses these as:
//   import_or_export
//     library_import -> import_specification (uri: configurable_uri | uri,
//                       deferred, alias: identifier, combinator*)
//     library_export -> configurable_uri -> uri -> string_literal
//   part_directive (uri), part_of_directive (uri | dotted_identifier_list)
//   library_name (dotted_identifier_list)

use super::helpers::{find_child_by_type, get_node_text};
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract an import or export directive from an `import_or_export` node.
///
/// Returns a symbol with:
///   - name: the URI string with quotes stripped (e.g., "dart:async", "package:flutter/material.dart")
///   - kind: Import or Export depending on the child node type
///   - signature: the full directive text (e.g., "import 'dart:async';")
pub(super) fn extract_import_or_export(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    // Determine whether this is an import or export by checking child node kinds
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "library_import" => return extract_library_import(base, &child, parent_id),
            "library_export" => return extract_library_export(base, &child, parent_id),
            _ => {}
        }
    }
    None
}

/// Extract from a `library_import` node: the URI plus its `deferred`, `as`
/// alias, and `show`/`hide` combinators.
fn extract_library_import(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let uri = extract_uri_from_subtree(node)?;
    let full_text = get_node_text(node);
    let mut metadata = HashMap::from([(
        "type".to_string(),
        serde_json::Value::String("import".to_string()),
    )]);
    if let Some(specification) = find_child_by_type(node, "import_specification") {
        if find_child_by_type(&specification, "deferred").is_some() {
            metadata.insert("deferred".to_string(), serde_json::Value::Bool(true));
        }
        if let Some(alias) = specification.child_by_field_name("alias") {
            metadata.insert(
                "alias".to_string(),
                serde_json::Value::String(get_node_text(&alias)),
            );
        }
        let mut cursor = specification.walk();
        for combinator in specification
            .children(&mut cursor)
            .filter(|child| child.kind() == "combinator")
        {
            let Some(keyword) = combinator.child(0).map(|keyword| keyword.kind()) else {
                continue;
            };
            let names: Vec<serde_json::Value> = {
                let mut names_cursor = combinator.walk();
                combinator
                    .named_children(&mut names_cursor)
                    .filter(|name| name.kind() == "identifier")
                    .map(|name| serde_json::Value::String(get_node_text(&name)))
                    .collect()
            };
            if matches!(keyword, "show" | "hide")
                && let Some(existing) = metadata
                    .entry(keyword.to_string())
                    .or_insert_with(|| serde_json::Value::Array(Vec::new()))
                    .as_array_mut()
            {
                existing.extend(names);
            }
        }
    }

    Some(base.create_symbol(
        node,
        uri,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(full_text),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    ))
}

/// `part 'x.g.dart';` and `part of 'x.dart';` link a library to its parts.
pub(super) fn extract_part_directive(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let (name, directive) = if node.kind() == "part_of_directive" {
        let name = find_child_by_type(node, "uri")
            .and_then(|uri| string_literal_text(&uri))
            .or_else(|| {
                find_child_by_type(node, "dotted_identifier_list").map(|list| get_node_text(&list))
            })?;
        (name, "part_of")
    } else {
        (
            string_literal_text(&node.child_by_field_name("uri")?)?,
            "part",
        )
    };
    Some(base.create_symbol(
        node,
        name,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(get_node_text(node)),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(HashMap::from([(
                "type".to_string(),
                serde_json::Value::String(directive.to_string()),
            )])),
            doc_comment: None,
            annotations: Vec::new(),
        },
    ))
}

/// `library app.models;` names the library.
pub(super) fn extract_library_name(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = get_node_text(&find_child_by_type(node, "dotted_identifier_list")?);
    Some(base.create_symbol(
        node,
        name,
        SymbolKind::Namespace,
        SymbolOptions {
            signature: Some(get_node_text(node)),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(HashMap::from([(
                "type".to_string(),
                serde_json::Value::String("library".to_string()),
            )])),
            doc_comment: base.find_doc_comment(node),
            annotations: Vec::new(),
        },
    ))
}

/// Extract from a `library_export` node.
fn extract_library_export(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let uri = extract_uri_from_subtree(node)?;
    let full_text = get_node_text(node);

    Some(base.create_symbol(
        node,
        uri,
        SymbolKind::Export,
        SymbolOptions {
            signature: Some(full_text),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(HashMap::from([(
                "type".to_string(),
                serde_json::Value::String("export".to_string()),
            )])),
            doc_comment: None,
            annotations: Vec::new(),
        },
    ))
}

/// The URI string of an import or export, without quotes. An import keeps it
/// in the `uri` field of its `import_specification`: a `configurable_uri`,
/// or a bare `uri` when the import is `deferred`.
fn extract_uri_from_subtree(node: &Node) -> Option<String> {
    let holder = find_child_by_type(node, "import_specification").unwrap_or(*node);
    let uri = holder
        .child_by_field_name("uri")
        .or_else(|| find_child_by_type(&holder, "configurable_uri"))
        .or_else(|| find_child_by_type(&holder, "uri"))?;
    let uri = if uri.kind() == "configurable_uri" {
        find_child_by_type(&uri, "uri")?
    } else {
        uri
    };
    string_literal_text(&uri)
}

fn string_literal_text(uri: &Node) -> Option<String> {
    let string_literal = find_child_by_type(uri, "string_literal")?;
    let raw_text = get_node_text(&string_literal);
    let stripped = raw_text
        .trim_start_matches('\'')
        .trim_start_matches('"')
        .trim_end_matches('\'')
        .trim_end_matches('"');
    (!stripped.is_empty()).then(|| stripped.to_string())
}
