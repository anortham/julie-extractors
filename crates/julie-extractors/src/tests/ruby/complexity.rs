use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn ruby_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (9): if, elsif, inner if, ternary (conditional), case,
    //     2 when arms, rescue, unless (tree-sitter-ruby dedupes nested if/for)
    //   loops (3): for, while, until
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (4): widget, count, enabled, extras (splat)
    let source = r#"class Calculator
  def evaluate(widget, count, enabled, *extras)
    total = 0
    if enabled
      for i in 0...count
        if i % 2 == 0
          total += i
        end
      end
    elsif count > 0
      total = count > 10 ? 1 : 0
    end
    case count
    when 1
      total += 1
    when 2
      total += 2
    else
      total -= 1
    end
    unless total.zero?
      while total > 100
        total /= 2
      end
    end
    until total >= count
      total += 1
    end
    [1, 2].each do |item|
      total += item
    end
    begin
      total += "label".length
    rescue StandardError
      total = -1
    end
    total
  end
end
"#;

    let results = extract("src/calculator.rb", source);
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
    assert_eq!(file_metric.decision_count, 9);
    assert_eq!(file_metric.loop_count, 3);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 9);
    assert_eq!(symbol_metric.loop_count, 3);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(4));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}
