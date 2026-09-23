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
        walk_tree_for_identifiers(base, tree.root_node(), symbols, 0);
    }
    base.identifiers.clone()
}

/// Recursively walk tree extracting identifiers from each node
fn walk_tree_for_identifiers(
    base: &mut BaseExtractor,
    node: Node,
    containing_symbols: &[Symbol],
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    // Extract identifier from this node if applicable
    extract_identifier_from_node(base, node, containing_symbols);

    // Recursively walk children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree_for_identifiers(base, child, containing_symbols, child_depth);
    }
}

/// Extract identifier from a single node based on its kind
fn extract_identifier_from_node(
    base: &mut BaseExtractor,
    node: Node,
    containing_symbols: &[Symbol],
) {
    match node.kind() {
        // Backreferences: tree-sitter-regex uses "backreference_escape" for \k
        // But doesn't properly parse the <name> part, so we need to extract manually
        "backreference_escape" => {
            // Get the full text context around this node to find the group name
            let start_byte = node.start_byte();
            let content_after = &base.content[start_byte..];

            // Try to extract \k<name> pattern manually
            if content_after.starts_with("\\k<")
                && let Some(end_pos) = content_after.find('>')
            {
                // SAFETY: Check char boundary before slicing to prevent UTF-8 panic
                if content_after.is_char_boundary(3) && content_after.is_char_boundary(end_pos) {
                    let group_name = content_after[3..end_pos].to_string();
                    if !group_name.is_empty() {
                        let containing_symbol_id =
                            find_containing_symbol_id(node, containing_symbols);

                        base.create_identifier(
                            &node,
                            group_name,
                            IdentifierKind::Call,
                            containing_symbol_id,
                        );
                    }
                }
            }
        }

        // Original "backreference" node type (if tree-sitter-regex ever adds proper support)
        "backreference" => {
            let backref_text = base.get_node_text(&node);

            // Try to extract named backreference (e.g., \k<email>)
            if let Some(group_name) = flags::extract_backref_group_name(&backref_text) {
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);

                base.create_identifier(
                    &node,
                    group_name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                );
            }
            // Note: Numeric backreferences (\1, \2) don't have names to track
        }

        // Python-style named backreference: (?P=name)
        "named_group_backreference" => {
            if let Some(name_node) = groups::group_name_node(node) {
                let group_name = base.get_node_text(&name_node);
                let containing_symbol_id = find_containing_symbol_id(node, containing_symbols);
                base.create_identifier(
                    &node,
                    group_name,
                    IdentifierKind::Call,
                    containing_symbol_id,
                );
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
