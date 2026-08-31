use std::collections::HashMap;

use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Tree};

use super::attach_containing_symbols;
#[cfg(all(test, feature = "test-capability-matrix"))]
use super::code_structural_facts::code_structural_fact_pattern_ids_for_language;
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

const FSHARP_PATTERNS: &[StructuralPattern] = &[StructuralPattern {
    pattern_id: "fsharp.attribute.v1",
    capture_name: "attribute",
    node_kinds: &["attribute"],
    query_family: "metadata",
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
    collect_node(
        tree.root_node(),
        language,
        file_path,
        patterns,
        &mut facts,
        0,
    );
    attach_containing_symbols(&mut facts, symbols);
    if language == "fsharp" {
        attach_fsharp_attribute_symbols(&mut facts, symbols);
    }
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
    pattern_ids.extend(code_structural_fact_pattern_ids_for_language(language));
    pattern_ids.extend(framework_structural_fact_pattern_ids_for_language(language));
    pattern_ids.extend(web_structural_fact_pattern_ids_for_language(language));
    pattern_ids.extend(data_structural_fact_pattern_ids_for_language(language));
    pattern_ids.extend(sql_structural_fact_pattern_ids_for_language(language));
    if language == "rust" {
        pattern_ids.push("rust.doc_test.v1");
    }
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
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    for pattern in patterns {
        if pattern.node_kinds.contains(&node.kind()) {
            facts.push(fact_for_node(file_path, language, node, *pattern));
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_node(child, language, file_path, patterns, facts, child_depth);
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

fn attach_fsharp_attribute_symbols(facts: &mut [StructuralFact], symbols: &[Symbol]) {
    for fact in facts {
        if fact.pattern_id != "fsharp.attribute.v1" {
            continue;
        }
        if let Some(containing_symbol_id) = fact.containing_symbol_id.as_deref()
            && symbols
                .iter()
                .any(|symbol| symbol.id == containing_symbol_id && !symbol.annotations.is_empty())
        {
            continue;
        }
        let parent_symbol_id = fact.containing_symbol_id.as_deref();
        let annotated_symbol = symbols
            .iter()
            .filter(|symbol| !symbol.annotations.is_empty() && symbol.start_byte >= fact.end_byte)
            .filter(|symbol| match parent_symbol_id {
                Some(parent_symbol_id) => symbol.parent_id.as_deref() == Some(parent_symbol_id),
                None => symbol.parent_id.is_none(),
            })
            .min_by_key(|symbol| (symbol.start_byte, symbol.end_byte))
            .or_else(|| {
                symbols
                    .iter()
                    .filter(|symbol| {
                        !symbol.annotations.is_empty() && symbol.start_byte >= fact.end_byte
                    })
                    .min_by_key(|symbol| (symbol.start_byte, symbol.end_byte))
            });
        fact.containing_symbol_id = annotated_symbol.map(|symbol| symbol.id.clone());
    }
}

fn patterns_for_language(language: &str) -> &'static [StructuralPattern] {
    match language {
        "c" => C_PATTERNS,
        "cpp" => CPP_PATTERNS,
        "fsharp" => FSHARP_PATTERNS,
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
