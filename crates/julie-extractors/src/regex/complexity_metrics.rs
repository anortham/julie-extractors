use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use crate::base::complexity_metrics::{
    body_covers_meaningful_share, contains, overlaps, symbol_span,
};
use crate::base::kinds::SymbolKind;
use crate::base::span::NormalizedSpan;
use crate::base::types::{ComplexityMetric, Symbol, stable_location_id};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const ALGORITHM_ID: &str = "julie-regex-complexity-v1";

#[derive(Default)]
struct RegexComplexityStats {
    decision_count: u32,
    loop_count: u32,
    max_nesting_depth: u32,
}

struct MetricScopeInput {
    scope: &'static str,
    symbol_id: Option<String>,
    span: NormalizedSpan,
}

pub fn collect_complexity_metrics(
    tree: &Tree,
    file_path: &str,
    symbols: &[Symbol],
) -> Vec<ComplexityMetric> {
    let mut metrics = Vec::new();
    let root = tree.root_node();
    let file_span = NormalizedSpan::from_node(&root);
    metrics.push(metric_for_scope(
        file_path,
        MetricScopeInput {
            scope: "file",
            symbol_id: None,
            span: file_span,
        },
        &root,
    ));

    for symbol in symbols.iter().filter(|symbol| is_callable(&symbol.kind)) {
        let span = metric_span_for_symbol(symbol);
        metrics.push(metric_for_scope(
            file_path,
            MetricScopeInput {
                scope: "symbol",
                symbol_id: Some(symbol.id.clone()),
                span,
            },
            &root,
        ));
    }

    metrics.sort_by(|left, right| {
        left.start_byte
            .cmp(&right.start_byte)
            .then(left.end_byte.cmp(&right.end_byte))
            .then(left.scope.cmp(&right.scope))
            .then(left.symbol_id.cmp(&right.symbol_id))
            .then(left.id.cmp(&right.id))
    });
    metrics
}

fn metric_for_scope(file_path: &str, input: MetricScopeInput, root: &Node<'_>) -> ComplexityMetric {
    let mut stats = RegexComplexityStats::default();
    collect_stats(*root, input.span, 0, 0, &mut stats);
    let identity = input.symbol_id.as_deref().unwrap_or("file");
    let metadata = HashMap::from([
        (
            "metric_version".to_string(),
            serde_json::Value::Number(serde_json::Number::from(1)),
        ),
        (
            "decision_signal_kinds".to_string(),
            serde_json::Value::String("alternation,conditional".to_string()),
        ),
        (
            "loop_signal_kinds".to_string(),
            serde_json::Value::String("quantifier".to_string()),
        ),
        (
            "nesting_signal_kinds".to_string(),
            serde_json::Value::String("group,lookaround".to_string()),
        ),
    ]);

    ComplexityMetric {
        id: stable_location_id(
            file_path,
            &format!("complexity:{}:{identity}", input.scope),
            input.span,
        ),
        file_path: file_path.to_string(),
        language: "regex".to_string(),
        scope: input.scope.to_string(),
        symbol_id: input.symbol_id,
        algorithm_id: ALGORITHM_ID.to_string(),
        covered_lines: input.span.end_line.saturating_sub(input.span.start_line) + 1,
        covered_bytes: input.span.end_byte.saturating_sub(input.span.start_byte),
        decision_count: stats.decision_count,
        loop_count: stats.loop_count,
        max_nesting_depth: stats.max_nesting_depth,
        parameter_count: None,
        start_line: input.span.start_line,
        start_column: input.span.start_column,
        end_line: input.span.end_line,
        end_column: input.span.end_column,
        start_byte: input.span.start_byte,
        end_byte: input.span.end_byte,
        metadata: Some(metadata),
    }
}

fn collect_stats(
    node: Node<'_>,
    span: NormalizedSpan,
    current_depth: u32,
    tree_depth: u32,
    stats: &mut RegexComplexityStats,
) {
    if !should_visit_tree_depth(tree_depth) {
        return;
    }

    if !overlaps(node, span) {
        return;
    }

    let kind = node.kind();
    let group_like = matches!(
        kind,
        "group"
            | "capturing_group"
            | "anonymous_capturing_group"
            | "named_capturing_group"
            | "non_capturing_group"
            | "lookahead_assertion"
            | "lookbehind_assertion"
            | "positive_lookahead"
            | "negative_lookahead"
            | "positive_lookbehind"
            | "negative_lookbehind"
    );
    let mut next_depth = current_depth;

    if contains(span, node) {
        if matches!(kind, "alternation" | "disjunction" | "conditional") {
            stats.decision_count += 1;
        }
        if matches!(
            kind,
            "quantifier"
                | "quantified_expression"
                | "zero_or_more"
                | "one_or_more"
                | "optional"
                | "count_quantifier"
        ) {
            stats.loop_count += 1;
        }
    }
    if contains(span, node) && group_like {
        next_depth = current_depth + 1;
        stats.max_nesting_depth = stats.max_nesting_depth.max(next_depth);
    }

    let Some(child_depth) = child_tree_depth(tree_depth) else {
        return;
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_stats(child, span, next_depth, child_depth, stats);
    }
}

fn is_callable(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    )
}

fn metric_span_for_symbol(symbol: &Symbol) -> NormalizedSpan {
    let declaration_span = symbol_span(symbol);
    match symbol.body_span {
        Some(body) if body_covers_meaningful_share(declaration_span, body) => body,
        _ => declaration_span,
    }
}
