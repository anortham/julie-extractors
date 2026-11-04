use super::support::{extract, extract_from, find, find_kind};
use crate::base::SymbolKind;
use serde_json::Value;

const MODULE: &str = r#"-module(bank).

-export([open/1]).

-define(MAX_BALANCE, 1000000).
-define(LOG(Msg), io:format("~p~n", [Msg])).

-record(account, {id :: integer(), balance = 0 :: integer()}).

-type account() :: #account{}.
-opaque token() :: binary().

-callback init(Args :: term()) -> {ok, term()}.

open(Id) ->
    #account{id = Id}.

route({get, Path}) ->
    Path;
route({post, Path}) ->
    Path;
route(_) ->
    undefined.
"#;

#[test]
fn extracts_module_symbol_from_module_attribute() {
    let symbols = extract(MODULE);
    let module = find(&symbols, "bank");

    assert_eq!(module.kind, SymbolKind::Module);
    assert_eq!(module.signature.as_deref(), Some("-module(bank)"));
    assert_eq!(module.parent_id, None);
}

#[test]
fn parents_declarations_to_the_module_symbol() {
    let symbols = extract(MODULE);
    let module_id = find(&symbols, "bank").id.clone();

    for name in ["account", "MAX_BALANCE", "token", "open"] {
        assert_eq!(
            find(&symbols, name).parent_id.as_deref(),
            Some(module_id.as_str()),
            "{name} should be parented to the module"
        );
    }
}

#[test]
fn function_signature_carries_name_arity_and_clause_head() {
    let symbols = extract(MODULE);
    let open = find(&symbols, "open");

    assert_eq!(open.kind, SymbolKind::Function);
    assert_eq!(open.signature.as_deref(), Some("open/1(Id)"));
}

#[test]
fn multiple_clauses_collapse_into_one_symbol() {
    let symbols = extract(MODULE);
    let routes: Vec<_> = symbols
        .iter()
        .filter(|symbol| symbol.name == "route")
        .collect();

    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].signature.as_deref(), Some("route/1({get, Path})"));
    assert_eq!(
        routes[0].metadata.as_ref().unwrap().get("clause_count"),
        Some(&Value::from(3))
    );
}

#[test]
fn multi_clause_symbol_spans_through_the_last_clause() {
    let symbols = extract(MODULE);
    let route = find(&symbols, "route");
    let last_clause_end = MODULE.rfind("undefined.").unwrap() + "undefined.".len();

    assert_eq!(
        route.start_byte as usize,
        MODULE.find("route({get").unwrap()
    );
    assert_eq!(route.end_byte as usize, last_clause_end);
}

#[test]
fn body_hash_moves_when_a_later_clause_changes() {
    let before = extract("-module(m).\nf(1) -> a;\nf(2) -> b.\n");
    let after = extract("-module(m).\nf(1) -> a;\nf(2) -> c.\n");

    assert!(find(&before, "f").body_hash.is_some());
    assert_ne!(find(&before, "f").body_hash, find(&after, "f").body_hash);
}

#[test]
fn body_span_covers_the_clause_bodies_not_the_first_brace_run() {
    let code = "-module(m).\nopen(Id) ->\n    #account{id = Id}.\n";
    let symbols = extract(code);
    let body = find(&symbols, "open").body_span.expect("body span");

    assert_eq!(body.start_byte as usize, code.find("->").unwrap());
    assert_eq!(body.end_byte as usize, code.trim_end().len());
}

#[test]
fn same_name_different_arity_are_separate_symbols() {
    let symbols = extract("-module(m).\nf() -> ok.\nf(X) -> X.\n");
    let signatures: Vec<_> = symbols
        .iter()
        .filter(|symbol| symbol.name == "f")
        .map(|symbol| symbol.signature.clone().unwrap())
        .collect();

    assert_eq!(signatures, vec!["f/0()".to_string(), "f/1(X)".to_string()]);
}

#[test]
fn extracts_record_with_its_fields() {
    let symbols = extract(MODULE);
    let record = find_kind(&symbols, "account", SymbolKind::Struct);

    assert_eq!(record.kind, SymbolKind::Struct);
    assert_eq!(
        record.signature.as_deref(),
        Some("-record(account, {id :: integer(), balance = 0 :: integer()})")
    );

    let id_field = find(&symbols, "id");
    assert_eq!(id_field.kind, SymbolKind::Field);
    assert_eq!(id_field.parent_id.as_deref(), Some(record.id.as_str()));
    assert_eq!(id_field.signature.as_deref(), Some("id :: integer()"));

    let balance_field = find(&symbols, "balance");
    assert_eq!(balance_field.kind, SymbolKind::Field);
    assert_eq!(
        balance_field.signature.as_deref(),
        Some("balance = 0 :: integer()")
    );
}

#[test]
fn module_and_record_fields_have_no_implementation_bodies() {
    let symbols = extract(MODULE);

    for name in ["bank", "id", "balance"] {
        let symbol = find(&symbols, name);
        assert_eq!(symbol.body_span, None, "{name}");
        assert_eq!(symbol.body_hash, None, "{name}");
    }
}

#[test]
fn application_resource_module_body_spans_the_descriptor_and_contains_its_env_value() {
    let code = include_str!("../../../../../fixtures/extraction/erlang/app_resource/kv.app.src");
    let symbols = extract_from("src/kv.app.src", code);
    let application = find(&symbols, "kv");
    let application_body = application.body_span.expect("application descriptor body");
    let application_start = code.find("{application, kv,").unwrap() as u32;
    let application_end = code.rfind('.').unwrap() as u32;
    let expected_application_body = "{application, kv,\n [{description, \"Key-value store\"},\n  {vsn, \"1.0.0\"},\n  {mod, {kv_app, []}},\n  {applications, [kernel, stdlib, cowboy]},\n  {included_applications, [kv_audit]},\n  {env, [{http_port, 8080},\n         {pool, [{size, 10}, {overflow, 5}]}]}]}";

    assert_eq!(application.kind, SymbolKind::Module);
    assert_eq!(application_body.start_byte, application_start);
    assert_eq!(application_body.end_byte, application_end);
    assert_eq!(
        &code[application_start as usize..application_end as usize],
        expected_application_body
    );
    assert_eq!(
        application.body_hash.as_deref(),
        Some("fc642927fad21bd4152e553463f744b7")
    );

    let port = find(&symbols, "http_port");
    let port_body = port.body_span.expect("environment value body");
    assert_eq!(port.parent_id.as_deref(), Some(application.id.as_str()));
    assert_eq!(
        &code[port_body.start_byte as usize..port_body.end_byte as usize],
        "8080"
    );
    assert!(
        port_body.start_byte >= application_body.start_byte
            && port_body.end_byte <= application_body.end_byte
    );
}

#[test]
fn extracts_macros_with_and_without_arguments() {
    let symbols = extract(MODULE);

    let constant = find(&symbols, "MAX_BALANCE");
    assert_eq!(constant.kind, SymbolKind::Constant);
    assert_eq!(
        constant.signature.as_deref(),
        Some("-define(MAX_BALANCE, 1000000)")
    );
    let constant_body = constant.body_span.expect("macro replacement body");
    let constant_start = MODULE.find("1000000").unwrap() as u32;
    assert_eq!(constant_body.start_byte, constant_start);
    assert_eq!(
        constant_body.end_byte,
        constant_start + "1000000".len() as u32
    );
    assert_eq!(
        &MODULE[constant_body.start_byte as usize..constant_body.end_byte as usize],
        "1000000"
    );
    assert_eq!(
        constant.body_hash.as_deref(),
        Some("8155bc545f84d9652f1012ef2bdfb6eb")
    );
    assert!(
        !constant
            .metadata
            .as_ref()
            .unwrap()
            .contains_key("macro_arity")
    );

    let log = find(&symbols, "LOG");
    assert_eq!(log.kind, SymbolKind::Constant);
    let log_body = log.body_span.expect("macro replacement body");
    let replacement = "io:format(\"~p~n\", [Msg])";
    let log_start = MODULE.find(replacement).unwrap() as u32;
    assert_eq!(log_body.start_byte, log_start);
    assert_eq!(log_body.end_byte, log_start + replacement.len() as u32);
    assert_eq!(
        &MODULE[log_body.start_byte as usize..log_body.end_byte as usize],
        replacement
    );
    assert_eq!(
        log.body_hash.as_deref(),
        Some("06a13bc1eaf9677a1817e1506f24baf9")
    );
    assert_eq!(
        log.metadata.as_ref().unwrap().get("macro_arity"),
        Some(&Value::from(1))
    );
}

#[test]
fn extracts_type_alias_distinctly_from_the_record_of_the_same_name() {
    let symbols = extract(MODULE);
    let account_type = find_kind(&symbols, "account", SymbolKind::Type);

    assert_eq!(
        account_type.signature.as_deref(),
        Some("-type account() :: #account{}")
    );
    assert_eq!(account_type.metadata.as_ref().unwrap().get("opaque"), None);
}

#[test]
fn extracts_opaque_type_with_opaque_metadata() {
    let symbols = extract(MODULE);
    let token = find(&symbols, "token");

    assert_eq!(token.kind, SymbolKind::Type);
    assert_eq!(
        token.signature.as_deref(),
        Some("-opaque token() :: binary()")
    );
    assert_eq!(
        token.metadata.as_ref().unwrap().get("opaque"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        token.metadata.as_ref().unwrap().get("arity"),
        Some(&Value::from(0))
    );
}

#[test]
fn type_arity_counts_declared_parameters() {
    let symbols = extract("-module(m).\n-type pair(A, B) :: {A, B}.\n");
    let pair = find(&symbols, "pair");

    assert_eq!(pair.kind, SymbolKind::Type);
    assert_eq!(
        pair.metadata.as_ref().unwrap().get("arity"),
        Some(&Value::from(2))
    );
}

#[test]
fn extracts_behaviour_callback_as_a_public_function() {
    let symbols = extract(MODULE);
    let init = find(&symbols, "init");

    assert_eq!(init.kind, SymbolKind::Function);
    assert_eq!(
        init.metadata.as_ref().unwrap().get("callback"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        init.metadata.as_ref().unwrap().get("arity"),
        Some(&Value::from(1))
    );
}

#[test]
fn flags_eunit_test_functions() {
    let symbols = extract("-module(m).\nbalance_test() -> ok.\nbalance(X) -> X.\n");

    assert_eq!(
        find(&symbols, "balance_test")
            .metadata
            .as_ref()
            .unwrap()
            .get("is_test"),
        Some(&Value::Bool(true))
    );
    assert!(
        !find(&symbols, "balance")
            .metadata
            .as_ref()
            .unwrap()
            .contains_key("is_test")
    );
}

#[test]
fn body_hash_ignores_erlang_comments() {
    let without = extract("-module(m).\naudit(A) ->\n    A.\n");
    let with = extract("-module(m).\naudit(A) ->\n    % explain the passthrough\n    A.\n");

    assert_eq!(
        find(&without, "audit").body_hash,
        find(&with, "audit").body_hash
    );
    assert!(find(&without, "audit").body_hash.is_some());
}
