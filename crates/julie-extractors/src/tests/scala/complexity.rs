use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn scala_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (7): if, else-if, inner if, match + 2 case_clause arms,
    //     catch type_case_clause
    //   loops (2): for, while
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (3): widget, count, enabled
    let source = r#"class Calculator {
  def evaluate(widget: Widget, count: Int, enabled: Boolean): Int = {
    var total = 0
    if (enabled) {
      for (i <- 0 until count) {
        if (i % 2 == 0) {
          total += i
        }
      }
    } else if (count > 0) {
      total = if (count > 10) 1 else 0
    }
    count match {
      case 1 => total += 1
      case _ => total -= 1
    }
    given (total > 0) {
      total += 1
    }
    for (item <- List(1, 2)) {
      while (total > 100) {
        total /= 2
      }
    }
    do {
      total -= 1
    } while (total > 50)
    try {
      total += "label".length
    } catch {
      case _: Exception => total = -1
    }
    total
  }
}
"#;

    let results = extract("src/Calculator.scala", source);
    let evaluate = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "evaluate" && symbol.kind == crate::base::SymbolKind::Method)
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
    assert_eq!(file_metric.decision_count, 7);
    assert_eq!(file_metric.loop_count, 2);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 7);
    assert_eq!(symbol_metric.loop_count, 2);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(3));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}
