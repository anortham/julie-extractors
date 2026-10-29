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
    let mut metadata = import_metadata("import", "import", &uri);
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
    let (name, directive, form) = if node.kind() == "part_of_directive" {
        let name = find_child_by_type(node, "uri")
            .and_then(|uri| string_literal_text(&uri))
            .or_else(|| {
                find_child_by_type(node, "dotted_identifier_list").map(|list| get_node_text(&list))
            })?;
        (name, "part_of", "part of")
    } else {
        (
            string_literal_text(&node.child_by_field_name("uri")?)?,
            "part",
            "part",
        )
    };
    let metadata = import_metadata(directive, form, &name);
    Some(base.create_symbol(
        node,
        name,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(get_node_text(node)),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    ))
}

/// `library app.models;` names the library. An unnamed `library;` names
/// nothing, so it writes no symbol.
pub(super) fn extract_library_name(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = match node.kind() {
        "library_name" => get_node_text(&find_child_by_type(node, "dotted_identifier_list")?),
        _ => misparsed_library_name(node)?,
    };
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

/// tree-sitter-dart reads a one-segment `library blog;` that a declaration
/// follows as a `top_level_variable_declaration` of the variable `blog` with
/// the type `library`. Dart forbids a built-in identifier as a type, so that
/// shape is always the library directive. Returns the library name.
pub(super) fn misparsed_library_name(node: &Node) -> Option<String> {
    if node.kind() != "top_level_variable_declaration"
        || get_node_text(&find_child_by_type(node, "type")?) != "library"
    {
        return None;
    }
    let list = find_child_by_type(node, "initialized_identifier_list")?;
    let mut cursor = list.walk();
    let declarators: Vec<Node> = list.named_children(&mut cursor).collect();
    let [declarator] = declarators.as_slice() else {
        return None;
    };
    if declarator.kind() != "initialized_identifier" || declarator.named_child_count() != 1 {
        return None;
    }
    Some(get_node_text(&declarator.child_by_field_name("name")?))
}

/// Extract from a `library_export` node.
fn extract_library_export(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let uri = extract_uri_from_subtree(node)?;
    let full_text = get_node_text(node);
    let metadata = import_metadata("export", "export", &uri);

    Some(base.create_symbol(
        node,
        uri,
        SymbolKind::Export,
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

/// Metadata shared by every directive: its `type`, the directive
/// keyword as `form`, and the URI or library name as `source`.
fn import_metadata(
    directive_type: &str,
    form: &str,
    source: &str,
) -> HashMap<String, serde_json::Value> {
    HashMap::from([
        (
            "type".to_string(),
            serde_json::Value::String(directive_type.to_string()),
        ),
        (
            "form".to_string(),
            serde_json::Value::String(form.to_string()),
        ),
        (
            "source".to_string(),
            serde_json::Value::String(source.to_string()),
        ),
    ])
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
