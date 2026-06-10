use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn tsx_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (2): if, else-if (nested if_statement)
    //   loops (1): for
    //   max nesting depth (2): if -> for
    //   parameters (2): count, enabled
    let source = r#"
function evaluate(count: number, enabled: boolean): number {
    let total = 0;
    if (enabled) {
        for (let i = 1; i <= count; i++) {
            total += i;
        }
    } else if (count > 0) {
        total = 1;
    }
    return total;
}

export function Badge(props: { label: string }) {
    return <button>{props.label}</button>;
}
"#;

    let results = extract("src/Badge.tsx", source);
    let evaluate = results
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == "evaluate" && symbol.kind == crate::base::SymbolKind::Function
        })
        .expect("expected evaluate function symbol");
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
    assert_eq!(file_metric.language, "tsx");
    assert_eq!(file_metric.symbol_id, None);
    assert_eq!(file_metric.decision_count, 2);
    assert_eq!(file_metric.loop_count, 1);
    assert_eq!(file_metric.max_nesting_depth, 2);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 2);
    assert_eq!(symbol_metric.loop_count, 1);
    assert_eq!(symbol_metric.max_nesting_depth, 2);
    assert_eq!(symbol_metric.parameter_count, Some(2));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}
