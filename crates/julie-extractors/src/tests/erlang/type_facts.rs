use super::support::parse;
use crate::base::{Symbol, SymbolKind, TypeInfo};
use crate::erlang::ErlangExtractor;
use std::path::PathBuf;

fn extract(code: &str) -> (Vec<Symbol>, ErlangExtractor) {
    let tree = parse(code);
    let mut extractor = ErlangExtractor::new(
        "erlang".to_string(),
        "bank.erl".to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn symbol<'a>(symbols: &'a [Symbol], name: &str, kind: SymbolKind) -> &'a Symbol {
    symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn fact<'a>(
    extractor: &'a ErlangExtractor,
    symbols: &[Symbol],
    name: &str,
    kind: SymbolKind,
) -> &'a TypeInfo {
    let symbol = symbol(symbols, name, kind);
    extractor
        .base
        .type_info
        .get(&symbol.id)
        .unwrap_or_else(|| panic!("missing type fact for {name}"))
}

fn no_fact(extractor: &ErlangExtractor, symbols: &[Symbol], name: &str, kind: SymbolKind) {
    let symbol = symbol(symbols, name, kind);
    assert!(
        !extractor.base.type_info.contains_key(&symbol.id),
        "unexpected type fact for {name}"
    );
}

fn role(symbol: &Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get("role"))
        .and_then(|v| v.as_str())
}

fn variables_named<'a>(symbols: &'a [Symbol], name: &str) -> Vec<&'a Symbol> {
    symbols
        .iter()
        .filter(|s| s.name == name && s.kind == SymbolKind::Variable)
        .collect()
}

#[test]
fn multi_clause_record_pattern_emits_one_parameter_per_name() {
    let source = r#"
-module(bank).
-record(state, {n = 0}).

run(#state{} = S, N) ->
    {S, N};
run(S, 0) ->
    S.
"#;
    let (symbols, extractor) = extract(source);
    let run = symbol(&symbols, "run", SymbolKind::Function);
    let params = variables_named(&symbols, "S");
    assert_eq!(params.len(), 1);
    let state = params[0];
    assert_eq!(role(state), Some("parameter"));
    assert_eq!(state.parent_id.as_deref(), Some(run.id.as_str()));
    let n = symbol(&symbols, "N", SymbolKind::Variable);
    assert_eq!(role(n), Some("parameter"));
    assert_eq!(n.parent_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(variables_named(&symbols, "N").len(), 1);
    let fact = fact(&extractor, &symbols, "S", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "state");
    assert!(!fact.is_inferred);
    no_fact(&extractor, &symbols, "N", SymbolKind::Variable);
}

#[test]
fn body_record_literal_assigns_inferred_fact() {
    let source = r#"
-module(bank).
-record(req, {id}).

go(X) ->
    R = #req{id = X},
    R.
"#;
    let (symbols, extractor) = extract(source);
    let go = symbol(&symbols, "go", SymbolKind::Function);
    let x = symbol(&symbols, "X", SymbolKind::Variable);
    assert_eq!(role(x), Some("parameter"));
    assert_eq!(x.parent_id.as_deref(), Some(go.id.as_str()));
    no_fact(&extractor, &symbols, "X", SymbolKind::Variable);
    let r = symbol(&symbols, "R", SymbolKind::Variable);
    assert_ne!(role(r), Some("parameter"));
    assert_eq!(r.parent_id.as_deref(), Some(go.id.as_str()));
    let fact = fact(&extractor, &symbols, "R", SymbolKind::Variable);
    assert_eq!(fact.resolved_type, "req");
    assert!(fact.is_inferred);
}

#[test]
fn maps_new_assignment_is_symbol_without_fact() {
    let source = r#"
-module(bank).

scratch() ->
    M = maps:new(),
    M.
"#;
    let (symbols, extractor) = extract(source);
    let scratch = symbol(&symbols, "scratch", SymbolKind::Function);
    let m = symbol(&symbols, "M", SymbolKind::Variable);
    assert_eq!(m.parent_id.as_deref(), Some(scratch.id.as_str()));
    no_fact(&extractor, &symbols, "M", SymbolKind::Variable);
}

#[test]
fn unknown_record_literal_records_no_fact() {
    let source = r#"
-module(bank).

run() ->
    Client = #missing{},
    Client.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "Client", SymbolKind::Variable);
}

#[test]
fn qualified_remote_constructor_records_no_fact() {
    let source = r#"
-module(bank).

run() ->
    Client = other:new(),
    Client.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "Client", SymbolKind::Variable);
}

#[test]
fn artifact_types_never_carry_non_base_names() {
    let source = r#"
-module(bank).

-record(account, {id}).

-type account() :: #account{}.
-opaque token() :: binary().
-type result(T) :: {ok, T} | {error, term()}.

-spec listing() -> [atom()].
listing() ->
    [].

-spec pair() -> {ok, integer()}.
pair() ->
    {ok, 1}.

-spec status() -> ok.
status() ->
    ok.

-spec size() -> non_neg_integer().
size() ->
    0.

-spec remote() -> unicode:chardata().
remote() ->
    <<>>.

-spec callback() -> fun(() -> ok).
callback() ->
    fun() -> ok end.

-spec annotated() -> Result :: integer().
annotated() ->
    1.
"#;
    let tree = parse(source);
    let results = crate::factory::extract_symbols_and_relationships(
        &tree,
        "bank.erl",
        source,
        "erlang",
        &PathBuf::from("/tmp/test"),
    )
    .unwrap();
    let resolved = |name: &str, kind: SymbolKind| -> Option<String> {
        let symbol = symbol(&results.symbols, name, kind);
        results
            .types
            .get(&symbol.id)
            .map(|info| info.resolved_type.clone())
    };
    assert_eq!(
        resolved("account", SymbolKind::Type).as_deref(),
        Some("account")
    );
    assert_eq!(
        resolved("token", SymbolKind::Type).as_deref(),
        Some("binary")
    );
    assert_eq!(resolved("result", SymbolKind::Type), None);
    assert_eq!(resolved("listing", SymbolKind::Function), None);
    assert_eq!(resolved("pair", SymbolKind::Function), None);
    assert_eq!(
        resolved("status", SymbolKind::Function).as_deref(),
        Some("ok")
    );
    assert_eq!(
        resolved("size", SymbolKind::Function).as_deref(),
        Some("non_neg_integer")
    );
    assert_eq!(
        resolved("remote", SymbolKind::Function).as_deref(),
        Some("unicode:chardata")
    );
    assert_eq!(resolved("callback", SymbolKind::Function), None);
    assert_eq!(
        resolved("annotated", SymbolKind::Function).as_deref(),
        Some("integer")
    );
    for info in results.types.values() {
        let value = info.resolved_type.as_str();
        assert!(
            !value.contains(['[', '(', '{', '#', '|', ' ']),
            "non-base resolved_type {value}"
        );
    }
}

fn extract_path(file_path: &str, code: &str) -> (Vec<Symbol>, ErlangExtractor) {
    let tree = parse(code);
    let mut extractor = ErlangExtractor::new(
        "erlang".to_string(),
        file_path.to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    (symbols, extractor)
}

fn inferred_type(extractor: &ErlangExtractor, symbols: &[Symbol], name: &str) -> String {
    let fact = fact(extractor, symbols, name, SymbolKind::Variable);
    assert!(fact.is_inferred, "fact for {name} is not inferred");
    fact.resolved_type.clone()
}

fn declared_metadata(
    extractor: &ErlangExtractor,
    symbols: &[Symbol],
    name: &str,
) -> Option<String> {
    fact(extractor, symbols, name, SymbolKind::Variable)
        .metadata
        .as_ref()
        .and_then(|m| m.get("declared"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

#[test]
fn local_call_takes_the_same_file_spec_return_type() {
    let source = r#"
-module(bank).
-record(workspace, {root}).

-spec load() -> #workspace{}.
load() ->
    #workspace{}.

-spec open(integer()) -> account().
open(_Id) ->
    ok.

-spec fetch() -> unicode:chardata().
fetch() ->
    <<>>.

-spec total() -> Total :: non_neg_integer().
total() ->
    0.

run() ->
    W = load(),
    A = open(1),
    C = fetch(),
    T = total(),
    {W, A, C, T}.
"#;
    let (symbols, extractor) = extract(source);
    assert_eq!(inferred_type(&extractor, &symbols, "W"), "workspace");
    assert_eq!(
        declared_metadata(&extractor, &symbols, "W").as_deref(),
        Some("#workspace{}")
    );
    assert_eq!(inferred_type(&extractor, &symbols, "A"), "account");
    assert_eq!(inferred_type(&extractor, &symbols, "C"), "unicode:chardata");
    assert_eq!(inferred_type(&extractor, &symbols, "T"), "non_neg_integer");
}

#[test]
fn self_qualified_calls_take_the_same_file_spec_return_type() {
    let source = r#"
-module(bank).

-spec load() -> state().
load() ->
    ok.

run() ->
    M = ?MODULE:load(),
    B = bank:load(),
    {M, B}.
"#;
    let (symbols, extractor) = extract(source);
    assert_eq!(inferred_type(&extractor, &symbols, "M"), "state");
    assert_eq!(inferred_type(&extractor, &symbols, "B"), "state");
}

#[test]
fn chained_match_gives_every_variable_the_call_type() {
    let source = r#"
-module(bank).

-spec load() -> state().
load() ->
    ok.

run() ->
    A = B = load(),
    {A, B}.
"#;
    let (symbols, extractor) = extract(source);
    assert_eq!(inferred_type(&extractor, &symbols, "A"), "state");
    assert_eq!(inferred_type(&extractor, &symbols, "B"), "state");
}

#[test]
fn multi_clause_spec_with_one_return_type_records_it() {
    let source = r#"
-module(bank).

-spec load(atom()) -> state(); (binary()) -> state().
load(_) ->
    ok.

run() ->
    S = load(a),
    S.
"#;
    let (symbols, extractor) = extract(source);
    assert_eq!(inferred_type(&extractor, &symbols, "S"), "state");
}

#[test]
fn multi_clause_spec_with_different_return_types_records_nothing() {
    let source = r#"
-module(bank).

-spec load(atom()) -> state(); (binary()) -> other().
load(_) ->
    ok.

run() ->
    S = load(a),
    S.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "S", SymbolKind::Variable);
}

#[test]
fn call_to_another_module_records_nothing() {
    let source = r#"
-module(bank).

-spec load() -> state().
load() ->
    ok.

run() ->
    S = other:load(),
    S.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "S", SymbolKind::Variable);
}

#[test]
fn call_with_a_different_arity_records_nothing() {
    let source = r#"
-module(bank).

-spec load() -> state().
load() ->
    ok.

load(_) ->
    ok.

run() ->
    S = load(1),
    S.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "S", SymbolKind::Variable);
}

#[test]
fn call_to_a_function_without_a_spec_records_nothing() {
    let source = r#"
-module(bank).

load() ->
    ok.

run() ->
    S = load(),
    S.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "S", SymbolKind::Variable);
}

#[test]
fn spec_without_a_same_file_definition_records_nothing() {
    let source = r#"
-module(bank).
-import(store, [load/0]).

-spec load() -> state().

run() ->
    S = load(),
    S.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "S", SymbolKind::Variable);
}

#[test]
fn type_variable_returns_record_nothing() {
    let source = r#"
-module(bank).

-spec id(T) -> T.
id(X) ->
    X.

-spec pick() -> T when T :: state().
pick() ->
    ok.

run() ->
    I = id(1),
    P = pick(),
    {I, P}.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "I", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "P", SymbolKind::Variable);
}

#[test]
fn returns_without_a_single_base_name_record_nothing() {
    let source = r#"
-module(bank).

-spec open() -> {ok, state()} | {error, term()}.
open() ->
    ok.

-spec names() -> [state()].
names() ->
    [].

run() ->
    O = open(),
    N = names(),
    {O, N}.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "O", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "N", SymbolKind::Variable);
}

#[test]
fn fun_macro_and_catch_calls_record_nothing() {
    let source = r#"
-module(bank).
-define(LOAD(), load()).

-spec load() -> state().
load() ->
    ok.

run() ->
    F = fun load/0,
    V = F(),
    M = ?LOAD(),
    C = catch load(),
    {V, M, C}.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "V", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "M", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "C", SymbolKind::Variable);
}

#[test]
fn module_macro_in_a_header_records_nothing() {
    let source = r#"
-spec load() -> state().
load() ->
    ok.

run() ->
    M = ?MODULE:load(),
    L = load(),
    {M, L}.
"#;
    let (symbols, extractor) = extract_path("bank.hrl", source);
    no_fact(&extractor, &symbols, "M", SymbolKind::Variable);
    assert_eq!(inferred_type(&extractor, &symbols, "L"), "state");
}

#[test]
fn variable_bound_to_different_types_across_clauses_records_nothing() {
    let source = r#"
-module(bank).
-record(req, {id}).
-record(resp, {id}).

-spec load() -> state().
load() ->
    ok.

run(a) ->
    S = load(),
    R = #req{},
    {S, R};
run(b) ->
    S = load(),
    R = #resp{},
    {S, R};
run(c) ->
    S = load(),
    R = undefined,
    {S, R}.
"#;
    let (symbols, extractor) = extract(source);
    assert_eq!(variables_named(&symbols, "R").len(), 1);
    assert_eq!(inferred_type(&extractor, &symbols, "S"), "state");
    no_fact(&extractor, &symbols, "R", SymbolKind::Variable);
}

#[test]
fn variable_bound_to_an_unknown_value_later_records_nothing() {
    let source = r#"
-module(bank).

-spec load() -> state().
load() ->
    ok.

run(a) ->
    S = load(),
    S;
run(b) ->
    S = other:load(),
    S.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "S", SymbolKind::Variable);
}

#[test]
fn duplicate_specs_with_different_return_types_record_nothing() {
    let source = r#"
-module(bank).

-spec load() -> state().
-spec load() -> other().
load() ->
    ok.

run() ->
    S = load(),
    S.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "S", SymbolKind::Variable);
}

#[test]
fn variable_call_named_like_a_quoted_function_records_nothing() {
    let source = r#"
-module(bank).

-spec 'Load'() -> state().
'Load'() ->
    ok.

run(Load) ->
    V = Load(),
    Q = 'Load'(),
    {V, Q}.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "V", SymbolKind::Variable);
    assert_eq!(inferred_type(&extractor, &symbols, "Q"), "state");
}

#[test]
fn record_update_keeps_the_record_type_across_clauses() {
    let source = r#"
-module(bank).
-record(stream, {id, state}).

next(new, Id) ->
    Stream = #stream{id = Id},
    Stream;
next(Stream0, Next) ->
    Stream = Stream0#stream{state = Next},
    Stream.
"#;
    let (symbols, extractor) = extract(source);
    assert_eq!(inferred_type(&extractor, &symbols, "Stream"), "stream");
}

#[test]
fn record_update_of_another_file_record_records_nothing() {
    let source = r#"
-module(bank).

next(Stream0) ->
    Stream = Stream0#stream{state = done},
    Stream.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "Stream", SymbolKind::Variable);
}
