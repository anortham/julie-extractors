use super::support::{extract_with_types, find, find_kind};
use crate::base::SymbolKind;

const MODULE: &str = r#"-module(bank).

-export([open/1, close/2, tidy/0]).
-export_type([account/0]).

-type account() :: #account{}.
-opaque token() :: binary().
-type result(T) :: {ok, T} | {error, term()}.

-callback init(Args :: term()) -> {ok, term()}.

-spec open(integer()) -> {ok, account()} | {error, term()}.
open(Id) ->
    {ok, Id}.

-spec close(account(), integer()) -> ok.
close(_Account, _Amount) ->
    ok.

tidy() ->
    ok.
"#;

#[test]
fn function_spec_return_type_becomes_a_type_fact() {
    let (symbols, types) = extract_with_types(MODULE);
    let open = find(&symbols, "open");

    assert_eq!(
        types.get(&open.id).map(String::as_str),
        Some("{ok, account()} | {error, term()}")
    );
}

#[test]
fn spec_matches_the_function_it_annotates_by_name_and_arity() {
    let (symbols, types) = extract_with_types(MODULE);
    let close = find(&symbols, "close");

    assert_eq!(types.get(&close.id).map(String::as_str), Some("ok"));
}

#[test]
fn function_without_a_spec_has_no_type_fact() {
    let (symbols, types) = extract_with_types(MODULE);
    let tidy = find(&symbols, "tidy");

    assert_eq!(types.get(&tidy.id), None);
}

#[test]
fn type_alias_records_its_declared_form() {
    let (symbols, types) = extract_with_types(MODULE);
    let account = find_kind(&symbols, "account", SymbolKind::Type);

    assert_eq!(
        types.get(&account.id).map(String::as_str),
        Some("#account{}")
    );
}

#[test]
fn opaque_type_records_its_declared_form() {
    let (symbols, types) = extract_with_types(MODULE);
    let token = find_kind(&symbols, "token", SymbolKind::Type);

    assert_eq!(types.get(&token.id).map(String::as_str), Some("binary()"));
}

#[test]
fn parameterised_type_is_keyed_by_arity() {
    let (symbols, types) = extract_with_types(MODULE);
    let result = find_kind(&symbols, "result", SymbolKind::Type);

    assert_eq!(
        types.get(&result.id).map(String::as_str),
        Some("{ok, T} | {error, term()}")
    );
}

#[test]
fn callback_return_type_becomes_a_type_fact() {
    let (symbols, types) = extract_with_types(MODULE);
    let init = find(&symbols, "init");

    assert_eq!(
        types.get(&init.id).map(String::as_str),
        Some("{ok, term()}")
    );
}

#[test]
fn spec_and_callback_of_the_same_identity_stay_separate() {
    let code = r#"-module(server).

-callback handle(term()) -> callback_result.

-spec handle(term()) -> function_result.
handle(_Request) ->
    ok.
"#;
    let (symbols, types) = extract_with_types(code);
    let callback = symbols
        .iter()
        .find(|symbol| {
            symbol.name == "handle"
                && symbol
                    .signature
                    .as_deref()
                    .unwrap()
                    .starts_with("-callback")
        })
        .expect("callback symbol");
    let function = symbols
        .iter()
        .find(|symbol| {
            symbol.name == "handle"
                && symbol.kind == SymbolKind::Function
                && symbol.id != callback.id
        })
        .expect("function symbol");

    assert_eq!(
        types.get(&callback.id).map(String::as_str),
        Some("callback_result")
    );
    assert_eq!(
        types.get(&function.id).map(String::as_str),
        Some("function_result")
    );
}

#[test]
fn multi_line_spec_is_normalised_to_one_line() {
    let code = r#"-module(bank).

-spec wrapped(
        integer()
      ) ->
        {ok, integer()}.
wrapped(Id) ->
    {ok, Id}.
"#;
    let (symbols, types) = extract_with_types(code);
    let wrapped = find(&symbols, "wrapped");

    assert_eq!(
        types.get(&wrapped.id).map(String::as_str),
        Some("{ok, integer()}")
    );
}

#[test]
fn spec_with_a_when_guard_records_the_return_type_not_the_guard() {
    let code = r#"-module(bank).

-spec guarded(X) -> Y when X :: integer(), Y :: atom().
guarded(X) ->
    X.
"#;
    let (symbols, types) = extract_with_types(code);
    let guarded = find(&symbols, "guarded");

    assert_eq!(types.get(&guarded.id).map(String::as_str), Some("Y"));
}

#[test]
fn multi_clause_spec_records_the_first_clause_return_type() {
    let code = r#"-module(bank).

-spec route(get) -> read; (post) -> write.
route(get) ->
    read;
route(post) ->
    write.
"#;
    let (symbols, types) = extract_with_types(code);
    let route = find(&symbols, "route");

    assert_eq!(types.get(&route.id).map(String::as_str), Some("read"));
}

#[test]
fn spec_types_do_not_leak_into_call_identifiers() {
    let (_, identifiers) = super::support::extract_with_identifiers(MODULE);

    assert!(
        !identifiers
            .iter()
            .any(|identifier| identifier.name == "term"),
        "`term()` inside a -spec must not be read as a call; got {:?}",
        identifiers
            .iter()
            .map(|identifier| identifier.name.as_str())
            .collect::<Vec<_>>()
    );
}
