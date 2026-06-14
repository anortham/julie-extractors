use super::resolve_alias_anchor_target;
use crate::base::{BaseExtractor, Relationship, RelationshipKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Tree};

pub(super) fn extract_relationships(
    base: &BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let mut relationships = Vec::new();
    let mut seen = HashSet::new();
    walk_tree(
        base,
        tree.root_node(),
        symbols,
        &mut relationships,
        &mut seen,
        0,
    );
    relationships
}

fn walk_tree(
    base: &BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    seen: &mut HashSet<(String, String, u32, String)>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "alias" {
        extract_alias_relationship(base, node, symbols, relationships, seen);
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_tree(base, child, symbols, relationships, seen, child_depth);
    }
}

fn extract_alias_relationship(
    base: &BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    seen: &mut HashSet<(String, String, u32, String)>,
) {
    let Some(alias_name) = alias_name(base, node) else {
        return;
    };
    let Some(target) = resolve_alias_anchor_target(symbols, &alias_name) else {
        return;
    };
    let Some(source) = base.find_containing_symbol(&node, symbols) else {
        return;
    };

    let line_number = (node.start_position().row + 1) as u32;
    let key = (
        source.id.clone(),
        target.id.clone(),
        line_number,
        alias_name.clone(),
    );
    if !seen.insert(key) {
        return;
    }

    let mut metadata = HashMap::new();
    metadata.insert("alias".to_string(), Value::String(alias_name));

    relationships.push(base.create_relationship(
        source.id.clone(),
        target.id.clone(),
        RelationshipKind::References,
        &node,
        Some(1.0),
        Some(metadata),
    ));
}

fn alias_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "alias_name" {
            return Some(base.get_node_text(&child));
        }
    }
    None
}
