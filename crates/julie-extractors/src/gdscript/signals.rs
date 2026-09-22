//! Signal extraction for GDScript

use super::helpers::{doc_comment, find_child_by_type};
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use tree_sitter::Node;

/// Extract signal statement
pub(super) fn extract_signal_statement(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
) -> Option<Symbol> {
    let name_node = find_child_by_type(&node, "name")?;
    let name = base.get_node_text(&name_node);
    let signature = base.get_node_text(&node);

    let doc = doc_comment(base, node);
    let mut symbol = base.create_symbol(
        &node,
        name,
        SymbolKind::Event,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.cloned(),
            metadata: None,
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    symbol.doc_comment = doc;
    Some(symbol)
}
