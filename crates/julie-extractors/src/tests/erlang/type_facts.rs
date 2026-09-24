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

#[test]
fn ok_tuple_match_takes_the_ok_payload_of_the_spec() {
    let source = r#"
-module(bank).

-spec load() -> {ok, state()}.
load() ->
    {ok, #{}}.

-spec open() -> {ok, conn()} | {error, term()}.
open() ->
    {ok, 1}.

-spec start() -> {ok, Pid :: server()} | ignore | {error, any(), any()}.
start() ->
    ignore.

run() ->
    {ok, S} = load(),
    {ok, C} = ?MODULE:open(),
    {'ok', P} = Started = start(),
    {ok, _} = load(),
    {S, C, P, Started}.
"#;
    let (symbols, extractor) = extract(source);
    assert_eq!(inferred_type(&extractor, &symbols, "S"), "state");
    assert_eq!(
        declared_metadata(&extractor, &symbols, "S").as_deref(),
        Some("state()")
    );
    assert_eq!(inferred_type(&extractor, &symbols, "C"), "conn");
    assert_eq!(inferred_type(&extractor, &symbols, "P"), "server");
    no_fact(&extractor, &symbols, "Started", SymbolKind::Variable);
    assert!(variables_named(&symbols, "_").is_empty());
}

#[test]
fn ok_tuple_match_without_one_certain_payload_records_nothing() {
    let source = r#"
-module(bank).
-type result() :: {ok, state()}.

-spec two() -> {ok, a()} | {ok, b()}.
two() -> ok.

-spec named() -> {ok, a()} | result().
named() -> ok.

-spec var() -> {ok, T} when T :: a().
var() -> ok.

-spec atom() -> {ok, done}.
atom() -> ok.

-spec list() -> {ok, [a()]}.
list() -> ok.

-spec failed() -> {error, a()}.
failed() -> ok.

-spec alias() -> result().
alias() -> ok.

-spec load() -> {ok, a()}.
load() -> ok.

run() ->
    {ok, Two} = two(),
    {ok, Named} = named(),
    {ok, Var} = var(),
    {ok, Atom} = atom(),
    {ok, List} = list(),
    {ok, Failed} = failed(),
    {ok, Alias} = alias(),
    {ok, Remote} = other:load(),
    {ok, Fun} = (fun load/0)(),
    {ok, Known} = load(),
    {Two, Named, Var, Atom, List, Failed, Alias, Remote, Fun, Known}.
"#;
    let (symbols, extractor) = extract(source);
    for name in [
        "Two", "Named", "Var", "Atom", "List", "Failed", "Alias", "Remote", "Fun",
    ] {
        no_fact(&extractor, &symbols, name, SymbolKind::Variable);
    }
    assert_eq!(inferred_type(&extractor, &symbols, "Known"), "a");
}

#[test]
fn only_an_ok_pair_pattern_binds_a_local() {
    let source = r#"
-module(bank).

-spec load() -> {ok, a()}.
load() -> ok.

run() ->
    {error, Reason} = load(),
    {ok, Value, Extra} = load(),
    {Tag, Payload} = load(),
    {Reason, Value, Extra, Tag, Payload}.
"#;
    let (symbols, _) = extract(source);
    for name in ["Reason", "Value", "Extra", "Tag", "Payload"] {
        assert!(variables_named(&symbols, name).is_empty(), "{name} bound");
    }
}

#[test]
fn atom_literal_spec_return_records_nothing() {
    let source = r#"
-module(bank).
-type ok() :: {ok, integer()}.

-spec stop() -> ok.
stop() -> ok.

-spec quit() -> Result :: 'ok'.
quit() -> ok.

-spec start() -> ok().
start() -> {ok, 1}.

run() ->
    A = stop(),
    Q = quit(),
    B = start(),
    {A, Q, B}.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "A", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "Q", SymbolKind::Variable);
    assert_eq!(inferred_type(&extractor, &symbols, "B"), "ok");
    assert_eq!(
        declared_metadata(&extractor, &symbols, "B").as_deref(),
        Some("ok()")
    );
}

#[test]
fn spec_return_without_values_records_nothing() {
    let source = r#"
-module(bank).

-spec halt() -> no_return().
halt() -> exit(x).

-spec never() -> none().
never() -> exit(x).

-spec load() -> state().
load() -> ok.

run() ->
    G = halt(),
    N = never(),
    S = load(),
    {G, N, S}.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "G", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "N", SymbolKind::Variable);
    assert_eq!(inferred_type(&extractor, &symbols, "S"), "state");
}

#[test]
fn variable_bound_by_any_other_pattern_in_the_function_records_nothing() {
    let source = r#"
-module(bank).
-record(state, {c}).

-spec load() -> #state{}.
load() -> #state{}.

a(x) -> S = load(), S;
a(y) -> {error, S} = other:get(), S.
b(x) -> S = load(), S;
b(y) -> [S | _] = other:get(), S.
c(x) -> S = load(), S;
c(y) -> case other:get() of S -> S end.
d(x) -> S = load(), S;
d(y) -> receive S -> S end.
e(x) -> S = load(), S;
e(y) -> try other:get() catch _:S -> S end.
f(x) -> S = load(), S;
f(y) -> [S || S <- other:get()].
g(x) -> S = load(), S;
g(y) -> {ok, S, _} = other:get(), S.
h(x) -> S = load(), S;
h(y) -> #{k := S} = other:get(), S.
i(x) -> S = load(), S;
i(y) -> F = fun(S) -> S end, F(1).
j(x) -> S = load(), S;
j(y) -> #state{c = S} = load(), S.
k(x) -> S = load(), S;
k(y) -> maybe {error, S} ?= other:get(), S end.
kept(x) -> Kept = load(), Kept;
kept(y) -> {error, R} = other:get(), R.
"#;
    let (symbols, extractor) = extract(source);
    let locals = variables_named(&symbols, "S");
    assert_eq!(locals.len(), 11);
    for local in locals {
        assert!(
            !extractor.base.type_info.contains_key(&local.id),
            "unexpected type fact for S in {:?}",
            local.parent_id
        );
    }
    assert_eq!(inferred_type(&extractor, &symbols, "Kept"), "state");
}

#[test]
fn maybe_conditional_match_binds_like_a_match() {
    let source = r#"
-module(bank).
-record(state, {c}).

-spec open() -> {ok, #state{}} | {error, term()}.
open() -> {ok, #state{}}.

-spec load() -> #state{}.
load() -> #state{}.

-spec named() -> {ok, a()} | result().
named() -> ok.

run() ->
    maybe
        {ok, A} ?= open(),
        B ?= load(),
        C = load(),
        {ok, N} ?= named(),
        {ok, R} ?= other:open(),
        {A, B, C, N, R}
    end.
"#;
    let (symbols, extractor) = extract(source);
    assert_eq!(inferred_type(&extractor, &symbols, "A"), "state");
    assert_eq!(inferred_type(&extractor, &symbols, "B"), "state");
    assert_eq!(inferred_type(&extractor, &symbols, "C"), "state");
    no_fact(&extractor, &symbols, "N", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "R", SymbolKind::Variable);
}

#[test]
fn parenthesized_and_block_values_take_the_call_type() {
    let source = r#"
-module(bank).
-record(state, {c}).

-spec load() -> #state{}.
load() -> #state{}.

-spec open() -> {ok, #state{}} | {error, term()}.
open() -> {ok, #state{}}.

run() ->
    U = (load()),
    V = begin log(), load() end,
    {ok, K} = (open()),
    {ok, L} = begin open() end,
    W = begin load(), other:get() end,
    {U, V, K, L, W}.
"#;
    let (symbols, extractor) = extract(source);
    for name in ["U", "V", "K", "L"] {
        assert_eq!(inferred_type(&extractor, &symbols, name), "state", "{name}");
    }
    no_fact(&extractor, &symbols, "W", SymbolKind::Variable);
}

#[test]
fn named_list_and_map_types_record_their_base_name_but_literal_syntax_does_not() {
    let source = r#"
-module(bank).

-spec l1() -> list(integer()).
l1() -> [].

-spec l2() -> nonempty_list(integer()).
l2() -> [1].

-spec m1() -> map().
m1() -> #{}.

-spec l3() -> [integer()].
l3() -> [].

-spec m2() -> #{atom() => integer()}.
m2() -> #{}.

run() ->
    A = l1(),
    B = l2(),
    C = m1(),
    D = l3(),
    E = m2(),
    {A, B, C, D, E}.
"#;
    let (symbols, extractor) = extract(source);
    assert_eq!(inferred_type(&extractor, &symbols, "A"), "list");
    assert_eq!(inferred_type(&extractor, &symbols, "B"), "nonempty_list");
    assert_eq!(inferred_type(&extractor, &symbols, "C"), "map");
    no_fact(&extractor, &symbols, "D", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "E", SymbolKind::Variable);
}

#[test]
fn record_match_inside_another_pattern_keeps_the_record_type() {
    let source = r#"
-module(bank).
-record(state, {c}).

run(x) -> S = #state{c = 1}, S;
run(y) -> case other:get() of S = #state{} -> S end.
wrap(x) -> W = #state{c = 1}, W;
wrap(y) -> {ok, #state{} = W} = other:get(), W.
"#;
    let (symbols, extractor) = extract(source);
    assert_eq!(inferred_type(&extractor, &symbols, "S"), "state");
    assert_eq!(inferred_type(&extractor, &symbols, "W"), "state");
}

#[test]
fn comments_inside_call_arguments_do_not_change_the_arity() {
    let source = r#"
-module(bank).
-record(a, {x}).
-record(b, {x}).

-spec load(term()) -> #a{}.
load(_) -> #a{}.

-spec load(term(), term()) -> #b{}.
load(_, _) -> #b{}.

run() ->
    C1 = load(1 %% note
    ),
    C2 = load(1, %% note
              2),
    {C1, C2}.
"#;
    let (symbols, extractor) = extract(source);
    assert_eq!(inferred_type(&extractor, &symbols, "C1"), "a");
    assert_eq!(inferred_type(&extractor, &symbols, "C2"), "b");
}

#[test]
fn comments_inside_spec_arguments_do_not_change_the_arity() {
    let source = r#"
-module(bank).
-record(a, {x}).
-record(b, {x}).

-spec get(Key :: atom() %% the lookup key
    ) -> #a{}.
get(_) -> #a{}.

get(_, _) -> #b{}.

run() ->
    D1 = get(k, v),
    D2 = get(k),
    {D1, D2}.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "D1", SymbolKind::Variable);
    assert_eq!(inferred_type(&extractor, &symbols, "D2"), "a");
}

#[test]
fn comments_inside_definition_arguments_do_not_change_the_arity() {
    let source = r#"
-module(bank).

load(_ %% only one
    ) -> ok.
"#;
    let (symbols, _) = extract(source);
    let load = symbol(&symbols, "load", SymbolKind::Function);
    assert_eq!(
        load.metadata.as_ref().and_then(|m| m.get("arity")),
        Some(&serde_json::json!(1))
    );
}

#[test]
fn spec_clauses_with_the_same_base_name_but_other_declared_types_record_nothing() {
    let source = r#"
-module(bank).
-record(state, {x}).
-type state() :: #{atom() => term()}.

-spec pick(a) -> #state{}; (b) -> state().
pick(a) -> #state{};
pick(b) -> #{}.

-spec open(a) -> {ok, #state{}} | error; (b) -> {ok, state()} | error.
open(a) -> {ok, #state{}};
open(b) -> {ok, #{}}.

-spec li(a) -> list(integer()); (b) -> list(atom()).
li(a) -> [1];
li(b) -> [b].

-spec same(a) -> state(); (b) -> state().
same(_) -> #{}.

run() ->
    G1 = pick(b),
    {ok, G2} = open(a),
    L = li(a),
    S = same(a),
    {G1, G2, L, S}.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "G1", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "G2", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "L", SymbolKind::Variable);
    assert_eq!(inferred_type(&extractor, &symbols, "S"), "state");
}

#[test]
fn definitions_split_by_a_preprocessor_conditional_record_nothing() {
    let source = r#"
-module(bank).
-record(a, {x}).
-record(b, {x}).

-ifdef(TEST).
-spec load() -> #a{}.
load() -> #a{}.
-else.
load() -> #b{}.
-endif.

-if(?OTP_RELEASE >= 27).
-spec open() -> #a{}.
open() -> #a{}.
-elif(?OTP_RELEASE >= 26).
-spec open() -> #a{}.
open() -> #a{}.
-endif.

-ifdef(TEST).
-spec only() -> #a{}.
only() -> #a{}.
-endif.

run() ->
    E1 = load(),
    E2 = open(),
    E3 = only(),
    {E1, E2, E3}.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "E1", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "E2", SymbolKind::Variable);
    assert_eq!(inferred_type(&extractor, &symbols, "E3"), "a");
}

#[test]
fn macro_arguments_hide_the_arity_and_record_nothing() {
    let source = r#"
-module(bank).
-define(TWO, 1, 2).
-define(ONE, 1).
-record(a, {x}).
-record(b, {x}).

-spec load(term()) -> #a{}.
load(_) -> #a{}.

-spec load(term(), term()) -> #b{}.
load(_, _) -> #b{}.

-spec open(?ONE) -> #a{}.
open(_) -> #a{}.

run() ->
    C2 = load(?TWO),
    C1 = load(1),
    O = open(1),
    {C1, C2, O}.
"#;
    let (symbols, extractor) = extract(source);
    no_fact(&extractor, &symbols, "C2", SymbolKind::Variable);
    no_fact(&extractor, &symbols, "O", SymbolKind::Variable);
    assert_eq!(inferred_type(&extractor, &symbols, "C1"), "a");
}
