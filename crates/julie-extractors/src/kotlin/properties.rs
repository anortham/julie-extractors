//! Property and constructor parameter extraction for Kotlin
//!
//! This module handles extraction of properties, constructor parameters,
//! and related metadata.

use super::helpers;
use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract a Kotlin property declaration
pub(super) fn extract_property(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
    parent_kind: Option<SymbolKind>,
    initializer_index: &type_facts::InitializerIndex,
) -> Option<Symbol> {
    // Look for name in variable_declaration first (the proper place for property names)
    let mut name_node = None;
    let var_decl = node
        .children(&mut node.walk())
        .find(|n| n.kind() == "variable_declaration");
    if let Some(var_decl) = var_decl {
        name_node = var_decl
            .children(&mut var_decl.walk())
            .find(|n| n.kind() == "identifier");
    }

    // Fallback: look for identifier at top level (for interface properties)
    if name_node.is_none() {
        name_node = node
            .children(&mut node.walk())
            .find(|n| n.kind() == "identifier");
    }
    let raw_name = name_node.map(|n| base.get_node_text(&n))?;
    let name = helpers::strip_backticks(&raw_name).to_string();

    let modifiers = helpers::extract_modifiers(base, node);
    let property_type = helpers::extract_property_type(base, node);
    let annotations = helpers::extract_annotations(base, node);

    // Check for val/var in binding_pattern_kind for interface properties
    let mut is_val = node.children(&mut node.walk()).any(|n| n.kind() == "val");
    let mut is_var = node.children(&mut node.walk()).any(|n| n.kind() == "var");

    if !is_val && !is_var {
        let binding_pattern = node
            .children(&mut node.walk())
            .find(|n| n.kind() == "binding_pattern_kind");
        if let Some(binding_pattern) = binding_pattern {
            is_val = binding_pattern
                .children(&mut binding_pattern.walk())
                .any(|n| n.kind() == "val");
            is_var = binding_pattern
                .children(&mut binding_pattern.walk())
                .any(|n| n.kind() == "var");
        }
    }

    let binding = if is_val {
        "val"
    } else if is_var {
        "var"
    } else {
        "val"
    };
    let receiver_type = helpers::extract_receiver_type(base, node);
    let mut signature = match &receiver_type {
        Some(receiver_type) => format!("{} {}.{}", binding, receiver_type, raw_name),
        None => format!("{} {}", binding, raw_name),
    };

    if !modifiers.is_empty() {
        signature = format!("{} {}", modifiers.join(" "), signature);
    }

    if let Some(ref property_type) = property_type {
        signature.push_str(&format!(": {}", property_type));
    }

    // Add initializer value if present (especially for const val)
    if let Some(initializer) = helpers::extract_property_initializer(base, node) {
        signature.push_str(&format!(" = {}", initializer));
    }

    // Check for property delegation (by lazy, by Delegates.notNull(), etc.)
    if let Some(delegation) = helpers::extract_property_delegation(base, node) {
        signature.push_str(&format!(" {}", delegation));
    }

    let is_const = modifiers.contains(&"const".to_string());
    let symbol_kind = if parent_kind.as_ref().is_some_and(helpers::is_callable_kind) {
        SymbolKind::Variable
    } else if is_const && is_val {
        SymbolKind::Constant
    } else {
        SymbolKind::Property
    };

    let visibility =
        (symbol_kind != SymbolKind::Variable).then(|| helpers::determine_visibility(&modifiers));

    let mut metadata = HashMap::from([
        (
            "type".to_string(),
            Value::String(if is_const { "constant" } else { "property" }.to_string()),
        ),
        ("modifiers".to_string(), Value::String(modifiers.join(","))),
        ("isVal".to_string(), Value::String(is_val.to_string())),
        ("isVar".to_string(), Value::String(is_var.to_string())),
    ]);

    // Store property type for type inference
    if let Some(property_type) = property_type {
        metadata.insert("propertyType".to_string(), Value::String(property_type));
    }
    if let Some(receiver_type) = receiver_type {
        metadata.insert("extendedType".to_string(), Value::String(receiver_type));
    }
    super::types::record_raw_name(&name, &raw_name, &mut metadata);

    // Extract KDoc comment
    let doc_comment = base.find_doc_comment(node);

    let symbol = base.create_symbol(
        node,
        name,
        symbol_kind,
        SymbolOptions {
            signature: Some(signature),
            visibility,
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    );
    type_facts::record_property_facts(base, &symbol.id, *node, initializer_index);
    Some(symbol)
}

/// Extract constructor parameters and create symbols for them
pub(super) fn extract_constructor_parameters(
    base: &mut BaseExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) {
    // First find the class_parameters container, then extract class_parameter nodes as properties
    let class_parameters = node
        .children(&mut node.walk())
        .find(|n| n.kind() == "class_parameters");

    if let Some(class_parameters) = class_parameters {
        for child in class_parameters.children(&mut class_parameters.walk()) {
            if child.kind() == "class_parameter" {
                let name_node = child
                    .children(&mut child.walk())
                    .find(|n| n.kind() == "identifier");
                let Some(raw_name) = name_node.map(|n| base.get_node_text(&n)) else {
                    continue;
                };
                let name = helpers::strip_backticks(&raw_name).to_string();

                // A parameter without `val`/`var` is a constructor parameter,
                // not a stored property; its signature carries no binding.
                let binding = child
                    .children(&mut child.walk())
                    .find(|n| matches!(n.kind(), "val" | "var"))
                    .map(|n| base.get_node_text(&n));

                let type_node = child.children(&mut child.walk()).find(|n| {
                    matches!(
                        n.kind(),
                        "user_type" | "type" | "nullable_type" | "type_reference" | "function_type"
                    )
                });
                let param_type = type_node
                    .map(|n| base.get_node_text(&n))
                    .unwrap_or_default();

                let modifier_list = helpers::extract_modifiers(base, &child);
                let modifiers = child
                    .children(&mut child.walk())
                    .find(|n| n.kind() == "modifiers")
                    .map(|n| base.get_node_text(&n))
                    .unwrap_or_default();

                // Get default value (handle various literal types and expressions)
                let default_value = child.children(&mut child.walk()).find(|n| {
                    matches!(
                        n.kind(),
                        "number_literal"
                            | "string_literal"
                            | "boolean_literal"
                            | "expression"
                            | "call_expression"
                    )
                });
                let default_val = default_value
                    .map(|n| format!(" = {}", base.get_node_text(&n)))
                    .unwrap_or_default();

                // Build the base signature: [modifiers] binding name[: type][ = default]
                let final_signature = {
                    let mut signature = match &binding {
                        Some(binding) => format!("{binding} {raw_name}"),
                        None => raw_name.clone(),
                    };
                    if !param_type.is_empty() {
                        signature.push_str(&format!(": {}", param_type));
                    }

                    // Check for default value (either from matched literal or from = token)
                    if !default_val.is_empty() {
                        signature.push_str(&default_val);
                    } else {
                        // Alternative: look for assignment pattern (= value)
                        let children: Vec<Node> = child.children(&mut child.walk()).collect();
                        if let Some(equal_index) =
                            children.iter().position(|n| base.get_node_text(n) == "=")
                            && equal_index + 1 < children.len()
                        {
                            let value_node = &children[equal_index + 1];
                            signature.push_str(&format!(" = {}", base.get_node_text(value_node)));
                        }
                    }

                    // Add modifiers to signature if present
                    if !modifiers.is_empty() {
                        format!("{} {}", modifiers, signature)
                    } else {
                        signature
                    }
                };

                let visibility = if binding.is_some() {
                    helpers::determine_visibility(&modifier_list)
                } else {
                    crate::base::Visibility::Private
                };

                // Extract KDoc comment
                let doc_comment = base.find_doc_comment(&child);
                let annotations = helpers::extract_annotations(base, &child);

                let mut metadata = HashMap::from([
                    ("type".to_string(), Value::String("property".to_string())),
                    (
                        "binding".to_string(),
                        Value::String(binding.unwrap_or_else(|| "none".to_string())),
                    ),
                    ("dataType".to_string(), Value::String(param_type)),
                    (
                        "hasDefaultValue".to_string(),
                        Value::String((!default_val.is_empty()).to_string()),
                    ),
                ]);
                super::types::record_raw_name(&name, &raw_name, &mut metadata);

                let property_symbol = base.create_symbol(
                    &child,
                    name,
                    SymbolKind::Property,
                    SymbolOptions {
                        signature: Some(final_signature),
                        visibility: Some(visibility),
                        parent_id: parent_id.map(|s| s.to_string()),
                        metadata: Some(metadata),
                        doc_comment,
                        annotations,
                    },
                );
                if let Some(type_node) = type_node {
                    type_facts::record_declared_type(base, &property_symbol.id, type_node);
                }
                symbols.push(property_symbol);
            }
        }
    }
}

/// Extract a property `get()` or `set(value)` accessor as a method named `get`
/// or `set`, parented to its property, so the code it runs has a caller.
pub(super) fn extract_accessor(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = if node.kind() == "getter" {
        "get"
    } else {
        "set"
    };
    let body = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "function_body");
    let head_end = body.map_or(node.end_byte(), |body| body.start_byte());
    let head = base
        .content
        .get(node.start_byte()..head_end)
        .unwrap_or_default()
        .trim()
        .to_string();
    let signature = match body {
        Some(body) if base.get_node_text(&body).starts_with('=') => {
            format!("{head} {}", base.get_node_text(&body))
        }
        _ => head,
    };
    let modifiers = helpers::extract_modifiers(base, node);
    let metadata = HashMap::from([
        ("type".to_string(), Value::String("accessor".to_string())),
        (
            "accessor".to_string(),
            Value::String(node.kind().to_string()),
        ),
    ]);
    let mut symbol = base.create_symbol(
        node,
        name.to_string(),
        SymbolKind::Method,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(helpers::determine_visibility(&modifiers)),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment: None,
            annotations: helpers::extract_annotations(base, node),
        },
    );
    let body_node = body.and_then(|body| {
        body.named_children(&mut body.walk())
            .find(|child| !child.kind().contains("comment"))
    });
    symbol.body_span = body_node.map(|body| crate::base::NormalizedSpan::from_node(&body));
    symbol.body_hash = symbol
        .body_span
        .and_then(|span| crate::base::body::body_hash(&base.content, span, &base.language));
    Some(symbol)
}
