use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn swift_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (9): guard, if, else-if (nested if_statement), inner if,
    //     ternary, switch_statement, 2 switch_entries (case + default),
    //     catch_block (do_statement itself is do/catch, not counted)
    //   loops (4): for, for, while, repeat-while
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (3): widget (with external label), count, enabled.
    //     The labeled parameter has two identifiers (external + internal)
    //     and the grammar files the type under the `name` field too; both
    //     must not double-count.
    let source = r#"class Calculator {
    func evaluate(with widget: Widget, count: Int, enabled: Bool) -> Int {
        var total = 0
        guard count >= 0 else {
            return 0
        }
        if enabled {
            for i in 0..<count {
                if i % 2 == 0 {
                    total += i
                }
            }
        } else if count > 0 {
            total = count > 10 ? 1 : 0
        }
        switch count {
        case 1:
            total += 1
        default:
            total -= 1
        }
        for item in [1, 2] {
            while total > 100 {
                total /= 2
            }
        }
        repeat {
            total -= 1
        } while total > 50
        do {
            total += try risky()
        } catch {
            total = -1
        }
        return total
    }
}
"#;

    let results = extract("src/Calculator.swift", source);
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
    assert_eq!(file_metric.loop_count, 4);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 9);
    assert_eq!(symbol_metric.loop_count, 4);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(3));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}

#[test]
fn swift_init_parameters_are_counted() {
    // init_declaration keeps its parameters as direct children (no container
    // node); both parameters must be counted exactly once.
    let source = r#"class Box {
    init(id: Int, label: String) {
        self.id = id
    }
}
"#;

    let results = extract("src/Box.swift", source);
    let init_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "symbol" && metric.parameter_count.is_some())
        .expect("expected init symbol metric");
    assert_eq!(init_metric.parameter_count, Some(2));
}
