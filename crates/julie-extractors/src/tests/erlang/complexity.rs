use std::path::Path;

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("src/flow.erl", source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn file_metric(results: &crate::ExtractionResults) -> &crate::base::ComplexityMetric {
    results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "file")
        .expect("expected file complexity metric")
}

fn symbol_metric<'a>(
    results: &'a crate::ExtractionResults,
    name: &str,
) -> &'a crate::base::ComplexityMetric {
    let symbol = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("expected symbol {name}"));
    results
        .complexity_metrics
        .iter()
        .find(|metric| metric.scope == "symbol" && metric.symbol_id.as_deref() == Some(&symbol.id))
        .unwrap_or_else(|| panic!("expected symbol complexity metric for {name}"))
}

#[test]
fn erlang_case_guards_and_comprehensions_count_as_decisions_and_loops() {
    // Hand-tallied expectations:
    //   decisions (6): head guard, case_expr, three cr_clause arms, arm guard
    //   loops (1): list comprehension
    //   max nesting depth (3): case_expr -> cr_clause -> guard_clause
    let source = r#"-module(flow).
-export([classify/2]).

classify(Value, Items) when is_integer(Value) ->
    Doubled = [X * 2 || X <- Items],
    case Value of
        0 -> {zero, Doubled};
        N when N > 10 -> {big, N};
        _ -> other
    end.
"#;

    let results = extract(source);
    let file = file_metric(&results);

    assert_eq!(file.algorithm_id, "julie-ast-complexity-v1");
    assert_eq!(file.symbol_id, None);
    assert_eq!(file.decision_count, 6);
    assert_eq!(file.loop_count, 1);
    assert_eq!(file.max_nesting_depth, 3);
    assert_eq!(file.parameter_count, None);

    let classify = symbol_metric(&results, "classify");
    assert_eq!(classify.decision_count, 6);
    assert_eq!(classify.loop_count, 1);
    assert_eq!(classify.max_nesting_depth, 3);
    assert_eq!(classify.parameter_count, None);
    assert!(classify.end_byte > classify.start_byte);
}

#[test]
fn multi_clause_symbol_complexity_counts_decisions_in_later_clauses() {
    // Hand-tallied expectations: clause one branches nowhere, so every decision
    // lives in clause two — its head guard, the case_expr, and two cr_clause
    // arms.
    let source = r#"-module(flow).
-export([route/1]).

route(default) ->
    ok;
route(Value) when is_integer(Value) ->
    case Value of
        0 -> zero;
        _ -> other
    end.
"#;

    let results = extract(source);
    let route = symbol_metric(&results, "route");

    assert_eq!(route.decision_count, 4);
    assert_eq!(route.max_nesting_depth, 2);
    assert_eq!(route.end_byte as usize, source.trim_end().len());
}

#[test]
fn erlang_try_receive_and_catch_branches_count_as_decisions() {
    // Hand-tallied expectations:
    //   serve/0 decisions (6): receive_expr, one cr_clause, receive_after,
    //                          try_expr, one of-clause, one catch_clause
    //   drain/1 decisions (1): catch_expr
    let source = r#"-module(flow).
-export([serve/0, drain/1]).

serve() ->
    receive
        {call, From} ->
            try handle(From) of
                Result -> Result
            catch
                error:Reason -> Reason
            end
    after 1000 ->
        timeout
    end.

drain(Pid) ->
    catch exit(Pid, kill).

handle(From) ->
    From.
"#;

    let results = extract(source);

    let serve = symbol_metric(&results, "serve");
    assert_eq!(serve.decision_count, 6);
    assert_eq!(serve.loop_count, 0);

    let drain = symbol_metric(&results, "drain");
    assert_eq!(drain.decision_count, 1);

    let handle = symbol_metric(&results, "handle");
    assert_eq!(handle.decision_count, 0);
    assert_eq!(handle.max_nesting_depth, 0);
}
