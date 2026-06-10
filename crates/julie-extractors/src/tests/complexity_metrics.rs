use std::collections::BTreeSet;
use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn rust_complexity_metrics_emit_file_and_symbol_scopes() {
    let source = r#"pub fn score(items: &[i32], enabled: bool) -> i32 {
    let mut total = 0;
    if enabled {
        for item in items {
            if *item > 0 {
                total += *item;
            }
        }
    } else {
        total = -1;
    }
    total
}
"#;

    let results = extract("src/lib.rs", source);
    let score = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "score")
        .expect("expected score symbol");
    let file_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "file")
        .expect("expected file complexity metric");
    let symbol_metric = results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "symbol" && metric.symbol_id.as_deref() == Some(&score.id))
        .expect("expected score symbol complexity metric");

    assert_eq!(file_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(file_metric.symbol_id, None);
    assert_eq!(file_metric.decision_count, 2);
    assert_eq!(file_metric.loop_count, 1);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);
    assert!(file_metric.covered_lines >= symbol_metric.covered_lines);
    assert!(file_metric.covered_bytes >= symbol_metric.covered_bytes);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 2);
    assert_eq!(symbol_metric.loop_count, 1);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(2));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}

#[derive(Debug)]
struct ComplexityCase {
    file_path: &'static str,
    source: &'static str,
    expected_decisions: u32,
    expected_loops: u32,
    expected_depth: u32,
    expected_parameters: u32,
}

#[test]
fn supported_complexity_languages_emit_file_and_symbol_metrics() {
    let cases = [
        ComplexityCase {
            file_path: "src/lib.rs",
            source: r#"pub fn run(a: i32, b: i32) -> i32 {
    if a > b {
        a
    } else {
        b
    }
}
"#,
            expected_decisions: 1,
            expected_loops: 0,
            expected_depth: 1,
            expected_parameters: 2,
        },
        ComplexityCase {
            file_path: "src/service.go",
            source: r#"package main

func run(items []int, enabled bool) int {
    total := 0
    for _, item := range items {
        if enabled {
            total += item
        }
    }
    return total
}
"#,
            expected_decisions: 1,
            expected_loops: 1,
            expected_depth: 2,
            expected_parameters: 2,
        },
        ComplexityCase {
            file_path: "src/service.py",
            source: r#"def run(items, enabled):
    total = 0
    for item in items:
        if enabled:
            total += item
    return total
"#,
            expected_decisions: 1,
            expected_loops: 1,
            expected_depth: 2,
            expected_parameters: 2,
        },
        ComplexityCase {
            file_path: "src/service.js",
            source: r#"export function run(items, enabled) {
    let total = 0;
    for (const item of items) {
        if (enabled) {
            total += item;
        }
    }
    return total;
}
"#,
            expected_decisions: 1,
            expected_loops: 1,
            expected_depth: 2,
            expected_parameters: 2,
        },
        ComplexityCase {
            file_path: "src/service.ts",
            source: r#"export function run(items: number[], enabled: boolean): number {
    let total = 0;
    for (const item of items) {
        if (enabled) {
            total += item;
        }
    }
    return total;
}
"#,
            expected_decisions: 1,
            expected_loops: 1,
            expected_depth: 2,
            expected_parameters: 2,
        },
        ComplexityCase {
            file_path: "src/Service.cs",
            source: r#"public class Service {
    public int Run(int count, bool enabled) {
        int total = 0;
        for (int index = 0; index < count; index++) {
            if (enabled) {
                total += index;
            }
        }
        return total;
    }
}
"#,
            expected_decisions: 1,
            expected_loops: 1,
            expected_depth: 2,
            expected_parameters: 2,
        },
        ComplexityCase {
            file_path: "src/Service.java",
            source: r#"public class Service {
    public int run(int count, boolean enabled) {
        int total = 0;
        for (int index = 0; index < count; index++) {
            if (enabled) {
                total += index;
            }
        }
        return total;
    }
}
"#,
            expected_decisions: 1,
            expected_loops: 1,
            expected_depth: 2,
            expected_parameters: 2,
        },
        ComplexityCase {
            file_path: "src/service.c",
            source: r#"int run(int count, int enabled) {
    int total = 0;
    for (int index = 0; index < count; index++) {
        if (enabled) {
            total += index;
        }
    }
    return total;
}
"#,
            expected_decisions: 1,
            expected_loops: 1,
            expected_depth: 2,
            expected_parameters: 2,
        },
        ComplexityCase {
            file_path: "src/service.cpp",
            source: r#"int run(int count, bool enabled) {
    int total = 0;
    for (int index = 0; index < count; index++) {
        if (enabled) {
            total += index;
        }
    }
    return total;
}
"#,
            expected_decisions: 1,
            expected_loops: 1,
            expected_depth: 2,
            expected_parameters: 2,
        },
    ];

    for case in cases {
        let results = extract(case.file_path, case.source);
        let scopes = results
            .complexity_metrics
            .iter()
            .map(|metric| metric.scope.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            scopes,
            BTreeSet::from(["file", "symbol"]),
            "{} should emit file and symbol complexity scopes",
            case.file_path
        );

        let symbol_metric = results
            .complexity_metrics
            .iter()
            .find(|metric| metric.scope == "symbol" && metric.parameter_count.is_some())
            .unwrap_or_else(|| {
                panic!(
                    "{} should emit a callable symbol metric with parameter count",
                    case.file_path
                )
            });
        assert_eq!(
            symbol_metric.algorithm_id, "julie-ast-complexity-v1",
            "{} emitted wrong algorithm id",
            case.file_path
        );
        assert_eq!(symbol_metric.decision_count, case.expected_decisions);
        assert_eq!(symbol_metric.loop_count, case.expected_loops);
        assert_eq!(symbol_metric.max_nesting_depth, case.expected_depth);
        assert_eq!(
            symbol_metric.parameter_count,
            Some(case.expected_parameters)
        );
    }
}
