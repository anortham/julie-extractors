//! Visibility/access specifier logic for C++ symbols
//! Determines visibility (public/private/protected) by walking access_specifier nodes
//! in class/struct/union bodies.

use crate::base::{BaseExtractor, Visibility};
use tree_sitter::Node;

/// Extract visibility for any C++ member (field, method, constructor, etc.)
/// This is the main public interface for visibility extraction.
///
/// C++ visibility is determined by the most recent `access_specifier` node
/// (public:/private:/protected:) preceding the target node within a
/// class/struct/union body. Defaults: class -> private, struct/union -> public.
pub(super) fn extract_cpp_visibility(base: &BaseExtractor, node: Node) -> Visibility {
    extract_field_visibility(base, node)
}

/// Alias used internally by declarations for variable/field nodes
pub(super) fn extract_visibility_from_node(base: &BaseExtractor, node: Node) -> Visibility {
    extract_field_visibility(base, node)
}

fn extract_field_visibility(base: &BaseExtractor, node: Node) -> Visibility {
    // C++ visibility is determined by the most recent access_spec node in the class body
    // Walk up to find the parent class/struct, then walk through siblings backwards

    // First, find the parent class/struct/union specifier
    let parent = find_parent_class_or_struct(node);

    match parent {
        Some((parent_kind, parent_node)) => {
            // Determine default visibility based on parent type
            let default_visibility = if parent_kind == "class_specifier" {
                Visibility::Private // class defaults to private
            } else {
                Visibility::Public // struct and union default to public
            };

            // Find the field_list body
            let field_list = parent_node
                .children(&mut parent_node.walk())
                .find(|c| c.kind() == "field_declaration_list");

            if let Some(field_list) = field_list {
                // Walk through field_list children to find the most recent access_spec before our node
                find_access_spec_before_node(base, field_list, node, default_visibility)
            } else {
                default_visibility
            }
        }
        None => Visibility::Public, // Not inside a class/struct, assume public
    }
}

/// Find the parent class_specifier or struct_specifier node
fn find_parent_class_or_struct(mut node: Node) -> Option<(&'static str, Node)> {
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "class_specifier" => return Some(("class_specifier", parent)),
            "struct_specifier" => return Some(("struct_specifier", parent)),
            "union_specifier" => return Some(("union_specifier", parent)),
            _ => node = parent,
        }
    }
    None
}

/// Find the most recent access_spec before the target node
fn find_access_spec_before_node(
    base: &BaseExtractor,
    field_list: Node,
    target: Node,
    default_visibility: Visibility,
) -> Visibility {
    let target_start = target.start_position();
    let mut current_visibility = default_visibility;

    // Walk through all children of field_list
    for child in field_list.children(&mut field_list.walk()) {
        let child_pos = child.start_position();

        // If we've passed the target node, return the last visibility we saw
        if child_pos >= target_start {
            break;
        }

        // Check if this is an access_specifier node (private, protected, public keywords)
        // Note: tree-sitter uses "access_specifier" not "access_spec"
        if child.kind() == "access_specifier" {
            current_visibility = access_label_visibility(&base.get_node_text(&child));
        }
    }

    current_visibility
}

/// The Qt pre-pass rewrites a bare `Q_SIGNALS:` label into `public:`, and keeps
/// the access word of `public Q_SLOTS:`, so the node text starts with the access
/// word or with the macro. The grammar admits no other access label, and
/// everything after a signals label is public in Qt.
fn access_label_visibility(text: &str) -> Visibility {
    let text = text.trim_start();
    let end = text
        .find(|character: char| !(character.is_alphanumeric() || character == '_'))
        .unwrap_or(text.len());
    match &text[..end] {
        "private" => Visibility::Private,
        "protected" => Visibility::Protected,
        _ => Visibility::Public,
    }
}
