use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn bash_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (3): if, elif, inner if
    //   loops (2): for, while
    //   max nesting depth (3): if -> for -> inner if
    //   parameters: bash functions use positional $1/$2/$3 with no AST parameter list
    let source = r#"evaluate() {
  local widget=$1 count=$2 enabled=$3
  local total=0
  if [ "$enabled" = "true" ]; then
    for i in $(seq 1 "$count"); do
      if [ $((i % 2)) -eq 0 ]; then
        total=$((total + i))
      fi
    done
  elif [ "$count" -gt 0 ]; then
    total=1
  fi
  while [ "$total" -gt 100 ]; do
    total=$((total / 2))
  done
}
"#;

    let results = extract("src/evaluate.sh", source);
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
    assert_eq!(symbol_metric.parameter_count, None);
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}
