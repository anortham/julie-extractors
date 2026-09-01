use super::type_facts;
/// Rust local and parameter symbols
/// - Function and method parameters (including `self`)
/// - `let` bindings with declared or constructed types
use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::rust::RustExtractor;
use serde_json::Value;
use std::collections::HashMap;
use std::collections::VecDeque;
use tree_sitter::Node;

/// True when a `parameter`/`self_parameter` node belongs to a function or
/// method declaration rather than a function type or closure.
pub(super) fn is_callable_parameter(node: Node) -> bool {
    node.parent().is_some_and(|parameters| {
        parameters.kind() == "parameters"
            && parameters.parent().is_some_and(|owner| {
                matches!(owner.kind(), "function_item" | "function_signature_item")
            })
    })
}

pub(super) fn extract_parameter(
    extractor: &mut RustExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let base = extractor.get_base_mut();
    let pattern = node.child_by_field_name("pattern")?;
    let name_node = binding_identifier(pattern)?;
    let name = base.get_node_text(&name_node);
    let signature = base.get_node_text(&node);
    let symbol = base.create_symbol(
        &node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id,
            doc_comment: None,
            metadata: Some(role_metadata("parameter")),
            annotations: Vec::new(),
        },
    );
    if pattern.kind() != "self"
        && let Some(type_node) = node.child_by_field_name("type")
    {
        type_facts::record_declared_type(base, &symbol.id, type_node);
    }
    Some(symbol)
}

pub(super) fn extract_self_parameter(
    extractor: &mut RustExtractor,
    node: Node,
    parent_id: Option<String>,
    impl_type_name: Option<&str>,
) -> Option<Symbol> {
    let base = extractor.get_base_mut();
    let signature = base.get_node_text(&node);
    let symbol = base.create_symbol(
        &node,
        "self".to_string(),
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id,
            doc_comment: None,
            metadata: Some(role_metadata("parameter")),
            annotations: Vec::new(),
        },
    );
    if let Some(impl_type_name) = impl_type_name {
        type_facts::record_impl_self_type(base, &symbol.id, impl_type_name);
    }
    Some(symbol)
}

pub(super) fn extract_let_local(
    extractor: &mut RustExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let base = extractor.get_base_mut();
    let pattern = node.child_by_field_name("pattern")?;
    let name_node = binding_identifier(pattern)?;
    let name = base.get_node_text(&name_node);
    let type_node = node.child_by_field_name("type");
    let is_mutable = node
        .children(&mut node.walk())
        .any(|c| c.kind() == "mutable_specifier")
        || pattern.kind() == "mut_pattern";

    let mut signature = String::from("let ");
    if is_mutable {
        signature.push_str("mut ");
    }
    signature.push_str(&name);
    if let Some(type_node) = type_node {
        signature.push_str(": ");
        signature.push_str(&base.get_node_text(&type_node));
    }

    let symbol = base.create_symbol(
        &node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id,
            doc_comment: None,
            metadata: Some(role_metadata("local")),
            annotations: Vec::new(),
        },
    );
    if let Some(type_node) = type_node {
        type_facts::record_declared_type(base, &symbol.id, type_node);
    } else if let Some(value) = node.child_by_field_name("value") {
        type_facts::record_initializer_type(base, &symbol.id, value);
    }
    Some(symbol)
}

/// Phase-2 walk of one impl-block method body: parameters and `let` bindings
/// become symbols parented to the method. Nested `function_item` subtrees are
/// skipped, matching phase 2's method-only symbol extraction.
pub(super) fn extract_callable_locals(
    extractor: &mut RustExtractor,
    fn_node: Node,
    parent_id: Option<String>,
    impl_type_name: Option<&str>,
    symbols: &mut Vec<Symbol>,
) {
    let mut pending = VecDeque::from([fn_node]);
    while let Some(node) = pending.pop_front() {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_item" => {}
                "parameter" => {
                    if is_callable_parameter(child)
                        && let Some(symbol) = extract_parameter(extractor, child, parent_id.clone())
                    {
                        symbols.push(symbol);
                    }
                }
                "self_parameter" => {
                    if is_callable_parameter(child)
                        && let Some(symbol) = extract_self_parameter(
                            extractor,
                            child,
                            parent_id.clone(),
                            impl_type_name,
                        )
                    {
                        symbols.push(symbol);
                    }
                }
                "let_declaration" => {
                    if let Some(symbol) = extract_let_local(extractor, child, parent_id.clone()) {
                        symbols.push(symbol);
                    }
                    pending.push_back(child);
                }
                _ => pending.push_back(child),
            }
        }
    }
}

fn binding_identifier(pattern: Node) -> Option<Node> {
    match pattern.kind() {
        "identifier" | "self" => Some(pattern),
        "mut_pattern" => {
            let mut cursor = pattern.walk();
            pattern
                .children(&mut cursor)
                .find(|child| child.kind() == "identifier")
        }
        _ => None,
    }
}

fn role_metadata(role: &str) -> HashMap<String, Value> {
    HashMap::from([("role".to_string(), Value::String(role.to_string()))])
}
