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
        extractor.base.type_info.get(&symbol.id).is_none(),
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
