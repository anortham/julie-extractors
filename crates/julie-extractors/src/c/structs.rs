//! Struct, union, and enum extraction for C code
//!
//! Handles extraction of struct definitions, union definitions, enum definitions,
//! and individual enum value symbols.

use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::c::CExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::HashMap;

use super::helpers;
use super::signatures;
use super::type_facts;

/// Extract a struct definition; a reference without a body declares nothing
pub(super) fn extract_struct(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    node.child_by_field_name("body")?;
    let struct_name = helpers::extract_struct_name(&extractor.base, node)?;
    let signature = signatures::build_struct_signature(&extractor.base, node);
    let annotations = helpers::child_attributes(&extractor.base, node);

    let doc_comment = extractor.base.find_doc_comment(&node);

    Some(extractor.base.create_symbol(
        &node,
        struct_name,
        SymbolKind::Struct,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: None,
            doc_comment,
            annotations,
        },
    ))
}

/// Extract a union definition
pub(super) fn extract_union(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    node.child_by_field_name("body")?;
    let union_name = helpers::extract_union_name(&extractor.base, node)?;
    let signature = signatures::build_union_signature(&extractor.base, node);
    let annotations = helpers::child_attributes(&extractor.base, node);

    let doc_comment = extractor.base.find_doc_comment(&node);

    Some(extractor.base.create_symbol(
        &node,
        union_name,
        SymbolKind::Union,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: None,
            doc_comment,
            annotations,
        },
    ))
}

/// Extract an enum definition
pub(super) fn extract_enum(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    node.child_by_field_name("body")?;
    let enum_name = helpers::extract_enum_name(&extractor.base, node)?;
    let signature = signatures::build_enum_signature(&extractor.base, node);

    let doc_comment = extractor.base.find_doc_comment(&node);

    Some(extractor.base.create_symbol(
        &node,
        enum_name,
        SymbolKind::Enum,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: None,
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Extract struct/union field symbols as `SymbolKind::Field` children. A C11
/// anonymous member (`union { long as_int; double as_float; };`) adds its fields
/// to the enclosing record; the fields of an unnamed record type behind a named
/// field (`struct { int line; } pos;`) belong to that field.
pub(super) fn extract_struct_field_symbols(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    parent_struct_id: &str,
) -> Vec<Symbol> {
    let mut field_symbols = Vec::new();
    collect_field_symbols(extractor, node, parent_struct_id, &mut field_symbols, 0);
    field_symbols
}

fn collect_field_symbols(
    extractor: &mut CExtractor,
    record: tree_sitter::Node,
    owner_id: &str,
    field_symbols: &mut Vec<Symbol>,
    depth: u32,
) {
    let (Some(body), Some(child_depth)) = (
        record.child_by_field_name("body"),
        child_tree_depth(depth).filter(|_| should_visit_tree_depth(depth)),
    ) else {
        return;
    };

    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if child.kind() != "field_declaration" {
            continue;
        }
        let anonymous_record = child.child_by_field_name("type").filter(|record| {
            matches!(record.kind(), "struct_specifier" | "union_specifier")
                && record.child_by_field_name("name").is_none()
        });

        let mut decl_cursor = child.walk();
        let declarators: Vec<_> = child
            .children_by_field_name("declarator", &mut decl_cursor)
            .collect();
        if declarators.is_empty()
            && let Some(record) = anonymous_record
        {
            collect_field_symbols(extractor, record, owner_id, field_symbols, child_depth);
            continue;
        }
        for declarator in declarators {
            let Some(field_id) =
                push_field_symbol(extractor, child, declarator, owner_id, field_symbols)
            else {
                continue;
            };
            if let Some(record) = anonymous_record {
                collect_field_symbols(extractor, record, &field_id, field_symbols, child_depth);
            }
        }
    }
}

#[inline(never)]
fn push_field_symbol(
    extractor: &mut CExtractor,
    declaration: tree_sitter::Node,
    declarator: tree_sitter::Node,
    owner_id: &str,
    field_symbols: &mut Vec<Symbol>,
) -> Option<String> {
    let field_name = helpers::find_field_identifier_name(&extractor.base, declarator)?;
    let field_type = declaration
        .child_by_field_name("type")
        .map(|t| extractor.base.get_node_text(&t))
        .unwrap_or_default();
    let signature = format!(
        "{} {}",
        field_type,
        extractor.base.get_node_text(&declarator)
    );
    let doc_comment = extractor.base.find_doc_comment(&declaration);
    let annotations = helpers::child_attributes(&extractor.base, declaration);

    let field_symbol = extractor.base.create_symbol(
        &declarator,
        field_name,
        SymbolKind::Field,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: Some(owner_id.to_string()),
            metadata: Some(HashMap::from([(
                "fieldType".to_string(),
                Value::String(field_type),
            )])),
            doc_comment,
            annotations,
        },
    );

    type_facts::record_declared_from_declaration(
        &mut extractor.base,
        &field_symbol.id,
        declaration,
        declarator,
    );
    let field_id = field_symbol.id.clone();
    field_symbols.push(field_symbol);
    Some(field_id)
}

/// Extract enum value symbols
pub(super) fn extract_enum_value_symbols(
    extractor: &mut CExtractor,
    node: tree_sitter::Node,
    parent_enum_id: Option<&str>,
) -> Vec<Symbol> {
    let mut enum_value_symbols = Vec::new();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "enumerator_list" {
            let mut enum_cursor = child.walk();
            for enum_child in child.children(&mut enum_cursor) {
                if enum_child.kind() == "enumerator"
                    && let Some(name_node) = enum_child.child_by_field_name("name")
                {
                    let name = extractor.base.get_node_text(&name_node);
                    let value = enum_child
                        .child_by_field_name("value")
                        .map(|v| extractor.base.get_node_text(&v));

                    let mut signature = name.clone();
                    if let Some(ref val) = value {
                        signature = format!("{} = {}", signature, val);
                    }

                    let doc_comment = extractor.base.find_doc_comment(&enum_child);

                    let enum_value_symbol = extractor.base.create_symbol(
                        &enum_child,
                        name,
                        SymbolKind::Constant,
                        SymbolOptions {
                            signature: Some(signature),
                            visibility: Some(Visibility::Public),
                            parent_id: parent_enum_id.map(str::to_string),
                            metadata: if value.is_some() {
                                Some(HashMap::from([(
                                    "value".to_string(),
                                    Value::String(value.unwrap_or_default()),
                                )]))
                            } else {
                                None
                            },
                            doc_comment,
                            annotations: Vec::new(),
                        },
                    );

                    enum_value_symbols.push(enum_value_symbol);
                }
            }
        }
    }

    enum_value_symbols
}
