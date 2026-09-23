use super::helpers::{
    ImplBlockInfo, associated_item_owner, effective_visibility, extract_extern_modifier,
    extract_impl_target_names, extract_visibility, find_doc_comment, has_async_keyword,
    has_unsafe_keyword, item_annotations,
};
use super::signatures::extract_return_type;
/// Rust function and method extraction
/// - Functions and methods
/// - Impl blocks and two-phase processing
use crate::base::{Symbol, SymbolKind, SymbolOptions};
use crate::rust::RustExtractor;
use crate::test_detection::apply_callable_test_metadata;
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

/// Extract function parameters from a function node
pub(super) fn extract_function_parameters(
    base: &crate::base::BaseExtractor,
    node: Node,
) -> Vec<String> {
    let mut parameters = Vec::new();
    let param_list = node.child_by_field_name("parameters");

    if let Some(params) = param_list {
        for child in params.children(&mut params.walk()) {
            if child.kind() == "parameter" {
                let param_text = base.get_node_text(&child);
                parameters.push(param_text);
            } else if child.kind() == "self_parameter" {
                // Handle &self, &mut self, self, etc.
                let self_text = base.get_node_text(&child);
                parameters.push(self_text);
            }
        }
    }

    parameters
}

/// Extract function or method definition
pub(super) fn extract_function(
    extractor: &mut RustExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let base = extractor.get_base_mut();
    let name_node = node.child_by_field_name("name");
    let name = name_node.map(|n| base.get_node_text(&n))?;

    let owner = associated_item_owner(node);
    let kind = if owner.is_some() {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    };

    // Extract function signature components
    let visibility = extract_visibility(base, node);
    let is_async = has_async_keyword(base, node);
    let is_unsafe = has_unsafe_keyword(base, node);
    let extern_modifier = extract_extern_modifier(base, node);
    let params = extract_function_parameters(base, node);
    let return_type = extract_return_type(base, node);

    // Extract generic type parameters
    let type_params = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "type_parameters")
        .map(|c| base.get_node_text(&c))
        .unwrap_or_default();

    // Extract where clause
    let where_clause = node
        .children(&mut node.walk())
        .find(|c| c.kind() == "where_clause")
        .map(|c| format!(" {}", base.get_node_text(&c)))
        .unwrap_or_default();

    // Build signature
    let mut signature = String::new();
    if !visibility.is_empty() {
        signature.push_str(&visibility);
    }
    if !extern_modifier.is_empty() {
        signature.push_str(&format!("{} ", extern_modifier));
    }
    if is_unsafe {
        signature.push_str("unsafe ");
    }
    if is_async {
        signature.push_str("async ");
    }
    signature.push_str(&format!("fn {}{}", name, type_params));
    signature.push_str(&format!("({})", params.join(", ")));
    if !return_type.is_empty() {
        signature.push_str(&format!(" -> {}", return_type));
    }
    signature.push_str(&where_clause);

    let visibility_enum = effective_visibility(base, node);
    let annotations = item_annotations(base, node);
    let annotation_keys: Vec<String> = annotations
        .iter()
        .map(|marker| marker.annotation_key.clone())
        .collect();

    let mut metadata = HashMap::new();
    let impl_type_name = owner
        .filter(|owner| owner.kind() == "impl_item")
        .and(extractor.current_impl_type.clone());
    if let Some(impl_type_name) = &impl_type_name {
        metadata.insert(
            "impl_type_name".to_string(),
            Value::String(impl_type_name.clone()),
        );
        metadata.insert(
            "impl_parent_id_resolved".to_string(),
            Value::Bool(parent_id.is_some()),
        );
    }
    let base = extractor.get_base_mut();

    apply_callable_test_metadata(
        "rust",
        &name,
        &base.file_path,
        &kind,
        &annotation_keys,
        None,
        &mut metadata,
    );

    let symbol = base.create_symbol(
        &node,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility_enum),
            parent_id,
            doc_comment: find_doc_comment(base, node),
            metadata: Some(metadata),
            annotations,
        },
    );
    super::type_facts::record_return_type(base, &symbol.id, node, impl_type_name.as_deref());
    Some(symbol)
}

/// Store information about an impl block for phase 2 processing
pub(super) fn extract_impl(extractor: &mut RustExtractor, node: Node, parent_id: Option<String>) {
    let base = extractor.get_base_mut();
    let targets = extract_impl_target_names(base, node);
    let Some(type_name) = targets.type_name else {
        return;
    };
    let tree_index = extractor.current_macro_tree;
    extractor.add_impl_block(ImplBlockInfo {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        type_name,
        parent_id,
        tree_index,
    });
}

/// Phase 2: walk each impl block's items with the implemented type as their
/// parent. The type is looked up in the impl's own scope first, so a method
/// in `mod b` attaches to `b::Config`, not to a same-named `a::Config`.
pub(super) fn process_impl_blocks(
    extractor: &mut RustExtractor,
    tree: &Tree,
    symbols: &mut Vec<Symbol>,
) {
    let impl_blocks = extractor.get_impl_blocks().to_vec();

    for impl_block in impl_blocks {
        let is_type = |symbol: &&Symbol| {
            symbol.name == impl_block.type_name
                && matches!(
                    symbol.kind,
                    SymbolKind::Class
                        | SymbolKind::Struct
                        | SymbolKind::Enum
                        | SymbolKind::Union
                        | SymbolKind::Interface
                )
        };
        let parent_id = symbols
            .iter()
            .filter(is_type)
            .find(|symbol| symbol.parent_id == impl_block.parent_id)
            .or_else(|| symbols.iter().find(is_type))
            .map(|symbol| symbol.id.clone());

        let block_tree = impl_block
            .tree_index
            .and_then(|index| extractor.macro_trees.get(index).cloned())
            .unwrap_or_else(|| tree.clone());
        let Some(node) = block_tree
            .root_node()
            .descendant_for_byte_range(impl_block.start_byte, impl_block.end_byte)
        else {
            continue;
        };
        let Some(declaration_list) = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "declaration_list")
        else {
            continue;
        };

        let previous_macro_tree = extractor.current_macro_tree;
        extractor.current_macro_tree = impl_block.tree_index;
        extractor.current_impl_type = Some(impl_block.type_name.clone());
        for child in declaration_list.children(&mut declaration_list.walk()) {
            extractor.walk_impl_item(child, symbols, parent_id.clone());
        }
        extractor.current_impl_type = None;
        extractor.current_macro_tree = previous_macro_tree;
    }
}
