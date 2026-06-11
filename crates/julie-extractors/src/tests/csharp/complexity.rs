use std::path::Path;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn csharp_complexity_metrics_emit_file_and_symbol_scopes() {
    // Hand-tallied expectations:
    //   decisions (11): if, else-if, inner if, ternary, switch statement,
    //     2 switch sections (case + default), switch expression,
    //     2 switch expression arms, catch clause
    //   loops (3): for, foreach, while
    //   max nesting depth (3): if -> for -> inner if
    //   parameters (3): widget, count, enabled
    let source = r#"public class Calculator
{
    public int Evaluate(Widget widget, int count, bool enabled)
    {
        int total = 0;
        if (enabled)
        {
            for (int i = 0; i < count; i++)
            {
                if (i % 2 == 0)
                {
                    total += i;
                }
            }
        }
        else if (count > 0)
        {
            total = count > 10 ? 1 : 0;
        }
        switch (count)
        {
            case 1:
                total += 1;
                break;
            default:
                total -= 1;
                break;
        }
        var label = count switch
        {
            1 => "one",
            _ => "many",
        };
        foreach (var item in new[] { 1, 2 })
        {
            while (total > 100)
            {
                total /= 2;
            }
        }
        try
        {
            total += label.Length;
        }
        catch (System.Exception)
        {
            total = -1;
        }
        return total;
    }
}
"#;

    let results = extract("src/Calculator.cs", source);
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
    assert_eq!(file_metric.decision_count, 11);
    assert_eq!(file_metric.loop_count, 3);
    assert_eq!(file_metric.max_nesting_depth, 3);
    assert_eq!(file_metric.parameter_count, None);

    assert_eq!(symbol_metric.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(symbol_metric.decision_count, 11);
    assert_eq!(symbol_metric.loop_count, 3);
    assert_eq!(symbol_metric.max_nesting_depth, 3);
    assert_eq!(symbol_metric.parameter_count, Some(3));
    assert!(symbol_metric.end_byte > symbol_metric.start_byte);
}
