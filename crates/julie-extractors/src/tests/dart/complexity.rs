use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn dart_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (11): if, else-if (nested if_statement), inner if, ternary
    //     (conditional_expression), switch_statement, switch_statement_case,
    //     switch_statement_default, switch_expression,
    //     2 switch_expression_cases, catch_clause
    //   loops (4): C-style for, for-in, while, do-while
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (5): widget, count, enabled, plus named extra and bonus
    //     inside the optional_formal_parameters group
    //
    // Dart symbols span only the signature node; the symbol metric must still
    // cover the sibling function_body (body_sibling_node_kinds).
    let source = r#"class Calculator {
  int evaluate(Widget widget, int count, bool enabled, {int extra = 0, int? bonus}) {
    var total = 0;
    if (enabled) {
      for (var i = 0; i < count; i++) {
        if (i % 2 == 0) {
          total += i;
        }
      }
    } else if (count > 0) {
      total = count > 10 ? 1 : 0;
    }
    switch (count) {
      case 1:
        total += 1;
        break;
      default:
        total -= 1;
    }
    final label = switch (count) {
      1 => 'one',
      _ => 'many',
    };
    for (final item in [1, 2]) {
      while (total > 100) {
        total ~/= 2;
      }
    }
    do {
      total -= 1;
    } while (total > 50);
    try {
      total += label.length;
    } catch (error) {
      total = -1;
    }
    return total;
  }
}
"#;

    let results = extract("src/calculator.dart", source);
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
    assert_eq!(file_metric.decision_count, 11);
    assert_eq!(file_metric.loop_count, 4);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 11);
    assert_eq!(symbol_metric.loop_count, 4);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(5));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}

#[test]
fn dart_constructor_field_parameters_are_counted() {
    // `this.id` parses as formal_parameter wrapping constructor_param with no
    // `name` field; arity must still resolve to one.
    let source = r#"class Worker {
  final int id;

  Worker(this.id);
}
"#;

    let results = extract("src/worker.dart", source);
    let constructor_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "symbol" && metric.parameter_count.is_some())
        .expect("expected constructor symbol metric");
    assert_eq!(constructor_metric.parameter_count, Some(1));
}
