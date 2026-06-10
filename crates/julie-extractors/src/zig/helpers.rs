use std::collections::HashSet;

use crate::base::{AnnotationMarker, BaseExtractor};
use tree_sitter::Node;

/// Helper methods for Zig extractor - visibility, context checking, and AST navigation
pub(super) fn is_public_function(base: &BaseExtractor, node: Node) -> bool {
    has_declaration_keyword(base, node, "pub")
}

pub(super) fn is_export_function(base: &BaseExtractor, node: Node) -> bool {
    has_declaration_keyword(base, node, "export")
}

pub(super) fn is_public_declaration(base: &BaseExtractor, node: Node) -> bool {
    has_declaration_keyword(base, node, "pub")
}

pub(super) fn is_inline_function(base: &BaseExtractor, node: Node) -> bool {
    has_declaration_keyword(base, node, "inline")
}

pub(super) fn is_inside_struct(node: Node) -> bool {
    // Walk up the tree to see if we're inside a struct declaration
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "struct_declaration" | "container_declaration" | "enum_declaration" => {
                return true;
            }
            _ => {
                current = parent.parent();
            }
        }
    }
    false
}

pub(super) fn extract_function_declaration_annotations(
    base: &BaseExtractor,
    node: Node,
) -> Vec<AnnotationMarker> {
    let mut markers = Vec::new();
    let mut seen = HashSet::new();

    if is_export_function(base, node) && seen.insert("export".to_string()) {
        markers.push(zig_annotation("export", "export"));
    }
    if is_inline_function(base, node) && seen.insert("inline".to_string()) {
        markers.push(zig_annotation("inline", "inline"));
    }
    if let Some(marker) = extern_convention_annotation(base, node)
        && seen.insert(marker.annotation_key.clone())
    {
        markers.push(marker);
    }

    markers
}

pub(super) fn extract_variable_declaration_annotations(
    base: &BaseExtractor,
    node: Node,
) -> Vec<AnnotationMarker> {
    let mut markers = Vec::new();
    let mut seen = HashSet::new();

    if is_export_declaration(base, node) && seen.insert("export".to_string()) {
        markers.push(zig_annotation("export", "export"));
    }
    if has_declaration_keyword(base, node, "threadlocal") && seen.insert("threadlocal".to_string())
    {
        markers.push(zig_annotation("threadlocal", "threadlocal"));
    }
    if has_declaration_keyword(base, node, "comptime") && seen.insert("comptime".to_string()) {
        markers.push(zig_annotation("comptime", "comptime"));
    }
    if let Some(marker) = align_annotation(base, node)
        && seen.insert(marker.annotation_key.clone())
    {
        markers.push(marker);
    }

    markers
}

fn is_export_declaration(base: &BaseExtractor, node: Node) -> bool {
    has_declaration_keyword(base, node, "export")
}

fn has_declaration_keyword(base: &BaseExtractor, node: Node, keyword: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == keyword || base.get_node_text(&child) == keyword {
            return true;
        }
    }

    if let Some(prev) = node.prev_sibling()
        && (prev.kind() == keyword || base.get_node_text(&prev) == keyword)
    {
        return true;
    }

    false
}

fn extern_convention_annotation(base: &BaseExtractor, node: Node) -> Option<AnnotationMarker> {
    base.find_child_by_type(&node, "extern")?;
    let linkage = base
        .find_child_by_type(&node, "string")
        .map(|string_node| base.get_node_text(&string_node))
        .filter(|text| !text.is_empty())?;
    let linkage = linkage.trim_matches('"');
    let raw = format!("extern {linkage}");
    Some(AnnotationMarker {
        annotation: raw.clone(),
        annotation_key: "extern".to_string(),
        raw_text: Some(raw),
        carrier: None,
    })
}

fn align_annotation(base: &BaseExtractor, node: Node) -> Option<AnnotationMarker> {
    let node_text = base.get_node_text(&node);
    let start = node_text.find("align(")?;
    let end = node_text[start..].find(')')? + start;
    let raw = node_text[start..=end].to_string();
    Some(AnnotationMarker {
        annotation: raw.clone(),
        annotation_key: "align".to_string(),
        raw_text: Some(raw),
        carrier: None,
    })
}

fn zig_annotation(annotation: &str, raw_text: &str) -> AnnotationMarker {
    AnnotationMarker {
        annotation: annotation.to_string(),
        annotation_key: annotation.to_ascii_lowercase(),
        raw_text: Some(raw_text.to_string()),
        carrier: None,
    }
}
