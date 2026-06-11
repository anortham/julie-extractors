use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn lua_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (3): if, elseif, inner if
    //   loops (4): numeric for, generic for, while, repeat
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (3): widget, count, enabled
    let source = r#"function evaluate(widget, count, enabled)
    local total = 0
    if enabled then
        for i = 1, count do
            if i % 2 == 0 then
                total = total + i
            end
        end
    elseif count > 0 then
        total = count > 10 and 1 or 0
    end
    for key, value in pairs({a = 1}) do
        total = total + value
    end
    while total > 100 do
        total = math.floor(total / 2)
    end
    repeat
        total = total - 1
    until total <= 50
    return total
end
"#;

    let results = extract("src/evaluate.lua", source);
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
    assert_eq!(file_metric.decision_count, 3);
    assert_eq!(file_metric.loop_count, 4);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 3);
    assert_eq!(symbol_metric.loop_count, 4);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(3));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}
