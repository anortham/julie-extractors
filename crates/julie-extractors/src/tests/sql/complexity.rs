use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical SQL extraction should succeed")
}

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/sql/basic/source.sql");

#[test]
fn sql_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations for fixtures/extraction/sql/basic/source.sql:
    //   decisions (5): 1 join, 4 WHERE predicates (view, CTE, UPDATE, trigger body)
    //   loops (0): no WHILE nodes in fixture
    //   max nesting depth (2): outer SELECT + CTE SELECT containers
    let results = extract("fixtures/extraction/sql/basic/source.sql", FIXTURE_SOURCE);

    let file_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "file")
        .expect("expected file complexity metric");
    assert_eq!(file_metric.algorithm_id, "julie-sql-complexity-v1");
    assert_eq!(file_metric.language, "sql");
    assert_eq!(file_metric.symbol_id, None);
    assert_eq!(file_metric.decision_count, 5);
    assert_eq!(file_metric.loop_count, 0);
    assert_eq!(file_metric.max_nesting_depth, 2);
    assert_eq!(file_metric.parameter_count, None);

    let trigger = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "refresh_active_workers")
        .expect("expected trigger symbol");
    let trigger_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "symbol" && metric.symbol_id.as_deref() == Some(&trigger.id))
        .expect("expected trigger symbol complexity metric");
    assert_eq!(trigger_metric.algorithm_id, "julie-sql-complexity-v1");
    assert!(
        trigger_metric.start_byte <= trigger.start_byte,
        "metric span should cover trigger declaration start"
    );
    assert!(
        trigger_metric.end_byte >= trigger.end_byte,
        "metric span should cover trigger callable body: trigger_end={} metric_end={}",
        trigger.end_byte,
        trigger_metric.end_byte
    );
    assert!(
        trigger_metric.decision_count > 0,
        "trigger INSERT ... SELECT ... WHERE should emit predicate complexity: {trigger_metric:?}"
    );
    assert_eq!(trigger_metric.loop_count, 0);
}

#[test]
fn sql_callable_symbol_complexity_uses_body_span_with_predicate_evidence() {
    let source = r#"
CREATE FUNCTION count_active_workers()
RETURNS INTEGER
BEGIN
    DECLARE v_count INTEGER;
    SELECT COUNT(*) INTO v_count
    FROM workers
    WHERE id > 0;
    RETURN v_count;
END;
"#;

    let results = extract("count_active_workers.sql", source);
    let function = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "count_active_workers")
        .expect("expected function symbol");
    let function_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| {
            metric.scope == "symbol" && metric.symbol_id.as_deref() == Some(&function.id)
        })
        .expect("expected function symbol complexity metric");

    assert!(
        function_metric.end_byte > function.end_byte,
        "metric span should expand through split routine siblings; function_end={} metric_end={}",
        function.end_byte,
        function_metric.end_byte
    );
    assert!(
        function_metric.decision_count > 0,
        "function body should include WHERE complexity: function={function:?}, metric={function_metric:?}"
    );
    assert_eq!(function_metric.max_nesting_depth, 1);
}
