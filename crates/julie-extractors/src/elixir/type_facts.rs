use crate::base::types::TypeNameRules;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, find_child_by_type};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

use super::helpers;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &[],
};

pub(super) fn record_struct_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    struct_name: &str,
    is_inferred: bool,
) {
    base.record_declared_type_fact_with_declared(
        symbol_id,
        struct_name,
        struct_name,
        &TYPE_NAME_RULES,
        is_inferred,
    );
}

pub(super) fn unqualified_struct_name(base: &BaseExtractor, node: Node) -> Option<String> {
    if node.kind() != "map" {
        return None;
    }
    let alias = struct_alias(node)?;
    let name = base.get_node_text(&alias);
    if name.is_empty() || name.contains('.') {
        None
    } else {
        Some(name)
    }
}

pub(super) fn struct_alias(map_node: Node) -> Option<Node> {
    let struct_node = find_child_by_type(&map_node, "struct")?;
    let mut cursor = struct_node.walk();
    struct_node
        .named_children(&mut cursor)
        .next()
        .filter(|inner| inner.kind() == "alias")
}

fn is_quote_call(base: &BaseExtractor, node: Node) -> bool {
    node.kind() == "call"
        && node.child_by_field_name("target").is_some_and(|target| {
            target.kind() == "identifier" && base.get_node_text(&target) == "quote"
        })
}

pub(super) fn extract_body_locals(
    base: &mut BaseExtractor,
    def_node: &Node,
    callable_id: &str,
    symbols: &mut Vec<Symbol>,
    depth: u32,
) {
    if let Some(block) = helpers::extract_do_block(def_node) {
        walk_assignments(base, block, callable_id, symbols, depth);
    }
    if let Some(args) = find_child_by_type(def_node, "arguments") {
        let mut cursor = args.walk();
        for child in args.named_children(&mut cursor) {
            if matches!(child.kind(), "keywords" | "do_block") {
                walk_assignments(base, child, callable_id, symbols, depth);
            }
        }
    }
}

fn walk_assignments(
    base: &mut BaseExtractor,
    node: Node,
    callable_id: &str,
    symbols: &mut Vec<Symbol>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) || is_quote_call(base, node) {
        return;
    }

    if node.kind() == "binary_operator"
        && node
            .child_by_field_name("operator")
            .is_some_and(|op| op.kind() == "=")
        && let Some(left) = node.child_by_field_name("left")
        && left.kind() == "identifier"
    {
        let name = base.get_node_text(&left);
        if name != "_" && !(name.starts_with("__") && name.ends_with("__")) {
            let signature = base.get_node_text(&node);
            let symbol = base.create_symbol(
                &left,
                name,
                SymbolKind::Variable,
                SymbolOptions {
                    signature: Some(signature),
                    parent_id: Some(callable_id.to_string()),
                    ..Default::default()
                },
            );
            if let Some(right) = node.child_by_field_name("right")
                && let Some(struct_name) = unqualified_struct_name(base, right)
            {
                record_struct_fact(base, &symbol.id, &struct_name, true);
            }
            symbols.push(symbol);
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_assignments(base, child, callable_id, symbols, child_depth);
    }
}
