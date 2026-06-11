use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use crate::base::complexity_metrics::{
    body_covers_meaningful_share, contains, overlaps, symbol_span,
};
use crate::base::kinds::SymbolKind;
use crate::base::span::NormalizedSpan;
use crate::base::types::{ComplexityMetric, Symbol, stable_location_id};

const ALGORITHM_ID: &str = "julie-sql-complexity-v1";

#[derive(Default)]
struct SqlComplexityStats {
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
    source: &str,
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
        source,
        &root,
    ));

    for symbol in symbols.iter().filter(|symbol| is_callable(&symbol.kind)) {
        let span = metric_span_for_symbol(tree, source, symbol);
        metrics.push(metric_for_scope(
            file_path,
            MetricScopeInput {
                scope: "symbol",
                symbol_id: Some(symbol.id.clone()),
                span,
            },
            source,
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

fn metric_for_scope(
    file_path: &str,
    input: MetricScopeInput,
    source: &str,
    root: &Node<'_>,
) -> ComplexityMetric {
    let mut stats = SqlComplexityStats::default();
    collect_stats(*root, source, input.span, 0, &mut stats);
    let identity = input.symbol_id.as_deref().unwrap_or("file");
    let metadata = HashMap::from([
        (
            "metric_version".to_string(),
            serde_json::Value::Number(serde_json::Number::from(1)),
        ),
        (
            "decision_signal_kinds".to_string(),
            serde_json::Value::String(
                "join,set_operation,case,where,having,error_where".to_string(),
            ),
        ),
    ]);

    ComplexityMetric {
        id: stable_location_id(
            file_path,
            &format!("complexity:{}:{identity}", input.scope),
            input.span,
        ),
        file_path: file_path.to_string(),
        language: "sql".to_string(),
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
    source: &str,
    span: NormalizedSpan,
    current_depth: u32,
    stats: &mut SqlComplexityStats,
) {
    if !overlaps(node, span) {
        return;
    }

    let kind = node.kind();
    let query_container = matches!(kind, "select" | "subquery" | "cte" | "with_clause");
    let mut next_depth = current_depth;

    if contains(span, node) {
        let text = node.utf8_text(source.as_bytes()).unwrap_or("");
        if is_leaf_decision_signal(node, source, span, kind, text) {
            stats.decision_count += 1;
        }
        if kind == "while" {
            stats.loop_count += 1;
        }
        if query_container {
            next_depth = current_depth + 1;
            stats.max_nesting_depth = stats.max_nesting_depth.max(next_depth);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_stats(child, source, span, next_depth, stats);
    }
}

fn is_decision_signal(kind: &str, text: &str) -> bool {
    match kind {
        "join" | "union" | "intersect" | "except" | "case" | "when" | "where" | "having" => true,
        "ERROR" => is_error_predicate_fragment(text),
        _ => false,
    }
}

fn is_error_predicate_fragment(text: &str) -> bool {
    let upper = text.trim_start().to_ascii_uppercase();
    upper.starts_with("WHERE ") || upper.starts_with("JOIN ")
}

fn is_leaf_decision_signal(
    node: Node<'_>,
    source: &str,
    span: NormalizedSpan,
    kind: &str,
    text: &str,
) -> bool {
    if !is_decision_signal(kind, text) {
        return false;
    }
    if kind != "ERROR" {
        return true;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !contains(span, child) {
            continue;
        }
        let child_text = child.utf8_text(source.as_bytes()).unwrap_or("");
        if is_decision_signal(child.kind(), child_text) {
            return false;
        }
    }
    true
}

fn is_callable(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    )
}

fn metric_span_for_symbol(tree: &Tree, source: &str, symbol: &Symbol) -> NormalizedSpan {
    let declaration_span = symbol_span(symbol);
    if let Some(body) = symbol.body_span
        && body_covers_meaningful_share(declaration_span, body)
    {
        return body;
    }
    if let Some(block_span) = enclosing_routine_body_span(tree, source, declaration_span) {
        return block_span;
    }
    declaration_span
}

fn enclosing_routine_body_span(
    tree: &Tree,
    source: &str,
    declaration_span: NormalizedSpan,
) -> Option<NormalizedSpan> {
    let anchor = declaration_span.start_byte as usize;
    let node = tree
        .root_node()
        .descendant_for_byte_range(anchor, anchor.saturating_add(1))?;
    let mut current = Some(node);
    let mut best: Option<NormalizedSpan> = None;
    while let Some(node) = current {
        if let Some(span) = routine_body_span_from_node(node, source) {
            let contains_declaration_start = span.start_byte <= declaration_span.start_byte
                && span.end_byte > declaration_span.start_byte;
            if contains_declaration_start {
                let span_bytes = span.end_byte.saturating_sub(span.start_byte);
                let best_bytes = best
                    .map(|candidate| candidate.end_byte.saturating_sub(candidate.start_byte))
                    .unwrap_or(0);
                if span_bytes > best_bytes {
                    best = Some(span);
                }
            }
        }
        current = node.parent();
    }
    best
}

fn routine_body_span_from_node(node: Node<'_>, source: &str) -> Option<NormalizedSpan> {
    if matches!(
        node.kind(),
        "compound_statement"
            | "block"
            | "begin_end"
            | "create_trigger"
            | "create_function"
            | "create_procedure"
            | "create_function_statement"
    ) {
        return Some(NormalizedSpan::from_node(&node));
    }

    if node.kind() != "ERROR" {
        return None;
    }

    let text = node.utf8_text(source.as_bytes()).ok()?;
    if !is_sql_routine_declaration_text(text) {
        return None;
    }

    let upper = text.to_ascii_uppercase();
    if upper.contains("BEGIN") && upper.contains("END") {
        return Some(NormalizedSpan::from_node(&node));
    }

    if upper.contains("BEGIN")
        && node
            .parent()
            .is_some_and(|parent| parent.kind() == "program")
    {
        return expand_split_routine_span(node, source);
    }

    None
}

fn is_sql_routine_declaration_text(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    upper.contains("CREATE TRIGGER")
        || upper.contains("CREATE FUNCTION")
        || upper.contains("CREATE PROCEDURE")
}

fn expand_split_routine_span(start_node: Node<'_>, source: &str) -> Option<NormalizedSpan> {
    let start_byte = start_node.start_byte();
    let mut end_byte = start_node.end_byte();
    let mut sibling = start_node.next_sibling();
    while let Some(sib) = sibling {
        end_byte = sib.end_byte();
        let chunk = source.get(start_byte..end_byte)?;
        if chunk.to_ascii_uppercase().contains("END;") {
            break;
        }
        sibling = sib.next_sibling();
    }
    NormalizedSpan::from_content_range(source, start_byte, end_byte)
}
