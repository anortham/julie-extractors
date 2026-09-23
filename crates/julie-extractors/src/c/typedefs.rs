//! Typedef extraction and post-processing for C code
//!
//! Handles extraction of type definitions, typedef name resolution,
//! function pointer typedef detection, and alignment attribute fixes.

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::c::CExtractor;
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

static STRUCT_ALIGN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"typedef\s+struct\s+(ALIGN\([^)]+\))").unwrap());

use super::helpers;
use super::signatures;

/// Extract one symbol per declarator of a `typedef`. A plain name typedef of a
/// struct, union, or enum body takes that kind; every other typedef is a `Type`.
pub(super) fn extract_type_definition(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let type_node = node.child_by_field_name("type");
    let underlying_type = type_node
        .map(|t| extractor.base.get_node_text(&t))
        .unwrap_or_default();
    let body_kind = type_node
        .filter(|t| t.child_by_field_name("body").is_some())
        .and_then(|t| match t.kind() {
            "struct_specifier" => Some(SymbolKind::Struct),
            "union_specifier" => Some(SymbolKind::Union),
            "enum_specifier" => Some(SymbolKind::Enum),
            _ => None,
        });
    let prefix = typedef_prefix(&extractor.base, node);
    let doc_comment = extractor.base.find_doc_comment(&node);

    let mut cursor = node.walk();
    let declarators: Vec<_> = node
        .children_by_field_name("declarator", &mut cursor)
        .collect();
    declarators
        .into_iter()
        .filter_map(|declarator| {
            let target = helpers::declarator_target(declarator)?;
            let name = extractor.base.get_node_text(&target.name);
            let kind = match &body_kind {
                Some(kind) if declarator.kind() == "type_identifier" => kind.clone(),
                _ => SymbolKind::Type,
            };
            let type_label = match kind {
                SymbolKind::Struct => "struct",
                SymbolKind::Union => "union",
                SymbolKind::Enum => "enum",
                _ => "typedef",
            };
            let is_struct = matches!(kind, SymbolKind::Struct | SymbolKind::Union);
            let signature = collapse_whitespace(&format!(
                "typedef {} {}",
                prefix,
                extractor.base.get_node_text(&declarator)
            ));
            Some(extractor.base.create_symbol(
                &node,
                name.clone(),
                kind,
                SymbolOptions {
                    signature: Some(signature),
                    visibility: Some(Visibility::Public),
                    parent_id: parent_id.map(|s| s.to_string()),
                    metadata: Some(HashMap::from([
                        ("type".to_string(), Value::String(type_label.to_string())),
                        ("name".to_string(), Value::String(name)),
                        (
                            "underlyingType".to_string(),
                            Value::String(underlying_type.clone()),
                        ),
                        ("isStruct".to_string(), Value::String(is_struct.to_string())),
                    ])),
                    doc_comment: doc_comment.clone(),
                    annotations: Vec::new(),
                },
            ))
        })
        .collect()
}

/// The typedef text between `typedef` and the first declarator, with any
/// struct, union, or enum body shortened to `{ ... }`.
fn typedef_prefix(base: &BaseExtractor, node: tree_sitter::Node) -> String {
    let declarators_start = node
        .child_by_field_name("declarator")
        .map_or(node.end_byte(), |d| d.start_byte());
    let mut parts = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.start_byte() >= declarators_start {
            break;
        }
        if !child.is_named() || child.kind() == "comment" {
            continue;
        }
        match child.child_by_field_name("body") {
            Some(body) => {
                let head = base.content[child.start_byte()..body.start_byte()].trim();
                parts.push(format!("{head} {{ ... }}"));
            }
            None => parts.push(base.get_node_text(&child)),
        }
    }
    parts.join(" ")
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract from expression statement (special case for typedef names)
pub(super) fn extract_from_expression_statement(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            let identifier_name = extractor.base.get_node_text(&child);

            if helpers::follows_detached_typedef_body(node) {
                let signature =
                    signatures::build_typedef_signature(&extractor.base, &node, &identifier_name);
                let doc_comment = extractor.base.find_doc_comment(&node);
                return Some(extractor.base.create_symbol(
                    &node,
                    identifier_name.clone(),
                    SymbolKind::Struct,
                    SymbolOptions {
                        signature: Some(signature),
                        visibility: Some(Visibility::Public),
                        parent_id: parent_id.map(|s| s.to_string()),
                        metadata: Some(HashMap::from([
                            ("type".to_string(), Value::String("struct".to_string())),
                            ("name".to_string(), Value::String(identifier_name)),
                            (
                                "fromExpressionStatement".to_string(),
                                Value::String("true".to_string()),
                            ),
                        ])),
                        doc_comment,
                        annotations: Vec::new(),
                    },
                ));
            }
        }
    }
    None
}

/// Fix struct alignment attributes in post-processing
pub(super) fn fix_struct_alignment_attributes(symbols: &mut [Symbol]) {
    for symbol in symbols.iter_mut() {
        if matches!(
            symbol.kind,
            SymbolKind::Type | SymbolKind::Struct | SymbolKind::Union
        ) && let Some(signature) = &symbol.signature
            && let Some(captures) = STRUCT_ALIGN_RE.captures(signature)
            && let Some(align_match) = captures.get(1)
        {
            let align_attr = align_match.as_str();
            if !signature.contains(&format!("struct {}", align_attr)) {
                let fixed_signature =
                    signature.replace("struct", &format!("struct {}", align_attr));
                symbol.signature = Some(fixed_signature);
            }
        }
    }
}
