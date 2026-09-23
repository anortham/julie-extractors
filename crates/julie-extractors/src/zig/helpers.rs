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
            "struct_declaration" | "union_declaration" | "enum_declaration"
            | "opaque_declaration" => {
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
    if has_declaration_keyword(base, node, "noinline") && seen.insert("noinline".to_string()) {
        markers.push(zig_annotation("noinline", "noinline"));
    }
    if let Some(marker) = extern_convention_annotation(base, node)
        && seen.insert(marker.annotation_key.clone())
    {
        markers.push(marker);
    }
    if let Some(convention) = base.find_child_by_type(&node, "calling_convention") {
        let raw = base.get_node_text(&convention);
        markers.push(AnnotationMarker {
            annotation: raw.clone(),
            annotation_key: "callconv".to_string(),
            raw_text: Some(raw),
            carrier: None,
        });
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
    let raw = match base
        .find_child_by_type(&node, "string")
        .map(|string_node| base.get_node_text(&string_node))
        .filter(|text| !text.is_empty())
    {
        Some(linkage) => format!("extern {}", linkage.trim_matches('"')),
        None => "extern".to_string(),
    };
    Some(AnnotationMarker {
        annotation: raw.clone(),
        annotation_key: "extern".to_string(),
        raw_text: Some(raw),
        carrier: None,
    })
}

/// The declaration's own `align(N)`; an alignment inside the value
/// (`[]align(a) T`) belongs to that type, not to the declaration.
fn align_annotation(base: &BaseExtractor, node: Node) -> Option<AnnotationMarker> {
    let raw = base.get_node_text(&base.find_child_by_type(&node, "byte_alignment")?);
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

/// tree-sitter-zig parses a prefix `!x` in expression position as an
/// `error_union_type` with no `error` field, bound to the first operand only
/// (`!self.isOpen()` is `(!self).isOpen()`). Outside a type slot that node is a
/// logical not, never a type.
pub(super) fn is_logical_not(node: Node) -> bool {
    if node.kind() != "error_union_type" || node.child_by_field_name("error").is_some() {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    let in_type_field = parent
        .child_by_field_name("type")
        .is_some_and(|type_node| type_node.id() == node.id());
    !in_type_field
        && !matches!(
            parent.kind(),
            "pointer_type"
                | "nullable_type"
                | "optional_type"
                | "slice_type"
                | "array_type"
                | "error_union_type"
        )
}

/// The operand of a logical not (see [`is_logical_not`]), or the node itself.
pub(super) fn unwrap_logical_not(mut node: Node) -> Node {
    while is_logical_not(node) {
        match node.child_by_field_name("ok") {
            Some(operand) => node = operand,
            None => break,
        }
    }
    node
}

/// Zig doc comments: the `///` lines directly above a declaration. A container
/// declaration without them takes the `//!` lines that open its member list.
pub(crate) fn find_zig_doc_comment(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut lines = Vec::new();
    let mut current = node.prev_sibling();
    let mut next_row = node.start_position().row;
    while let Some(comment) = current.filter(|comment| comment.kind() == "comment") {
        let text = base.get_node_text(&comment);
        if !text.trim_start().starts_with("///")
            || text.trim_start().starts_with("////")
            || comment.end_position().row + 1 < next_row
        {
            break;
        }
        lines.push(text);
        next_row = comment.start_position().row;
        current = comment.prev_sibling();
    }
    if lines.is_empty() {
        return container_doc_comment(base, node);
    }
    lines.reverse();
    Some(lines.join("\n"))
}

fn container_doc_comment(base: &BaseExtractor, node: Node) -> Option<String> {
    if node.kind() != "variable_declaration" {
        return None;
    }
    let container = super::type_facts::initializer_node(node).filter(|value| {
        matches!(
            value.kind(),
            "struct_declaration" | "union_declaration" | "enum_declaration" | "opaque_declaration"
        )
    })?;
    let lines: Vec<String> = container
        .children(&mut container.walk())
        .skip_while(|child| child.kind() != "{")
        .skip(1)
        .take_while(|child| child.kind() == "comment")
        .map(|comment| base.get_node_text(&comment))
        .take_while(|text| text.trim_start().starts_with("//!"))
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}
