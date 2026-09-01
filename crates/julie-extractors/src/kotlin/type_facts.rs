//! Declared-type fact recording for Kotlin.

use super::helpers;
use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Node;

pub(super) const KOTLIN_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &[],
    generic_open: &['<'],
};

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

pub(super) fn record_constructor_call(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value: Node,
    type_names: &HashSet<String>,
) {
    if value.kind() != "call_expression" {
        return;
    }
    let children: Vec<Node> = {
        let mut cursor = value.walk();
        value.children(&mut cursor).collect()
    };
    if children
        .iter()
        .any(|child| child.kind() == "navigation_expression")
    {
        return;
    }
    let Some(callee) = children
        .iter()
        .find(|child| child.kind() == "identifier" || child.kind() == "simple_identifier")
    else {
        return;
    };
    let name = helpers::strip_backticks(&base.get_node_text(callee)).to_string();
    if !type_names.contains(&name) {
        return;
    }
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &name,
        &name,
        &KOTLIN_TYPE_NAME_RULES,
        true,
    );
}

pub(super) fn record_property_facts(
    base: &mut BaseExtractor,
    symbol_id: &str,
    node: Node,
    type_names: &HashSet<String>,
) {
    if let Some(type_node) = property_type_node(node) {
        record_declared_type(base, symbol_id, type_node);
        return;
    }
    if let Some(initializer) = property_initializer_node(base, node) {
        record_constructor_call(base, symbol_id, initializer, type_names);
    }
}

pub(super) fn declared_type_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|child| {
        matches!(
            child.kind(),
            "user_type" | "type" | "nullable_type" | "type_reference" | "function_type"
        )
    })
}

pub(super) fn collect_type_names(base: &BaseExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_type_names_into(base, root, 0, &mut names);
    names
}

fn collect_type_names_into(
    base: &BaseExtractor,
    node: Node,
    depth: u32,
    names: &mut HashSet<String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if matches!(
        node.kind(),
        "class_declaration" | "object_declaration" | "enum_declaration"
    ) && let Some((name, _)) = helpers::declared_name(base, &node)
    {
        names.insert(name);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_type_names_into(base, child, child_depth, names);
    }
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    let Some(base_name) = base_type_name(base, type_node) else {
        return;
    };
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &KOTLIN_TYPE_NAME_RULES,
        is_inferred,
    );
}

fn base_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let core = unwrap_type_wrappers(node)?;
    match core.kind() {
        "user_type" => user_type_base_name(base, core),
        "identifier" | "simple_identifier" => {
            Some(helpers::strip_backticks(&base.get_node_text(&core)).to_string())
        }
        _ => None,
    }
}

fn unwrap_type_wrappers(node: Node) -> Option<Node> {
    let mut current = node;
    for _ in 0..8 {
        match current.kind() {
            "type" | "type_reference" | "nullable_type" | "parenthesized_type"
            | "non_nullable_type" => {
                let mut cursor = current.walk();
                current = current.named_children(&mut cursor).next()?;
            }
            _ => return Some(current),
        }
    }
    Some(current)
}

fn user_type_base_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let children: Vec<Node> = {
        let mut cursor = node.walk();
        node.children(&mut cursor).collect()
    };
    let identifiers: Vec<String> = children
        .iter()
        .filter(|child| child.kind() == "identifier" || child.kind() == "simple_identifier")
        .map(|child| helpers::strip_backticks(&base.get_node_text(child)).to_string())
        .collect();
    if identifiers.is_empty() {
        None
    } else {
        Some(identifiers.join("."))
    }
}

fn property_type_node(node: Node) -> Option<Node> {
    let var_decl = {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find(|child| child.kind() == "variable_declaration")
    };
    if let Some(var_decl) = var_decl
        && let Some(type_node) = declared_type_child(var_decl)
    {
        return Some(type_node);
    }
    declared_type_child(node)
}

fn property_initializer_node<'a>(base: &BaseExtractor, node: Node<'a>) -> Option<Node<'a>> {
    let children: Vec<Node<'a>> = {
        let mut cursor = node.walk();
        node.children(&mut cursor).collect()
    };
    let assignment_index = children
        .iter()
        .position(|child| base.get_node_text(child) == "=")?;
    children.get(assignment_index + 1).copied()
}
