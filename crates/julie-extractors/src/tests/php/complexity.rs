use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn php_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (10): if, else-if, inner if, ternary, switch statement,
    //     2 case_statement arms, match expression + 1 match arm, catch clause
    //   loops (4): for, foreach, while, do-while
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (4): widget, count, enabled, extras (variadic)
    let source = r#"<?php

class Calculator {
    public function evaluate(Widget $widget, int $count, bool $enabled, int ...$extras): int {
        $total = 0;
        if ($enabled) {
            for ($i = 0; $i < $count; $i++) {
                if ($i % 2 === 0) {
                    $total += $i;
                }
            }
        } elseif ($count > 0) {
            $total = $count > 10 ? 1 : 0;
        }
        switch ($count) {
            case 1:
                $total += 1;
                break;
            default:
                $total -= 1;
                break;
        }
        $label = match ($count) {
            1 => 'one',
            default => 'many',
        };
        foreach ([1, 2] as $item) {
            while ($total > 100) {
                $total = intdiv($total, 2);
            }
        }
        do {
            $total -= 1;
        } while ($total > 50);
        try {
            $total += strlen($label);
        } catch (Exception $error) {
            $total = -1;
        }
        return $total;
    }
}
"#;

    let results = extract("src/Calculator.php", source);
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
    assert_eq!(file_metric.decision_count, 10);
    assert_eq!(file_metric.loop_count, 4);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 10);
    assert_eq!(symbol_metric.loop_count, 4);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(4));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}
