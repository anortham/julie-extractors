//! Declaration extraction for Scala
//!
//! Functions, imports, packages, type aliases, given instances, and extensions.

use super::helpers;
use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::test_detection::apply_callable_test_metadata;
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract a Scala function/method definition
pub(super) fn extract_function(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = helpers::get_name(base, node)?;
    let modifiers = helpers::extract_modifiers(base, node);
    let annotations = helpers::extract_annotations(base, node);
    let annotation_keys: Vec<String> = annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect();
    let type_params = helpers::extract_type_parameters(base, node);
    let parameters = helpers::extract_parameters(base, node);
    let return_type = helpers::extract_return_type(base, node);

    let mut signature = "def".to_string();

    // Add modifiers
    let sig_modifiers: Vec<&String> = modifiers
        .iter()
        .filter(|m| !helpers::is_access_modifier(m))
        .collect();
    if !sig_modifiers.is_empty() {
        signature = format!(
            "{} {}",
            sig_modifiers
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            signature
        );
    }

    signature.push_str(&format!(" {}", name));

    if let Some(tp) = type_params {
        signature.push_str(&tp);
    }

    signature.push_str(&parameters.unwrap_or_else(|| "()".to_string()));

    if let Some(ref rt) = return_type {
        signature.push_str(&format!(": {}", rt));
    }

    let (name, symbol_kind, type_label) = if name == "this" {
        if let Some(class_name) = helpers::enclosing_type_name(base, node) {
            (class_name, SymbolKind::Constructor, "constructor")
        } else if parent_id.is_some() {
            (name, SymbolKind::Method, "method")
        } else {
            (name, SymbolKind::Function, "function")
        }
    } else if parent_id.is_some() {
        (name, SymbolKind::Method, "method")
    } else {
        (name, SymbolKind::Function, "function")
    };

    let visibility = helpers::determine_visibility(&modifiers);

    let mut metadata = HashMap::from([
        ("type".to_string(), Value::String(type_label.to_string())),
        ("modifiers".to_string(), Value::String(modifiers.join(","))),
    ]);

    if let Some(return_type) = return_type {
        metadata.insert("returnType".to_string(), Value::String(return_type));
    }

    let doc_comment = base.find_doc_comment(node);

    apply_callable_test_metadata(
        "scala",
        &name,
        &base.file_path,
        &symbol_kind,
        &annotation_keys,
        doc_comment.as_deref(),
        &mut metadata,
    );

    Some(base.create_symbol(
        node,
        name,
        symbol_kind,
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

/// Extract a Scala import declaration
pub(super) fn extract_import(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    // Get the full import text, strip the "import " prefix
    let full_text = base.get_node_text(node);
    let name = full_text
        .strip_prefix("import ")
        .unwrap_or(&full_text)
        .to_string();

    let doc_comment = base.find_doc_comment(node);

    Some(base.create_symbol(
        node,
        name.clone(),
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(format!("import {}", name)),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(HashMap::from([(
                "type".to_string(),
                Value::String("import".to_string()),
            )])),
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

/// Extract a Scala package clause, named by its `name` field so that a
/// packaging block (`package http { ... }`) is named `http`.
pub(super) fn extract_package(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = node
        .child_by_field_name("name")
        .map(|name| base.get_node_text(&name))?;

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

/// Extract a Scala 2 `package object name { ... }`, the namespace that holds
/// package-level definitions.
pub(super) fn extract_package_object(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = helpers::get_name(base, node)?;
    let mut signature = format!("package object {name}");
    if let Some(extends) = helpers::extract_extends(base, node) {
        signature.push_str(&format!(" {extends}"));
    }
    let doc_comment = base.find_doc_comment(node);
    let annotations = helpers::extract_annotations(base, node);

    Some(base.create_symbol(
        node,
        name,
        SymbolKind::Namespace,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(HashMap::from([(
                "type".to_string(),
                Value::String("package_object".to_string()),
            )])),
            doc_comment,
            annotations,
        },
    ))
}

/// Extract a Scala type alias (type X = Y)
pub(super) fn extract_type_alias(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = helpers::get_name(base, node)?;
    let modifiers = helpers::extract_modifiers(base, node);
    let annotations = helpers::extract_annotations(base, node);
    let _type_params = helpers::extract_type_parameters(base, node);

    let full_text = base.get_node_text(node);
    let signature = full_text.trim().to_string();

    let visibility = helpers::determine_visibility(&modifiers);
    let doc_comment = base.find_doc_comment(node);

    Some(base.create_symbol(
        node,
        name,
        SymbolKind::Type,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(HashMap::from([
                ("type".to_string(), Value::String("typealias".to_string())),
                ("modifiers".to_string(), Value::String(modifiers.join(","))),
            ])),
            doc_comment,
            annotations,
        },
    ))
}

/// Extract a Scala 3 given definition. An anonymous given takes the name the
/// compiler synthesizes from its type: `given Show[String]` is
/// `given_Show_String`.
pub(super) fn extract_given(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let given_type = node.child_by_field_name("return_type");
    let name = node
        .child_by_field_name("name")
        .map(|name| base.get_node_text(&name))
        .or_else(|| given_type.and_then(|given_type| synthesized_given_name(base, given_type)))
        .unwrap_or_else(|| "given".to_string());
    let annotations = helpers::extract_annotations(base, node);
    let full_text = base.get_node_text(node);
    let signature = full_text
        .lines()
        .next()
        .unwrap_or(&full_text)
        .trim()
        .to_string();

    let doc_comment = base.find_doc_comment(node);
    let mut metadata = HashMap::from([
        ("type".to_string(), Value::String("given".to_string())),
        ("given".to_string(), Value::Bool(true)),
    ]);
    if let Some(given_type) = given_type {
        metadata.insert(
            "givenType".to_string(),
            Value::String(base.get_node_text(&given_type)),
        );
    }

    let symbol = base.create_symbol(
        node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    );
    if let Some(given_type) = given_type {
        type_facts::record_declared_type(base, &symbol.id, given_type);
    }
    Some(symbol)
}

/// `given_` followed by the simple names of the type and of its type
/// arguments' heads: `Show[List[Int]]` gives `given_Show_List`.
fn synthesized_given_name(base: &BaseExtractor, given_type: Node) -> Option<String> {
    fn simple_name(base: &BaseExtractor, node: Node) -> Option<String> {
        let head = if node.kind() == "generic_type" {
            node.child_by_field_name("type")?
        } else {
            node
        };
        let text = base.get_node_text(&head);
        let simple = text.rsplit('.').next()?.trim();
        (!simple.is_empty()).then(|| simple.to_string())
    }
    let mut parts = vec![simple_name(base, given_type)?];
    if let Some(arguments) = given_type.child_by_field_name("type_arguments") {
        let mut cursor = arguments.walk();
        parts.extend(
            arguments
                .named_children(&mut cursor)
                .filter_map(|argument| simple_name(base, argument)),
        );
    }
    Some(format!("given_{}", parts.join("_")))
}

/// Extract a Scala 3 extension definition, named by the type it extends
/// (`extension (s: Shape)` is `Shape`), as Swift names `extension Shape`.
pub(super) fn extract_extension(
    base: &mut BaseExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let full_text = base.get_node_text(node);
    let annotations = helpers::extract_annotations(base, node);
    let signature = full_text
        .lines()
        .next()
        .unwrap_or(&full_text)
        .trim()
        .to_string();

    let extended_type = node
        .child_by_field_name("parameters")
        .and_then(|parameters| {
            parameters
                .named_children(&mut parameters.walk())
                .find(|child| child.kind() == "parameter")
        })
        .and_then(|parameter| parameter.child_by_field_name("type"))
        .map(|type_node| base.get_node_text(&type_node));
    let name = extended_type
        .as_deref()
        .map(|extended_type| {
            type_facts::base_type_name_from_text(extended_type)
                .and_then(|name| name.rsplit('.').next().map(str::to_string))
                .unwrap_or_else(|| extended_type.to_string())
        })
        .unwrap_or_else(|| "extension".to_string());

    let doc_comment = base.find_doc_comment(node);
    let mut metadata = HashMap::from([
        ("type".to_string(), Value::String("extension".to_string())),
        ("extension".to_string(), Value::Bool(true)),
    ]);
    if let Some(extended_type) = extended_type {
        metadata.insert("extendedType".to_string(), Value::String(extended_type));
    }

    Some(base.create_symbol(
        node,
        name,
        SymbolKind::Module,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    ))
}
