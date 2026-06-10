use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn zig_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (7): if, else-if, inner if, inline if, switch_expression,
    //     one switch_case arm, catch_expression
    //   loops (3): for, for, while
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (4): widget, count, enabled, extras
    let source = r#"const Widget = struct {};

pub fn evaluate(
    widget: Widget,
    count: i32,
    enabled: bool,
    extras: []const i32,
) i32 {
    var total: i32 = 0;
    if (enabled) {
        for (extras, 0..) |_, i| {
            if (@rem(i, 2) == 0) {
                total += i;
            }
        }
    } else if (count > 0) {
        total = if (count > 10) 1 else 0;
    }
    switch (count) {
        1 => total += 1,
        else => total -= 1,
    }
    for (0..count) |i| {
        while (total > 100) {
            total /= 2;
        }
    }
    const label = catch total + 1 else |_| -1;
    return total;
}
"#;

    let results = extract("src/evaluate.zig", source);
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
    assert_eq!(file_metric.decision_count, 7);
    assert_eq!(file_metric.loop_count, 3);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 7);
    assert_eq!(symbol_metric.loop_count, 3);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(4));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}
