use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use super::kinds::SymbolKind;
use super::span::NormalizedSpan;
use super::types::{ComplexityMetric, Symbol, stable_location_id};

const ALGORITHM_ID: &str = "julie-ast-complexity-v1";

#[derive(Debug, Clone, Copy)]
struct ComplexityLanguageConfig {
    decision_node_kinds: &'static [&'static str],
    loop_node_kinds: &'static [&'static str],
    parameter_container_node_kinds: &'static [&'static str],
    parameter_node_kinds: &'static [&'static str],
}

#[derive(Default)]
struct ComplexityStats {
    decision_count: u32,
    loop_count: u32,
    max_nesting_depth: u32,
}

struct MetricScopeInput {
    scope: &'static str,
    symbol_id: Option<String>,
    span: NormalizedSpan,
    parameter_count: Option<u32>,
}

pub fn collect_complexity_metrics(
    language: &str,
    tree: &Tree,
    file_path: &str,
    symbols: &[Symbol],
) -> Vec<ComplexityMetric> {
    let Some(config) = config_for_language(language) else {
        return Vec::new();
    };

    let mut metrics = Vec::new();
    let root = tree.root_node();
    let file_span = NormalizedSpan::from_node(&root);
    metrics.push(metric_for_scope(
        file_path,
        language,
        MetricScopeInput {
            scope: "file",
            symbol_id: None,
            span: file_span,
            parameter_count: None,
        },
        &root,
        config,
    ));

    for symbol in symbols.iter().filter(|symbol| is_callable(&symbol.kind)) {
        let metric_span = symbol.body_span.unwrap_or_else(|| symbol_span(symbol));
        let parameter_count = parameter_count_for_symbol(root, symbol, config);
        metrics.push(metric_for_scope(
            file_path,
            language,
            MetricScopeInput {
                scope: "symbol",
                symbol_id: Some(symbol.id.clone()),
                span: metric_span,
                parameter_count,
            },
            &root,
            config,
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

#[cfg(all(test, feature = "test-capability-matrix"))]
pub(crate) fn complexity_metric_scopes_for_language(language: &str) -> Vec<&'static str> {
    if config_for_language(language).is_some() {
        vec!["file", "symbol"]
    } else {
        Vec::new()
    }
}

fn metric_for_scope(
    file_path: &str,
    language: &str,
    input: MetricScopeInput,
    root: &Node<'_>,
    config: ComplexityLanguageConfig,
) -> ComplexityMetric {
    let mut stats = ComplexityStats::default();
    let MetricScopeInput {
        scope,
        symbol_id,
        span,
        parameter_count,
    } = input;
    collect_stats(*root, span, config, 0, &mut stats);
    let identity = symbol_id.as_deref().unwrap_or("file");
    let metadata = HashMap::from([(
        "metric_version".to_string(),
        serde_json::Value::Number(serde_json::Number::from(1)),
    )]);

    ComplexityMetric {
        id: stable_location_id(file_path, &format!("complexity:{scope}:{identity}"), span),
        file_path: file_path.to_string(),
        language: language.to_string(),
        scope: scope.to_string(),
        symbol_id,
        algorithm_id: ALGORITHM_ID.to_string(),
        covered_lines: span.end_line.saturating_sub(span.start_line) + 1,
        covered_bytes: span.end_byte.saturating_sub(span.start_byte),
        decision_count: stats.decision_count,
        loop_count: stats.loop_count,
        max_nesting_depth: stats.max_nesting_depth,
        parameter_count,
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        metadata: Some(metadata),
    }
}

fn collect_stats(
    node: Node<'_>,
    span: NormalizedSpan,
    config: ComplexityLanguageConfig,
    current_depth: u32,
    stats: &mut ComplexityStats,
) {
    if !overlaps(node, span) {
        return;
    }

    let counted = contains(span, node)
        && (config.decision_node_kinds.contains(&node.kind())
            || config.loop_node_kinds.contains(&node.kind()));
    let next_depth = if counted {
        let next = current_depth + 1;
        stats.max_nesting_depth = stats.max_nesting_depth.max(next);
        if config.decision_node_kinds.contains(&node.kind()) {
            stats.decision_count += 1;
        } else {
            stats.loop_count += 1;
        }
        next
    } else {
        current_depth
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_stats(child, span, config, next_depth, stats);
    }
}

fn parameter_count_for_symbol(
    root: Node<'_>,
    symbol: &Symbol,
    config: ComplexityLanguageConfig,
) -> Option<u32> {
    let span = symbol_span(symbol);
    let container = find_first_parameter_container(root, span, config)?;
    let mut count = 0;
    let mut cursor = container.walk();
    for child in container.children(&mut cursor) {
        if config.parameter_node_kinds.contains(&child.kind()) {
            count += parameter_arity(child);
        }
    }
    Some(count)
}

fn find_first_parameter_container<'tree>(
    node: Node<'tree>,
    span: NormalizedSpan,
    config: ComplexityLanguageConfig,
) -> Option<Node<'tree>> {
    if !overlaps(node, span) {
        return None;
    }
    if contains(span, node) && config.parameter_container_node_kinds.contains(&node.kind()) {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_first_parameter_container(child, span, config) {
            return Some(found);
        }
    }
    None
}

fn parameter_arity(node: Node<'_>) -> u32 {
    let mut declarator_count = 0;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "identifier" | "field_identifier") {
            declarator_count += 1;
        }
    }
    declarator_count.max(1)
}

fn is_callable(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function
            | SymbolKind::Method
            | SymbolKind::Constructor
            | SymbolKind::Destructor
            | SymbolKind::Operator
    )
}

fn symbol_span(symbol: &Symbol) -> NormalizedSpan {
    NormalizedSpan {
        start_line: symbol.start_line,
        start_column: symbol.start_column,
        end_line: symbol.end_line,
        end_column: symbol.end_column,
        start_byte: symbol.start_byte,
        end_byte: symbol.end_byte,
    }
}

fn contains(span: NormalizedSpan, node: Node<'_>) -> bool {
    node.start_byte() as u32 >= span.start_byte && node.end_byte() as u32 <= span.end_byte
}

fn overlaps(node: Node<'_>, span: NormalizedSpan) -> bool {
    (node.start_byte() as u32) < span.end_byte && (node.end_byte() as u32) > span.start_byte
}

fn config_for_language(language: &str) -> Option<ComplexityLanguageConfig> {
    match language {
        "c" => Some(C_LIKE_CONFIG),
        "cpp" => Some(C_LIKE_CONFIG),
        "go" => Some(GO_CONFIG),
        "javascript" => Some(ECMASCRIPT_CONFIG),
        "python" => Some(PYTHON_CONFIG),
        "rust" => Some(RUST_CONFIG),
        "typescript" => Some(ECMASCRIPT_CONFIG),
        _ => None,
    }
}

const RUST_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &["if_expression", "match_expression"],
    loop_node_kinds: &["for_expression", "while_expression", "loop_expression"],
    parameter_container_node_kinds: &["parameters"],
    parameter_node_kinds: &["parameter", "self_parameter"],
};

const GO_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "switch_statement",
        "type_switch_statement",
        "select_statement",
    ],
    loop_node_kinds: &["for_statement"],
    parameter_container_node_kinds: &["parameter_list"],
    parameter_node_kinds: &["parameter_declaration"],
};

const PYTHON_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "elif_clause",
        "try_statement",
        "except_clause",
        "match_statement",
        "case_clause",
        "conditional_expression",
    ],
    loop_node_kinds: &["for_statement", "while_statement"],
    parameter_container_node_kinds: &["parameters"],
    parameter_node_kinds: &[
        "identifier",
        "default_parameter",
        "typed_parameter",
        "typed_default_parameter",
        "list_splat_pattern",
        "dictionary_splat_pattern",
    ],
};

const ECMASCRIPT_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "switch_statement",
        "switch_case",
        "catch_clause",
        "ternary_expression",
    ],
    loop_node_kinds: &[
        "for_statement",
        "for_in_statement",
        "while_statement",
        "do_statement",
    ],
    parameter_container_node_kinds: &["formal_parameters"],
    parameter_node_kinds: &[
        "identifier",
        "required_parameter",
        "optional_parameter",
        "assignment_pattern",
        "rest_pattern",
    ],
};

const C_LIKE_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "switch_statement",
        "case_statement",
        "conditional_expression",
    ],
    loop_node_kinds: &["for_statement", "while_statement", "do_statement"],
    parameter_container_node_kinds: &["parameter_list"],
    parameter_node_kinds: &["parameter_declaration", "optional_parameter_declaration"],
};
