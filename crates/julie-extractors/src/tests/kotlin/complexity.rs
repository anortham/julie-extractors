use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn kotlin_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (11): if, else-if (nested if_expression), inner if,
    //     expression-if (Kotlin's ternary), when #1 + 2 when_entries,
    //     when #2 + 2 when_entries, catch_block
    //   loops (4): for, for, while, do-while
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (3): widget, count, enabled
    let source = r#"class Calculator {
    fun evaluate(widget: Widget, count: Int, enabled: Boolean): Int {
        var total = 0
        if (enabled) {
            for (i in 0 until count) {
                if (i % 2 == 0) {
                    total += i
                }
            }
        } else if (count > 0) {
            total = if (count > 10) 1 else 0
        }
        when (count) {
            1 -> total += 1
            else -> total -= 1
        }
        val label = when (count) {
            1 -> "one"
            else -> "many"
        }
        for (item in listOf(1, 2)) {
            while (total > 100) {
                total /= 2
            }
        }
        do {
            total -= 1
        } while (total > 50)
        try {
            total += label.length
        } catch (error: Exception) {
            total = -1
        }
        return total
    }
}
"#;

    let results = extract("src/Calculator.kt", source);
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
    assert_eq!(symbol_metric.parameter_count, Some(3));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}

#[test]
fn kotlin_secondary_constructor_parameters_are_counted() {
    // The secondary constructor declares one parameter (id). The primary
    // constructor's class_parameters are part of the class declaration, not
    // the secondary constructor's span, and must not leak into the count.
    let source = r#"class Box(val id: Int, label: String) {
    constructor(id: Int) : this(id, "x") {
        log(id)
    }
}
"#;

    let results = extract("src/Box.kt", source);
    let constructor_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "symbol" && metric.parameter_count.is_some())
        .expect("expected constructor symbol metric");
    assert_eq!(constructor_metric.parameter_count, Some(1));
}
