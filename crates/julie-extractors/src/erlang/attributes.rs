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
use crate::base::{Symbol, SymbolKind, SymbolOptions, TestRole, Visibility};
use crate::test_detection::apply_test_role;

pub(super) fn extract_module(
    extractor: &mut ErlangExtractor,
    node: &Node,
    module_doc: Option<String>,
) -> Option<Symbol> {
    let name = first_atom_text(&extractor.base, node)?;
    let signature = attribute_signature(&extractor.base, node);
    let doc_comment = super::doc::module_doc_for(extractor, node, module_doc);
    let annotations = super::doc::annotations_for(extractor, node);

    let mut metadata = HashMap::new();
    if extractor.test_module.is_test_container() {
        apply_test_role(&mut metadata, TestRole::TestContainer);
    }

    Some(super::doc::keep_doc(
        extractor.base.create_symbol(
            node,
            name,
            SymbolKind::Module,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: None,
                metadata: Some(metadata),
                doc_comment: doc_comment.clone(),
                annotations,
            },
        ),
        doc_comment,
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
    let visibility = if extractor.is_header || extractor.exported_records.contains(&name) {
        Visibility::Public
    } else {
        Visibility::Private
    };

    let record = super::doc::keep_doc(
        extractor.base.create_symbol(
            node,
            name,
            SymbolKind::Struct,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(visibility.clone()),
                parent_id: parent_id.map(String::from),
                metadata: None,
                doc_comment: doc_comment.clone(),
                annotations,
            },
        ),
        doc_comment,
    );

    let record_id = record.id.clone();
    symbols.push(record);

    for field in child_named_kinds(node, "record_field") {
        if let Some(symbol) = extract_record_field(extractor, &field, &record_id, &visibility) {
            symbols.push(symbol);
        }
    }
}

/// A record field, with the base type its `:: Type` annotation declares as a
/// type fact.
fn extract_record_field(
    extractor: &mut ErlangExtractor,
    node: &Node,
    record_id: &str,
    visibility: &Visibility,
) -> Option<Symbol> {
    let name = first_atom_text(&extractor.base, node)?;
    let signature = attribute_signature(&extractor.base, node);

    let symbol = extractor.base.create_symbol(
        node,
        name,
        SymbolKind::Field,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility.clone()),
            parent_id: Some(record_id.to_string()),
            metadata: None,
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    if let Some(declared) = node
        .child_by_field_name("ty")
        .and_then(|field_type| field_type.named_child(0))
        .and_then(|declared| super::types::base_type_name(&extractor.base, &declared))
    {
        super::type_facts::record_record_fact(&mut extractor.base, &symbol.id, &declared, false);
    }
    Some(symbol)
}

fn declaration_visibility(extractor: &ErlangExtractor) -> Visibility {
    if extractor.is_header {
        Visibility::Public
    } else {
        Visibility::Private
    }
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

    Some(super::doc::keep_doc(
        extractor.base.create_symbol(
            node,
            name,
            SymbolKind::Constant,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(declaration_visibility(extractor)),
                parent_id: parent_id.map(String::from),
                metadata: Some(metadata),
                doc_comment: doc_comment.clone(),
                annotations,
            },
        ),
        doc_comment,
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

    let visibility = if extractor.exported_types.contains(&(name.clone(), arity)) {
        Visibility::Public
    } else {
        declaration_visibility(extractor)
    };

    let mut metadata = HashMap::new();
    metadata.insert("arity".to_string(), Value::Number(arity.into()));
    match node.kind() {
        "opaque" => {
            metadata.insert("opaque".to_string(), Value::Bool(true));
        }
        "nominal" => {
            metadata.insert("nominal".to_string(), Value::Bool(true));
        }
        _ => {}
    }

    let signature = attribute_signature(&extractor.base, node);
    let doc_comment = super::doc::doc_for(extractor, node);
    let annotations = super::doc::annotations_for(extractor, node);

    Some(super::doc::keep_doc(
        extractor.base.create_symbol(
            node,
            name,
            SymbolKind::Type,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(visibility),
                parent_id: parent_id.map(String::from),
                metadata: Some(metadata),
                doc_comment: doc_comment.clone(),
                annotations,
            },
        ),
        doc_comment,
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

    Some(super::doc::keep_doc(
        extractor.base.create_symbol(
            node,
            name,
            SymbolKind::Function,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(String::from),
                metadata: Some(metadata),
                doc_comment: doc_comment.clone(),
                annotations,
            },
        ),
        doc_comment,
    ))
}
