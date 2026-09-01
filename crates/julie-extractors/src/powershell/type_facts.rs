use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

use super::helpers::{find_class_name_node, find_command_name_node};

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['['],
};

const EXPR_WRAPPERS: &[&str] = &[
    "pipeline",
    "pipeline_chain",
    "logical_expression",
    "bitwise_expression",
    "comparison_expression",
    "additive_expression",
    "multiplicative_expression",
    "format_expression",
    "range_expression",
    "array_literal_expression",
    "unary_expression",
    "expression_with_unary_operator",
];

pub(super) fn record_declared_type_literal(base: &mut BaseExtractor, symbol_id: &str, node: Node) {
    let Some(type_node) = find_first_kind(node, "type_literal", 0) else {
        return;
    };
    record_type_literal(base, symbol_id, type_node, false);
}

pub(super) fn record_assignment_facts(base: &mut BaseExtractor, symbol_id: &str, node: Node) {
    if let Some(left) = direct_child(node, "left_assignment_expression")
        && let Some(type_node) = find_first_kind(left, "type_literal", 0)
    {
        record_type_literal(base, symbol_id, type_node, false);
    }

    if let Some(value) = node.child_by_field_name("value") {
        record_inferred_rhs(base, symbol_id, value, node);
    }
}

pub(super) fn enclosing_class_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if candidate.kind() == "class_statement" {
            return find_class_name_node(candidate).map(|n| base.get_node_text(&n));
        }
        current = candidate.parent();
    }
    None
}

pub(super) fn this_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let variable = direct_child(node, "variable")?;
    let name = strip_variable_name(&base.get_node_text(&variable));
    if !name.eq_ignore_ascii_case("this") {
        return None;
    }
    enclosing_class_name(base, node)
}

pub(super) fn invocation_member_name<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
) -> Option<(Node<'a>, String)> {
    let member_name = direct_child(node, "member_name")?;
    let simple = find_first_kind(member_name, "simple_name", 0)?;
    Some((simple, base.get_node_text(&simple)))
}

pub(super) fn assignment_variable_node(node: Node) -> Option<Node> {
    let left = direct_child(node, "left_assignment_expression").unwrap_or(node);
    find_first_kind(left, "variable", 0)
}

fn record_type_literal(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    is_inferred: bool,
) {
    let declared = base.get_node_text(&type_node);
    let Some(base_text) = inner_type_text(&declared) else {
        return;
    };
    if base_text.eq_ignore_ascii_case("void") {
        return;
    }
    base.record_declared_type_fact_with_declared(
        symbol_id,
        base_text,
        declared.trim(),
        &TYPE_NAME_RULES,
        is_inferred,
    );
}

fn record_inferred_rhs(base: &mut BaseExtractor, symbol_id: &str, value: Node, origin: Node) {
    let core = unwrap_expr(value);
    if let Some(type_name) = inferred_constructor_name(base, core, origin) {
        base.record_declared_type_fact_with_declared(
            symbol_id,
            &type_name,
            &type_name,
            &TYPE_NAME_RULES,
            true,
        );
    }
}

fn inferred_constructor_name(base: &BaseExtractor, core: Node, origin: Node) -> Option<String> {
    match core.kind() {
        "invokation_expression" | "invocation_expression" => {
            let (_, member) = invocation_member_name(base, core)?;
            if !member.eq_ignore_ascii_case("new") {
                return None;
            }
            let type_node = direct_child(core, "type_literal")?;
            let declared = base.get_node_text(&type_node);
            let inner = inner_type_text(&declared)?;
            if inner.contains('.') {
                return None;
            }
            same_file_class(base, origin, inner).then(|| inner.to_string())
        }
        "command" | "command_expression" => new_object_type_name(base, core, origin),
        _ => None,
    }
}

fn new_object_type_name(base: &BaseExtractor, command: Node, origin: Node) -> Option<String> {
    let name_node = find_command_name_node(command)?;
    if !base
        .get_node_text(&name_node)
        .eq_ignore_ascii_case("New-Object")
    {
        return None;
    }
    let elements = direct_child(command, "command_elements")?;
    let mut cursor = elements.walk();
    for child in elements.children(&mut cursor) {
        if matches!(
            child.kind(),
            "command_argument_sep" | "command_parameter" | "redirection"
        ) {
            continue;
        }
        let text = base.get_node_text(&child).trim().to_string();
        if text.is_empty() || text.starts_with('-') {
            continue;
        }
        if text.contains('.') {
            return None;
        }
        return same_file_class(base, origin, &text).then_some(text);
    }
    None
}

fn same_file_class(base: &BaseExtractor, origin: Node, name: &str) -> bool {
    find_class_named(file_root(origin), name, base, 0)
}

fn file_root(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

fn find_class_named(node: Node, name: &str, base: &BaseExtractor, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.kind() == "class_statement"
        && find_class_name_node(node)
            .is_some_and(|n| base.get_node_text(&n).eq_ignore_ascii_case(name))
    {
        return true;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| find_class_named(child, name, base, child_depth))
}

fn unwrap_expr(node: Node) -> Node {
    let mut current = node;
    loop {
        if !EXPR_WRAPPERS.contains(&current.kind()) {
            return current;
        }
        let Some(child) = first_named_child(current) else {
            return current;
        };
        current = child;
    }
}

fn inner_type_text(declared: &str) -> Option<&str> {
    let trimmed = declared.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .map(str::trim)
        .filter(|inner| !inner.is_empty())?;
    if inner.eq_ignore_ascii_case("void") {
        return None;
    }
    Some(inner)
}

fn strip_variable_name(raw: &str) -> String {
    raw.replace('$', "")
        .replace("Global:", "")
        .replace("Script:", "")
        .replace("Local:", "")
        .replace("Using:", "")
}

fn direct_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}

fn find_first_kind<'a>(node: Node<'a>, kind: &str, depth: u32) -> Option<Node<'a>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.kind() == kind {
        return Some(node);
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_first_kind(child, kind, child_depth) {
            return Some(found);
        }
    }
    None
}
