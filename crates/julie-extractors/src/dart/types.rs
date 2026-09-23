// Dart Extractor - Types Extraction
//
// Methods for extracting type aliases, enums, mixins, and extensions

use super::helpers::*;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract enum definition
pub(super) fn extract_enum(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = find_child_by_type(node, "identifier")?;
    let name = get_node_text(&name_node);
    let annotations = extract_annotation_markers(node);

    let symbol = base.create_symbol(
        node,
        name.clone(),
        SymbolKind::Enum,
        SymbolOptions {
            signature: Some(format!("enum {}", name)),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|id| id.to_string()),
            metadata: Some(HashMap::new()),
            doc_comment: base.find_doc_comment(node),
            annotations,
        },
    );

    Some(symbol)
}

/// Extract enum constant
pub(super) fn extract_enum_constant(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    if node.kind() != "enum_constant" {
        return None;
    }

    let name_node = find_child_by_type(node, "identifier")?;
    let constant_name = get_node_text(&name_node);
    let annotations = extract_annotation_markers(node);

    // Check if there are arguments (enhanced enum)
    let argument_part = find_child_by_type(node, "argument_part");
    let signature = if let Some(arg_node) = argument_part {
        format!("{}{}", constant_name, get_node_text(&arg_node))
    } else {
        constant_name.clone()
    };

    let symbol = base.create_symbol(
        node,
        constant_name,
        SymbolKind::EnumMember,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|id| id.to_string()),
            metadata: Some(HashMap::new()),
            doc_comment: base.find_doc_comment(node),
            annotations,
        },
    );

    Some(symbol)
}

/// Extract mixin definition
pub(super) fn extract_mixin(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = find_child_by_type(node, "identifier")?;
    let name = get_node_text(&name_node);
    let annotations = extract_annotation_markers(node);

    let source = get_node_text(node);
    let has_on_constraint = source.contains(" on ");
    let type_node =
        find_child_by_type(node, "type").or_else(|| find_child_by_type(node, "type_identifier"));

    let signature = if let (true, Some(type_n)) = (has_on_constraint, type_node) {
        let constraint_type = get_node_text(&type_n);
        format!("mixin {} on {}", name, constraint_type)
    } else {
        format!("mixin {}", name)
    };

    let constraint_type_name = if has_on_constraint {
        type_node.map(|n| get_node_text(&n))
    } else {
        None
    };

    let mut symbol = base.create_symbol(
        node,
        name,
        SymbolKind::Interface,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|id| id.to_string()),
            metadata: Some(HashMap::new()),
            doc_comment: base.find_doc_comment(node),
            annotations,
        },
    );

    // Add metadata
    symbol
        .metadata
        .get_or_insert_with(HashMap::new)
        .insert("isMixin".to_string(), serde_json::Value::Bool(true));
    if let Some(constraint_type) = constraint_type_name {
        symbol.metadata.get_or_insert_with(HashMap::new).insert(
            "constraintType".to_string(),
            serde_json::Value::String(constraint_type),
        );
    }

    Some(symbol)
}

/// Extract extension definition. An unnamed `extension on T` is named
/// `<extension on T>`, since its members still need a parent.
pub(super) fn extract_extension(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let annotations = extract_annotation_markers(node);

    let has_on_clause = find_child_by_type(node, "on").is_some();
    let type_node = node
        .child_by_field_name("class")
        .or_else(|| find_child_by_type(node, "type"))
        .or_else(|| find_child_by_type(node, "type_identifier"));
    let name = match node
        .child_by_field_name("name")
        .or_else(|| find_child_by_type(node, "identifier"))
    {
        Some(name_node) => get_node_text(&name_node),
        None => format!("<extension on {}>", get_node_text(&type_node?)),
    };

    let signature = match (has_on_clause, type_node) {
        (true, Some(type_n)) if name.starts_with('<') => {
            format!("extension on {}", get_node_text(&type_n))
        }
        (true, Some(type_n)) => format!("extension {} on {}", name, get_node_text(&type_n)),
        _ => format!("extension {}", name),
    };

    let extended_type_name = if has_on_clause {
        type_node.map(|n| get_node_text(&n))
    } else {
        None
    };

    let mut symbol = base.create_symbol(
        node,
        name,
        SymbolKind::Module,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|id| id.to_string()),
            metadata: Some(HashMap::new()),
            doc_comment: base.find_doc_comment(node),
            annotations,
        },
    );

    // Add metadata
    symbol
        .metadata
        .get_or_insert_with(HashMap::new)
        .insert("isExtension".to_string(), serde_json::Value::Bool(true));
    if let Some(extended_type) = extended_type_name {
        symbol.metadata.get_or_insert_with(HashMap::new).insert(
            "extendedType".to_string(),
            serde_json::Value::String(extended_type),
        );
    }

    Some(symbol)
}

/// `extension type const Id(int value) implements Object`: a class-like
/// wrapper over its representation type.
pub(super) fn extract_extension_type(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = get_node_text(&name_node);
    let header_end = node
        .child_by_field_name("body")
        .map_or(node.end_byte(), |body| body.start_byte());
    let signature = base.content[node.start_byte()..header_end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let representation_type = node
        .child_by_field_name("representation")
        .and_then(|representation| representation.child_by_field_name("type"))
        .map(|type_node| get_node_text(&type_node));
    let mut metadata =
        HashMap::from([("isExtensionType".to_string(), serde_json::Value::Bool(true))]);
    if let Some(representation_type) = representation_type {
        metadata.insert(
            "representationType".to_string(),
            serde_json::Value::String(representation_type),
        );
    }
    Some(base.create_symbol(
        node,
        name,
        SymbolKind::Class,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|id| id.to_string()),
            metadata: Some(metadata),
            doc_comment: base.find_doc_comment(node),
            annotations: extract_annotation_markers(node),
        },
    ))
}

/// The `(int value)` representation of an extension type is its one field.
pub(super) fn extract_representation_field(
    base: &mut BaseExtractor,
    node: &Node,
    extension_type_id: &str,
) -> Option<Symbol> {
    let representation = node.child_by_field_name("representation")?;
    let name_node = representation.child_by_field_name("name")?;
    let type_node = representation.child_by_field_name("type");
    let mut symbol = base.create_symbol(
        &representation,
        get_node_text(&name_node),
        SymbolKind::Field,
        SymbolOptions {
            signature: Some(
                get_node_text(&representation)
                    .trim_start_matches('(')
                    .trim_end_matches(')')
                    .trim()
                    .to_string(),
            ),
            visibility: Some(Visibility::Public),
            parent_id: Some(extension_type_id.to_string()),
            metadata: Some(HashMap::from([(
                "isFinal".to_string(),
                serde_json::Value::Bool(true),
            )])),
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    symbol.body_span = None;
    symbol.body_hash = None;
    if let Some(type_node) = type_node {
        super::type_facts::record_declared_type(base, &symbol.id, type_node);
    }
    Some(symbol)
}

/// Extract type alias (typedef)
pub(super) fn extract_typedef(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    if node.kind() != "type_alias" {
        return None;
    }

    // Get the typedef name
    let name_node = find_child_by_type(node, "type_identifier")?;
    let name = get_node_text(&name_node);
    let is_private = name.starts_with('_');
    let annotations = extract_annotation_markers(node);

    // Build signature with typedef keyword and generic parameters
    let type_params_node = find_child_by_type(node, "type_parameters");
    let type_params = type_params_node
        .map(|n| get_node_text(&n))
        .unwrap_or_default();

    // Get the type being aliased (everything after =)
    let mut aliased_type = String::new();
    let mut cursor = node.walk();
    let mut found_equals = false;

    for child in node.children(&mut cursor) {
        if child.kind() == "=" {
            found_equals = true;
            continue;
        }
        if found_equals && child.kind() != ";" {
            aliased_type.push_str(&get_node_text(&child));
        }
    }

    let signature = if found_equals {
        format!("typedef {}{} = {}", name, type_params, aliased_type.trim())
    } else {
        let return_type = find_child_by_type(node, "type")
            .map(|type_node| get_node_text(&type_node))
            .unwrap_or_else(|| "dynamic".to_string());
        let parameters = find_child_by_type(node, "formal_parameter_list")
            .map(|list| get_node_text(&list))
            .unwrap_or_else(|| "()".to_string());
        aliased_type = format!("{return_type} Function{parameters}");
        get_node_text(node)
            .trim_end_matches(';')
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };

    let mut symbol = base.create_symbol(
        node,
        name,
        SymbolKind::Type,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(if is_private {
                Visibility::Private
            } else {
                Visibility::Public
            }),
            parent_id: parent_id.map(|id| id.to_string()),
            metadata: Some(HashMap::new()),
            doc_comment: base.find_doc_comment(node),
            annotations,
        },
    );

    // Add metadata
    symbol
        .metadata
        .get_or_insert_with(HashMap::new)
        .insert("isTypedef".to_string(), serde_json::Value::Bool(true));
    symbol.metadata.get_or_insert_with(HashMap::new).insert(
        "aliasedType".to_string(),
        serde_json::Value::String(aliased_type.trim().to_string()),
    );

    Some(symbol)
}
