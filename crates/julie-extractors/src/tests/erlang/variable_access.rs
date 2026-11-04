use super::support::{extract_with_identifiers, find, parse};
use crate::base::{Identifier, IdentifierKind};
use crate::erlang::ErlangExtractor;

#[derive(Clone, Copy, Debug)]
struct AccessWork {
    total: usize,
    traversal_node_visits: usize,
    scope_lookup_operations: usize,
    scope_frame_probes: usize,
    scope_bind_operations: usize,
    branch_join_items: usize,
    scope_frame_operations: usize,
}

fn extract_with_work(source: &str) -> (Vec<crate::base::Symbol>, Vec<Identifier>, AccessWork) {
    ErlangExtractor::reset_variable_access_work();
    let (symbols, identifiers) = extract_with_identifiers(source);
    let work = ErlangExtractor::take_variable_access_work();
    let summary = AccessWork {
        total: work.total_work(),
        traversal_node_visits: work.traversal_node_visits,
        scope_lookup_operations: work.scope_lookup_operations,
        scope_frame_probes: work.scope_frame_probes,
        scope_bind_operations: work.scope_bind_operations,
        branch_join_items: work.branch_join_items,
        scope_frame_operations: work.scope_frame_operations,
    };
    (symbols, identifiers, summary)
}

fn assert_variable_fact_sites(
    source: &str,
    symbols: &[crate::base::Symbol],
    identifiers: &[Identifier],
    expected_count: usize,
) {
    let function = find(symbols, "f");
    let variable_refs: Vec<_> = identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::VariableRef)
        .collect();

    assert_eq!(variable_refs.len(), expected_count);
    assert!(!variable_refs.is_empty());
    for identifier in variable_refs {
        assert_eq!(
            identifier.containing_symbol_id.as_deref(),
            Some(function.id.as_str())
        );
        assert_eq!(
            &source[identifier.start_byte as usize..identifier.end_byte as usize],
            identifier.name
        );
    }
}

fn assert_linear_work(work: &[AccessWork]) {
    assert_eq!(work.len(), 3);
    for pair in work.windows(2) {
        assert!(
            pair[1].total * 2 <= pair[0].total * 5,
            "work should grow by at most 2.5x: {} -> {}",
            pair[0].total,
            pair[1].total
        );
    }
}

#[test]
fn macro_body_uses_the_replacement_expression_span_and_hash() {
    let source = "-module(m).\n-define(LIMIT, 10).\n";
    let (symbols, _) = extract_with_identifiers(source);
    let macro_symbol = find(&symbols, "LIMIT");
    let body = macro_symbol.body_span.expect("macro replacement body");

    assert_eq!(
        (
            body.start_line,
            body.start_column,
            body.end_line,
            body.end_column
        ),
        (2, 15, 2, 17)
    );
    assert_eq!(
        &source[body.start_byte as usize..body.end_byte as usize],
        "10"
    );
    assert_eq!(
        macro_symbol.body_hash.as_deref(),
        Some("d3d9446802a44259755d38e6d163e820")
    );
}

fn variable_sites(identifiers: &[Identifier]) -> Vec<(&str, u32, u32, Option<&str>)> {
    identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::VariableRef)
        .map(|identifier| {
            (
                identifier.name.as_str(),
                identifier.start_line,
                identifier.start_column,
                identifier
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("access"))
                    .and_then(serde_json::Value::as_str),
            )
        })
        .collect()
}

#[test]
fn match_expressions_distinguish_fresh_bindings_from_repeated_reads() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).

f(X) ->
    Value = X,
    Value = Value + 1,
    Value.
"#,
    );

    assert_eq!(
        variable_sites(&identifiers),
        [
            ("Value", 4, 4, Some("write")),
            ("X", 4, 12, Some("read")),
            ("Value", 5, 4, Some("read")),
            ("Value", 5, 12, Some("read")),
            ("Value", 6, 4, Some("read")),
        ]
    );
}

#[test]
fn tuple_patterns_mix_fresh_writes_with_reads_of_bound_variables() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).

f(Bound, Tuple) ->
    {Fresh, Bound} = Tuple,
    {Fresh, Bound}.
"#,
    );

    assert_eq!(
        variable_sites(&identifiers),
        [
            ("Fresh", 4, 5, Some("write")),
            ("Bound", 4, 12, Some("read")),
            ("Tuple", 4, 21, Some("read")),
            ("Fresh", 5, 5, Some("read")),
            ("Bound", 5, 12, Some("read")),
        ]
    );
}

#[test]
fn guards_and_separate_function_clauses_read_only_their_local_head_bindings() {
    let (symbols, identifiers) = extract_with_identifiers(
        r#"-module(m).

f(0, Value) when is_integer(Value) -> Value;
f(N, Value) when N > 0 -> N + Value;
f(_, _Ignored) -> _Ignored.
"#,
    );
    let sites = variable_sites(&identifiers);
    let function = find(&symbols, "f");

    assert_eq!(
        sites
            .iter()
            .map(|(name, line, _, access)| (*name, *line, *access))
            .collect::<Vec<_>>(),
        [
            ("Value", 3, Some("read")),
            ("Value", 3, Some("read")),
            ("N", 4, Some("read")),
            ("N", 4, Some("read")),
            ("Value", 4, Some("read")),
            ("_Ignored", 5, Some("read")),
        ]
    );
    assert!(
        identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::VariableRef)
            .all(|identifier| identifier.containing_symbol_id.as_deref()
                == Some(function.id.as_str()))
    );
}

#[test]
fn case_branches_start_from_the_same_scope_and_join_shared_bindings() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).

f(Flag, Value) ->
    case Flag of
        true -> Branch = Value;
        false -> Branch = Value
    end,
    Branch.
"#,
    );
    let sites = variable_sites(&identifiers);

    assert_eq!(
        sites
            .iter()
            .map(|(name, line, _, access)| (*name, *line, *access))
            .collect::<Vec<_>>(),
        [
            ("Flag", 4, Some("read")),
            ("Branch", 5, Some("write")),
            ("Value", 5, Some("read")),
            ("Branch", 6, Some("write")),
            ("Value", 6, Some("read")),
            ("Branch", 8, Some("read")),
        ]
    );
}

#[test]
fn bindings_from_only_one_case_branch_do_not_escape_the_branch() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).

f(Flag, Value) ->
    case Flag of
        true -> Maybe = Value;
        false -> ok
    end,
    Maybe = Value,
    Maybe.
"#,
    );
    let sites = variable_sites(&identifiers);

    assert_eq!(
        sites
            .iter()
            .map(|(name, line, _, access)| (*name, *line, *access))
            .collect::<Vec<_>>(),
        [
            ("Flag", 4, Some("read")),
            ("Maybe", 5, Some("write")),
            ("Value", 5, Some("read")),
            ("Maybe", 8, Some("write")),
            ("Value", 8, Some("read")),
            ("Maybe", 9, Some("read")),
        ]
    );
}

#[test]
fn comprehension_generators_shadow_outer_names_and_keep_new_bindings_local() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).

f(X, Values) ->
    [X || X <- Values],
    X.

g(Values) ->
    [Y || Y <- Values],
    Y = 3,
    Y.

h(Values, More) ->
    [Z || Z <- Values, Z <- More].
"#,
    );
    let sites = variable_sites(&identifiers);

    assert_eq!(
        sites
            .iter()
            .map(|(name, line, _, access)| (*name, *line, *access))
            .collect::<Vec<_>>(),
        [
            ("X", 4, Some("read")),
            ("X", 4, Some("write")),
            ("Values", 4, Some("read")),
            ("X", 5, Some("read")),
            ("Y", 8, Some("read")),
            ("Y", 8, Some("write")),
            ("Values", 8, Some("read")),
            ("Y", 9, Some("write")),
            ("Y", 10, Some("read")),
            ("Z", 13, Some("read")),
            ("Z", 13, Some("write")),
            ("Values", 13, Some("read")),
            ("Z", 13, Some("write")),
            ("More", 13, Some("read")),
        ]
    );
}

#[test]
fn macro_arguments_are_reads_without_opaque_catch_bindings() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).
-define(LOG(X), X).

f(Value) ->
    try Value
    catch
        ?WITH_STACKTRACE(Class, Reason, Stacktrace)
            ?LOG(Reason)
    end.
"#,
    );
    let sites = variable_sites(&identifiers);

    assert_eq!(
        sites
            .iter()
            .map(|(name, line, _, access)| (*name, *line, *access))
            .collect::<Vec<_>>(),
        [
            ("Value", 5, Some("read")),
            ("Class", 7, Some("read")),
            ("Reason", 7, Some("read")),
            ("Stacktrace", 7, Some("read")),
            ("Reason", 8, Some("read")),
        ]
    );
}

#[test]
fn anonymous_fun_heads_capture_outer_vars_and_keep_local_bindings_scoped() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).

f(X) ->
    Fun = fun(X, Local) ->
        Inner = Local,
        {X, Inner}
    end,
    Inner = 1,
    {Fun, X, Inner}.
"#,
    );
    let sites = variable_sites(&identifiers);

    assert_eq!(
        sites
            .iter()
            .map(|(name, line, _, access)| (*name, *line, *access))
            .collect::<Vec<_>>(),
        [
            ("Fun", 4, Some("write")),
            ("X", 4, Some("read")),
            ("Inner", 5, Some("write")),
            ("Local", 5, Some("read")),
            ("X", 6, Some("read")),
            ("Inner", 6, Some("read")),
            ("Inner", 8, Some("write")),
            ("Fun", 9, Some("read")),
            ("X", 9, Some("read")),
            ("Inner", 9, Some("read")),
        ]
    );
}

#[test]
fn many_variable_reads_scale_linearly_with_ast_visits_and_scope_work() {
    let mut work = Vec::new();
    for count in [24, 48, 96] {
        let values = std::iter::repeat_n("Value", count)
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!("-module(m).\nf(Value) -> {{{values}}}.\n");
        let (symbols, identifiers, counts) = extract_with_work(&source);

        assert_variable_fact_sites(&source, &symbols, &identifiers, count);
        assert!(counts.traversal_node_visits >= count);
        assert!(counts.scope_lookup_operations > 0);
        assert!(counts.scope_bind_operations > 0);
        work.push(counts);
    }

    assert_linear_work(&work);
}

#[test]
fn branch_binding_churn_scales_with_nodes_and_join_candidates() {
    let mut work = Vec::new();
    for count in [24, 48, 96] {
        let bindings = (0..count)
            .map(|index| format!("Bound{index} = {index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let empty_branches = (1..count)
            .map(|branch| format!("{branch} -> ok"))
            .collect::<Vec<_>>()
            .join(";\n        ");
        let source = format!(
            "-module(m).\nf() ->\n    case ok of\n        0 -> {bindings}, ok;\n        {empty_branches}\n    end.\n"
        );
        let tree = parse(&source);
        assert!(!tree.root_node().has_error());
        let mut nodes = vec![tree.root_node()];
        let mut clauses = 0;
        while let Some(node) = nodes.pop() {
            clauses += usize::from(node.kind() == "cr_clause");
            let mut cursor = node.walk();
            nodes.extend(node.children(&mut cursor));
        }
        assert_eq!(clauses, count);
        let (symbols, identifiers, counts) = extract_with_work(&source);

        assert_variable_fact_sites(&source, &symbols, &identifiers, count);
        assert!(counts.traversal_node_visits > 0);
        assert!(counts.scope_lookup_operations >= count);
        assert!(counts.scope_bind_operations >= count);
        assert!(counts.branch_join_items >= count);
        assert!(
            counts.branch_join_items <= count * 3,
            "{counts:?} for {count} branch alternatives"
        );
        work.push(counts);
    }

    assert_linear_work(&work);
}

#[test]
fn deeply_nested_fun_captures_scale_without_frame_scanning() {
    let mut work = Vec::new();
    for count in [24, 48, 96] {
        let nested_funs = (0..count)
            .map(|index| format!("fun(Value{index}) ->"))
            .collect::<Vec<_>>()
            .join(" ");
        let values = (0..count)
            .map(|index| format!("Value{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let endings = std::iter::repeat_n("end", count)
            .collect::<Vec<_>>()
            .join(" ");
        let source = format!("-module(m).\nf() -> {nested_funs} {{{values}}} {endings}.\n");
        let (symbols, identifiers, counts) = extract_with_work(&source);

        assert_variable_fact_sites(&source, &symbols, &identifiers, count);
        assert!(counts.traversal_node_visits >= count);
        assert!(counts.scope_lookup_operations >= count);
        assert!(counts.scope_frame_probes <= counts.scope_lookup_operations * 2);
        assert!(counts.scope_bind_operations >= count);
        assert!(counts.scope_frame_operations >= count * 2);
        work.push(counts);
    }

    assert_linear_work(&work);
}
