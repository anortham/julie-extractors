use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, find_child_by_type};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

use super::type_facts;

pub(super) fn extract_parameter_symbols(
    base: &mut BaseExtractor,
    def_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let Some(head_args) = function_head_arguments(&def_node) else {
        return Vec::new();
    };

    let mut binds = Vec::new();
    collect_pattern_binds(base, head_args, &mut binds, 0);

    binds
        .into_iter()
        .map(|bind| {
            let name = base.get_node_text(&bind.name_node);
            let signature = base.get_node_text(&bind.pattern_node);
            let metadata = HashMap::from([(
                "role".to_string(),
                serde_json::Value::String("parameter".to_string()),
            )]);
            let symbol = base.create_symbol(
                &bind.pattern_node,
                name,
                SymbolKind::Variable,
                SymbolOptions {
                    signature: Some(signature),
                    parent_id: Some(callable_id.to_string()),
                    metadata: Some(metadata),
                    ..Default::default()
                },
            );
            if let Some(struct_name) = bind.declared_struct {
                type_facts::record_struct_fact(base, &symbol.id, &struct_name, false);
            }
            symbol
        })
        .collect()
}

struct PatternBind<'a> {
    name_node: Node<'a>,
    pattern_node: Node<'a>,
    declared_struct: Option<String>,
}

fn function_head_arguments<'a>(def_node: &Node<'a>) -> Option<Node<'a>> {
    let args = find_child_by_type(def_node, "arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        match child.kind() {
            "call" => return find_child_by_type(&child, "arguments"),
            "binary_operator" => {
                if let Some(left) = child.child_by_field_name("left")
                    && left.kind() == "call"
                {
                    return find_child_by_type(&left, "arguments");
                }
            }
            _ => {}
        }
    }
    None
}

fn collect_pattern_binds<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
    out: &mut Vec<PatternBind<'a>>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "identifier" => push_identifier_bind(base, node, node, None, out),
        "binary_operator" => collect_match_binds(base, node, out, depth),
        "unary_operator" => collect_unary_binds(base, node, out, depth),
        "call" | "dot" => {}
        _ => collect_child_binds(base, node, out, depth),
    }
}

fn collect_match_binds<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
    out: &mut Vec<PatternBind<'a>>,
    depth: u32,
) {
    let is_eq = node
        .child_by_field_name("operator")
        .is_some_and(|op| op.kind() == "=");
    if !is_eq {
        collect_child_binds(base, node, out, depth);
        return;
    }

    let left = node.child_by_field_name("left");
    let right = node.child_by_field_name("right");
    let struct_name = left.and_then(|left| type_facts::unqualified_struct_name(base, left));

    if let Some(right) = right {
        if right.kind() == "identifier" {
            push_identifier_bind(base, right, node, struct_name, out);
        } else {
            collect_pattern_binds(base, right, out, depth);
        }
    }
    if let Some(left) = left {
        collect_pattern_binds(base, left, out, depth);
    }
}

fn collect_unary_binds<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
    out: &mut Vec<PatternBind<'a>>,
    depth: u32,
) {
    let op = node.child_by_field_name("operator").map(|op| op.kind());
    if op == Some("^") {
        return;
    }
    collect_child_binds(base, node, out, depth);
}

fn collect_child_binds<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
    out: &mut Vec<PatternBind<'a>>,
    depth: u32,
) {
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_pattern_binds(base, child, out, child_depth);
    }
}

fn push_identifier_bind<'a>(
    base: &BaseExtractor,
    name_node: Node<'a>,
    pattern_node: Node<'a>,
    declared_struct: Option<String>,
    out: &mut Vec<PatternBind<'a>>,
) {
    let name = base.get_node_text(&name_node);
    if name == "_" || (name.starts_with("__") && name.ends_with("__")) {
        return;
    }
    out.push(PatternBind {
        name_node,
        pattern_node,
        declared_struct,
    });
}
