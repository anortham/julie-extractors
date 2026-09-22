//! Function and method extraction
//!
//! This module handles extraction of function declarations, methods, constructors,
//! and arrow functions assigned to variables.

use super::helpers;
use crate::base::{Symbol, SymbolKind, SymbolOptions, normalize_annotations};
use crate::javascript::test_symbols::apply_declared_test_metadata;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use crate::typescript::TypeScriptExtractor;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract a function declaration or arrow function
pub(super) fn extract_function(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name");
    let mut name = name_node.map(|n| extractor.base().get_node_text(&n));

    // Handle arrow functions assigned to variables
    if node.kind() == "arrow_function"
        && let Some(parent) = node.parent()
        && parent.kind() == "variable_declarator"
        && let Some(var_name_node) = parent.child_by_field_name("name")
    {
        name = Some(extractor.base().get_node_text(&var_name_node));
    }

    let name = name?;

    let signature = build_function_signature(extractor, &node, &name);
    let visibility = helpers::extract_ts_visibility(node);
    let content = extractor.base().content.clone();
    let decorator_texts = helpers::extract_decorator_texts(node, &content);
    let annotations = normalize_annotations(&decorator_texts, "typescript");
    let annotation_keys: Vec<String> = annotations
        .iter()
        .map(|marker| marker.annotation_key.clone())
        .collect();

    // Check for modifiers
    let is_async = helpers::has_modifier(node, "async");
    let is_generator = helpers::has_modifier(node, "*");

    let parameters = extract_parameters(extractor, &node);
    let return_type = extractor.base().get_field_text(&node, "return_type");
    let type_parameters = extract_type_parameters(extractor, &node);

    let mut metadata = HashMap::new();
    metadata.insert("isAsync".to_string(), serde_json::json!(is_async));
    metadata.insert("isGenerator".to_string(), serde_json::json!(is_generator));
    metadata.insert("parameters".to_string(), serde_json::json!(parameters));
    if let Some(return_type) = return_type {
        metadata.insert("returnType".to_string(), serde_json::json!(return_type));
    }
    metadata.insert(
        "typeParameters".to_string(),
        serde_json::json!(type_parameters),
    );

    // Extract JSDoc comment
    let doc_comment = extractor.base().find_doc_comment(&node);

    apply_declared_test_metadata(
        "typescript",
        &name,
        &extractor.base().file_path,
        &SymbolKind::Function,
        &annotation_keys,
        doc_comment.as_deref(),
        &mut metadata,
    );

    let symbol = extractor.base_mut().create_symbol(
        &node,
        name.clone(),
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility,
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    );

    Some(symbol)
}

/// Extract a method definition (inside a class)
pub(super) fn extract_method(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name");
    let name = name_node.map(|n| extractor.base().get_node_text(&n))?;

    // Determine if this is a constructor
    let symbol_kind = if name == "constructor" {
        SymbolKind::Constructor
    } else {
        SymbolKind::Method
    };

    // Check for modifiers before building signature (static goes into the text)
    let is_async = helpers::has_modifier(node, "async");
    let is_static = helpers::has_modifier(node, "static");
    let is_generator = helpers::has_modifier(node, "*");

    // Build base signature, then prepend static + decorators
    let base_sig = build_function_signature(extractor, &node, &name);

    // Extract decorators from preceding siblings (tree-sitter TS puts method decorators as siblings)
    let content = extractor.base().content.clone();
    let decorators = helpers::extract_preceding_decorator_names(node, &content);
    let decorator_texts = helpers::extract_preceding_decorator_texts(node, &content);
    let annotations = normalize_annotations(&decorator_texts, "typescript");
    let annotation_keys: Vec<String> = annotations
        .iter()
        .map(|marker| marker.annotation_key.clone())
        .collect();
    let decorator_prefix = helpers::decorator_prefix(&decorators);
    let static_prefix = if is_static { "static " } else { "" };
    let signature = format!("{}{}{}", decorator_prefix, static_prefix, base_sig);

    let visibility = helpers::extract_ts_visibility(node);

    let parameters = extract_parameters(extractor, &node);
    let return_type = extractor.base().get_field_text(&node, "return_type");
    let type_parameters = extract_type_parameters(extractor, &node);

    let mut metadata = HashMap::new();
    metadata.insert("isAsync".to_string(), serde_json::json!(is_async));
    metadata.insert("isStatic".to_string(), serde_json::json!(is_static));
    metadata.insert("isGenerator".to_string(), serde_json::json!(is_generator));
    metadata.insert("parameters".to_string(), serde_json::json!(parameters));
    if let Some(return_type) = return_type {
        metadata.insert("returnType".to_string(), serde_json::json!(return_type));
    }
    metadata.insert(
        "typeParameters".to_string(),
        serde_json::json!(type_parameters),
    );

    // Extract JSDoc comment
    let doc_comment = extractor.base().find_doc_comment(&node);

    apply_declared_test_metadata(
        "typescript",
        &name,
        &extractor.base().file_path,
        &symbol_kind,
        &annotation_keys,
        doc_comment.as_deref(),
        &mut metadata,
    );

    let symbol = extractor.base_mut().create_symbol(
        &node,
        name.clone(),
        symbol_kind,
        SymbolOptions {
            signature: Some(signature),
            visibility,
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    );

    Some(symbol)
}

/// Extract a variable declarator
pub(super) fn extract_variable(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name_node = node.child_by_field_name("name");
    let name = name_node.map(|n| extractor.base().get_node_text(&n))?;

    // Check if this variable contains an arrow function
    if let Some(value_node) = node.child_by_field_name("value")
        && value_node.kind() == "arrow_function"
    {
        // Extract as a function instead of a variable
        return extract_function(extractor, value_node, parent_id);
    }

    // Extract JSDoc comment
    let doc_comment = extractor.base().find_doc_comment(&node);

    let symbol = extractor.base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            parent_id: parent_id.map(str::to_string),
            doc_comment,
            ..Default::default()
        },
    );
    super::type_facts::record_variable_type_facts(extractor.base_mut(), &symbol.id, node);
    Some(symbol)
}

/// One variable per name a destructuring declarator binds:
/// `const { users, loading: busy = false, ...rest } = store` binds `users`,
/// `busy`, and `rest`. Each symbol spans its binding name.
pub(super) fn extract_destructured_variables(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let Some(pattern) = node
        .child_by_field_name("name")
        .filter(|name| matches!(name.kind(), "object_pattern" | "array_pattern"))
    else {
        return Vec::new();
    };
    let declaration = node
        .parent()
        .and_then(|parent| parent.child(0))
        .map(|keyword| extractor.base().get_node_text(&keyword))
        .unwrap_or_else(|| "const".to_string());
    let value = extractor
        .base()
        .get_field_text(&node, "value")
        .unwrap_or_default();
    let signature = format!(
        "{} {} = {}",
        declaration,
        extractor.base().get_node_text(&pattern),
        value
    );
    let doc_comment = node
        .parent()
        .and_then(|parent| extractor.base().find_doc_comment(&parent));
    let destructuring_type = if pattern.kind() == "array_pattern" {
        "array"
    } else {
        "object"
    };

    let mut bindings = Vec::new();
    collect_pattern_bindings(pattern, &mut bindings);
    bindings
        .into_iter()
        .map(|(binding, is_rest)| {
            let name = extractor.base().get_node_text(&binding);
            let mut metadata = HashMap::from([
                ("declarationType".to_string(), declaration.clone().into()),
                ("isDestructured".to_string(), true.into()),
                ("destructuringType".to_string(), destructuring_type.into()),
            ]);
            if is_rest {
                metadata.insert("isRestParameter".to_string(), true.into());
            }
            extractor.base_mut().create_symbol(
                &binding,
                name,
                SymbolKind::Variable,
                SymbolOptions {
                    signature: Some(signature.clone()),
                    parent_id: parent_id.map(str::to_string),
                    doc_comment: doc_comment.clone(),
                    metadata: Some(metadata),
                    ..Default::default()
                },
            )
        })
        .collect()
}

/// The identifiers a destructuring pattern binds, in source order, each with
/// whether it is a rest binding.
fn collect_pattern_bindings<'t>(pattern: Node<'t>, bindings: &mut Vec<(Node<'t>, bool)>) {
    let mut cursor = pattern.walk();
    for child in pattern.named_children(&mut cursor) {
        collect_binding_target(child, false, bindings, 0);
    }
}

fn collect_binding_target<'t>(
    node: Node<'t>,
    is_rest: bool,
    bindings: &mut Vec<(Node<'t>, bool)>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let target = match node.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            bindings.push((node, is_rest));
            return;
        }
        "pair_pattern" => node.child_by_field_name("value"),
        "object_assignment_pattern" | "assignment_pattern" => node.child_by_field_name("left"),
        "rest_pattern" => {
            let mut cursor = node.walk();
            let target = node.named_children(&mut cursor).next();
            if let Some(target) = target {
                collect_binding_target(target, true, bindings, child_depth);
            }
            return;
        }
        "object_pattern" | "array_pattern" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                collect_binding_target(child, false, bindings, child_depth);
            }
            return;
        }
        _ => None,
    };
    if let Some(target) = target {
        collect_binding_target(target, false, bindings, child_depth);
    }
}

/// Build a function signature string (e.g., "foo(x, y): string")
fn build_function_signature(extractor: &TypeScriptExtractor, node: &Node, name: &str) -> String {
    let params = extractor
        .base()
        .get_field_text(node, "parameters")
        .or_else(|| extractor.base().get_field_text(node, "formal_parameters"))
        .unwrap_or_else(|| "()".to_string());
    let return_type = extractor.base().get_field_text(node, "return_type");

    let mut signature = format!("{}{}", name, params);
    if let Some(return_type) = return_type {
        signature.push_str(&format!(": {}", return_type));
    }

    signature
}

/// Extract type parameters from a function (e.g., <T, U> in generics)
fn extract_type_parameters(extractor: &TypeScriptExtractor, node: &Node) -> Vec<String> {
    if let Some(type_params) = node.child_by_field_name("type_parameters") {
        let mut params = Vec::new();
        let mut cursor = type_params.walk();
        for child in type_params.children(&mut cursor) {
            if child.kind() == "type_parameter" {
                params.push(extractor.base().get_node_text(&child));
            }
        }
        params
    } else {
        Vec::new()
    }
}

/// Extract function parameters
fn extract_parameters(extractor: &TypeScriptExtractor, node: &Node) -> Vec<String> {
    if let Some(params) = node.child_by_field_name("parameters") {
        let mut parameters = Vec::new();
        let mut cursor = params.walk();
        for child in params.children(&mut cursor) {
            if child.kind() == "parameter" || child.kind() == "identifier" {
                parameters.push(extractor.base().get_node_text(&child));
            }
        }
        parameters
    } else {
        Vec::new()
    }
}
