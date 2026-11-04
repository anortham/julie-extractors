use std::path::Path;

use crate::ExtractionResults;
use crate::base::ComplexityMetric;

fn extract(source: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical("src/complexity.c", source, Path::new("/repo"))
        .expect("canonical C extraction should succeed")
}

fn metric<'a>(
    results: &'a ExtractionResults,
    scope: &str,
    symbol_id: Option<&str>,
) -> &'a ComplexityMetric {
    results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == scope && metric.symbol_id.as_deref() == symbol_id)
        .unwrap_or_else(|| panic!("missing {scope} metric for {symbol_id:?}"))
}

#[test]
fn complexity_metrics_count_only_syntax_owned_by_each_scope() {
    let source = r#"/* if (comment) { for (;;) {} } */
const char *label = "if (string) { for (;;) {} }";

int score(int count) {
    if (count > 0) {
        for (int i = 0; i < count; i++) {
            count--;
        }
    }
    return count;
}
"#;
    let results = extract(source);
    let score = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "score")
        .expect("expected score function");
    let function_metric = metric(&results, "symbol", Some(&score.id));
    let file_metric = metric(&results, "file", None);

    assert_eq!(results.complexity_metrics.len(), 2);
    assert_eq!(file_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(file_metric.start_byte, 0);
    assert_eq!(file_metric.end_byte as usize, source.len());
    assert_eq!(file_metric.decision_count, 1);
    assert_eq!(file_metric.loop_count, 1);
    assert_eq!(file_metric.max_nesting_depth, 2);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(function_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(function_metric.parameter_count, Some(1));
    assert_eq!(function_metric.decision_count, 1);
    assert_eq!(function_metric.loop_count, 1);
    assert_eq!(function_metric.max_nesting_depth, 2);
    let body_span = score.body_span.as_ref().expect("score body span");
    assert_eq!(function_metric.start_byte, body_span.start_byte);
    assert_eq!(function_metric.end_byte, body_span.end_byte);
}
