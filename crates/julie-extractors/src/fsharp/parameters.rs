use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

use super::types;

struct PatternBinding<'a> {
    name: String,
    span: Node<'a>,
    type_node: Option<Node<'a>>,
}

pub(super) fn extract_parameter_symbols(
    base: &mut BaseExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    for binding in pattern_bindings(base, callable_node) {
        let signature = base.get_node_text(&binding.span);
        let metadata = HashMap::from([(
            "role".to_string(),
            serde_json::Value::String("parameter".to_string()),
        )]);
        symbols.push(base.create_symbol(
            &binding.span,
            binding.name,
            SymbolKind::Variable,
            SymbolOptions {
                signature: Some(signature),
                parent_id: Some(callable_id.to_string()),
                metadata: Some(metadata),
                ..Default::default()
            },
        ));
    }
    symbols
}

pub(super) fn record_parameter_facts(base: &mut BaseExtractor, callable_node: Node, symbols: &[Symbol]) {
    for binding in pattern_bindings(base, callable_node) {
        let Some(type_node) = binding.type_node else {
            continue;
        };
        let Some(symbol) = symbols.iter().find(|symbol| {
            symbol.name == binding.name
                && symbol.start_byte == binding.span.start_byte() as u32
                && symbol.end_byte == binding.span.end_byte() as u32
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("role"))
                    .is_some_and(|role| role == "parameter")
        }) else {
            continue;
        };
        types::record_type_node(base, &symbol.id, type_node, false);
    }
}

fn pattern_bindings<'a>(base: &BaseExtractor, callable_node: Node<'a>) -> Vec<PatternBinding<'a>> {
    let mut bindings = Vec::new();
    for root in pattern_roots(callable_node) {
        collect_bindings(base, root, None, None, 0, &mut bindings);
    }
    bindings
}

fn pattern_roots(node: Node<'_>) -> Vec<Node<'_>> {
    match node.kind() {
        "declaration_expression" => {
            let mut cursor = node.walk();
            node.children(&mut cursor)
                .find(|child| child.kind() == "function_or_value_defn")
                .map(pattern_roots)
                .unwrap_or_default()
        }
        "function_or_value_defn" => {
            let mut cursor = node.walk();
            let left = node
                .children(&mut cursor)
                .find(|child| child.kind() == "function_declaration_left");
            let Some(left) = left else {
                return Vec::new();
            };
            let mut left_cursor = left.walk();
            left.children(&mut left_cursor)
                .filter(|child| child.kind() == "argument_patterns")
                .collect()
        }
        "member_defn" => {
            let mut cursor = node.walk();
            let Some(definition) = node
                .children(&mut cursor)
                .find(|child| child.kind() == "method_or_prop_defn")
            else {
                return Vec::new();
            };
            let mut args = Vec::new();
            for i in 0..definition.child_count() {
                if definition.field_name_for_child(i as u32) == Some("args")
                    && let Some(child) = definition.child(i as u32)

                {
                    args.push(child);
                }
            }
            args
        }
        _ => Vec::new(),
    }
}

fn collect_bindings<'a>(
    base: &BaseExtractor,
    node: Node<'a>,
    inherited_type: Option<Node<'a>>,
    span: Option<Node<'a>>,
    depth: u32,
    out: &mut Vec<PatternBinding<'a>>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "typed_pattern" => {
            let Some((pattern, type_node)) = typed_pattern_parts(node) else {
                return;
            };
            collect_bindings(base, pattern, Some(type_node), Some(node), depth, out);
        }
        "identifier" => {
            let name = base.get_node_text(&node);
            let name = name.trim();
            if name.is_empty() || name == "_" {
                return;
            }
            out.push(PatternBinding {
                name: name.to_string(),
                span: span.unwrap_or(node),
                type_node: inherited_type,
            });
        }
        "long_identifier" | "long_identifier_or_op" | "identifier_pattern" => {
            if let Some(name_node) = terminal_identifier(node) {
                collect_bindings(base, name_node, inherited_type, span.or(Some(node)), depth, out);
            }
        }
        _ => {
            let Some(child_depth) = child_tree_depth(depth) else {
                return;
            };
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_bindings(base, child, inherited_type, span, child_depth, out);
            }
        }
    }
}

fn typed_pattern_parts(node: Node<'_>) -> Option<(Node<'_>, Node<'_>)> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    let type_node = children.iter().copied().rev().find(is_type_node)?;
    let pattern = children
        .iter()
        .copied()
        .find(|child| child.id() != type_node.id())?;
    Some((pattern, type_node))
}

fn is_type_node(node: &Node<'_>) -> bool {
    matches!(
        node.kind(),
        "simple_type"
            | "generic_type"
            | "atomic_type"
            | "compound_type"
            | "constrained_type"
            | "flexible_type"
            | "function_type"
            | "list_type"
            | "paren_type"
            | "postfix_type"
            | "static_type"
            | "struct_type"
            | "tuple_type"
            | "type_name"
            | "types"
            | "type_argument"
            | "anon_record_type"
    )
}

fn terminal_identifier(node: Node<'_>) -> Option<Node<'_>> {
    if node.kind() == "identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children.into_iter().rev().find_map(terminal_identifier)
}
