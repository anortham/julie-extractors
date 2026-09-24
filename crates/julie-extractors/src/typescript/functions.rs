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

/// Extract a function declaration, signature, or function-valued expression.
///
/// A function expression takes the name of the declarator that binds it, and
/// an anonymous `export default` function is named `default`. An overload
/// signature emits nothing: its implementation lists it under `overloads`.
pub(super) fn extract_function(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let parent = node.parent();
    let name = match parent {
        Some(parent) if parent.kind() == "variable_declarator" => parent
            .child_by_field_name("name")
            .map(|name| extractor.base().get_node_text(&name)),
        Some(parent)
            if parent.kind() == "export_statement" && node.kind() != "function_declaration" =>
        {
            node.child_by_field_name("name")
                .map(|name| extractor.base().get_node_text(&name))
                .or_else(|| Some("default".to_string()))
        }
        _ => node
            .child_by_field_name("name")
            .map(|name| extractor.base().get_node_text(&name)),
    }?;

    let is_signature = node.kind() == "function_signature";
    if is_signature
        && crate::javascript::exports::is_overload_signature(extractor.base(), node, &name)
    {
        return None;
    }

    let signature = build_function_signature(extractor, &node, &name);
    let visibility = helpers::extract_ts_visibility(node);
    let content = extractor.base().content.clone();
    let decorator_texts = helpers::extract_decorator_texts(node, &content);
    let annotations = normalize_annotations(&decorator_texts, "typescript");
    let annotation_keys: Vec<String> = annotations
        .iter()
        .map(|marker| marker.annotation_key.clone())
        .collect();

    let is_async = helpers::has_modifier(node, "async");
    let is_generator = helpers::has_modifier(node, "*");

    let mut metadata = callable_metadata(extractor, node, is_async, is_generator);
    if is_signature {
        metadata.insert("isDefinition".to_string(), serde_json::json!(false));
    }
    let overloads = overload_signatures(extractor, node, &name);
    if !overloads.is_empty() {
        metadata.insert("overloads".to_string(), serde_json::json!(overloads));
    }

    let doc_comment = extractor
        .base()
        .find_doc_comment(&node)
        .or_else(|| parent.and_then(|parent| declarator_doc_comment(extractor, parent)));

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
    super::type_facts::record_return_type_fact(extractor.base_mut(), &symbol.id, node);

    Some(symbol)
}

/// The doc comment of the declaration that binds a function expression
/// (`/** doc */ export const f = () => {}`).
fn declarator_doc_comment(extractor: &TypeScriptExtractor, parent: Node) -> Option<String> {
    if parent.kind() != "variable_declarator" {
        return None;
    }
    let declaration = parent.parent()?;
    extractor.base().find_doc_comment(&declaration).or_else(|| {
        declaration
            .parent()
            .filter(|wrapper| wrapper.kind() == "export_statement")
            .and_then(|wrapper| extractor.base().find_doc_comment(&wrapper))
    })
}

/// The overload signatures written directly before an implementation.
fn overload_signatures(extractor: &TypeScriptExtractor, node: Node, name: &str) -> Vec<String> {
    if !matches!(
        node.kind(),
        "function_declaration" | "generator_function_declaration"
    ) {
        return Vec::new();
    }
    crate::javascript::exports::overload_signature_nodes(extractor.base(), node, name)
        .into_iter()
        .map(|overload| build_function_signature(extractor, &overload, name))
        .collect()
}

fn callable_metadata(
    extractor: &TypeScriptExtractor,
    node: Node,
    is_async: bool,
    is_generator: bool,
) -> HashMap<String, serde_json::Value> {
    let mut metadata = HashMap::new();
    metadata.insert("isAsync".to_string(), serde_json::json!(is_async));
    metadata.insert("isGenerator".to_string(), serde_json::json!(is_generator));
    metadata.insert(
        "parameters".to_string(),
        serde_json::json!(extract_parameters(extractor, &node)),
    );
    if let Some(return_type) = annotation_text(extractor, node, "return_type") {
        metadata.insert("returnType".to_string(), serde_json::json!(return_type));
    }
    metadata.insert(
        "typeParameters".to_string(),
        serde_json::json!(extract_type_parameters(extractor, &node)),
    );
    metadata
}

/// The function a member-shaped declaration binds: `key: () => {}`,
/// `handler = () => {}` in a class body, or `const f = function () {}`.
pub(super) fn function_value(node: Node) -> Option<Node> {
    node.child_by_field_name("value")
        .map(unwrap_expression)
        .filter(|value| {
            matches!(
                value.kind(),
                "arrow_function" | "function_expression" | "generator_function"
            )
        })
}

/// Strip type assertions and parentheses around an initializer:
/// `({...}) as const`, `{...} satisfies T`, `value!`.
pub(super) fn unwrap_expression(node: Node) -> Node {
    let mut current = node;
    for _ in 0..8 {
        if !matches!(
            current.kind(),
            "parenthesized_expression"
                | "as_expression"
                | "satisfies_expression"
                | "non_null_expression"
        ) {
            break;
        }
        let mut cursor = current.walk();
        let Some(inner) = current.named_children(&mut cursor).next() else {
            break;
        };
        current = inner;
    }
    current
}

/// A class field or object-literal pair whose value is a function: a method
/// named by its key that owns the function's parameters and calls.
pub(super) fn extract_member_function(
    extractor: &mut TypeScriptExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let value = function_value(node)?;
    let name_node = node
        .child_by_field_name("name")
        .or_else(|| node.child_by_field_name("key"))?;
    let name = extractor.base().get_node_text(&name_node);

    let is_static = helpers::has_modifier(node, "static");
    let content = extractor.base().content.clone();
    let decorators = helpers::extract_decorator_names(node, &content);
    let annotations = normalize_annotations(
        &helpers::extract_decorator_texts(node, &content),
        "typescript",
    );
    let base_sig = build_function_signature(extractor, &value, &name);
    let static_prefix = if is_static { "static " } else { "" };
    let signature = format!(
        "{}{}{}",
        helpers::decorator_prefix(&decorators),
        static_prefix,
        base_sig
    );

    let mut metadata = callable_metadata(
        extractor,
        value,
        helpers::has_modifier(value, "async"),
        helpers::has_modifier(value, "*") || value.kind() == "generator_function",
    );
    metadata.insert("isStatic".to_string(), serde_json::json!(is_static));
    let doc_comment = extractor.base().find_doc_comment(&node);

    let symbol = extractor.base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Method,
        SymbolOptions {
            signature: Some(signature),
            visibility: helpers::extract_ts_visibility(node),
            parent_id: parent_id.map(str::to_string),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    );
    super::type_facts::record_return_type_fact(extractor.base_mut(), &symbol.id, value);
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

    let mut metadata = callable_metadata(extractor, node, is_async, is_generator);
    metadata.insert("isStatic".to_string(), serde_json::json!(is_static));

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
    super::type_facts::record_return_type_fact(extractor.base_mut(), &symbol.id, node);

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

    if let Some(value_node) = function_value(node) {
        return extract_function(extractor, value_node, parent_id);
    }
    let value = node.child_by_field_name("value").map(unwrap_expression);
    if value.is_some_and(|value| value.kind() == "class") {
        return None;
    }

    let doc_comment = extractor.base().find_doc_comment(&node);
    let visibility = helpers::extract_ts_visibility(node);

    if let Some(source) = value.and_then(|value| require_source(extractor, value)) {
        let metadata = HashMap::from([
            ("source".to_string(), serde_json::json!(source)),
            ("isCommonJS".to_string(), serde_json::json!(true)),
        ]);
        let signature = extractor.base().get_node_text(&node);
        return Some(extractor.base_mut().create_symbol(
            &node,
            name,
            SymbolKind::Import,
            SymbolOptions {
                signature: Some(signature),
                parent_id: parent_id.map(str::to_string),
                metadata: Some(metadata),
                doc_comment,
                ..Default::default()
            },
        ));
    }

    let symbol = extractor.base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            visibility,
            parent_id: parent_id.map(str::to_string),
            doc_comment,
            ..Default::default()
        },
    );
    super::type_facts::record_variable_type_facts(
        &mut extractor.base,
        &symbol.id,
        node,
        &extractor.return_types,
    );
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
        .or_else(|| {
            node.child_by_field_name("parameter")
                .map(|parameter| format!("({})", extractor.base().get_node_text(&parameter)))
        })
        .unwrap_or_else(|| "()".to_string());

    let mut signature = format!("{}{}", name, params);
    if let Some(return_type) = annotation_text(extractor, *node, "return_type") {
        signature.push_str(&format!(": {}", return_type));
    }

    signature
}

/// The text of a type annotation field without its leading `:`.
pub(super) fn annotation_text(
    extractor: &TypeScriptExtractor,
    node: Node,
    field: &str,
) -> Option<String> {
    let text = extractor.base().get_field_text(&node, field)?;
    let text = text.trim();
    Some(text.strip_prefix(':').unwrap_or(text).trim().to_string())
}

/// The module a `require("m")` call loads.
fn require_source(extractor: &TypeScriptExtractor, value: Node) -> Option<String> {
    if value.kind() != "call_expression" {
        return None;
    }
    let function = value.child_by_field_name("function")?;
    if function.kind() != "identifier" || extractor.base().get_node_text(&function) != "require" {
        return None;
    }
    let arguments = value.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let source = arguments
        .named_children(&mut cursor)
        .find(|argument| argument.kind() == "string")?;
    Some(
        extractor
            .base()
            .get_node_text(&source)
            .trim_matches(|c| c == '"' || c == '\'' || c == '`')
            .to_string(),
    )
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
        let mut cursor = params.walk();
        params
            .named_children(&mut cursor)
            .filter(|child| child.kind() != "comment")
            .map(|child| extractor.base().get_node_text(&child))
            .collect()
    } else if let Some(parameter) = node.child_by_field_name("parameter") {
        vec![extractor.base().get_node_text(&parameter)]
    } else {
        Vec::new()
    }
}
