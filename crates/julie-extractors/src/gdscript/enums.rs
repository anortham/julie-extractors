//! Enum extraction for GDScript

use super::helpers::{doc_comment, member_visibility};
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use tree_sitter::Node;

/// Extract an enum and its members. A named enum owns its members; the
/// members of an anonymous `enum { A, B }` belong to the enclosing class.
pub(super) fn extract_enum(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<&String>,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let member_parent = match node.child_by_field_name("name") {
        Some(name_node) => {
            let name = base.get_node_text(&name_node);
            let enum_symbol = create(
                base,
                node,
                name,
                SymbolKind::Enum,
                base.get_node_text(&node),
                parent_id,
            );
            let id = enum_symbol.id.clone();
            symbols.push(enum_symbol);
            Some(id)
        }
        None => parent_id.cloned(),
    };

    let Some(body) = node.child_by_field_name("body") else {
        return symbols;
    };
    let mut cursor = body.walk();
    for enumerator in body.named_children(&mut cursor) {
        if enumerator.kind() != "enumerator" {
            continue;
        }
        let Some(left) = enumerator.child_by_field_name("left") else {
            continue;
        };
        let name = base.get_node_text(&left);
        let signature = base.get_node_text(&enumerator);
        symbols.push(create(
            base,
            enumerator,
            name,
            SymbolKind::EnumMember,
            signature,
            member_parent.as_ref(),
        ));
    }
    symbols
}

fn create(
    base: &mut BaseExtractor,
    node: Node,
    name: String,
    kind: SymbolKind,
    signature: String,
    parent_id: Option<&String>,
) -> Symbol {
    let doc = doc_comment(base, node);
    let visibility = if kind == SymbolKind::Enum {
        member_visibility(&name)
    } else {
        Visibility::Public
    };
    let mut symbol = base.create_symbol(
        &node,
        name,
        kind,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(visibility),
            parent_id: parent_id.cloned(),
            metadata: None,
            doc_comment: None,
            annotations: Vec::new(),
        },
    );
    symbol.doc_comment = doc;
    if symbol.kind == SymbolKind::EnumMember {
        symbol.body_span = None;
        symbol.body_hash = None;
    }
    symbol
}
