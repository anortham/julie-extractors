//! Symbol emission for Erlang module attributes: `-module`, `-record`,
//! `-define`, `-type`/`-opaque`, and `-callback`.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Node;

use super::ErlangExtractor;
use super::helpers::{
    arg_count, attribute_signature, child_named_kinds, find_child_by_type, first_atom_text,
    named_children, unquote_atom,
};
use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};

pub(super) fn extract_module(
    extractor: &mut ErlangExtractor,
    node: &Node,
    module_doc: Option<String>,
) -> Option<Symbol> {
    let name = first_atom_text(&extractor.base, node)?;
    let signature = attribute_signature(&extractor.base, node);
    let doc_comment = extractor.base.find_doc_comment(node).or(module_doc);
    let annotations = super::doc::annotations_for(extractor, node);

    let mut metadata = HashMap::new();
    if extractor.test_module.is_test_container() {
        metadata.insert("test_container".to_string(), Value::Bool(true));
    }

    Some(extractor.base.create_symbol(
        node,
        name,
        SymbolKind::Module,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: None,
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    ))
}

/// Pushes the record symbol followed by its field symbols so declaration order
/// is preserved in the emitted list.
pub(super) fn extract_record(
    extractor: &mut ErlangExtractor,
    node: &Node,
    symbols: &mut Vec<Symbol>,
    parent_id: Option<&str>,
) {
    let Some(name) = first_atom_text(&extractor.base, node) else {
        return;
    };
    let signature = attribute_signature(&extractor.base, node);
    let doc_comment = super::doc::doc_for(extractor, node);
    let annotations = super::doc::annotations_for(extractor, node);

    let record = extractor.base.create_symbol(
        node,
        name,
        SymbolKind::Struct,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id: parent_id.map(String::from),
            metadata: None,
            doc_comment,
            annotations,
        },
    );

    let record_id = record.id.clone();
    symbols.push(record);

    for field in child_named_kinds(node, "record_field") {
        if let Some(symbol) = extract_record_field(extractor, &field, &record_id) {
            symbols.push(symbol);
        }
    }
}

fn extract_record_field(
    extractor: &mut ErlangExtractor,
    node: &Node,
    record_id: &str,
) -> Option<Symbol> {
    let name = first_atom_text(&extractor.base, node)?;
    let signature = attribute_signature(&extractor.base, node);

    Some(extractor.base.create_symbol(
        node,
        name,
        SymbolKind::Field,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id: Some(record_id.to_string()),
            metadata: None,
            doc_comment: None,
            annotations: Vec::new(),
        },
    ))
}

pub(super) fn extract_macro(
    extractor: &mut ErlangExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let lhs = find_child_by_type(node, "macro_lhs")?;
    let name_node = named_children(&lhs).into_iter().next()?;
    let name = unquote_atom(&extractor.base.get_node_text(&name_node));
    if name.is_empty() {
        return None;
    }

    let signature = attribute_signature(&extractor.base, node);
    let doc_comment = super::doc::doc_for(extractor, node);
    let annotations = super::doc::annotations_for(extractor, node);

    let mut metadata = HashMap::new();
    if let Some(args) = find_child_by_type(&lhs, "var_args") {
        metadata.insert(
            "macro_arity".to_string(),
            Value::Number(arg_count(&args).into()),
        );
    }

    Some(extractor.base.create_symbol(
        node,
        name,
        SymbolKind::Constant,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Private),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    ))
}

pub(super) fn extract_type(
    extractor: &mut ErlangExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let type_name = find_child_by_type(node, "type_name")?;
    let name = first_atom_text(&extractor.base, &type_name)?;
    let arity = find_child_by_type(&type_name, "var_args")
        .map(|args| arg_count(&args))
        .unwrap_or(0);

    let opaque = node.kind() == "opaque";
    let visibility = if extractor.exported_types.contains(&(name.clone(), arity)) {
        Visibility::Public
    } else {
        Visibility::Private
    };

    let mut metadata = HashMap::new();
    metadata.insert("arity".to_string(), Value::Number(arity.into()));
    if opaque {
        metadata.insert("opaque".to_string(), Value::Bool(true));
    }

    let signature = attribute_signature(&extractor.base, node);
    let doc_comment = super::doc::doc_for(extractor, node);
    let annotations = super::doc::annotations_for(extractor, node);

    Some(extractor.base.create_symbol(
        node,
        name,
        SymbolKind::Type,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    ))
}

pub(super) fn extract_callback(
    extractor: &mut ErlangExtractor,
    node: &Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let name = first_atom_text(&extractor.base, node)?;
    let arity = find_child_by_type(node, "type_sig")
        .and_then(|sig| find_child_by_type(&sig, "expr_args"))
        .map(|args| arg_count(&args))
        .unwrap_or(0);

    let mut metadata = HashMap::new();
    metadata.insert("callback".to_string(), Value::Bool(true));
    metadata.insert("arity".to_string(), Value::Number(arity.into()));

    let signature = attribute_signature(&extractor.base, node);
    let doc_comment = super::doc::doc_for(extractor, node);
    let annotations = super::doc::annotations_for(extractor, node);

    Some(extractor.base.create_symbol(
        node,
        name,
        SymbolKind::Function,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(String::from),
            metadata: Some(metadata),
            doc_comment,
            annotations,
        },
    ))
}
