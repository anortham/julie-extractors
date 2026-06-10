use std::collections::HashMap;

use tree_sitter::{Node, Tree};

#[cfg(all(test, feature = "test-capability-matrix"))]
use super::data_structural_facts::data_structural_fact_pattern_ids_for_language;
#[cfg(all(test, feature = "test-capability-matrix"))]
use super::framework_structural_facts::framework_structural_fact_pattern_ids_for_language;
use super::span::NormalizedSpan;
#[cfg(all(test, feature = "test-capability-matrix"))]
use super::sql_structural_facts::sql_structural_fact_pattern_ids_for_language;
use super::types::{StructuralFact, Symbol, stable_location_id};
#[cfg(all(test, feature = "test-capability-matrix"))]
use super::web_structural_facts::web_structural_fact_pattern_ids_for_language;

#[derive(Debug, Clone, Copy)]
struct StructuralPattern {
    pattern_id: &'static str,
    capture_name: &'static str,
    node_kinds: &'static [&'static str],
    query_family: &'static str,
}

const RUST_PATTERNS: &[StructuralPattern] = &[StructuralPattern {
    pattern_id: "rust.unsafe_block.v1",
    capture_name: "unsafe_block",
    node_kinds: &["unsafe_block"],
    query_family: "safety",
}];

const GO_PATTERNS: &[StructuralPattern] = &[
    StructuralPattern {
        pattern_id: "go.goroutine_launch.v1",
        capture_name: "go_statement",
        node_kinds: &["go_statement"],
        query_family: "concurrency",
    },
    StructuralPattern {
        pattern_id: "go.defer_statement.v1",
        capture_name: "defer_statement",
        node_kinds: &["defer_statement"],
        query_family: "lifecycle",
    },
];

const PYTHON_PATTERNS: &[StructuralPattern] = &[StructuralPattern {
    pattern_id: "python.decorated_definition.v1",
    capture_name: "decorated_definition",
    node_kinds: &["decorated_definition"],
    query_family: "metadata",
}];

const JAVASCRIPT_PATTERNS: &[StructuralPattern] = &[StructuralPattern {
    pattern_id: "javascript.await_expression.v1",
    capture_name: "await_expression",
    node_kinds: &["await_expression"],
    query_family: "async",
}];

const JSX_PATTERNS: &[StructuralPattern] = &[StructuralPattern {
    pattern_id: "jsx.await_expression.v1",
    capture_name: "await_expression",
    node_kinds: &["await_expression"],
    query_family: "async",
}];

const TYPESCRIPT_PATTERNS: &[StructuralPattern] = &[StructuralPattern {
    pattern_id: "typescript.await_expression.v1",
    capture_name: "await_expression",
    node_kinds: &["await_expression"],
    query_family: "async",
}];

const TSX_PATTERNS: &[StructuralPattern] = &[StructuralPattern {
    pattern_id: "tsx.await_expression.v1",
    capture_name: "await_expression",
    node_kinds: &["await_expression"],
    query_family: "async",
}];

const C_PATTERNS: &[StructuralPattern] = &[StructuralPattern {
    pattern_id: "c.preprocessor_definition.v1",
    capture_name: "preprocessor_definition",
    node_kinds: &["preproc_def", "preproc_function_def"],
    query_family: "preprocessor",
}];

const CPP_PATTERNS: &[StructuralPattern] = &[StructuralPattern {
    pattern_id: "cpp.preprocessor_definition.v1",
    capture_name: "preprocessor_definition",
    node_kinds: &["preproc_def", "preproc_function_def"],
    query_family: "preprocessor",
}];

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
    sort_structural_facts(&mut facts);
    facts
}

pub(crate) fn sort_structural_facts(facts: &mut [StructuralFact]) {
    facts.sort_by(|left, right| {
        left.start_byte
            .cmp(&right.start_byte)
            .then(left.end_byte.cmp(&right.end_byte))
            .then(left.pattern_id.cmp(&right.pattern_id))
            .then(left.capture_name.cmp(&right.capture_name))
            .then(left.id.cmp(&right.id))
    });
}

#[cfg(all(test, feature = "test-capability-matrix"))]
pub(crate) fn structural_fact_pattern_ids_for_language(language: &str) -> Vec<&'static str> {
    let mut pattern_ids = patterns_for_language(language)
        .iter()
        .map(|pattern| pattern.pattern_id)
        .collect::<Vec<_>>();
    pattern_ids.extend(framework_structural_fact_pattern_ids_for_language(language));
    pattern_ids.extend(web_structural_fact_pattern_ids_for_language(language));
    pattern_ids.extend(data_structural_fact_pattern_ids_for_language(language));
    pattern_ids.extend(sql_structural_fact_pattern_ids_for_language(language));
    pattern_ids.sort();
    pattern_ids.dedup();
    pattern_ids
}

fn collect_node(
    node: Node<'_>,
    language: &str,
    file_path: &str,
    patterns: &[StructuralPattern],
    facts: &mut Vec<StructuralFact>,
) {
    for pattern in patterns {
        if pattern.node_kinds.contains(&node.kind()) {
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
        "c" => C_PATTERNS,
        "cpp" => CPP_PATTERNS,
        "go" => GO_PATTERNS,
        "javascript" => JAVASCRIPT_PATTERNS,
        "jsx" => JSX_PATTERNS,
        "python" => PYTHON_PATTERNS,
        "rust" => RUST_PATTERNS,
        "tsx" => TSX_PATTERNS,
        "typescript" => TYPESCRIPT_PATTERNS,
        _ => &[],
    }
}
