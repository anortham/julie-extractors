//! Variable reads inside arithmetic contexts: `(( ))`, `$(( ))`, C-style `for`
//! headers, indexed-array subscripts, and `unset`.

use crate::base::BaseExtractor;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Node;

const ARITHMETIC_PARENTS: &[&str] = &[
    "binary_expression",
    "unary_expression",
    "postfix_expression",
    "parenthesized_expression",
    "ternary_expression",
    "arithmetic_expansion",
    "compound_statement",
];

/// True when `node` (a `variable_name` or bare `word`) reads a variable by name.
pub(super) fn is_arithmetic_read(
    base: &BaseExtractor,
    node: Node,
    associative_arrays: &HashSet<String>,
) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    if node.kind() == "variable_name" {
        return parent.kind() == "unset_command"
            || (ARITHMETIC_PARENTS.contains(&parent.kind()) && !inside_test_command(node));
    }
    if !is_identifier(&base.get_node_text(&node)) {
        return false;
    }
    if parent.kind() == "subscript" && parent.child_by_field_name("index") == Some(node) {
        return parent
            .child_by_field_name("name")
            .is_some_and(|array| !associative_arrays.contains(&base.get_node_text(&array)));
    }
    ARITHMETIC_PARENTS.contains(&parent.kind()) && in_c_style_for_header(node)
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn inside_test_command(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "test_command" => return true,
            "arithmetic_expansion" | "compound_statement" | "c_style_for_statement" => {
                return false;
            }
            _ => current = parent.parent(),
        }
    }
    false
}

fn in_c_style_for_header(node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "c_style_for_statement" {
            return parent
                .child_by_field_name("body")
                .is_none_or(|body| node.start_byte() < body.start_byte());
        }
        current = parent.parent();
    }
    false
}

/// Names declared as associative arrays (`declare -A`), whose subscripts are string keys.
pub(super) fn associative_array_names(base: &BaseExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_associative_arrays(base, root, 0, &mut names);
    names
}

fn collect_associative_arrays(
    base: &BaseExtractor,
    node: Node,
    depth: u32,
    names: &mut HashSet<String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "declaration_command" {
        let mut cursor = node.walk();
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        let associative = children.iter().any(|child| {
            let text = base.get_node_text(child);
            child.kind() == "word" && text.starts_with('-') && text.contains('A')
        });
        if associative {
            for child in children {
                let name = match child.kind() {
                    "variable_name" => Some(child),
                    "variable_assignment" => child.child_by_field_name("name"),
                    _ => None,
                };
                if let Some(name) = name {
                    names.insert(base.get_node_text(&name));
                }
            }
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_associative_arrays(base, child, child_depth, names);
    }
}
