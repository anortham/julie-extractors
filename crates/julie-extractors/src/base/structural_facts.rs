use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use super::span::NormalizedSpan;
use super::types::{StructuralFact, Symbol, stable_location_id};

#[derive(Debug, Clone, Copy)]
struct StructuralPattern {
    pattern_id: &'static str,
    capture_name: &'static str,
    node_kind: &'static str,
    query_family: &'static str,
}

pub fn collect_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let patterns = patterns_for_language(language);
    if patterns.is_empty() {
        return Vec::new();
    }

    let mut facts = Vec::new();
    collect_node(tree.root_node(), language, file_path, patterns, &mut facts);
    attach_containing_symbols(&mut facts, symbols);
    facts.sort_by(|left, right| {
        left.start_byte
            .cmp(&right.start_byte)
            .then(left.end_byte.cmp(&right.end_byte))
            .then(left.pattern_id.cmp(&right.pattern_id))
            .then(left.capture_name.cmp(&right.capture_name))
            .then(left.id.cmp(&right.id))
    });
    facts
}

fn collect_node(
    node: Node<'_>,
    language: &str,
    file_path: &str,
    patterns: &[StructuralPattern],
    facts: &mut Vec<StructuralFact>,
) {
    for pattern in patterns {
        if node.kind() == pattern.node_kind {
            facts.push(fact_for_node(file_path, language, node, *pattern));
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_node(child, language, file_path, patterns, facts);
    }
}

fn fact_for_node(
    file_path: &str,
    language: &str,
    node: Node<'_>,
    pattern: StructuralPattern,
) -> StructuralFact {
    let span = NormalizedSpan::from_node(&node);
    let metadata = HashMap::from([
        (
            "pattern_version".to_string(),
            serde_json::Value::Number(serde_json::Number::from(1)),
        ),
        (
            "query_family".to_string(),
            serde_json::Value::String(pattern.query_family.to_string()),
        ),
    ]);

    StructuralFact {
        id: stable_location_id(
            file_path,
            &format!("{}:{}", pattern.pattern_id, pattern.capture_name),
            span,
        ),
        file_path: file_path.to_string(),
        language: language.to_string(),
        pattern_id: pattern.pattern_id.to_string(),
        capture_name: pattern.capture_name.to_string(),
        node_kind: node.kind().to_string(),
        containing_symbol_id: None,
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        confidence: 1.0,
        metadata: Some(metadata),
    }
}

fn attach_containing_symbols(facts: &mut [StructuralFact], symbols: &[Symbol]) {
    for fact in facts {
        fact.containing_symbol_id = containing_symbol_id(fact, symbols);
    }
}

fn containing_symbol_id(fact: &StructuralFact, symbols: &[Symbol]) -> Option<String> {
    symbols
        .iter()
        .filter(|symbol| symbol.start_byte <= fact.start_byte && symbol.end_byte >= fact.end_byte)
        .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
        .map(|symbol| symbol.id.clone())
}

fn patterns_for_language(language: &str) -> &'static [StructuralPattern] {
    match language {
        "rust" => &[StructuralPattern {
            pattern_id: "rust.unsafe_block.v1",
            capture_name: "unsafe_block",
            node_kind: "unsafe_block",
            query_family: "safety",
        }],
        _ => &[],
    }
}
