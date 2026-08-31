use super::FSharpExtractor;
use crate::base::{BaseExtractor, NormalizedSpan, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

pub(super) fn collect_literals(extractor: &mut FSharpExtractor, root: Node, symbols: &[Symbol]) {
    walk(extractor, root, symbols, 0);
}

fn walk(extractor: &mut FSharpExtractor, node: Node, symbols: &[Symbol], depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "const"
        && let Some(literal_node) = first_named_child(node)
        && is_supported_literal(literal_node.kind())
    {
        let containing_symbol_id = extractor
            .base()
            .find_containing_symbol(&literal_node, symbols)
            .map(|symbol| symbol.id.clone());
        let raw = extractor.base().get_node_text(&literal_node);
        let text = decode_literal_text(extractor.base(), &literal_node, &raw);
        extractor.base().record_literal_at_span(
            NormalizedSpan::from_node(&literal_node),
            text,
            None,
            0,
            containing_symbol_id,
        );
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(extractor, child, symbols, child_depth);
    }
}

fn is_supported_literal(kind: &str) -> bool {
    matches!(
        kind,
        "string"
            | "triple_quoted_string"
            | "verbatim_string"
            | "char"
            | "bytearray"
            | "verbatim_bytearray"
            | "int"
            | "float"
            | "decimal"
            | "bool"
            | "unit"
    )
}

fn decode_literal_text(base: &BaseExtractor, node: &Node, raw: &str) -> String {
    match node.kind() {
        "string"
        | "triple_quoted_string"
        | "verbatim_string"
        | "bytearray"
        | "verbatim_bytearray"
        | "char" => strip_delimiters(raw),
        _ => {
            let text = base.get_node_text(node);
            if text.is_empty() {
                raw.to_string()
            } else {
                text
            }
        }
    }
}

fn strip_delimiters(raw: &str) -> String {
    let trimmed = raw.trim();
    for delimiter in ["\"\"\"", "@\"", "\"", "'", "@'", "[<"] {
        if trimmed.starts_with(delimiter) && trimmed.ends_with(delimiter) {
            return trimmed[delimiter.len()..trimmed.len() - delimiter.len()].to_string();
        }
    }
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0] as char;
        let last = trimmed.as_bytes()[trimmed.len() - 1] as char;
        if matches!((first, last), ('"', '"') | ('\'', '\'')) {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }
    trimmed.to_string()
}

fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}
