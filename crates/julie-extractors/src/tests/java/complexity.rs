use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn java_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (11): if, else-if, inner if, ternary, switch statement
    //     (switch_expression node), 2 switch_block_statement_groups
    //     (case + default), arrow switch expression, 2 switch rules,
    //     catch clause
    //   loops (3): for, enhanced-for, while
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (4): widget, count, enabled, extras (varargs)
    let source = r#"public class Calculator {
    public int evaluate(Widget widget, int count, boolean enabled, int... extras) {
        int total = 0;
        if (enabled) {
            for (int i = 0; i < count; i++) {
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
                break;
        }
        String label = switch (count) {
            case 1 -> "one";
            default -> "many";
        };
        for (int extra : extras) {
            while (total > 100) {
                total /= 2;
            }
        }
        try {
            total += label.length();
        } catch (Exception error) {
            total = -1;
        }
        return total;
    }
}
"#;

    let results = extract("src/Calculator.java", source);
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
    assert_eq!(file_metric.loop_count, 3);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 11);
    assert_eq!(symbol_metric.loop_count, 3);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(4));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}
