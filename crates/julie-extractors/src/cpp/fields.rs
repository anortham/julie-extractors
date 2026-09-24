//! Field and multi-variable declaration extraction for C++
//! Handles class/struct member fields and multi-declarator statements like `int x, y, z;`

use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use tree_sitter::Node;

use super::declarators;
use super::helpers;
use super::signatures;
use super::type_facts::{self, ReturnTypeIndex};
use super::visibility;

/// Extract one field (or constant) row per name a field declaration introduces.
/// A method declaration introduces no object name and yields no rows.
pub(super) fn extract_field(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Vec<Symbol> {
    let names = declarators::object_names(node);
    if names.is_empty() {
        return Vec::new();
    }

    let storage_class = helpers::extract_storage_class(base, node);
    let type_specifiers = helpers::extract_type_specifiers(base, node);
    let is_constant = helpers::is_constant_declaration(&storage_class, &type_specifiers);
    let is_static_member = helpers::is_static_member_variable(node, &storage_class);
    let kind = if is_constant || is_static_member {
        SymbolKind::Constant
    } else {
        SymbolKind::Field
    };
    let doc_comment = base.find_doc_comment(&node);
    let vis = visibility::extract_visibility_from_node(base, node);

    names
        .into_iter()
        .map(|(declarator, name_node)| {
            let name = base.get_node_text(&name_node);
            let signature = signatures::build_field_signature(base, node, &name);
            let symbol = base.create_symbol(
                &node,
                name,
                kind.clone(),
                SymbolOptions {
                    signature: Some(signature),
                    visibility: Some(vis.clone()),
                    parent_id: parent_id.map(String::from),
                    metadata: None,
                    doc_comment: doc_comment.clone(),
                    annotations: Vec::new(),
                },
            );
            type_facts::record_field_fact(base, &symbol.id, node, Some(declarator));
            symbol
        })
        .collect()
}

/// Extract additional variables from multi-variable declarations
/// For declarations like `int x = 1, y = 2, z = 3;` the first variable (x)
/// is extracted by extract_declaration(). This function extracts the remaining
/// variables (y, z) so they all appear as separate symbols.
pub(super) fn extract_multi_declarations(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&str>,
    return_types: &ReturnTypeIndex,
) -> Vec<Symbol> {
    declarators::object_names(node)
        .into_iter()
        .skip(1)
        .map(|(declarator, name_node)| {
            super::declarations::object_symbol(
                base,
                node,
                declarator,
                name_node,
                parent_id,
                return_types,
            )
        })
        .collect()
}
