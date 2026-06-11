use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn gdscript_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (3): if, elif, inner if
    //   loops (2): for, while
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (3): widget, count, enabled
    let source = r#"
func evaluate(widget: String, count: int, enabled: bool) -> int:
    var total = 0
    if enabled:
        for i in range(1, count + 1):
            if i % 2 == 0:
                total += i
    elif count > 0:
        total = 1
    while total > 100:
        total = total / 2
    return total
"#;

    let results = extract("src/evaluate.gd", source);
    let evaluate = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "evaluate")
        .expect("expected evaluate symbol");
    let file_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "file")
        .expect("expected file complexity metric");
    let symbol_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| {
            metric.scope == "symbol" && metric.symbol_id.as_deref() == Some(&evaluate.id)
        })
        .expect("expected evaluate symbol complexity metric");

    assert_eq!(file_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(file_metric.symbol_id, None);
    assert_eq!(file_metric.decision_count, 3);
    assert_eq!(file_metric.loop_count, 2);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 3);
    assert_eq!(symbol_metric.loop_count, 2);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(3));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}

#[test]
fn gdscript_complexity_metrics_count_match_statement() {
    // Hand-tallied expectations:
    //   decisions (4): match_statement plus three pattern_section arms
    //   loops (0)
    let source = r#"
func label_for(code: String) -> int:
    match code:
        "a":
            return 1
        "b":
            return 2
        _:
            return 0
"#;

    let results = extract("src/label_for.gd", source);
    let label_for = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "label_for")
        .expect("expected label_for symbol");
    let file_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "file")
        .expect("expected file complexity metric");
    let symbol_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| {
            metric.scope == "symbol" && metric.symbol_id.as_deref() == Some(&label_for.id)
        })
        .expect("expected label_for symbol complexity metric");

    assert_eq!(file_metric.decision_count, 4);
    assert_eq!(file_metric.loop_count, 0);
    assert_eq!(symbol_metric.decision_count, 4);
    assert_eq!(symbol_metric.loop_count, 0);
    assert_eq!(symbol_metric.parameter_count, Some(1));
}
