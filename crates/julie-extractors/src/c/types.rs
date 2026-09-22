//! Type and attribute extraction methods
//!
//! This module provides functionality for extracting type information from the syntax tree,
//! including return types, parameters, qualifiers, and attributes.

use super::helpers;
use crate::base::BaseExtractor;

/// Extract return type from a function definition
pub(super) fn extract_return_type(base: &BaseExtractor, node: tree_sitter::Node) -> String {
    let pointer_depth = helpers::function_declarator_target(node).map_or(0, |t| t.pointer_depth);
    return_type_with_pointer_depth(base, node, pointer_depth)
}

/// The declaration's base type followed by one `*` per pointer level of the declarator
pub(super) fn return_type_with_pointer_depth(
    base: &BaseExtractor,
    node: tree_sitter::Node,
    pointer_depth: usize,
) -> String {
    let mut cursor = node.walk();
    let base_types: Vec<String> = node
        .children(&mut cursor)
        .filter(|child| {
            matches!(
                child.kind(),
                "primitive_type"
                    | "type_identifier"
                    | "sized_type_specifier"
                    | "struct_specifier"
                    | "union_specifier"
                    | "enum_specifier"
            )
        })
        .map(|child| base.get_node_text(&child))
        .collect();
    let base_type = if base_types.is_empty() {
        "void".to_string()
    } else {
        base_types.join(" ")
    };
    format!("{}{}", base_type, "*".repeat(pointer_depth))
}

/// Extract storage class from a declaration (static, extern, etc.)
pub(super) fn extract_storage_class(
    base: &BaseExtractor,
    node: tree_sitter::Node,
) -> Option<String> {
    let mut storage_classes = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "storage_class_specifier" {
            storage_classes.push(base.get_node_text(&child));
        }
    }

    if storage_classes.is_empty() {
        None
    } else {
        Some(storage_classes.join(" "))
    }
}

/// Extract type qualifiers from a declaration (const, volatile, etc.)
pub(super) fn extract_type_qualifiers(
    base: &BaseExtractor,
    node: tree_sitter::Node,
) -> Option<String> {
    let mut qualifiers = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "type_qualifier" {
            qualifiers.push(base.get_node_text(&child));
        }
    }

    if qualifiers.is_empty() {
        None
    } else {
        Some(qualifiers.join(" "))
    }
}

/// Extract the data type of one declarator: the declaration's base type plus its pointer levels
pub(super) fn extract_variable_type(
    base: &BaseExtractor,
    node: tree_sitter::Node,
    declarator: tree_sitter::Node,
) -> String {
    let mut cursor = node.walk();
    let Some(base_type) = node
        .children(&mut cursor)
        .filter(|child| {
            matches!(
                child.kind(),
                "primitive_type"
                    | "type_identifier"
                    | "sized_type_specifier"
                    | "struct_specifier"
                    | "enum_specifier"
            )
        })
        .last()
        .map(|child| base.get_node_text(&child))
    else {
        return String::new();
    };
    let pointer_depth = helpers::declarator_target(declarator).map_or(0, |t| t.pointer_depth);
    format!("{}{}", base_type, "*".repeat(pointer_depth))
}

/// Extract array specifier information from a declarator
pub(super) fn extract_array_specifier(
    base: &BaseExtractor,
    declarator: tree_sitter::Node,
) -> Option<String> {
    if let Some(array_decl) = helpers::find_node_by_type(declarator, "array_declarator") {
        // Extract array size information
        let mut sizes = Vec::new();
        let mut cursor = array_decl.walk();
        let mut found_identifier = false;

        for child in array_decl.children(&mut cursor) {
            if child.kind() == "identifier" && !found_identifier {
                found_identifier = true;
                continue; // Skip the variable name
            }
            if child.kind() != "[" && child.kind() != "]" && found_identifier {
                sizes.push(base.get_node_text(&child));
            }
        }

        if sizes.is_empty() {
            Some("[]".to_string())
        } else {
            Some(format!("[{}]", sizes.join(", ")))
        }
    } else {
        None
    }
}

/// Extract initializer from an init_declarator node
pub(super) fn extract_initializer(
    base: &BaseExtractor,
    declarator: tree_sitter::Node,
) -> Option<String> {
    if declarator.kind() == "init_declarator" {
        // Look for initializer after '='
        let mut found_equals = false;
        let mut cursor = declarator.walk();

        for child in declarator.children(&mut cursor) {
            if base.get_node_text(&child) == "=" {
                found_equals = true;
            } else if found_equals {
                return Some(base.get_node_text(&child));
            }
        }
    }
    None
}

/// Extract struct attributes (PACKED, ALIGNED, etc.)
pub(super) fn extract_struct_attributes(
    base: &BaseExtractor,
    node: tree_sitter::Node,
) -> Vec<String> {
    let mut attributes = Vec::new();
    let node_text = base.get_node_text(&node);

    if node_text.contains("PACKED") {
        attributes.push("PACKED".to_string());
    }
    if node_text.contains("ALIGNED") {
        attributes.push("ALIGNED".to_string());
    }

    attributes
}

/// Extract alignment attributes (ALIGN macro, etc.)
pub(super) fn extract_alignment_attributes(
    base: &BaseExtractor,
    node: tree_sitter::Node,
) -> Vec<String> {
    let mut attributes = Vec::new();

    let node_text = base.get_node_text(&node);

    // Check for ALIGN(CACHE_LINE_SIZE) or similar patterns
    if let Some(align_start) = node_text.find("ALIGN(")
        && let Some(align_end) = node_text[align_start..].find(')')
    {
        let end_idx = align_start + align_end + 1;
        // SAFETY: Check char boundary before slicing to prevent UTF-8 panic
        if node_text.is_char_boundary(align_start) && node_text.is_char_boundary(end_idx) {
            let align_attr = &node_text[align_start..end_idx];
            attributes.push(align_attr.to_string());
        }
    }

    // Check parent node if this is a typedef struct
    if let Some(parent) = node.parent() {
        let parent_text = base.get_node_text(&parent);
        if let Some(align_start) = parent_text.find("ALIGN(")
            && let Some(align_end) = parent_text[align_start..].find(')')
        {
            let end_idx = align_start + align_end + 1;
            // SAFETY: Check char boundary before slicing to prevent UTF-8 panic
            if parent_text.is_char_boundary(align_start) && parent_text.is_char_boundary(end_idx) {
                let align_attr = &parent_text[align_start..end_idx];
                if !attributes.contains(&align_attr.to_string()) {
                    attributes.push(align_attr.to_string());
                }
            }
        }
    }

    attributes
}
