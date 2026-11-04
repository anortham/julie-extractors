use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use crate::base::complexity_metrics::{body_covers_meaningful_share, symbol_span};
use crate::base::kinds::SymbolKind;
use crate::base::span::NormalizedSpan;
use crate::base::types::{ComplexityMetric, Symbol, stable_location_id};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

use super::context::{XSLT_NS, declared_namespaces, tag_name_text, tag_of};

const ALGORITHM_ID: &str = "julie-ast-complexity-v1";
const DECISION_SIGNAL_KINDS: &str = "if,when";
const LOOP_SIGNAL_KINDS: &str = "for-each,for-each-group,iterate";
const NESTING_SIGNAL_KINDS: &str = "if,when,for-each,for-each-group,iterate";

#[derive(Clone, Copy, Default)]
struct ComplexityStats {
    decision_count: u32,
    loop_count: u32,
    max_nesting_depth: u32,
}

impl ComplexityStats {
    fn add(&mut self, decision: bool, loop_signal: bool, depth: u32) {
        if decision {
            self.decision_count += 1;
        }
        if loop_signal {
            self.loop_count += 1;
        }
        if decision || loop_signal {
            self.max_nesting_depth = self.max_nesting_depth.max(depth);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct CallableStats {
    recognized: bool,
    parameter_count: u32,
    complexity: ComplexityStats,
}

struct TraversalState {
    namespace_bindings: HashMap<String, Vec<String>>,
    namespace_scopes: Vec<Vec<String>>,
    element_starts: Vec<u32>,
    active_callables: Vec<Option<usize>>,
    symbol_by_start: HashMap<u32, usize>,
    file_complexity: ComplexityStats,
    callable_stats: Vec<CallableStats>,
    contains_xslt: bool,
    #[cfg(test)]
    visited_nodes: usize,
    #[cfg(test)]
    namespace_lookups: usize,
    #[cfg(test)]
    namespace_binding_pushes: usize,
    #[cfg(test)]
    namespace_binding_pops: usize,
}

pub(crate) fn collect_complexity_metrics(
    tree: &Tree,
    source: &str,
    file_path: &str,
    symbols: &[Symbol],
) -> Vec<ComplexityMetric> {
    let state = collect(tree, source, symbols);
    if !state.contains_xslt {
        return Vec::new();
    }

    let mut metrics = vec![metric(
        file_path,
        "file",
        None,
        NormalizedSpan::from_node(&tree.root_node()),
        state.file_complexity,
        None,
    )];

    for (index, symbol) in symbols.iter().enumerate() {
        let callable = state.callable_stats[index];
        if !callable.recognized {
            continue;
        }
        metrics.push(metric(
            file_path,
            "symbol",
            Some(symbol.id.as_str()),
            symbol_metric_span(symbol),
            callable.complexity,
            Some(callable.parameter_count),
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

#[cfg(test)]
fn traversal_operation_counts(tree: &Tree, source: &str, symbols: &[Symbol]) -> [usize; 4] {
    let state = collect(tree, source, symbols);
    [
        state.visited_nodes,
        state.namespace_lookups,
        state.namespace_binding_pushes,
        state.namespace_binding_pops,
    ]
}

fn collect(tree: &Tree, source: &str, symbols: &[Symbol]) -> TraversalState {
    let symbol_by_start = symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| symbol.kind == SymbolKind::Function)
        .map(|(index, symbol)| (symbol.start_byte, index))
        .collect();
    let mut state = TraversalState {
        namespace_bindings: HashMap::new(),
        namespace_scopes: Vec::new(),
        element_starts: Vec::new(),
        active_callables: Vec::new(),
        symbol_by_start,
        file_complexity: ComplexityStats::default(),
        callable_stats: vec![CallableStats::default(); symbols.len()],
        contains_xslt: false,
        #[cfg(test)]
        visited_nodes: 0,
        #[cfg(test)]
        namespace_lookups: 0,
        #[cfg(test)]
        namespace_binding_pushes: 0,
        #[cfg(test)]
        namespace_binding_pops: 0,
    };

    walk(tree.root_node(), source, symbols, &mut state, 0, 0, 0);
    state
}

fn walk(
    node: Node<'_>,
    source: &str,
    symbols: &[Symbol],
    state: &mut TraversalState,
    tree_depth: u32,
    file_control_depth: u32,
    callable_control_depth: u32,
) {
    if !should_visit_tree_depth(tree_depth) {
        return;
    }

    #[cfg(test)]
    {
        state.visited_nodes += 1;
    }

    let mut next_file_control_depth = file_control_depth;
    let mut next_callable_control_depth = callable_control_depth;
    let mut opened_callable = false;
    let is_element = node.kind() == "element";

    if is_element {
        let parent_element_start = state.element_starts.last().copied();
        let mut scope = Vec::new();
        for (prefix, uri) in declared_namespaces(source, node) {
            state
                .namespace_bindings
                .entry(prefix.clone())
                .or_default()
                .push(uri);
            #[cfg(test)]
            {
                state.namespace_binding_pushes += 1;
            }
            scope.push(prefix);
        }
        state.namespace_scopes.push(scope);
        state.element_starts.push(node.start_byte() as u32);

        if let Some(local_name) = xslt_local_name(source, node, state) {
            state.contains_xslt |= is_xslt_content_element(local_name);
            if is_xslt_callable(local_name) {
                let symbol_index = state
                    .symbol_by_start
                    .get(&(node.start_byte() as u32))
                    .copied();
                if let Some(index) = symbol_index {
                    state.callable_stats[index].recognized = true;
                }
                state.active_callables.push(symbol_index);
                opened_callable = true;
                next_callable_control_depth = 0;
            }

            if local_name == "param"
                && let Some(Some(index)) = state.active_callables.last()
                && parent_element_start == Some(symbols[*index].start_byte)
            {
                state.callable_stats[*index].parameter_count += 1;
            }

            let decision = is_xslt_decision(local_name);
            let loop_signal = is_xslt_loop(local_name);
            if decision || loop_signal {
                next_file_control_depth += 1;
                state
                    .file_complexity
                    .add(decision, loop_signal, next_file_control_depth);
                if let Some(Some(index)) = state.active_callables.last() {
                    next_callable_control_depth += 1;
                    state.callable_stats[*index].complexity.add(
                        decision,
                        loop_signal,
                        next_callable_control_depth,
                    );
                }
            }
        }
    }

    if let Some(child_depth) = child_tree_depth(tree_depth) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            walk(
                child,
                source,
                symbols,
                state,
                child_depth,
                next_file_control_depth,
                next_callable_control_depth,
            );
        }
    }

    if opened_callable {
        state.active_callables.pop();
    }
    if is_element {
        state.element_starts.pop();
        let scope = state
            .namespace_scopes
            .pop()
            .expect("element namespace scope");
        for prefix in scope {
            let remove_prefix = state
                .namespace_bindings
                .get_mut(&prefix)
                .is_some_and(|uris| {
                    uris.pop();
                    uris.is_empty()
                });
            if remove_prefix {
                state.namespace_bindings.remove(&prefix);
            }
            #[cfg(test)]
            {
                state.namespace_binding_pops += 1;
            }
        }
    }
}

fn xslt_local_name<'a>(
    source: &'a str,
    element: Node<'_>,
    state: &mut TraversalState,
) -> Option<&'a str> {
    let tag = tag_of(element)?;
    let name = tag_name_text(source, tag)?;
    let (prefix, local_name) = name
        .split_once(':')
        .map_or(("", name), |(prefix, local_name)| (prefix, local_name));
    #[cfg(test)]
    {
        state.namespace_lookups += 1;
    }
    let namespace = state.namespace_bindings.get(prefix)?.last()?;
    (namespace == XSLT_NS).then_some(local_name)
}

fn is_xslt_content_element(local_name: &str) -> bool {
    matches!(
        local_name,
        "stylesheet"
            | "transform"
            | "package"
            | "template"
            | "function"
            | "if"
            | "choose"
            | "when"
            | "otherwise"
            | "for-each"
            | "for-each-group"
            | "iterate"
    )
}

fn is_xslt_callable(local_name: &str) -> bool {
    matches!(local_name, "template" | "function")
}

fn is_xslt_decision(local_name: &str) -> bool {
    matches!(local_name, "if" | "when")
}

fn is_xslt_loop(local_name: &str) -> bool {
    matches!(local_name, "for-each" | "for-each-group" | "iterate")
}

fn symbol_metric_span(symbol: &Symbol) -> NormalizedSpan {
    let declaration = symbol_span(symbol);
    let Some(body) = symbol.body_span else {
        return declaration;
    };
    let body = NormalizedSpan {
        start_line: body.start_line,
        start_column: body.start_column,
        end_line: body.end_line,
        end_column: body.end_column,
        start_byte: body.start_byte,
        end_byte: body.end_byte,
    };
    if body_covers_meaningful_share(declaration, body) {
        body
    } else {
        declaration
    }
}

fn metric(
    file_path: &str,
    scope: &str,
    symbol_id: Option<&str>,
    span: NormalizedSpan,
    stats: ComplexityStats,
    parameter_count: Option<u32>,
) -> ComplexityMetric {
    let identity = symbol_id.unwrap_or("file");
    let metadata = HashMap::from([
        (
            "metric_version".to_string(),
            serde_json::Value::Number(serde_json::Number::from(1)),
        ),
        (
            "decision_signal_kinds".to_string(),
            serde_json::Value::String(DECISION_SIGNAL_KINDS.to_string()),
        ),
        (
            "loop_signal_kinds".to_string(),
            serde_json::Value::String(LOOP_SIGNAL_KINDS.to_string()),
        ),
        (
            "nesting_signal_kinds".to_string(),
            serde_json::Value::String(NESTING_SIGNAL_KINDS.to_string()),
        ),
    ]);

    ComplexityMetric {
        id: stable_location_id(file_path, &format!("complexity:{scope}:{identity}"), span),
        file_path: file_path.to_string(),
        language: "xml".to_string(),
        scope: scope.to_string(),
        symbol_id: symbol_id.map(str::to_string),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_shadow_churn_keeps_visits_bindings_and_lookups_linear() {
        let counts = [8, 16, 32].map(|depth| {
            let mut source = String::from(
                "<x:stylesheet xmlns:x=\"http://www.w3.org/1999/XSL/Transform\" xmlns:q=\"urn:base\">",
            );
            for index in 0..depth {
                source.push_str(&format!(
                    "<q:group xmlns:q=\"urn:scope:{index}\">"
                ));
            }
            source.push_str(
                "<x:template name=\"deep\"><x:if test=\"1\"><x:value-of select=\".\"/></x:if></x:template>",
            );
            for _ in 0..depth {
                source.push_str("</q:group>");
            }
            source.push_str("</x:stylesheet>");
            let tree = crate::tests::xml::support::parse(&source);
            assert!(
                !tree.root_node().has_error(),
                "{}",
                tree.root_node().to_sexp().chars().take(800).collect::<String>()
            );
            let symbols = crate::tests::xml::support::extract_from("scale.xslt", &source);
            let metrics = collect_complexity_metrics(&tree, &source, "scale.xslt", &symbols);
            assert_eq!(metrics.len(), 2);
            let callable = symbols
                .iter()
                .find(|symbol| symbol.name == "deep")
                .expect("nested XSLT template symbol");
            let symbol_metric = metrics
                .iter()
                .find(|metric| metric.symbol_id.as_deref() == Some(&callable.id))
                .expect("nested XSLT callable metric");
            assert_eq!(
                (symbol_metric.decision_count, symbol_metric.loop_count),
                (1, 0)
            );
            let body = callable.body_span.expect("template body span");
            assert_eq!(
                (symbol_metric.start_byte, symbol_metric.end_byte),
                (body.start_byte, body.end_byte)
            );
            assert_eq!(
                &source[symbol_metric.start_byte as usize..symbol_metric.end_byte as usize],
                "<x:if test=\"1\"><x:value-of select=\".\"/></x:if>"
            );
            (depth, traversal_operation_counts(&tree, &source, &symbols))
        });

        assert_eq!(counts[0].0 * 2, counts[1].0);
        assert_eq!(counts[1].0 * 2, counts[2].0);
        for operations in counts.map(|(_, operations)| operations) {
            assert_eq!(operations[2], operations[3]);
            assert!(operations[0] > 0);
            assert!(operations[1] > 0);
            assert!(operations[2] > 0);
        }
        for (depth, operations) in counts {
            assert_eq!(operations[1], depth + 4);
            assert_eq!(operations[2], depth + 2);
        }
        assert!(counts[1].1[0] * 10 <= counts[0].1[0] * 25);
        assert!(counts[2].1[0] * 10 <= counts[1].1[0] * 25);
        assert!(counts[1].1[1] * 10 <= counts[0].1[1] * 25);
        assert!(counts[2].1[1] * 10 <= counts[1].1[1] * 25);
        assert!(counts[1].1[2] * 10 <= counts[0].1[2] * 25);
        assert!(counts[2].1[2] * 10 <= counts[1].1[2] * 25);
    }
}
