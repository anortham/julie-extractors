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

/// Extract canonical attribute markers from C# attribute lists.
pub fn extract_annotations(base: &BaseExtractor, node: &Node) -> Vec<AnnotationMarker> {
    let raw_attributes: Vec<String> = node
        .children(&mut node.walk())
        .filter(|child| child.kind() == "attribute_list")
        .map(|child| base.get_node_text(&child))
        .collect();

    normalize_annotations(&raw_attributes, "csharp")
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

/// Extract return type from a method node
pub fn extract_return_type(base: &BaseExtractor, node: &Node) -> Option<String> {
    // Find method name identifier - comes before parameter_list (may have type_parameter_list in between)
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let param_list_index = children.iter().position(|c| c.kind() == "parameter_list")?;

    // Look backwards from parameter_list to find the method name identifier
    let name_node = children[..param_list_index]
        .iter()
        .rev()
        .find(|c| c.kind() == "identifier")?;

    let name_index = children.iter().position(|c| std::ptr::eq(c, name_node))?;
    // Look for return type, but exclude modifiers
    let return_type_node = children[..name_index].iter().find(|c| {
        matches!(
            c.kind(),
            "predefined_type"
                | "identifier"
                | "qualified_name"
                | "generic_name"
                | "array_type"
                | "nullable_type"
                | "tuple_type"
        )
    });

    return_type_node.map(|node| base.get_node_text(node))
}

/// Extract property type from a property declaration
pub fn extract_property_type(base: &BaseExtractor, node: &Node) -> Option<String> {
    // In C# property declarations, the type is typically the first significant node
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();

    // Skip modifiers and find the type node
    let modifiers = [
        "public",
        "private",
        "protected",
        "internal",
        "static",
        "virtual",
        "override",
        "abstract",
    ];

    for child in &children {
        let child_text = base.get_node_text(child);

        // Skip modifier nodes
        if modifiers.contains(&child_text.as_str()) {
            continue;
        }

        // Look for type nodes
        if matches!(
            child.kind(),
            "predefined_type"
                | "identifier"
                | "qualified_name"
                | "generic_name"
                | "array_type"
                | "nullable_type"
                | "tuple_type"
        ) {
            return Some(child_text);
        }
    }

    None
}

/// Extract field type from a field declaration
pub fn extract_field_type(base: &BaseExtractor, node: &Node) -> Option<String> {
    // Field type is the first child of variable_declaration
    let mut cursor = node.walk();
    let var_declaration = node
        .children(&mut cursor)
        .find(|c| c.kind() == "variable_declaration")?;

    let mut var_cursor = var_declaration.walk();
    let type_node = var_declaration.children(&mut var_cursor).find(|c| {
        matches!(
            c.kind(),
            "predefined_type"
                | "identifier"
                | "qualified_name"
                | "generic_name"
                | "array_type"
                | "nullable_type"
        )
    });

    type_node.map(|node| base.get_node_text(&node))
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
