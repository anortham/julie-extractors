//! Vue adapter for call-style JavaScript/TypeScript test roles.
//!
//! Vue parses each embedded script independently, delegates role materialization
//! to the shared JS/TS `test_calls` seam, then remaps the resulting symbols back
//! into the host SFC. This keeps framework vocabulary and role metadata out of
//! the Vue extractor.

use super::parsing::VueSection;
use crate::base::{BaseExtractor, EmbeddedSpanOffset, NormalizedSpan, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Parser};

pub(super) fn extract_script_test_symbols(
    base: &BaseExtractor,
    section: &VueSection,
) -> Vec<Symbol> {
    let Some(tree) = parse_script_section(section) else {
        return Vec::new();
    };

    let mut embedded_base = BaseExtractor::new(
        base.language.clone(),
        base.file_path.clone(),
        section.content.clone(),
        std::path::Path::new(""),
    );
    embedded_base.file_path = base.file_path.clone();

    let mut symbols = Vec::new();
    walk_test_calls(&mut embedded_base, tree.root_node(), &mut symbols, None, 0);

    let byte_offset = section_byte_offset(&base.content, section.start_line);
    let Some(offset) = EmbeddedSpanOffset::from_host_byte(&base.content, byte_offset) else {
        return Vec::new();
    };
    remap_to_host(&mut symbols, base, offset);
    symbols
}

fn parse_script_section(section: &VueSection) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    let language = match section.lang.as_deref() {
        Some("ts" | "typescript") => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        _ => tree_sitter_javascript::LANGUAGE.into(),
    };
    parser.set_language(&language).ok()?;
    parser.parse(&section.content, None)
}

fn walk_test_calls(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &mut Vec<Symbol>,
    parent_container_id: Option<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let mut next_parent_id = parent_container_id.clone();
    if node.kind() == "call_expression"
        && let Some(symbol) =
            crate::test_calls::extract_test_call(base, node, parent_container_id.as_deref())
    {
        let is_container = symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("test_container"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        if is_container {
            next_parent_id = Some(symbol.id.clone());
        }
        symbols.push(symbol);
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_test_calls(base, child, symbols, next_parent_id.clone(), child_depth);
    }
}

fn remap_to_host(symbols: &mut [Symbol], base: &BaseExtractor, offset: EmbeddedSpanOffset) {
    let mut symbol_id_map = HashMap::new();
    for symbol in symbols.iter_mut() {
        let old_id = symbol.id.clone();
        symbol.file_path = base.file_path.clone();
        symbol.language = base.language.clone();
        let span = NormalizedSpan {
            start_line: symbol.start_line,
            start_column: symbol.start_column,
            end_line: symbol.end_line,
            end_column: symbol.end_column,
            start_byte: symbol.start_byte,
            end_byte: symbol.end_byte,
        };
        symbol.apply_normalized_span(offset.apply(span));
        symbol.body_span = symbol.body_span.map(|span| offset.apply(span));
        symbol.code_context = base.extract_code_context(
            symbol.start_line.saturating_sub(1) as usize,
            symbol.end_line.saturating_sub(1) as usize,
        );
        symbol.refresh_id();
        symbol_id_map.insert(old_id, symbol.id.clone());
    }

    for symbol in symbols {
        if let Some(parent_id) = symbol.parent_id.as_mut()
            && let Some(host_parent_id) = symbol_id_map.get(parent_id)
        {
            *parent_id = host_parent_id.clone();
        }
    }
}

fn section_byte_offset(content: &str, start_line: usize) -> usize {
    content
        .split_inclusive('\n')
        .take(start_line)
        .map(str::len)
        .sum()
}
