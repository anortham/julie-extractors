//! Erlang string-literal call-argument capture.
//!
//! The carrier is the verbatim callee: the bare function atom for a local or
//! auto-imported call, and the `module:function` join for a remote call. `kind`
//! stays `Other`; classification and the carrier gate run later in the artifact
//! language-policy pass.

use super::support::parse;
use crate::base::{Literal, LiteralKind};
use crate::erlang::ErlangExtractor;
use std::path::PathBuf;

fn capture(code: &str) -> Vec<Literal> {
    let tree = parse(code);
    let mut extractor = ErlangExtractor::new(
        "erlang".to_string(),
        "bank.erl".to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    extractor.extract_identifiers(&tree, &symbols);
    extractor.base.take_literals()
}

fn find<'a>(literals: &'a [Literal], text: &str) -> &'a Literal {
    literals
        .iter()
        .find(|literal| literal.literal_text == text)
        .unwrap_or_else(|| panic!("no literal {text:?}; got {}", inventory(literals)))
}

fn inventory(literals: &[Literal]) -> String {
    format!(
        "{:?}",
        literals
            .iter()
            .map(|literal| (literal.literal_text.as_str(), literal.carrier.clone()))
            .collect::<Vec<_>>()
    )
}

#[test]
fn remote_and_local_call_arguments_carry_the_verbatim_callee() {
    let code = r#"-module(bank).
-export([log/2]).

log(Amount, Label) ->
    io:format("balance ~p~n", [Amount]),
    validate("positive", Label).

validate(_Kind, Label) ->
    Label.
"#;

    let literals = capture(code);

    let remote = find(&literals, "balance ~p~n");
    assert_eq!(remote.carrier.as_deref(), Some("io:format"));
    assert_eq!(remote.arg_position, 0);
    assert_eq!(remote.kind, LiteralKind::Other);

    let local = find(&literals, "positive");
    assert_eq!(local.carrier.as_deref(), Some("validate"));
    assert_eq!(local.arg_position, 0);
}

#[test]
fn argument_position_counts_over_the_whole_argument_list() {
    let code = r#"-module(bank).
-export([store/1]).

store(Key) ->
    ets:insert("audit", Key, "trailing").
"#;

    let literals = capture(code);

    assert_eq!(find(&literals, "audit").arg_position, 0);
    assert_eq!(find(&literals, "trailing").arg_position, 2);
}

#[test]
fn call_argument_literals_anchor_to_the_enclosing_function() {
    let code = r#"-module(bank).
-export([log/0]).

log() ->
    io:format("hello").
"#;

    let tree = parse(code);
    let mut extractor = ErlangExtractor::new(
        "erlang".to_string(),
        "bank.erl".to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    extractor.extract_identifiers(&tree, &symbols);
    let literals = extractor.base.take_literals();

    let log = symbols
        .iter()
        .find(|symbol| symbol.name == "log")
        .expect("expected log symbol");
    assert_eq!(
        find(&literals, "hello").containing_symbol_id.as_deref(),
        Some(log.id.as_str())
    );
}

#[test]
fn macro_body_call_arguments_are_captured() {
    let code = r#"-module(bank).

-define(LOG(Msg), io:format("~p~n", [Msg])).
"#;

    let literals = capture(code);

    assert_eq!(
        find(&literals, "~p~n").carrier.as_deref(),
        Some("io:format")
    );
}

#[test]
fn declaration_strings_are_not_call_argument_literals() {
    let code = r#"-module(bank).
-moduledoc "Account ledger entry points.".

-include_lib("stdlib/include/assert.hrl").

-doc "Read the stored balance.".
-spec balance(integer()) -> integer().
balance(Id) ->
    Id.
"#;

    assert!(
        capture(code).is_empty(),
        "declaration text is not executable code; got {}",
        inventory(&capture(code))
    );
}
