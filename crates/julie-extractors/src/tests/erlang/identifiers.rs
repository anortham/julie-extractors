use super::support::{extract_with_identifiers, find, identifier_inventory, named, only};
use crate::base::IdentifierKind;

#[test]
fn local_call_binds_to_the_calling_function() {
    let (symbols, identifiers) = extract_with_identifiers(
        r#"-module(m).
-export([f/1]).

f(X) -> g(X).

g(X) -> X.
"#,
    );

    let call = only(&identifiers, "g");
    assert_eq!(call.kind, IdentifierKind::Call);
    assert_eq!(
        call.containing_symbol_id.as_deref(),
        Some(find(&symbols, "f").id.as_str())
    );
}

#[test]
fn remote_call_emits_a_module_type_usage_and_a_call() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).

f(X) -> lists:reverse(X).
"#,
    );

    let module = only(&identifiers, "lists");
    assert_eq!(module.kind, IdentifierKind::TypeUsage);
    let call = only(&identifiers, "reverse");
    assert_eq!(call.kind, IdentifierKind::Call);
    assert!(
        module.start_column < call.start_column,
        "module qualifier must be anchored on the module atom: {}",
        identifier_inventory(&identifiers)
    );
}

#[test]
fn fun_reference_is_distinguishable_from_a_call_to_the_same_function() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).

f(X) ->
    Ref = fun g/1,
    g(X),
    Ref.

g(X) -> X.
"#,
    );

    let rows = named(&identifiers, "g");
    assert_eq!(rows.len(), 2, "{}", identifier_inventory(&identifiers));
    let kinds: Vec<_> = rows.iter().map(|row| row.kind.clone()).collect();
    assert!(kinds.contains(&IdentifierKind::Call));
    assert!(kinds.contains(&IdentifierKind::VariableRef));
}

#[test]
fn external_fun_reference_emits_a_module_type_usage_and_a_variable_ref() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).

f() -> fun lists:reverse/1.
"#,
    );

    assert_eq!(only(&identifiers, "lists").kind, IdentifierKind::TypeUsage);
    assert_eq!(
        only(&identifiers, "reverse").kind,
        IdentifierKind::VariableRef
    );
}

#[test]
fn imported_function_call_attributes_to_the_import_module() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).
-import(lists, [reverse/1]).

f(X) -> reverse(X).
"#,
    );

    assert_eq!(only(&identifiers, "reverse").kind, IdentifierKind::Call);
    assert_eq!(only(&identifiers, "lists").kind, IdentifierKind::TypeUsage);
}

#[test]
fn import_attribution_is_arity_sensitive() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).
-import(lists, [reverse/1]).

f(X, Y) -> reverse(X, Y).
"#,
    );

    assert_eq!(only(&identifiers, "reverse").kind, IdentifierKind::Call);
    assert!(
        named(&identifiers, "lists").is_empty(),
        "{}",
        identifier_inventory(&identifiers)
    );
}

#[test]
fn auto_imported_bif_calls_emit_no_module_reference() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).

f(X) when is_list(X) ->
    _ = length(X),
    _ = self(),
    spawn(fun g/0).

g() -> ok.
"#,
    );

    for name in ["length", "self", "spawn", "is_list"] {
        assert_eq!(
            only(&identifiers, name).kind,
            IdentifierKind::Call,
            "{name} must be a plain call"
        );
    }
    assert!(
        identifiers
            .iter()
            .all(|identifier| identifier.kind != IdentifierKind::TypeUsage),
        "auto-imported BIFs must not synthesise module references: {}",
        identifier_inventory(&identifiers)
    );
}

#[test]
fn macro_usage_with_arguments_is_a_call_and_bare_macro_usage_is_a_variable_ref() {
    let (symbols, identifiers) = extract_with_identifiers(
        r#"-module(m).
-define(LOG(Msg), Msg).
-define(LIMIT, 10).

f(X) ->
    ?LOG(X),
    ?LIMIT.
"#,
    );

    assert_eq!(only(&identifiers, "LOG").kind, IdentifierKind::Call);
    assert_eq!(
        only(&identifiers, "LIMIT").kind,
        IdentifierKind::VariableRef
    );
    assert_eq!(
        only(&identifiers, "LOG").containing_symbol_id.as_deref(),
        Some(find(&symbols, "f").id.as_str())
    );
}

#[test]
fn macro_body_calls_bind_to_the_macro_symbol() {
    let (symbols, identifiers) = extract_with_identifiers(
        r#"-module(m).
-define(LOG(Msg), io:format(Msg)).
"#,
    );

    let call = only(&identifiers, "format");
    assert_eq!(call.kind, IdentifierKind::Call);
    assert_eq!(
        call.containing_symbol_id.as_deref(),
        Some(find(&symbols, "LOG").id.as_str())
    );
    assert_eq!(only(&identifiers, "io").kind, IdentifierKind::TypeUsage);
}

#[test]
fn record_construction_emits_record_and_field_references() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).
-record(account, {id, balance}).

f(Id) -> #account{id = Id, balance = 0}.
"#,
    );

    assert_eq!(
        only(&identifiers, "account").kind,
        IdentifierKind::TypeUsage
    );
    assert_eq!(only(&identifiers, "id").kind, IdentifierKind::MemberAccess);
    assert_eq!(
        only(&identifiers, "balance").kind,
        IdentifierKind::MemberAccess
    );
}

#[test]
fn record_field_access_and_update_emit_record_and_field_references() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).
-record(account, {id, balance}).

f(Acct) -> Acct#account{balance = Acct#account.balance + 1}.
"#,
    );

    assert_eq!(named(&identifiers, "account").len(), 2);
    assert!(
        named(&identifiers, "account")
            .iter()
            .all(|row| row.kind == IdentifierKind::TypeUsage)
    );
    let fields = named(&identifiers, "balance");
    assert_eq!(fields.len(), 2, "{}", identifier_inventory(&identifiers));
    assert!(
        fields
            .iter()
            .all(|row| row.kind == IdentifierKind::MemberAccess)
    );
}

#[test]
fn record_index_expression_references_the_record_and_the_field() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).
-record(account, {id, balance}).

f() -> #account.balance.
"#,
    );

    assert_eq!(
        only(&identifiers, "account").kind,
        IdentifierKind::TypeUsage
    );
    assert_eq!(
        only(&identifiers, "balance").kind,
        IdentifierKind::MemberAccess
    );
}

#[test]
fn record_patterns_in_function_heads_reference_the_record() {
    let (symbols, identifiers) = extract_with_identifiers(
        r#"-module(m).
-record(account, {id}).

f(#account{id = Id}) -> Id.
"#,
    );

    let record = only(&identifiers, "account");
    assert_eq!(record.kind, IdentifierKind::TypeUsage);
    assert_eq!(
        record.containing_symbol_id.as_deref(),
        Some(find(&symbols, "f").id.as_str())
    );
}

#[test]
fn type_signatures_do_not_emit_call_identifiers() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).
-record(account, {id :: integer()}).
-type acct() :: #account{}.
-opaque token() :: binary().
-callback init(Args :: term()) -> {ok, term()}.
-spec f(#account{}) -> list().

f(Acct) -> Acct.
"#,
    );

    assert!(
        identifiers.is_empty(),
        "type signatures spell type names with call nodes and must stay out of the identifier tier: {}",
        identifier_inventory(&identifiers)
    );
}

#[test]
fn later_clauses_bind_identifiers_to_the_same_function_symbol() {
    let (symbols, identifiers) = extract_with_identifiers(
        r#"-module(m).

f(0) -> zero();
f(N) -> nonzero(N).

zero() -> 0.

nonzero(N) -> N.
"#,
    );

    let expected = find(&symbols, "f").id.as_str();
    assert_eq!(
        only(&identifiers, "zero").containing_symbol_id.as_deref(),
        Some(expected)
    );
    assert_eq!(
        only(&identifiers, "nonzero")
            .containing_symbol_id
            .as_deref(),
        Some(expected)
    );
}

#[test]
fn dynamic_call_through_a_variable_emits_no_identifier() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).

f(Fun, X) -> Fun(X).
"#,
    );

    assert!(
        identifiers.is_empty(),
        "{}",
        identifier_inventory(&identifiers)
    );
}

#[test]
fn export_and_attribute_declarations_emit_no_identifiers() {
    let (_, identifiers) = extract_with_identifiers(
        r#"-module(m).
-behaviour(gen_server).
-include_lib("stdlib/include/assert.hrl").
-export([f/0]).
-export_type([acct/0]).
-import(lists, [reverse/1]).

f() -> ok.
"#,
    );

    assert!(
        identifiers.is_empty(),
        "{}",
        identifier_inventory(&identifiers)
    );
}
