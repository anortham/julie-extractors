use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn vbnet_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (3): if, elseif, inner if
    //   loops (2): for, while
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (2): count, enabled
    let source = r#"
Namespace Fixture
    Public Class Calculator
        Public Function Evaluate(count As Integer, enabled As Boolean) As Integer
            Dim total As Integer = 0
            If enabled Then
                For i As Integer = 1 To count
                    If i Mod 2 = 0 Then
                        total += i
                    End If
                Next
            ElseIf count > 0 Then
                total = 1
            End If
            While total > 100
                total = total \ 2
            End While
            Return total
        End Function
    End Class
End Namespace
"#;

    let results = extract("src/Calculator.vb", source);
    let evaluate = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Evaluate")
        .expect("expected Evaluate symbol");
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
        .expect("expected Evaluate symbol complexity metric");

    assert_eq!(file_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(file_metric.symbol_id, None);
    assert_eq!(file_metric.decision_count, 3);
    assert_eq!(file_metric.loop_count, 2);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 3);
    assert_eq!(symbol_metric.loop_count, 2);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(2));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}
