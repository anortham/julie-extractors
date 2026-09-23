//! Declaration extraction for Kotlin
//!
//! This module handles extraction of functions, packages, imports,
//! and type aliases. Split from types.rs for file size compliance.

use super::helpers;
use crate::base::body::body_hash;
use crate::base::{BaseExtractor, NormalizedSpan, Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::test_detection::apply_callable_test_metadata;
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract a Kotlin function declaration
pub(super) fn extract_function(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
    parent_kind: Option<SymbolKind>,
) -> Option<Symbol> {
    let (name, raw_name) = helpers::declared_name(base, node)?;

    let modifiers = helpers::extract_modifiers(base, node);
    let annotations = helpers::extract_annotations(base, node);
    let annotation_keys: Vec<String> = annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect();
    let type_params = helpers::extract_type_parameters(base, node);
    let receiver_type = helpers::extract_receiver_type(base, node);
    let parameters = helpers::extract_parameters(base, node);
    let return_type = helpers::extract_return_type(base, node);

    // Correct Kotlin signature order: modifiers + fun + typeParams + name
    let mut signature = "fun".to_string();

    if !modifiers.is_empty() {
        signature = format!("{} {}", modifiers.join(" "), signature);
    }

    if let Some(type_params) = type_params {
        signature.push_str(&format!(" {}", type_params));
    }

    // Add receiver type for extension functions (e.g., String.functionName)
    if let Some(receiver_type) = &receiver_type {
        signature.push_str(&format!(" {}.{}", receiver_type, raw_name));
    } else {
        signature.push_str(&format!(" {}", raw_name));
    }

    signature.push_str(&parameters.unwrap_or_else(|| "()".to_string()));

    if let Some(ref return_type) = return_type {
        signature.push_str(&format!(": {}", return_type));
    }

    // Check for where clause (sibling node)
    if let Some(where_clause) = helpers::extract_where_clause(base, node) {
        signature.push_str(&format!(" {}", where_clause));
    }

    // Check for expression body (= expression)
    let function_body = node
        .children(&mut node.walk())
        .find(|n| n.kind() == "function_body");
    if let Some(function_body) = function_body {
        let body_text = base.get_node_text(&function_body);
        if body_text.starts_with('=') {
            signature.push_str(&format!(" {}", body_text));
        }
    }

    let is_local = parent_kind.as_ref().is_some_and(helpers::is_callable_kind);
    let is_member = parent_id.is_some() && !is_local;
    let symbol_kind = if modifiers.contains(&"operator".to_string()) {
        SymbolKind::Operator
    } else if is_member {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    };

    let visibility = (!is_local).then(|| helpers::determine_visibility(&modifiers));

    let mut metadata = HashMap::from([
        (
            "type".to_string(),
            Value::String(if is_member { "method" } else { "function" }.to_string()),
        ),
        ("modifiers".to_string(), Value::String(modifiers.join(","))),
    ]);

    // Store return type for type inference
    if let Some(return_type) = return_type {
        metadata.insert("returnType".to_string(), Value::String(return_type));
    }
    if let Some(receiver_type) = receiver_type {
        metadata.insert("extendedType".to_string(), Value::String(receiver_type));
    }
    super::types::record_raw_name(&name, &raw_name, &mut metadata);

    // Extract KDoc comment
    let doc_comment = base.find_doc_comment(node);

    apply_callable_test_metadata(
        "kotlin",
        &name,
        &base.file_path,
        &symbol_kind,
        &annotation_keys,
        doc_comment.as_deref(),
        &mut metadata,
    );

    let mut symbol = base.create_symbol(
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
    apply_function_body_span(base, function_body, &mut symbol);
    Some(symbol)
}

/// A function body is its `block`, or the expression after `=`; an abstract
/// or interface function has none. The shared text heuristic cannot tell a
/// body brace from one inside an annotation argument or a default value.
fn apply_function_body_span(
    base: &BaseExtractor,
    function_body: Option<Node>,
    symbol: &mut Symbol,
) {
    let body = function_body.and_then(|function_body| first_named_non_comment(function_body));
    apply_body_span(base, body, symbol);
}

/// Replace the heuristic body span of a class, object or property symbol
/// with its syntactic body: the class body, or a property's initializer,
/// delegate or getter body. A declaration without one has no body.
pub(super) fn apply_declaration_body_span(base: &BaseExtractor, node: &Node, symbol: &mut Symbol) {
    let child = |kinds: &[&str]| {
        node.children(&mut node.walk())
            .find(|child| kinds.contains(&child.kind()))
    };
    let body = match node.kind() {
        "class_declaration"
        | "object_declaration"
        | "companion_object"
        | "interface_declaration" => {
            child(&["class_body", "enum_class_body"]).or_else(|| spec_constructor_lambda(node))
        }
        "property_declaration" => {
            let initializer = node
                .children(&mut node.walk())
                .skip_while(|child| child.kind() != "=")
                .find(|child| child.is_named() && !child.kind().contains("comment"));
            initializer
                .or_else(|| child(&["property_delegate"]).and_then(first_named_non_comment))
                .or_else(|| {
                    child(&["getter"])
                        .and_then(|getter| {
                            getter
                                .children(&mut getter.walk())
                                .find(|c| c.kind() == "function_body")
                        })
                        .and_then(first_named_non_comment)
                })
        }
        _ => return,
    };
    apply_body_span(base, body, symbol);
}

/// `class LengthSpec : StringSpec({ ... })` keeps its body in the lambda it
/// passes to the supertype constructor, as Kotest and Spek specs do.
fn spec_constructor_lambda<'tree>(node: &Node<'tree>) -> Option<Node<'tree>> {
    let specifiers = node
        .children(&mut node.walk())
        .find(|child| child.kind() == "delegation_specifiers")?;
    let mut cursor = specifiers.walk();
    specifiers
        .named_children(&mut cursor)
        .filter_map(|specifier| specifier.named_child(0))
        .filter(|invocation| invocation.kind() == "constructor_invocation")
        .find_map(|invocation| {
            let arguments = invocation
                .children(&mut invocation.walk())
                .find(|child| child.kind() == "value_arguments")?;
            let mut cursor = arguments.walk();
            arguments
                .named_children(&mut cursor)
                .filter_map(|argument| argument.named_child(0))
                .find(|expression| expression.kind() == "lambda_literal")
        })
}

fn first_named_non_comment(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| !child.kind().contains("comment"))
}

fn apply_body_span(base: &BaseExtractor, body: Option<Node>, symbol: &mut Symbol) {
    symbol.body_span = body.map(|body| NormalizedSpan::from_node(&body));
    symbol.body_hash = symbol
        .body_span
        .and_then(|span| body_hash(&base.content, span, &base.language));
}

/// Extract a Kotlin secondary constructor
///
/// Secondary constructors use the `constructor` keyword and delegate to the
/// primary constructor via `this(...)` or to a parent class via `super(...)`.
/// Tree-sitter node type: `secondary_constructor` with children:
///   - `function_value_parameters` (the parameter list)
///   - `constructor_delegation_call` (the `this(...)` / `super(...)` call)
///   - `modifiers` (optional)
///   - `block` (optional body)
pub(super) fn extract_secondary_constructor(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
    class_name: &str,
) -> Option<Symbol> {
    let modifiers = helpers::extract_modifiers(base, node);
    let annotations = helpers::extract_annotations(base, node);
    let annotation_keys: Vec<String> = annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect();
    let parameters = helpers::extract_parameters(base, node);

    let mut signature = "constructor".to_string();

    if !modifiers.is_empty() {
        signature = format!("{} {}", modifiers.join(" "), signature);
    }

    signature.push_str(&parameters.unwrap_or_else(|| "()".to_string()));

    // Append delegation call (`: this(...)` or `: super(...)`) if present
    if let Some(delegation) = node
        .children(&mut node.walk())
        .find(|n| n.kind() == "constructor_delegation_call")
    {
        let delegation_text = base.get_node_text(&delegation);
        signature.push_str(&format!(": {}", delegation_text));
    }

    let visibility = helpers::determine_visibility(&modifiers);

    // Extract KDoc comment
    let doc_comment = base.find_doc_comment(node);

    let mut metadata = HashMap::from([
        ("type".to_string(), Value::String("constructor".to_string())),
        ("modifiers".to_string(), Value::String(modifiers.join(","))),
        (
            "constructorKind".to_string(),
            Value::String("secondary".to_string()),
        ),
    ]);

    apply_callable_test_metadata(
        "kotlin",
        class_name,
        &base.file_path,
        &SymbolKind::Constructor,
        &annotation_keys,
        doc_comment.as_deref(),
        &mut metadata,
    );

    Some(base.create_symbol(
        node,
        class_name.to_string(),
        SymbolKind::Constructor,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    ))
}

/// Extract a Kotlin package declaration
pub(super) fn extract_package(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    // Look for qualified_identifier which contains the full package name
    let name = node
        .children(&mut node.walk())
        .find(|n| n.kind() == "qualified_identifier")
        .map(|n| base.get_node_text(&n))?;

    // Extract KDoc comment
    let doc_comment = base.find_doc_comment(node);

    Some(base.create_symbol(
        node,
        name.clone(),
        SymbolKind::Namespace,
        SymbolOptions {
            signature: Some(format!("package {}", name)),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(HashMap::from([(
                "type".to_string(),
                Value::String("package".to_string()),
            )])),
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Extract a Kotlin import statement. `import a.B as C` keeps the alias in
/// the signature and publishes `alias`, `local_name` and `importedName`;
/// `import a.b.*` publishes `isWildcard`.
pub(super) fn extract_import(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = node
        .children(&mut node.walk())
        .find(|n| n.kind() == "qualified_identifier")
        .map(|n| base.get_node_text(&n))?;
    let alias = node
        .children(&mut node.walk())
        .skip_while(|child| child.kind() != "as")
        .find(|child| child.kind() == "identifier")
        .map(|alias| base.get_node_text(&alias));
    let is_wildcard = node
        .children(&mut node.walk())
        .any(|child| child.kind() == "*");

    let mut signature = format!("import {name}");
    let mut metadata = HashMap::from([("type".to_string(), Value::String("import".to_string()))]);
    if is_wildcard {
        signature.push_str(".*");
        metadata.insert("isWildcard".to_string(), Value::Bool(true));
    }
    if let Some(alias) = &alias {
        signature.push_str(&format!(" as {alias}"));
        let imported_name = name.rsplit('.').next().unwrap_or(&name).to_string();
        metadata.insert("alias".to_string(), Value::String(alias.clone()));
        metadata.insert("local_name".to_string(), Value::String(alias.clone()));
        metadata.insert("importedName".to_string(), Value::String(imported_name));
    }

    let doc_comment = base.find_doc_comment(node);
    let mut symbol = base.create_symbol(
        node,
        name,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    );
    symbol.body_span = None;
    symbol.body_hash = None;
    Some(symbol)
}

/// Extract a Kotlin type alias
pub(super) fn extract_type_alias(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let (name, raw_name) = helpers::declared_name(base, node)?;

    let modifiers = helpers::extract_modifiers(base, node);
    let type_params = helpers::extract_type_parameters(base, node);
    let annotations = helpers::extract_annotations(base, node);

    // Find the aliased type (after =) - may consist of multiple nodes
    let mut aliased_type = String::new();
    let children: Vec<Node> = node.children(&mut node.walk()).collect();
    if let Some(equal_index) = children.iter().position(|n| base.get_node_text(n) == "=")
        && equal_index + 1 < children.len()
    {
        // Concatenate all nodes after the = (e.g., "suspend" + "(T) -> Unit")
        let type_nodes = &children[equal_index + 1..];
        aliased_type = type_nodes
            .iter()
            .map(|n| base.get_node_text(n))
            .collect::<Vec<String>>()
            .join(" ");
    }

    let mut signature = format!("typealias {}", raw_name);

    if !modifiers.is_empty() {
        signature = format!("{} {}", modifiers.join(" "), signature);
    }

    if let Some(type_params) = type_params {
        signature.push_str(&type_params);
    }

    if !aliased_type.is_empty() {
        signature.push_str(&format!(" = {}", aliased_type));
    }

    let visibility = helpers::determine_visibility(&modifiers);

    // Extract KDoc comment
    let doc_comment = base.find_doc_comment(node);

    let mut metadata = HashMap::from([
        ("type".to_string(), Value::String("typealias".to_string())),
        ("modifiers".to_string(), Value::String(modifiers.join(","))),
        ("aliasedType".to_string(), Value::String(aliased_type)),
    ]);
    super::types::record_raw_name(&name, &raw_name, &mut metadata);

    Some(base.create_symbol(
        node,
        name,
        SymbolKind::Type,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    ))
}
