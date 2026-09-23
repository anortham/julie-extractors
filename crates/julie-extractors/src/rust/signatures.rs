/// Rust function signatures and related declarations
/// - Function signatures (extern functions)
/// - Associated types
/// - Return type extraction
/// - Use declarations
use super::helpers::{
    associated_item_owner, effective_visibility, extract_visibility, find_doc_comment,
    item_annotations,
};
use crate::base::{Symbol, SymbolKind, SymbolOptions};
use crate::rust::RustExtractor;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract function return type from a function node
pub(super) fn extract_return_type(base: &crate::base::BaseExtractor, node: Node) -> String {
    let return_type_node = node.child_by_field_name("return_type");

    if let Some(ret_type) = return_type_node {
        let return_type = base.get_node_text(&ret_type);
        let return_type = return_type.trim();
        let return_type = return_type.strip_prefix("->").unwrap_or(return_type).trim();
        if !return_type.is_empty() {
            return return_type.to_string();
        }
    }

    String::new()
}

/// Extract function signature (for extern functions)
pub(super) fn extract_function_signature(
    extractor: &mut RustExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let base = extractor.get_base_mut();
    let name_node = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "identifier");
    let name = name_node.map(|n| base.get_node_text(&n))?;

    // Extract parameters
    let params_node = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "parameters");
    let params = params_node
        .map(|n| base.get_node_text(&n))
        .unwrap_or_else(|| "()".to_string());

    // Extract return type (after -> token)
    let children: Vec<_> = node.children(&mut node.walk()).collect();
    let arrow_index = children.iter().position(|c| c.kind() == "->");
    let return_type = if let Some(index) = arrow_index {
        if index + 1 < children.len() {
            format!(" -> {}", base.get_node_text(&children[index + 1]))
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let signature = format!(
        "{}fn {}{}{}",
        extract_visibility(base, node),
        name,
        params,
        return_type
    );
    let kind = if associated_item_owner(node).is_some() {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    };

    let symbol = base.create_symbol(
        &node,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(effective_visibility(base, node)),
            parent_id,
            doc_comment: find_doc_comment(base, node),
            metadata: Some(HashMap::new()),
            annotations: item_annotations(base, node),
        },
    );
    super::type_facts::record_return_type(base, &symbol.id, node, None);
    Some(symbol)
}

/// Extract associated type in a trait
pub(super) fn extract_associated_type(
    extractor: &mut RustExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let base = extractor.get_base_mut();
    let name_node = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "type_identifier");
    let name = name_node.map(|n| base.get_node_text(&n))?;

    // Extract trait bounds (: Debug + Clone, etc.)
    let trait_bounds = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "trait_bounds")
        .map(|c| base.get_node_text(&c))
        .unwrap_or_default();

    let signature = format!("type {}{}", name, trait_bounds);

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Type,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(effective_visibility(base, node)),
            parent_id,
            doc_comment: find_doc_comment(base, node),
            metadata: Some(HashMap::new()),
            annotations: item_annotations(base, node),
        },
    ))
}

/// Extract one import symbol per name bound by a `use` declaration or
/// `extern crate`. Every symbol spans the whole declaration, like TypeScript
/// import bindings. A glob import binds its prefix path (`use a::b::*` -> `a::b`).
pub(super) fn extract_use_symbols(
    extractor: &mut RustExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Vec<Symbol> {
    let base = extractor.get_base_mut();
    let use_text = base.get_node_text(&node);
    let visibility = effective_visibility(base, node);
    super::helpers::use_leaves(base, node)
        .into_iter()
        .map(|leaf| {
            let mut metadata = HashMap::new();
            if let Some(imported) = leaf.path.last() {
                metadata.insert(
                    "importedName".to_string(),
                    serde_json::Value::String(imported.clone()),
                );
            }
            if let Some(alias) = leaf.alias {
                metadata.insert("alias".to_string(), serde_json::Value::String(alias));
            }
            let mut symbol = base.create_symbol(
                &node,
                leaf.name,
                SymbolKind::Import,
                SymbolOptions {
                    signature: Some(use_text.clone()),
                    visibility: Some(visibility.clone()),
                    parent_id: parent_id.clone(),
                    doc_comment: None,
                    metadata: Some(metadata),
                    annotations: Vec::new(),
                },
            );
            symbol.doc_comment = find_doc_comment(base, node);
            symbol
        })
        .collect()
}
