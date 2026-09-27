use crate::base::{BaseExtractor, Identifier, IdentifierKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

use super::flags;
use super::{groups, helpers};

/// Extract all identifier usages (backreferences and named groups)
/// Following the Rust extractor reference implementation pattern
pub(super) fn extract_identifiers(
    base: &mut BaseExtractor,
    pattern_trees: &[tree_sitter::Tree],
    symbols: &[Symbol],
) -> Vec<Identifier> {
    for tree in pattern_trees {
        let root = tree.root_node();
        let pattern_symbols: Vec<Symbol> = symbols
            .iter()
            .filter(|symbol| {
                root.start_byte() as u32 <= symbol.start_byte
                    && symbol.end_byte <= root.end_byte() as u32
            })
            .cloned()
            .collect();
        walk_tree_for_identifiers(base, root, symbols, &pattern_symbols, 0);
    }
    base.identifiers.clone()
}

/// Recursively walk tree extracting identifiers from each node
fn walk_tree_for_identifiers(
    base: &mut BaseExtractor,
    node: Node,
    containing_symbols: &[Symbol],
    pattern_symbols: &[Symbol],
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    // Extract identifier from this node if applicable
    extract_identifier_from_node(base, node, containing_symbols, pattern_symbols);

    // Recursively walk children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_identifiers(
            base,
            child,
            containing_symbols,
            pattern_symbols,
            child_depth,
        );
    }
}

/// Extract identifier from a single node based on its kind
fn extract_identifier_from_node(
    base: &mut BaseExtractor,
    node: Node,
    containing_symbols: &[Symbol],
    pattern_symbols: &[Symbol],
) {
    match node.kind() {
        "backreference_escape" => {
            let start_byte = node.start_byte();
            let backref_text = base.get_node_text(&node);

            if backref_text.starts_with("\\k<")
                && let Some(end_pos) = backref_text.find('>')
            {
                if backref_text.is_char_boundary(3) && backref_text.is_char_boundary(end_pos) {
                    let group_name = backref_text[3..end_pos].to_string();
                    if !group_name.is_empty() {
                        let containing_symbol_id =
                            find_containing_symbol_id(node, containing_symbols);

                        if let Some(span) =
                            base.span_for_byte_range(start_byte + 3, start_byte + end_pos)
                        {
                            base.create_identifier_at_span(
                                span,
                                group_name,
                                IdentifierKind::Call,
                                containing_symbol_id,
                                None,
                            );
                        }
                    }
                }
            }
        }

        "backreference" => {
            let backref_text = base.get_node_text(&node);

            if let Some(group_name) = flags::extract_backref_group_name(&backref_text)
                && let Some(name_start) = backref_text
                    .find("\\k<")
                    .map(|start| start + 3)
                    .or_else(|| backref_text.find("(?P=").map(|start| start + 4))
                && let Some(span) = base.span_for_byte_range(
                    node.start_byte() + name_start,
                    node.start_byte() + name_start + group_name.len(),
                )
            {
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                base.create_identifier_at_span(
                    span,
                    group_name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                    None,
                );
            }
        }

        "named_group_backreference" => {
            if let Some(name_node) = groups::group_name_node(node) {
                let group_name = base.get_node_text(&name_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(
                    &name_node,
                    group_name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                );
            }
        }

        "conditional_condition" => {
            if let Some(name_node) = conditional_capture_name_node(node) {
                let group_name = base.get_node_text(&name_node);
                if pattern_symbols.iter().any(|symbol| {
                    symbol
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("named"))
                        .and_then(serde_json::Value::as_str)
                        == Some(group_name.as_str())
                }) {
                    let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                    base.create_identifier(
                        &name_node,
                        group_name,
                        IdentifierKind::Call,
                        containing_symbol_id,
                    );
                }
            }
        }

        // Named groups: (?<name>...) (these are "member access" in regex context)
        "named_capturing_group" => {
            if let Some(name_node) = groups::group_name_node(node) {
                let group_name = base.get_node_text(&name_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                base.create_identifier(
                    &name_node,
                    group_name,
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                );
            }
        }

        _ => {
            // Skip other node types for now
        }
    }
}

/// Find the ID of the symbol that contains this node
fn find_containing_symbol_id(node: Node, containing_symbols: &[Symbol]) -> Option<String> {
    helpers::innermost_symbol(containing_symbols, node).map(|s| s.id.clone())
}

fn conditional_capture_name_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let test = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "conditional_test")?;
    let mut cursor = test.walk();
    test.named_children(&mut cursor)
        .find(|child| child.kind() == "conditional_capture_name")
        .and_then(groups::group_name_node)
}
