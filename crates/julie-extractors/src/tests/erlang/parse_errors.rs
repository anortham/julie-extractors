use crate::base::{ParseDiagnosticKind, Visibility};
use crate::pipeline::extract_canonical;
use std::path::PathBuf;

const BROKEN: &str = r#"-module(bank).
-export([open/1]).

open(Id) ->
    #account{id = Id}.

broken(( ->

audit(A) ->
    A.
"#;

/// A macro that expands to a partial `catch` clause head. tree-sitter has no
/// preprocessor, so the form cannot parse and the failure cascades over every
/// later declaration unless recovery re-syncs.
const MACRO_CLAUSE_HEAD: &str = r#"-module(p).
-export([f/0, g/0]).

f() ->
    try
        risky()
    catch
        ?WITH_STACKTRACE(C, R, S)
            io:format("~p~p~p", [C, R, S])
    end.

g() -> ok.
"#;

fn extract(code: &str) -> crate::ExtractionResults {
    extract_canonical("bank.erl", code, &PathBuf::from("/tmp/test")).expect("extraction failed")
}

fn symbol_names(code: &str) -> Vec<String> {
    let mut names: Vec<String> = extract(code)
        .symbols
        .iter()
        .map(|symbol| symbol.name.clone())
        .collect();
    names.sort();
    names
}

#[test]
fn parse_errors_still_yield_the_declarations_that_parsed() {
    let names = symbol_names(BROKEN);

    assert!(names.contains(&"bank".to_string()), "got {names:?}");
    assert!(names.contains(&"open".to_string()), "got {names:?}");
}

#[test]
fn declarations_after_a_parse_error_are_recovered() {
    let names = symbol_names(BROKEN);

    assert!(
        names.contains(&"audit".to_string()),
        "audit/1 follows the broken form and must be recovered, got {names:?}"
    );
}

#[test]
fn function_after_a_macro_clause_head_is_recovered_with_its_identity() {
    let results = extract(MACRO_CLAUSE_HEAD);
    let recovered = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "g")
        .unwrap_or_else(|| {
            panic!(
                "g/0 must be recovered, got {:?}",
                symbol_names(MACRO_CLAUSE_HEAD)
            )
        });

    assert_eq!(recovered.signature.as_deref(), Some("g/0()"));
    assert_eq!(recovered.visibility, Some(Visibility::Public));
    assert_eq!(
        recovered
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("arity"))
            .and_then(serde_json::Value::as_u64),
        Some(0)
    );
}

#[test]
fn declarations_after_nested_parse_errors_are_recovered() {
    let code = r#"-module(p).
-export([tail/0]).

first() ->
    try
        risky()
    catch
        ?WITH_STACKTRACE(C, R, S)
            case C of
                ?ALSO_BROKEN(X) ->
                    X
            end
    end.

tail() -> ok.
"#;
    let names = symbol_names(code);

    assert!(names.contains(&"tail".to_string()), "got {names:?}");
}

#[test]
fn form_like_lines_inside_triple_quoted_strings_do_not_become_symbols() {
    let code = r#"-module(p).
-export([real/0]).

-doc """
ghost() -> not_code.
-record(ghost, {id}).
-define(GHOST, 1).
""".
broken(( ->

real() -> ok.
"#;

    assert_eq!(
        symbol_names(code),
        vec!["p".to_string(), "real".to_string()],
        "only the module and the real function may be extracted"
    );
}

#[test]
fn form_like_lines_inside_comment_blocks_do_not_become_symbols() {
    let code = r#"-module(p).
-export([real/0]).

broken(( ->

%% ghost() -> not_code.
%% -record(ghost, {id}).

real() -> ok.
"#;

    assert_eq!(
        symbol_names(code),
        vec!["p".to_string(), "real".to_string()],
        "commented-out declarations must never be extracted"
    );
}

#[test]
fn garbage_inside_an_error_region_does_not_synthesize_symbols() {
    let code = r#"-module(p).
-export([real/0]).

first() ->
    try
        risky()
    catch
        ?WITH_STACKTRACE(C, R, S)
            ) ] } ,, ->> ;; |||
            12345 <<>> #{} $x
    end.

real() -> ok.
"#;

    assert_eq!(
        symbol_names(code),
        vec!["first".to_string(), "p".to_string(), "real".to_string()],
        "punctuation and literals in a failed region must not become declarations"
    );
}

/// Arity is half of Erlang's function identity, and a damaged argument list
/// produces one out of punctuation: `second(( ->` parses as `second/1`. A
/// recovered form whose head does not parse is dropped rather than published
/// under an invented identity.
#[test]
fn a_recovered_function_with_a_damaged_argument_list_is_rejected() {
    let code = r#"-module(p).
-export([real/0]).

first() ->
    ?BROKEN(A) B.

second(( ->
    x

real() -> ok.
"#;
    let names = symbol_names(code);

    assert!(
        !names.contains(&"second".to_string()),
        "a head parsed from punctuation must not be admitted, got {names:?}"
    );
    assert!(names.contains(&"real".to_string()), "got {names:?}");
}

#[test]
fn recovered_declarations_carry_spans_from_the_original_source() {
    let recovered = extract(BROKEN)
        .symbols
        .into_iter()
        .find(|symbol| symbol.name == "audit")
        .expect("audit/1 must be recovered");

    assert_eq!(recovered.start_line, 9);
    assert_eq!(recovered.start_column, 0);
}

#[test]
fn recovered_functions_are_walked_for_identifiers_and_relationships() {
    let code = r#"-module(p).
-export([tail/0]).

first() ->
    try
        risky()
    catch
        ?WITH_STACKTRACE(C, R, S)
            io:format("~p", [C])
    end.

tail() ->
    helper(),
    lists:reverse([1, 2]).

helper() -> ok.
"#;
    let results = extract(code);

    assert!(
        results
            .identifiers
            .iter()
            .any(|identifier| identifier.name == "helper"),
        "the call inside a recovered function must produce an identifier, got {:?}",
        results
            .identifiers
            .iter()
            .map(|identifier| identifier.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        results
            .structured_pending_relationships
            .iter()
            .any(|pending| pending.target.display_name == "lists:reverse"),
        "a remote call inside a recovered function must produce a pending edge"
    );
}

#[test]
fn parse_errors_are_reported_as_diagnostics() {
    let results = extract(BROKEN);

    assert!(
        results.parse_diagnostics.iter().any(|diagnostic| matches!(
            diagnostic.kind,
            ParseDiagnosticKind::Error | ParseDiagnosticKind::Missing
        )),
        "expected an error or missing diagnostic, got {:?}",
        results.parse_diagnostics
    );
}

#[test]
fn clean_sources_report_no_diagnostics() {
    let results = extract("-module(bank).\n-export([open/1]).\nopen(Id) -> Id.\n");

    assert!(
        results.parse_diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        results.parse_diagnostics
    );
}
