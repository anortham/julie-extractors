use std::collections::HashMap;

use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::{Node, Parser, Tree};

use super::embedded_span::EmbeddedSpanOffset;
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
    /// Container children that group further parameter nodes without being
    /// parameters themselves (for example Dart `optional_formal_parameters`).
    parameter_group_node_kinds: &'static [&'static str],
    /// Node kinds that hold a callable's body as a *sibling* of the symbol's
    /// declaration node (for example Dart, where symbols span only the
    /// `function_signature` and the `function_body` follows it).
    body_sibling_node_kinds: &'static [&'static str],
    /// For grammars that encode control flow as generic `call` nodes (Elixir),
    /// count a `call` whose `target` identifier matches one of these names.
    call_decision_targets: &'static [&'static str],
    /// Same as `call_decision_targets`, but for loop constructs.
    call_loop_targets: &'static [&'static str],
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
    source: &str,
    file_path: &str,
    symbols: &[Symbol],
) -> Vec<ComplexityMetric> {
    if language == "vue" {
        return collect_vue_complexity_metrics(source, file_path, symbols);
    }

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
        source,
        &root,
        config,
    ));

    for symbol in symbols.iter().filter(|symbol| is_callable(&symbol.kind)) {
        let metric_span = complexity_span_for_symbol(language, root, symbol, config);
        let parameter_count = parameter_count_for_symbol(language, source, root, symbol, config);
        metrics.push(metric_for_scope(
            file_path,
            language,
            MetricScopeInput {
                scope: "symbol",
                symbol_id: Some(symbol.id.clone()),
                span: metric_span,
                parameter_count,
            },
            source,
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
    if config_for_language(language).is_some() || matches!(language, "vue" | "sql" | "regex") {
        vec!["file", "symbol"]
    } else {
        Vec::new()
    }
}

fn metric_for_scope(
    file_path: &str,
    language: &str,
    input: MetricScopeInput,
    source: &str,
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
    collect_stats(language, *root, source, span, config, 0, &mut stats);
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
    language: &str,
    node: Node<'_>,
    source: &str,
    span: NormalizedSpan,
    config: ComplexityLanguageConfig,
    current_depth: u32,
    stats: &mut ComplexityStats,
) {
    if !overlaps(node, span) {
        return;
    }

    let decision_kind = config.decision_node_kinds.contains(&node.kind())
        || call_target_matches(node, source, config.call_decision_targets);
    let loop_kind = config.loop_node_kinds.contains(&node.kind())
        || call_target_matches(node, source, config.call_loop_targets);
    // tree-sitter-ruby nests duplicate `if`/`for` wrappers around the same
    // construct; count only the outer node when parent and child share a kind.
    let decision = contains(span, node)
        && decision_kind
        && !is_same_kind_child_wrapper(language, node, true, config, source);
    let loop_node = contains(span, node)
        && loop_kind
        && !is_same_kind_child_wrapper(language, node, false, config, source);
    let counted = decision || loop_node;
    let next_depth = if counted {
        let next = current_depth + 1;
        stats.max_nesting_depth = stats.max_nesting_depth.max(next);
        if decision {
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
        collect_stats(language, child, source, span, config, next_depth, stats);
    }
}

fn is_same_kind_child_wrapper(
    language: &str,
    node: Node<'_>,
    decision: bool,
    config: ComplexityLanguageConfig,
    source: &str,
) -> bool {
    if language != "ruby" {
        return false;
    }
    let Some(parent) = node.parent() else {
        return false;
    };
    if parent.kind() != node.kind() {
        return false;
    }
    if decision {
        config.decision_node_kinds.contains(&node.kind())
            || call_target_matches(node, source, config.call_decision_targets)
    } else {
        config.loop_node_kinds.contains(&node.kind())
            || call_target_matches(node, source, config.call_loop_targets)
    }
}

fn call_target_matches(node: Node<'_>, source: &str, targets: &[&str]) -> bool {
    if targets.is_empty() || node.kind() != "call" {
        return false;
    }
    for field in ["target", "function"] {
        let Some(name_node) = node.child_by_field_name(field) else {
            continue;
        };
        if name_node.kind() != "identifier" {
            continue;
        }
        if name_node
            .utf8_text(source.as_bytes())
            .ok()
            .is_some_and(|name| targets.contains(&name))
        {
            return true;
        }
    }
    false
}

fn parameter_count_for_symbol(
    language: &str,
    source: &str,
    root: Node<'_>,
    symbol: &Symbol,
    config: ComplexityLanguageConfig,
) -> Option<u32> {
    if language == "elixir" {
        return parameter_count_for_elixir_symbol(source, root, symbol);
    }

    let span = symbol_span(symbol);
    let container = find_first_parameter_container(root, span, config)?;
    Some(count_container_parameters(container, config))
}

fn parameter_count_for_elixir_symbol(source: &str, root: Node<'_>, symbol: &Symbol) -> Option<u32> {
    let span = symbol_span(symbol);
    let def_call = find_elixir_def_call(root, source, span)?;
    let container = elixir_function_head_arguments(def_call)?;
    Some(count_elixir_parameters(container, source))
}

fn find_elixir_def_call<'tree>(
    node: Node<'tree>,
    source: &str,
    span: NormalizedSpan,
) -> Option<Node<'tree>> {
    find_elixir_def_call_at_depth(node, source, span, 0)
}

fn find_elixir_def_call_at_depth<'tree>(
    node: Node<'tree>,
    source: &str,
    span: NormalizedSpan,
    depth: u32,
) -> Option<Node<'tree>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    if !overlaps(node, span) {
        return None;
    }
    if contains(span, node) && call_target_matches(node, source, &["def", "defp"]) {
        return Some(node);
    }
    let child_depth = child_tree_depth(depth);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(child_depth) = child_depth
            && let Some(found) = find_elixir_def_call_at_depth(child, source, span, child_depth)
        {
            return Some(found);
        }
    }
    None
}

fn elixir_function_head_arguments(def_call: Node<'_>) -> Option<Node<'_>> {
    let args = child_by_kind(def_call, "arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        match child.kind() {
            "call" => {
                if let Some(head_args) = child_by_kind(child, "arguments") {
                    return Some(head_args);
                }
            }
            "binary_operator" => {
                if let Some(left) = child.child_by_field_name("left")
                    && left.kind() == "call"
                    && let Some(head_args) = child_by_kind(left, "arguments")
                {
                    return Some(head_args);
                }
            }
            _ => {}
        }
    }
    None
}

fn count_elixir_parameters(container: Node<'_>, _source: &str) -> u32 {
    let mut count = 0;
    let mut cursor = container.walk();
    for child in container.children(&mut cursor) {
        match child.kind() {
            "identifier" => count += 1,
            "call" => count += 1,
            _ => {}
        }
    }
    count
}

fn child_by_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn count_container_parameters(container: Node<'_>, config: ComplexityLanguageConfig) -> u32 {
    count_container_parameters_at_depth(container, config, 0)
}

fn count_container_parameters_at_depth(
    container: Node<'_>,
    config: ComplexityLanguageConfig,
    depth: u32,
) -> u32 {
    if !should_visit_tree_depth(depth) {
        return 0;
    }

    let mut count = 0;
    let mut cursor = container.walk();
    for child in container.children(&mut cursor) {
        if config.parameter_node_kinds.contains(&child.kind()) {
            count += parameter_arity(child);
        } else if config.parameter_group_node_kinds.contains(&child.kind())
            && let Some(child_depth) = child_tree_depth(depth)
        {
            count += count_container_parameters_at_depth(child, config, child_depth);
        }
    }
    count
}

/// Resolve the span used for symbol-scoped complexity. Prefer callable bodies,
/// including detached sibling bodies for grammars such as Dart, and fall back to
/// the declaration span when no usable body span is available.
fn complexity_span_for_symbol(
    language: &str,
    root: Node<'_>,
    symbol: &Symbol,
    config: ComplexityLanguageConfig,
) -> NormalizedSpan {
    let declaration_span = symbol_span(symbol);
    // An erlang function symbol spans every clause of its name/arity, and a
    // `guard_clause` in a clause HEAD is a counted decision. The body span
    // starts at the first clause's `->`, so measuring it would drop the first
    // clause's guard; the declaration span is head plus body for every clause,
    // which is the scope this metric wants.
    if language == "erlang" {
        return declaration_span;
    }
    let body_span = sibling_body_span(root, symbol, config).or(symbol.body_span);
    match body_span {
        Some(body) => {
            if (language == "scala" || language == "vbnet")
                && !body_covers_meaningful_share(declaration_span, body)
            {
                declaration_span
            } else {
                body
            }
        }
        None => declaration_span,
    }
}

pub(crate) fn body_covers_meaningful_share(
    declaration_span: NormalizedSpan,
    body: NormalizedSpan,
) -> bool {
    let declaration_bytes = declaration_span
        .end_byte
        .saturating_sub(declaration_span.start_byte);
    if declaration_bytes == 0 {
        return false;
    }
    let body_bytes = body.end_byte.saturating_sub(body.start_byte);
    body_bytes * 2 >= declaration_bytes
}

fn sibling_body_span(
    root: Node<'_>,
    symbol: &Symbol,
    config: ComplexityLanguageConfig,
) -> Option<NormalizedSpan> {
    if config.body_sibling_node_kinds.is_empty() {
        return None;
    }
    let span = symbol_span(symbol);
    let mut node =
        root.descendant_for_byte_range(span.start_byte as usize, span.end_byte as usize)?;
    loop {
        let mut sibling = node.next_named_sibling();
        while let Some(candidate) = sibling {
            if config.body_sibling_node_kinds.contains(&candidate.kind()) {
                return Some(NormalizedSpan::from_node(&candidate));
            }
            sibling = candidate.next_named_sibling();
        }
        node = node.parent()?;
        if node.end_byte() as u32 > span.end_byte {
            // The ancestor extends past the symbol's declaration; siblings
            // beyond this point belong to other declarations.
            return None;
        }
    }
}

fn find_first_parameter_container<'tree>(
    node: Node<'tree>,
    span: NormalizedSpan,
    config: ComplexityLanguageConfig,
) -> Option<Node<'tree>> {
    find_first_parameter_container_at_depth(node, span, config, 0)
}

fn find_first_parameter_container_at_depth<'tree>(
    node: Node<'tree>,
    span: NormalizedSpan,
    config: ComplexityLanguageConfig,
    depth: u32,
) -> Option<Node<'tree>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }

    if !overlaps(node, span) {
        return None;
    }
    if contains(span, node) && config.parameter_container_node_kinds.contains(&node.kind()) {
        return Some(node);
    }
    let child_depth = child_tree_depth(depth);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(child_depth) = child_depth
            && let Some(found) =
                find_first_parameter_container_at_depth(child, span, config, child_depth)
        {
            return Some(found);
        }
    }
    None
}

fn parameter_arity(node: Node<'_>) -> u32 {
    // Prefer the grammar's `name` field when the parameter node declares one
    // (for example C# `parameter` nodes, where a user-defined type annotation
    // is also an `identifier` and would otherwise be counted as a declarator).
    // Only identifier-like children count: tree-sitter-swift also attaches the
    // parameter *type* under the `name` field, which would double-count.
    let mut cursor = node.walk();
    let named_count = node
        .children_by_field_name("name", &mut cursor)
        .filter(|child| {
            matches!(
                child.kind(),
                "identifier" | "simple_identifier" | "field_identifier"
            )
        })
        .count() as u32;
    if named_count > 0 {
        return named_count;
    }

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

pub(crate) fn symbol_span(symbol: &Symbol) -> NormalizedSpan {
    NormalizedSpan {
        start_line: symbol.start_line,
        start_column: symbol.start_column,
        end_line: symbol.end_line,
        end_column: symbol.end_column,
        start_byte: symbol.start_byte,
        end_byte: symbol.end_byte,
    }
}

pub(crate) fn contains(span: NormalizedSpan, node: Node<'_>) -> bool {
    node.start_byte() as u32 >= span.start_byte && node.end_byte() as u32 <= span.end_byte
}

pub(crate) fn overlaps(node: Node<'_>, span: NormalizedSpan) -> bool {
    (node.start_byte() as u32) < span.end_byte && (node.end_byte() as u32) > span.start_byte
}

fn config_for_language(language: &str) -> Option<ComplexityLanguageConfig> {
    match language {
        "c" => Some(C_LIKE_CONFIG),
        "cpp" => Some(C_LIKE_CONFIG),
        "csharp" => Some(CSHARP_CONFIG),
        "dart" => Some(DART_CONFIG),
        "go" => Some(GO_CONFIG),
        "java" => Some(JAVA_CONFIG),
        "javascript" => Some(ECMASCRIPT_CONFIG),
        "kotlin" => Some(KOTLIN_CONFIG),
        "python" => Some(PYTHON_CONFIG),
        "rust" => Some(RUST_CONFIG),
        "swift" => Some(SWIFT_CONFIG),
        "typescript" => Some(ECMASCRIPT_CONFIG),
        "tsx" => Some(ECMASCRIPT_CONFIG),
        "jsx" => Some(ECMASCRIPT_CONFIG),
        "razor" => Some(CSHARP_CONFIG),
        "zig" => Some(ZIG_CONFIG),
        "php" => Some(PHP_CONFIG),
        "ruby" => Some(RUBY_CONFIG),
        "scala" => Some(SCALA_CONFIG),
        "elixir" => Some(ELIXIR_CONFIG),
        "erlang" => Some(ERLANG_CONFIG),
        "lua" => Some(LUA_CONFIG),
        "vbnet" => Some(VBNET_CONFIG),
        "r" => Some(R_CONFIG),
        "bash" => Some(BASH_CONFIG),
        "powershell" => Some(POWERSHELL_CONFIG),
        "gdscript" => Some(GDSCRIPT_CONFIG),
        "qml" => Some(QML_CONFIG),
        _ => None,
    }
}

/// Base for struct-update syntax: fields most languages leave empty.
const DEFAULT_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[],
    loop_node_kinds: &[],
    parameter_container_node_kinds: &[],
    parameter_node_kinds: &[],
    parameter_group_node_kinds: &[],
    body_sibling_node_kinds: &[],
    call_decision_targets: &[],
    call_loop_targets: &[],
};

const RUST_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &["if_expression", "match_expression"],
    loop_node_kinds: &["for_expression", "while_expression", "loop_expression"],
    parameter_container_node_kinds: &["parameters"],
    parameter_node_kinds: &["parameter", "self_parameter"],
    ..DEFAULT_CONFIG
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
    ..DEFAULT_CONFIG
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
    ..DEFAULT_CONFIG
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
    ..DEFAULT_CONFIG
};

const CSHARP_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "switch_statement",
        "switch_section",
        "switch_expression",
        "switch_expression_arm",
        "catch_clause",
        "conditional_expression",
    ],
    loop_node_kinds: &[
        "for_statement",
        "foreach_statement",
        "while_statement",
        "do_statement",
    ],
    parameter_container_node_kinds: &["parameter_list"],
    parameter_node_kinds: &["parameter"],
    ..DEFAULT_CONFIG
};

const JAVA_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "switch_expression",
        "switch_block_statement_group",
        "switch_rule",
        "catch_clause",
        "ternary_expression",
    ],
    loop_node_kinds: &[
        "for_statement",
        "enhanced_for_statement",
        "while_statement",
        "do_statement",
    ],
    parameter_container_node_kinds: &["formal_parameters"],
    parameter_node_kinds: &["formal_parameter", "spread_parameter", "receiver_parameter"],
    ..DEFAULT_CONFIG
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
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-kotlin-ng 1.1.0 node-types.json and
// a to_sexp() parse dump. Kotlin has no ternary operator: `if` is an
// expression (`if_expression`) and covers that role. `when_expression` plus
// each `when_entry` follow the switch-container-plus-arm convention.
const KOTLIN_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_expression",
        "when_expression",
        "when_entry",
        "catch_block",
    ],
    loop_node_kinds: &["for_statement", "while_statement", "do_while_statement"],
    parameter_container_node_kinds: &["function_value_parameters"],
    parameter_node_kinds: &["parameter"],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-swift 0.7.2 node-types.json and a
// to_sexp() parse dump. The grammar has no parameter container node:
// `parameter` nodes are direct children of the declaration, so the
// declarations themselves act as containers. `guard_statement` counts as a
// decision (it is Swift's early-exit conditional). `do_statement` is the
// do/catch construct, not a loop; only its `catch_block` counts.
const SWIFT_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "guard_statement",
        "switch_statement",
        "switch_entry",
        "catch_block",
        "ternary_expression",
    ],
    loop_node_kinds: &["for_statement", "while_statement", "repeat_while_statement"],
    parameter_container_node_kinds: &[
        "function_declaration",
        "init_declaration",
        "protocol_function_declaration",
    ],
    parameter_node_kinds: &["parameter"],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-dart 0.2.0 node-types.json and a
// to_sexp() parse dump. Dart symbols span only the signature node; the body
// is a sibling `function_body`, hence `body_sibling_node_kinds`. Named and
// optional positional parameters live inside an `optional_formal_parameters`
// group nested in the `formal_parameter_list`.
const DART_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "switch_statement",
        "switch_statement_case",
        "switch_statement_default",
        "switch_expression",
        "switch_expression_case",
        "catch_clause",
        "conditional_expression",
    ],
    loop_node_kinds: &["for_statement", "while_statement", "do_statement"],
    parameter_container_node_kinds: &["formal_parameter_list"],
    parameter_node_kinds: &["formal_parameter", "super_formal_parameter"],
    parameter_group_node_kinds: &["optional_formal_parameters"],
    body_sibling_node_kinds: &["function_body"],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-zig 1.1.2 node-types.json and a
// to_sexp() parse dump. Zig `if` appears as both `if_statement` and
// `if_expression`; switch arms are `switch_case` children of `switch_expression`.
const ZIG_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_expression",
        "if_statement",
        "switch_expression",
        "switch_case",
        "catch_expression",
    ],
    loop_node_kinds: &[
        "for_expression",
        "for_statement",
        "while_expression",
        "while_statement",
    ],
    parameter_container_node_kinds: &["parameters"],
    parameter_node_kinds: &["parameter"],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-php 0.24.2 php/node-types.json and a
// to_sexp() parse dump.
const PHP_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "else_if_clause",
        "switch_statement",
        "case_statement",
        "match_expression",
        "match_conditional_expression",
        "match_default_expression",
        "catch_clause",
        "conditional_expression",
    ],
    loop_node_kinds: &[
        "for_statement",
        "foreach_statement",
        "while_statement",
        "do_statement",
    ],
    parameter_container_node_kinds: &["formal_parameters"],
    parameter_node_kinds: &[
        "simple_parameter",
        "variadic_parameter",
        "property_promotion_parameter",
    ],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-ruby 0.23.1 node-types.json and a
// to_sexp() parse dump. `case` plus each `when` follow the switch-container
// plus-arm convention; `elsif` counts separately from `if`.
const RUBY_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if",
        "elsif",
        "unless",
        "case",
        "when",
        "conditional",
        "rescue",
    ],
    loop_node_kinds: &["for", "while", "until"],
    parameter_container_node_kinds: &["method_parameters"],
    parameter_node_kinds: &[
        "identifier",
        "optional_parameter",
        "keyword_parameter",
        "splat_parameter",
        "hash_splat_parameter",
        "block_parameter",
        "destructured_parameter",
        "forward_parameter",
    ],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-scala 0.26.0 node-types.json and a
// to_sexp() parse dump. `match_expression` plus each `case_clause` follow the
// switch-container-plus-arm convention; `guard` counts as an early-exit decision.
const SCALA_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_expression",
        "match_expression",
        "case_clause",
        "type_case_clause",
        "catch_clause",
        "guard",
        "given_conditional",
    ],
    loop_node_kinds: &["for_expression", "while_expression", "do_while_expression"],
    parameter_container_node_kinds: &["parameters", "class_parameters"],
    parameter_node_kinds: &["parameter", "class_parameter"],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-elixir 0.3.5 node-types.json and a
// to_sexp() parse dump. Control-flow macros parse as generic `call` nodes, so
// `call_decision_targets`/`call_loop_targets` cover `if`, `case`, and `for`.
// `stab_clause` arms inside `case`/`cond` and `rescue_block`/`catch_block`
// inside `try` count as explicit decision nodes.
const ELIXIR_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &["stab_clause", "rescue_block", "catch_block"],
    loop_node_kinds: &[],
    parameter_container_node_kinds: &[],
    parameter_node_kinds: &[],
    call_decision_targets: &["if", "unless", "case", "cond", "with"],
    call_loop_targets: &["for"],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-erlang 0.20.0 node-types.json and a
// to_sexp() parse dump. Erlang branches inside expressions: `case`, `if`,
// `try`, `receive`, `maybe`, and the old-style `catch Expr` each count as a
// container plus one per arm, following the switch-container-plus-arm
// convention. A `guard_clause` is one `;`-separated alternative of a guard
// sequence and counts wherever a guard appears, including a clause head.
// Clause-based dispatch of a definition (`function_clause`, `fun_clause`) is
// not counted: `clause_count` already records it, and every single-clause
// anonymous fun would otherwise cost a decision. Erlang has no loop statement,
// so comprehensions are the iteration construct. Clause heads bind arbitrary
// patterns, so there is no closed set of parameter node kinds and
// `parameter_count` stays NULL; arity is carried in the symbol signature.
const ERLANG_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "case_expr",
        "cr_clause",
        "if_expr",
        "if_clause",
        "try_expr",
        "catch_clause",
        "catch_expr",
        "receive_expr",
        "receive_after",
        "maybe_expr",
        "guard_clause",
    ],
    loop_node_kinds: &[
        "list_comprehension",
        "binary_comprehension",
        "map_comprehension",
    ],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-lua 0.5.0 node-types.json and a
// to_sexp() parse dump. `elseif_statement` counts separately from `if_statement`.
const LUA_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &["if_statement", "elseif_statement"],
    loop_node_kinds: &["for_statement", "while_statement", "repeat_statement"],
    parameter_container_node_kinds: &["parameters"],
    parameter_node_kinds: &["identifier"],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-vb-dotnet (rev 25dca4a)
// node-types.json and a to_sexp() parse dump. `select_case_statement` plus
// each `case_clause` follow the switch-container-plus-arm convention;
// `elseif_clause` counts separately from `if_statement`.
const VBNET_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "elseif_clause",
        "select_case_statement",
        "case_clause",
        "catch_block",
        "conditional_expression",
    ],
    loop_node_kinds: &[
        "for_statement",
        "for_each_statement",
        "while_statement",
        "do_statement",
    ],
    parameter_container_node_kinds: &["parameter_list"],
    parameter_node_kinds: &["parameter"],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-r 1.2.0 node-types.json and a
// to_sexp() parse dump. `else if` chains parse as nested `if_statement`
// nodes in the `alternative` field rather than a separate arm kind.
// `switch(...)` parses as a `call` node whose `function` field is the
// identifier `switch`.
const R_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &["if_statement"],
    loop_node_kinds: &["for_statement", "while_statement", "repeat_statement"],
    parameter_container_node_kinds: &["parameters"],
    parameter_node_kinds: &["parameter"],
    call_decision_targets: &["switch"],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-bash 0.25.1 node-types.json and a
// to_sexp() parse dump. `elif_clause` counts separately from `if_statement`;
// the grammar has no formal parameter container on `function_definition`.
const BASH_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &["if_statement", "elif_clause", "case_statement", "case_item"],
    loop_node_kinds: &["for_statement", "c_style_for_statement", "while_statement"],
    parameter_container_node_kinds: &[],
    parameter_node_kinds: &[],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-powershell (rev d398441)
// node-types.json and a to_sexp() parse dump. `switch_statement` plus each
// `switch_clause` follow the switch-container-plus-arm convention;
// `elseif_clause` counts separately from `if_statement`.
const POWERSHELL_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "elseif_clause",
        "switch_statement",
        "switch_clause",
        "catch_clause",
    ],
    loop_node_kinds: &[
        "for_statement",
        "foreach_statement",
        "while_statement",
        "do_statement",
    ],
    parameter_container_node_kinds: &[
        "function_parameter_declaration",
        "parameter_list",
        "class_method_parameter_list",
    ],
    parameter_node_kinds: &["script_parameter", "class_method_parameter"],
    parameter_group_node_kinds: &["parameter_list"],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-gdscript 6.1.0 node-types.json and
// a to_sexp() parse dump. `elif_clause` counts separately from `if_statement`.
// `match_statement` plus each `pattern_section` follow the switch-container-
// plus-arm convention.
const GDSCRIPT_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "elif_clause",
        "conditional_expression",
        "match_statement",
        "pattern_section",
    ],
    loop_node_kinds: &["for_statement", "while_statement"],
    parameter_container_node_kinds: &["parameters"],
    parameter_node_kinds: &[
        "identifier",
        "typed_parameter",
        "typed_default_parameter",
        "default_parameter",
        "variadic_parameter",
    ],
    ..DEFAULT_CONFIG
};

// Node kinds verified against tree-sitter-qmljs (rev 606a66b) node-types.json
// and a to_sexp() parse dump. QML control flow follows the qmljs/TypeScript
// grammar; `else if` chains parse as nested `if_statement` nodes.
const QML_CONFIG: ComplexityLanguageConfig = ComplexityLanguageConfig {
    decision_node_kinds: &[
        "if_statement",
        "switch_statement",
        "switch_case",
        "switch_default",
        "catch_clause",
        "conditional_expression",
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
    ..DEFAULT_CONFIG
};

fn collect_vue_complexity_metrics(
    source: &str,
    file_path: &str,
    symbols: &[Symbol],
) -> Vec<ComplexityMetric> {
    use crate::vue::parsing::{VueSection, parse_vue_sfc};

    let Ok(sections) = parse_vue_sfc(source) else {
        return Vec::new();
    };
    let script_sections: Vec<&VueSection> = sections
        .iter()
        .filter(|section| section.section_type == "script")
        .collect();
    if script_sections.is_empty() {
        return Vec::new();
    }

    let config = ECMASCRIPT_CONFIG;
    let mut metrics = Vec::new();
    let mut file_stats = ComplexityStats::default();
    let mut file_span: Option<NormalizedSpan> = None;

    for section in &script_sections {
        let Some(tree) = parse_vue_script_tree(section) else {
            continue;
        };
        let root = tree.root_node();
        let local_span = NormalizedSpan::from_node(&root);
        let byte_start = vue_section_byte_offset(source, section.start_line);
        let Some(offset) = EmbeddedSpanOffset::from_host_byte(source, byte_start as usize) else {
            continue;
        };
        let section_file_span = offset.apply(local_span);
        let mut section_stats = ComplexityStats::default();
        collect_stats(
            "typescript",
            root,
            &section.content,
            local_span,
            config,
            0,
            &mut section_stats,
        );
        merge_complexity_stats(&mut file_stats, section_stats);
        file_span = Some(merge_spans(file_span, section_file_span));
    }

    if let Some(span) = file_span {
        metrics.push(build_complexity_metric(
            file_path,
            "vue",
            MetricScopeInput {
                scope: "file",
                symbol_id: None,
                span,
                parameter_count: None,
            },
            file_stats,
        ));
    }

    for symbol in symbols.iter().filter(|symbol| is_callable(&symbol.kind)) {
        let Some((section, byte_start)) =
            vue_script_section_for_symbol(source, &script_sections, symbol)
        else {
            continue;
        };
        let Some(tree) = parse_vue_script_tree(section) else {
            continue;
        };
        let root = tree.root_node();
        let symbol_file_span = symbol
            .body_span
            .map(|body| NormalizedSpan {
                start_line: body.start_line,
                start_column: body.start_column,
                end_line: body.end_line,
                end_column: body.end_column,
                start_byte: body.start_byte,
                end_byte: body.end_byte,
            })
            .unwrap_or_else(|| symbol_span(symbol));
        let local_span = file_span_to_section_local(symbol_file_span, byte_start);
        let mut stats = ComplexityStats::default();
        collect_stats(
            "typescript",
            root,
            &section.content,
            local_span,
            config,
            0,
            &mut stats,
        );
        let local_declaration = file_span_to_section_local(symbol_span(symbol), byte_start);
        let parameter_count = find_first_parameter_container(root, local_declaration, config)
            .map(|container| count_container_parameters(container, config));
        metrics.push(build_complexity_metric(
            file_path,
            "vue",
            MetricScopeInput {
                scope: "symbol",
                symbol_id: Some(symbol.id.clone()),
                span: symbol_file_span,
                parameter_count,
            },
            stats,
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

fn build_complexity_metric(
    file_path: &str,
    language: &str,
    input: MetricScopeInput,
    stats: ComplexityStats,
) -> ComplexityMetric {
    let MetricScopeInput {
        scope,
        symbol_id,
        span,
        parameter_count,
    } = input;
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

fn merge_complexity_stats(into: &mut ComplexityStats, from: ComplexityStats) {
    into.decision_count += from.decision_count;
    into.loop_count += from.loop_count;
    into.max_nesting_depth = into.max_nesting_depth.max(from.max_nesting_depth);
}

fn merge_spans(left: Option<NormalizedSpan>, right: NormalizedSpan) -> NormalizedSpan {
    match left {
        Some(existing) => NormalizedSpan {
            start_line: existing.start_line.min(right.start_line),
            start_column: if right.start_line < existing.start_line {
                right.start_column
            } else if existing.start_line < right.start_line {
                existing.start_column
            } else {
                existing.start_column.min(right.start_column)
            },
            end_line: existing.end_line.max(right.end_line),
            end_column: if right.end_line > existing.end_line {
                right.end_column
            } else if existing.end_line > right.end_line {
                existing.end_column
            } else {
                existing.end_column.max(right.end_column)
            },
            start_byte: existing.start_byte.min(right.start_byte),
            end_byte: existing.end_byte.max(right.end_byte),
        },
        None => right,
    }
}

fn vue_section_byte_offset(content: &str, start_line: usize) -> u32 {
    content
        .split_inclusive('\n')
        .take(start_line)
        .map(str::len)
        .sum::<usize>() as u32
}

fn parse_vue_script_tree(section: &crate::vue::parsing::VueSection) -> Option<Tree> {
    let mut parser = Parser::new();
    let lang = section.lang.as_deref().unwrap_or("js");
    let ts_lang = if lang == "ts" || lang == "typescript" {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    } else {
        tree_sitter_javascript::LANGUAGE.into()
    };
    parser.set_language(&ts_lang).ok()?;
    parser.parse(&section.content, None)
}

fn vue_script_section_for_symbol<'a>(
    content: &str,
    sections: &[&'a crate::vue::parsing::VueSection],
    symbol: &Symbol,
) -> Option<(&'a crate::vue::parsing::VueSection, u32)> {
    for section in sections {
        let byte_start = vue_section_byte_offset(content, section.start_line);
        let byte_end = byte_start.saturating_add(section.content.len() as u32);
        if symbol.start_byte >= byte_start && symbol.end_byte <= byte_end {
            return Some((*section, byte_start));
        }
    }
    None
}

fn file_span_to_section_local(span: NormalizedSpan, byte_start: u32) -> NormalizedSpan {
    NormalizedSpan {
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte.saturating_sub(byte_start),
        end_byte: span.end_byte.saturating_sub(byte_start),
    }
}
