use crate::base::{BaseExtractor, Relationship, RelationshipKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Tree};

use super::{flags, groups, helpers};

pub(super) fn extract_relationships(
    base: &BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let named_groups = named_group_symbols(symbols);
    let numbered_groups = numbered_group_symbols(symbols);
    let capture_positions = capture_positions(tree.root_node());
    let mut relationships = Vec::new();
    let mut seen = HashSet::new();

    visit_node(
        base,
        tree.root_node(),
        symbols,
        &named_groups,
        &numbered_groups,
        &capture_positions,
        &mut relationships,
        &mut seen,
        0,
    );

    relationships
}

#[allow(clippy::too_many_arguments)]
fn visit_node(
    base: &BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    named_groups: &HashMap<String, &Symbol>,
    numbered_groups: &HashMap<usize, &Symbol>,
    capture_positions: &[(usize, usize)],
    relationships: &mut Vec<Relationship>,
    seen: &mut HashSet<(String, String, u32, usize)>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if let Some(group_name) = named_backreference_name(base, node)
        && let Some(target) = named_groups.get(&group_name)
        && let Some(source) = helpers::innermost_symbol(symbols, node)
    {
        push_backreference_relationship(
            base,
            source,
            target,
            node,
            seen,
            relationships,
            "named-backreference",
            Some(("name", Value::String(group_name))),
        );
    }

    if let Some(group_number) = numeric_backreference_number(base, node)
        && let Some(target) = numbered_groups.get(&group_number)
        && let Some(source) = helpers::innermost_symbol(symbols, node)
    {
        push_backreference_relationship(
            base,
            source,
            target,
            node,
            seen,
            relationships,
            "numeric-backreference",
            Some(("captureIndex", Value::Number((group_number as u64).into()))),
        );
    }

    if node.kind() == "conditional_condition" {
        if let Some((reference_type, capture_index, reference_node)) =
            numeric_conditional_reference(base, node, capture_positions)
            && let Some(target) = numbered_groups.get(&capture_index)
            && let Some(source) = helpers::innermost_symbol(symbols, reference_node)
        {
            push_backreference_relationship(
                base,
                source,
                target,
                reference_node,
                seen,
                relationships,
                reference_type,
                Some(("captureIndex", Value::from(capture_index))),
            );
        } else if let Some((group_name, reference_node)) = conditional_capture_name(base, node)
            && let Some(target) = named_groups.get(&group_name)
            && let Some(source) = helpers::innermost_symbol(symbols, reference_node)
        {
            push_backreference_relationship(
                base,
                source,
                target,
                reference_node,
                seen,
                relationships,
                "named-condition",
                Some(("name", Value::String(group_name))),
            );
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit_node(
            base,
            child,
            symbols,
            named_groups,
            numbered_groups,
            capture_positions,
            relationships,
            seen,
            child_depth,
        );
    }
}

fn named_group_symbols(symbols: &[Symbol]) -> HashMap<String, &Symbol> {
    symbols
        .iter()
        .filter_map(|symbol| {
            let name = symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("named"))
                .and_then(Value::as_str)?;
            Some((name.to_string(), symbol))
        })
        .collect()
}

fn numbered_group_symbols(symbols: &[Symbol]) -> HashMap<usize, &Symbol> {
    symbols
        .iter()
        .filter_map(|symbol| {
            let capture_index = symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("captureIndex"))
                .and_then(Value::as_u64)? as usize;
            Some((capture_index, symbol))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn push_backreference_relationship(
    base: &BaseExtractor,
    source: &Symbol,
    target: &Symbol,
    node: Node,
    seen: &mut HashSet<(String, String, u32, usize)>,
    relationships: &mut Vec<Relationship>,
    reference_type: &str,
    extra_metadata: Option<(&str, Value)>,
) {
    let line = (node.start_position().row + 1) as u32;
    let key = (
        source.id.clone(),
        target.id.clone(),
        line,
        node.start_byte(),
    );
    if !seen.insert(key) {
        return;
    }

    let mut metadata = HashMap::new();
    metadata.insert(
        "referenceType".to_string(),
        Value::String(reference_type.to_string()),
    );
    if let Some((key, value)) = extra_metadata {
        metadata.insert(key.to_string(), value);
    }

    relationships.push(base.create_relationship(
        source.id.clone(),
        target.id.clone(),
        RelationshipKind::References,
        &node,
        Some(1.0),
        Some(metadata),
    ));
}

fn named_backreference_name(base: &BaseExtractor, node: Node) -> Option<String> {
    match node.kind() {
        "backreference_escape" => {
            let content_after = base.content.get(node.start_byte()..)?;
            if !content_after.starts_with("\\k<") {
                return None;
            }
            let end_pos = content_after.find('>')?;
            if content_after.is_char_boundary(3) && content_after.is_char_boundary(end_pos) {
                let group_name = &content_after[3..end_pos];
                (!group_name.is_empty()).then(|| group_name.to_string())
            } else {
                None
            }
        }
        "backreference" => {
            let text = base.get_node_text(&node);
            flags::extract_backref_group_name(&text)
        }
        "named_group_backreference" => {
            let name = base.get_node_text(&super::groups::group_name_node(node)?);
            (!name.is_empty()).then_some(name)
        }
        _ => None,
    }
}

fn numeric_backreference_number(base: &BaseExtractor, node: Node) -> Option<usize> {
    if node.kind() != "decimal_escape" {
        return None;
    }

    let text = base.get_node_text(&node);
    flags::extract_group_number(&text)?.parse().ok()
}

pub(super) fn referenced_capture_numbers(base: &BaseExtractor, tree: &Tree) -> HashSet<usize> {
    let mut numbers = HashSet::new();
    let capture_positions = capture_positions(tree.root_node());
    collect_referenced_capture_numbers(base, tree.root_node(), &capture_positions, &mut numbers, 0);
    numbers
}

fn collect_referenced_capture_numbers(
    base: &BaseExtractor,
    node: Node,
    capture_positions: &[(usize, usize)],
    numbers: &mut HashSet<usize>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if let Some(number) = numeric_backreference_number(base, node) {
        numbers.insert(number);
    }
    if node.kind() == "conditional_condition"
        && let Some((_, capture_index, _)) =
            numeric_conditional_reference(base, node, capture_positions)
    {
        numbers.insert(capture_index);
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_referenced_capture_numbers(base, child, capture_positions, numbers, child_depth);
    }
}

fn conditional_capture_name<'tree>(
    base: &BaseExtractor,
    node: Node<'tree>,
) -> Option<(String, Node<'tree>)> {
    let test = named_child(node, "conditional_test")?;
    named_child(test, "conditional_capture_name")
        .and_then(groups::group_name_node)
        .map(|name| (base.get_node_text(&name), name))
}

fn numeric_conditional_reference<'tree>(
    base: &BaseExtractor,
    node: Node<'tree>,
    capture_positions: &[(usize, usize)],
) -> Option<(&'static str, usize, Node<'tree>)> {
    let test = named_child(node, "conditional_test")?;
    let numeric = named_child(test, "conditional_numeric_condition")?;
    let text = base.get_node_text(&numeric);
    let (reference_type, capture_index) = match text.as_bytes().first().copied() {
        Some(b'-') => (
            "relative-condition",
            relative_capture_index(&text, node, capture_positions)?,
        ),
        Some(b'+') => (
            "relative-condition",
            relative_capture_index(&text, node, capture_positions)?,
        ),
        _ => ("numeric-condition", text.parse::<usize>().ok()?),
    };
    (capture_index > 0).then_some((reference_type, capture_index, numeric))
}

fn relative_capture_index(
    reference: &str,
    condition: Node,
    capture_positions: &[(usize, usize)],
) -> Option<usize> {
    let distance = reference.get(1..)?.parse::<usize>().ok()?;
    if distance == 0 {
        return None;
    }
    match reference.as_bytes().first().copied()? {
        b'-' => capture_positions
            .iter()
            .filter(|(_, start_byte)| *start_byte < condition.start_byte())
            .rev()
            .nth(distance - 1)
            .map(|(capture_index, _)| *capture_index),
        b'+' => capture_positions
            .iter()
            .filter(|(_, start_byte)| *start_byte > condition.start_byte())
            .nth(distance - 1)
            .map(|(capture_index, _)| *capture_index),
        _ => None,
    }
}

fn capture_positions(root: Node) -> Vec<(usize, usize)> {
    let mut captures = Vec::new();
    collect_capture_positions(root, &mut captures, 0);
    captures
}

fn collect_capture_positions(node: Node, captures: &mut Vec<(usize, usize)>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if matches!(
        node.kind(),
        "anonymous_capturing_group" | "named_capturing_group"
    ) {
        captures.push((captures.len() + 1, node.start_byte()));
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_capture_positions(child, captures, child_depth);
    }
}

fn named_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}
