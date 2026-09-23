// C# Helper Methods
//
// Collection of utility functions for parsing C# AST nodes and extracting metadata

use crate::base::{
    AnnotationMarker, BaseExtractor, BodySpan, NormalizedSpan, Visibility, normalize_annotations,
};
use tree_sitter::Node;

/// Extract modifiers from a node (attributes and modifiers)
pub fn extract_modifiers(base: &BaseExtractor, node: &Node) -> Vec<String> {
    let mut attributes = Vec::new();
    let mut modifiers = Vec::new();

    let mut cursor = node.walk();

    // Extract attributes
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute_list" {
            attributes.push(base.get_node_text(&child));
        }
    }

    // Extract modifiers
    for child in node.children(&mut cursor) {
        if child.kind() == "modifier" {
            modifiers.push(base.get_node_text(&child));
        }
    }

    // Combine attributes and modifiers
    [attributes, modifiers].concat()
}

/// Extract canonical attribute markers from C# attribute lists. A targeted
/// list (`[return: NotNull]`) carries its target (`return`) as the carrier.
pub fn extract_annotations(base: &BaseExtractor, node: &Node) -> Vec<AnnotationMarker> {
    let mut markers: Vec<AnnotationMarker> = Vec::new();
    let mut cursor = node.walk();
    for list in node
        .children(&mut cursor)
        .filter(|child| child.kind() == "attribute_list")
    {
        let mut list_cursor = list.walk();
        let target = list
            .named_children(&mut list_cursor)
            .find(|child| child.kind() == "attribute_target_specifier")
            .map(|specifier| {
                base.get_node_text(&specifier)
                    .trim_end_matches(':')
                    .trim()
                    .to_string()
            });
        let mut list_cursor = list.walk();
        let attributes: Vec<String> = list
            .named_children(&mut list_cursor)
            .filter(|child| child.kind() == "attribute")
            .map(|attribute| base.get_node_text(&attribute))
            .collect();
        for mut marker in normalize_annotations(&attributes, "csharp") {
            if markers
                .iter()
                .any(|existing| existing.annotation_key == marker.annotation_key)
            {
                continue;
            }
            marker.carrier = target.clone();
            markers.push(marker);
        }
    }
    markers
}

/// Determine visibility from modifiers, falling back to the C# default for
/// the declaration's position: interface and enum members are public,
/// namespace-level types are internal, and every other member is private.
pub fn determine_visibility(modifiers: &[String], node: &Node) -> Visibility {
    crate::base::visibility::visibility_from_modifiers_with_default(
        modifiers,
        default_visibility(node),
    )
}

fn default_visibility(node: &Node) -> Visibility {
    let container = node
        .parent()
        .filter(|parent| {
            matches!(
                parent.kind(),
                "declaration_list" | "enum_member_declaration_list"
            )
        })
        .and_then(|list| list.parent())
        .or_else(|| node.parent());
    match container.map(|container| container.kind()) {
        Some("interface_declaration" | "enum_declaration" | "enum_member_declaration_list") => {
            Visibility::Public
        }
        Some(
            "compilation_unit"
            | "namespace_declaration"
            | "file_scoped_namespace_declaration"
            | "global_statement",
        ) if is_type_declaration(node) => Visibility::Internal,
        _ => Visibility::Private,
    }
}

fn is_type_declaration(node: &Node) -> bool {
    matches!(
        node.kind(),
        "class_declaration"
            | "interface_declaration"
            | "struct_declaration"
            | "record_declaration"
            | "record_struct_declaration"
            | "enum_declaration"
            | "delegate_declaration"
    )
}

/// Get C# visibility string including internal
pub fn get_csharp_visibility_string(modifiers: &[String], visibility: &Visibility) -> String {
    if modifiers.contains(&"public".to_string()) {
        "public".to_string()
    } else if modifiers.contains(&"private".to_string()) {
        "private".to_string()
    } else if modifiers.contains(&"protected".to_string()) {
        "protected".to_string()
    } else if modifiers.contains(&"internal".to_string()) {
        "internal".to_string()
    } else {
        visibility.as_storage_str().to_string()
    }
}

/// Extract base list (inheritance/implementation classes and interfaces)
pub fn extract_base_list(base: &BaseExtractor, node: &Node) -> Vec<String> {
    let mut cursor = node.walk();
    let base_list = node.children(&mut cursor).find(|c| c.kind() == "base_list");

    if let Some(base_list) = base_list {
        let mut base_cursor = base_list.walk();
        base_list
            .children(&mut base_cursor)
            .filter(|c| c.is_named() && c.kind() != "argument_list")
            .map(|c| base.get_node_text(&c))
            .collect()
    } else {
        Vec::new()
    }
}

/// Extract type parameters (generic type parameters like <T, U>)
pub fn extract_type_parameters(base: &BaseExtractor, node: &Node) -> Option<String> {
    let mut cursor = node.walk();
    let type_params = node
        .children(&mut cursor)
        .find(|c| c.kind() == "type_parameter_list");
    type_params.map(|tp| base.get_node_text(&tp))
}

/// The written return type of a method or local function.
pub fn extract_return_type(base: &BaseExtractor, node: &Node) -> Option<String> {
    node.child_by_field_name("returns")
        .or_else(|| node.child_by_field_name("type"))
        .map(|type_node| base.get_node_text(&type_node))
}

/// The written type of a property declaration.
pub fn extract_property_type(base: &BaseExtractor, node: &Node) -> Option<String> {
    node.child_by_field_name("type")
        .map(|type_node| base.get_node_text(&type_node))
}

/// The written type of a field declaration's variable declaration.
pub fn extract_field_type(base: &BaseExtractor, node: &Node) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|c| c.kind() == "variable_declaration")?
        .child_by_field_name("type")
        .map(|type_node| base.get_node_text(&type_node))
}

/// The body span of a C# declaration node: the block, arrow clause, accessor
/// list, or member list. Abstract, extern, interface, and partial-definition
/// members, delegates, fields, locals, and bodyless records have none.
pub(super) fn body_span(node: &Node, _content: &str) -> Option<BodySpan> {
    let body = match node.kind() {
        "namespace_declaration"
        | "class_declaration"
        | "interface_declaration"
        | "struct_declaration"
        | "record_declaration"
        | "record_struct_declaration"
        | "enum_declaration"
        | "method_declaration"
        | "constructor_declaration"
        | "destructor_declaration"
        | "operator_declaration"
        | "conversion_operator_declaration"
        | "local_function_statement"
        | "lambda_expression"
        | "anonymous_method_expression"
        | "accessor_declaration"
        | "extension_declaration" => node.child_by_field_name("body").or_else(|| {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .find(|child| child.kind() == "arrow_expression_clause")
        }),
        "property_declaration" | "indexer_declaration" | "event_declaration" => node
            .child_by_field_name("accessors")
            .or_else(|| node.child_by_field_name("value")),
        _ => None,
    }?;
    Some(NormalizedSpan::from_node(&body))
}
