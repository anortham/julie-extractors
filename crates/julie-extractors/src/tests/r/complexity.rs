use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn r_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (4): if, inner if, else-if (nested if), break guard if
    //   loops (3): for, while, repeat
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (3): widget, count, enabled
    let source = r#"
evaluate <- function(widget, count, enabled) {
  total <- 0
  if (enabled) {
    for (i in 1:count) {
      if (i %% 2 == 0) total <- total + i
    }
  } else if (count > 0) {
    total <- 1
  }
  while (total > 100) total <- total / 2
  repeat { total <- total - 1; if (total <= 50) break }
  total
}
"#;

    let results = extract("src/evaluate.R", source);
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
    assert_eq!(file_metric.decision_count, 4);
    assert_eq!(file_metric.loop_count, 3);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 4);
    assert_eq!(symbol_metric.loop_count, 3);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(3));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}

#[test]
fn r_complexity_metrics_count_switch_call() {
    // Hand-tallied expectations:
    //   decisions (1): switch(...) call
    //   loops (0)
    let source = r#"
label_for <- function(code) {
  switch(code,
    a = 1,
    b = 2,
    0
  )
}
"#;

    let results = extract("src/label_for.R", source);
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

    assert_eq!(file_metric.decision_count, 1);
    assert_eq!(file_metric.loop_count, 0);
    assert_eq!(symbol_metric.decision_count, 1);
    assert_eq!(symbol_metric.loop_count, 0);
    assert_eq!(symbol_metric.parameter_count, Some(1));
}
