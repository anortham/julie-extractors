use crate::base::{BaseExtractor, Relationship, RelationshipKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tree_sitter::{Node, Tree};

use super::{CaptureInventory, flags, groups, helpers};

pub(super) fn extract_relationships(
    base: &BaseExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let named_groups = named_group_symbols(symbols);
    let numbered_groups = numbered_group_symbols(symbols);
    let captures = CaptureInventory::of(tree.root_node(), &base.content);
    let mut relationships = Vec::new();
    let mut seen = HashSet::new();

    visit_node(
        base,
        tree.root_node(),
        symbols,
        &named_groups,
        &numbered_groups,
        &captures,
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
    named_groups: &HashMap<String, Vec<&Symbol>>,
    numbered_groups: &HashMap<usize, Vec<&Symbol>>,
    captures: &CaptureInventory,
    relationships: &mut Vec<Relationship>,
    seen: &mut HashSet<(String, String, u32, usize)>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if let Some(group_name) = named_backreference_name(base, node)
        && let Some(targets) = named_groups.get(&group_name)
        && let Some(source) = helpers::innermost_symbol(symbols, node)
    {
        for target in targets {
            push_backreference_relationship(
                base,
                source,
                target,
                node,
                seen,
                relationships,
                "named-backreference",
                Some(("name", Value::String(group_name.clone()))),
            );
        }
    }

    if let Some(group_number) = numeric_backreference_number(base, node)
        && let Some(targets) = numbered_groups.get(&group_number)
        && let Some(source) = helpers::innermost_symbol(symbols, node)
    {
        for target in targets {
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
    }

    if node.kind() == "conditional_condition" {
        if let Some((reference_type, capture_index, reference_node)) =
            numeric_conditional_reference(base, node, captures)
            && let Some(targets) = numbered_groups.get(&capture_index)
            && let Some(source) = helpers::innermost_symbol(symbols, reference_node)
        {
            for target in targets {
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
            }
        } else if let Some((group_name, reference_node)) = conditional_capture_name(base, node)
            && let Some(targets) = named_groups.get(&group_name)
            && let Some(source) = helpers::innermost_symbol(symbols, reference_node)
        {
            for target in targets {
                push_backreference_relationship(
                    base,
                    source,
                    target,
                    reference_node,
                    seen,
                    relationships,
                    "named-condition",
                    Some(("name", Value::String(group_name.clone()))),
                );
            }
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
            captures,
            relationships,
            seen,
            child_depth,
        );
    }
}

/// Branch-reset branches may repeat a group name, so one name can have several targets.
fn named_group_symbols(symbols: &[Symbol]) -> HashMap<String, Vec<&Symbol>> {
    let mut groups: HashMap<String, Vec<&Symbol>> = HashMap::new();
    for symbol in symbols {
        if let Some(name) = symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("named"))
            .and_then(Value::as_str)
        {
            groups.entry(name.to_string()).or_default().push(symbol);
        }
    }
    groups
}

/// Branch-reset branches share capture numbers, so one number can have several targets.
fn numbered_group_symbols(symbols: &[Symbol]) -> HashMap<usize, Vec<&Symbol>> {
    let mut groups: HashMap<usize, Vec<&Symbol>> = HashMap::new();
    for symbol in symbols {
        if let Some(capture_index) = symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("captureIndex"))
            .and_then(Value::as_u64)
        {
            groups
                .entry(capture_index as usize)
                .or_default()
                .push(symbol);
        }
    }
    groups
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

pub(super) fn referenced_capture_numbers(
    base: &BaseExtractor,
    tree: &Tree,
    captures: &CaptureInventory,
) -> HashSet<usize> {
    let mut numbers = HashSet::new();
    collect_referenced_capture_numbers(base, tree.root_node(), captures, &mut numbers, 0);
    numbers
}

fn collect_referenced_capture_numbers(
    base: &BaseExtractor,
    node: Node,
    captures: &CaptureInventory,
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
        && let Some((_, capture_index, _)) = numeric_conditional_reference(base, node, captures)
    {
        numbers.insert(capture_index);
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_referenced_capture_numbers(base, child, captures, numbers, child_depth);
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
    captures: &CaptureInventory,
) -> Option<(&'static str, usize, Node<'tree>)> {
    let test = named_child(node, "conditional_test")?;
    let numeric = named_child(test, "conditional_numeric_condition")?;
    let text = base.get_node_text(&numeric);
    let (reference_type, capture_index) = match text.as_bytes().first().copied() {
        Some(b'-' | b'+') => (
            "relative-condition",
            relative_capture_index(&text, captures.opened_before(node)?)?,
        ),
        _ => ("numeric-condition", text.parse::<usize>().ok()?),
    };
    (capture_index > 0).then_some((reference_type, capture_index, numeric))
}

/// PCRE2 counts relative references from the captures opened so far, so
/// `-1` is the latest opened number and `+1` the next one.
fn relative_capture_index(reference: &str, opened_before: usize) -> Option<usize> {
    let distance = reference.get(1..)?.parse::<usize>().ok()?;
    if distance == 0 {
        return None;
    }
    match reference.as_bytes().first().copied()? {
        b'-' => (opened_before + 1).checked_sub(distance),
        b'+' => opened_before.checked_add(distance),
        _ => None,
    }
}

fn named_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}
