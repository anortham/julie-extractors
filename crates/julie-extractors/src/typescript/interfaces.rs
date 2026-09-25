//! Interface, type alias, enum, property, and namespace extraction
//!
//! This module handles extraction of TypeScript-specific constructs including
//! interfaces, type aliases, enums, properties, and namespaces.

use super::helpers;
use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility, normalize_annotations};
use crate::typescript::TypeScriptExtractor;
use tree_sitter::Node;

/// Extract an interface declaration and its members (properties and methods)
pub(super) fn extract_interface(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();

    let name_node = node.child_by_field_name("name");
    let name = match name_node.map(|n| extractor.base().get_node_text(&n)) {
        Some(name) => name,
        None => return symbols,
    };

    // Extract JSDoc comment
    let doc_comment = extractor.base().find_doc_comment(&node);
    let visibility = helpers::extract_ts_visibility(node);

    let iface_symbol = extractor.base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Interface,
        SymbolOptions {
            visibility,
            parent_id: parent_id.map(str::to_string),
            doc_comment,
            ..Default::default()
        },
    );

    let parent_id = iface_symbol.id.clone();
    symbols.push(iface_symbol);

    // Extract interface members from the interface body
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            match child.kind() {
                "property_signature" => {
                    if let Some(member_name_node) = child.child_by_field_name("name") {
                        let member_name = extractor.base().get_node_text(&member_name_node);
                        if !member_name.is_empty() {
                            let signature = extractor.base().get_node_text(&child);
                            let member_symbol = extractor.base_mut().create_symbol(
                                &child,
                                member_name,
                                SymbolKind::Property,
                                SymbolOptions {
                                    parent_id: Some(parent_id.clone()),
                                    signature: Some(signature),
                                    ..Default::default()
                                },
                            );
                            super::type_facts::record_annotation_fact(
                                extractor.base_mut(),
                                &member_symbol.id,
                                child,
                            );
                            symbols.push(member_symbol);
                        }
                    }
                }
                "method_signature" => {
                    if let Some(member_name_node) = child.child_by_field_name("name") {
                        let member_name = extractor.base().get_node_text(&member_name_node);
                        if !member_name.is_empty() {
                            let signature = extractor.base().get_node_text(&child);
                            let member_symbol = extractor.base_mut().create_symbol(
                                &child,
                                member_name,
                                SymbolKind::Method,
                                SymbolOptions {
                                    parent_id: Some(parent_id.clone()),
                                    signature: Some(signature),
                                    ..Default::default()
                                },
                            );
                            super::type_facts::record_return_type_fact(
                                extractor.base_mut(),
                                &member_symbol.id,
                                child,
                            );
                            symbols.push(member_symbol);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    symbols
}

/// Extract a type alias declaration
pub(super) fn extract_type_alias(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name");
    let name = name_node.map(|n| extractor.base().get_node_text(&n))?;

    // Extract JSDoc comment
    let doc_comment = extractor.base().find_doc_comment(&node);
    let visibility = helpers::extract_ts_visibility(node);

    Some(extractor.base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Type,
        SymbolOptions {
            visibility,
            parent_id: parent_id.map(str::to_string),
            doc_comment,
            ..Default::default()
        },
    ))
}

/// Extract an enum declaration and its members
pub(super) fn extract_enum(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();

    let name_node = node.child_by_field_name("name");
    let name = match name_node.map(|n| extractor.base().get_node_text(&n)) {
        Some(name) => name,
        None => return symbols,
    };

    // Extract JSDoc comment
    let doc_comment = extractor.base().find_doc_comment(&node);
    let visibility = helpers::extract_ts_visibility(node);

    let enum_symbol = extractor.base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Enum,
        SymbolOptions {
            visibility,
            parent_id: parent_id.map(str::to_string),
            doc_comment,
            ..Default::default()
        },
    );

    let parent_id = enum_symbol.id.clone();
    symbols.push(enum_symbol);

    // Extract enum members from the enum body
    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            let member_name_node = match child.kind() {
                "enum_assignment" => child.child_by_field_name("name"),
                "property_identifier" | "string" => Some(child),
                _ => None,
            };
            let Some(member_name_node) = member_name_node else {
                continue;
            };
            let member_name = extractor
                .base()
                .get_node_text(&member_name_node)
                .trim_matches(|c| c == '"' || c == '\'')
                .to_string();
            if member_name.is_empty() {
                continue;
            }
            let member_symbol = extractor.base_mut().create_symbol(
                &child,
                member_name,
                SymbolKind::EnumMember,
                SymbolOptions {
                    parent_id: Some(parent_id.clone()),
                    ..Default::default()
                },
            );
            symbols.push(member_symbol);
        }
    }

    symbols
}

/// Extract a namespace: `namespace A.B {}`, `module Legacy {}`, or an
/// ambient module `declare module "x" {}` named by its module string.
pub(super) fn extract_namespace(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name")?;
    let name = extractor
        .base()
        .get_node_text(&name_node)
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .to_string();
    let doc_comment = extractor
        .base()
        .find_doc_comment(&node)
        .or_else(|| ambient_doc_comment(extractor, node));
    let mut metadata = std::collections::HashMap::new();
    if name_node.kind() == "string" {
        metadata.insert("isAmbientModule".to_string(), serde_json::json!(true));
    }

    Some(extractor.base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Namespace,
        SymbolOptions {
            visibility: helpers::extract_ts_visibility(node),
            parent_id: parent_id.map(str::to_string),
            doc_comment,
            metadata: (!metadata.is_empty()).then_some(metadata),
            ..Default::default()
        },
    ))
}

/// `declare global { ... }`: a namespace named `global` that parents the
/// global augmentations inside it.
pub(super) fn extract_global_augmentation(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    if !node
        .children(&mut cursor)
        .any(|child| child.kind() == "global")
    {
        return None;
    }
    let doc_comment = extractor.base().find_doc_comment(&node);
    let metadata = std::collections::HashMap::from([(
        "isGlobalAugmentation".to_string(),
        serde_json::json!(true),
    )]);
    Some(extractor.base_mut().create_symbol(
        &node,
        "global".to_string(),
        SymbolKind::Namespace,
        SymbolOptions {
            parent_id: parent_id.map(str::to_string),
            doc_comment,
            metadata: Some(metadata),
            ..Default::default()
        },
    ))
}

fn ambient_doc_comment(extractor: &TypeScriptExtractor, node: Node) -> Option<String> {
    node.parent()
        .filter(|parent| parent.kind() == "ambient_declaration")
        .and_then(|parent| extractor.base().find_doc_comment(&parent))
}

/// A constructor parameter with an accessibility or `readonly` modifier
/// (`private readonly repo: Repo`) also declares a class property. The
/// property spans the parameter name, so it never shares the parameter
/// symbol's span.
pub(super) fn extract_parameter_property(
    extractor: &mut TypeScriptExtractor,
    param_node: Node,
    class_id: Option<&str>,
) -> Option<Symbol> {
    if !matches!(
        param_node.kind(),
        "required_parameter" | "optional_parameter"
    ) {
        return None;
    }
    let is_readonly = helpers::has_readonly(param_node);
    let has_accessibility = param_node
        .children(&mut param_node.walk())
        .any(|child| child.kind() == "accessibility_modifier");
    let is_override = param_node
        .children(&mut param_node.walk())
        .any(|child| child.kind() == "override_modifier");
    if !(has_accessibility || is_readonly || is_override) {
        return None;
    }
    let name_node = param_node
        .child_by_field_name("pattern")
        .filter(|pattern| pattern.kind() == "identifier")?;
    let name = extractor.base().get_node_text(&name_node);
    let visibility = helpers::extract_ts_visibility(param_node).or(Some(Visibility::Public));
    let signature = extractor.base().get_node_text(&param_node);
    let metadata = std::collections::HashMap::from([
        ("isStatic".to_string(), serde_json::json!(false)),
        ("isReadonly".to_string(), serde_json::json!(is_readonly)),
        ("isParameterProperty".to_string(), serde_json::json!(true)),
    ]);
    let annotations = decorator_annotations(extractor, param_node);

    let symbol = extractor.base_mut().create_symbol(
        &name_node,
        name,
        SymbolKind::Property,
        SymbolOptions {
            signature: Some(signature),
            visibility,
            parent_id: class_id.map(str::to_string),
            metadata: Some(metadata),
            annotations,
            ..Default::default()
        },
    );
    super::type_facts::record_annotation_fact(extractor.base_mut(), &symbol.id, param_node);
    Some(symbol)
}

fn decorator_annotations(
    extractor: &TypeScriptExtractor,
    node: Node,
) -> Vec<crate::base::AnnotationMarker> {
    let content = &extractor.base().content;
    normalize_annotations(
        &helpers::extract_decorator_texts(node, content),
        "typescript",
    )
}

/// Extract a class field, or a property signature of a type alias's object
/// type. A field whose value is a function is a method.
pub(super) fn extract_property(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    if super::functions::function_value(node).is_some() {
        return super::functions::extract_member_function(extractor, node, parent_id);
    }

    let name_node = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("key"));
    let name = name_node.map(|n| extractor.base().get_node_text(&n))?;

    let visibility = helpers::extract_ts_visibility(node);

    let content = extractor.base().content.clone();
    let decorator_texts = helpers::extract_decorator_texts(node, &content);
    let annotations = decorator_annotations(extractor, node);

    let is_readonly = helpers::has_readonly(node);
    let is_static = helpers::has_modifier(node, "static");

    let mut sig_parts = Vec::new();
    let decorator_prefix = helpers::decorator_prefix(&decorator_texts);
    if !decorator_prefix.is_empty() {
        sig_parts.push(decorator_prefix.trim().to_string());
    }
    if is_static {
        sig_parts.push("static".to_string());
    }
    if is_readonly {
        sig_parts.push("readonly".to_string());
    }
    let type_annotation = super::functions::annotation_text(extractor, node, "type");
    let signature = if let Some(type_annotation) = type_annotation {
        let mut head = sig_parts;
        head.push(name.clone());
        Some(format!("{}: {}", head.join(" "), type_annotation))
    } else if !sig_parts.is_empty() {
        sig_parts.push(name.clone());
        Some(sig_parts.join(" "))
    } else {
        None
    };

    let doc_comment = extractor.base().find_doc_comment(&node);

    let mut metadata = std::collections::HashMap::new();
    metadata.insert("isStatic".to_string(), serde_json::json!(is_static));
    metadata.insert("isReadonly".to_string(), serde_json::json!(is_readonly));

    let symbol = extractor.base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Property,
        SymbolOptions {
            signature,
            visibility,
            parent_id: parent_id.map(str::to_string),
            doc_comment,
            metadata: Some(metadata),
            annotations,
        },
    );
    super::type_facts::record_annotation_fact(extractor.base_mut(), &symbol.id, node);
    if let Some(value) = node.child_by_field_name("value") {
        crate::javascript::type_facts::record_new_expression_fact(
            extractor.base_mut(),
            &symbol.id,
            value,
            &super::type_facts::TYPE_NAME_RULES,
        );
    }
    Some(symbol)
}
