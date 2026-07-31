use super::support::{
    extract_from_with_relationships, extract_with_relationships, find, pending_inventory,
    pending_named, relationship_inventory,
};
use crate::base::RelationshipKind;

#[test]
fn same_file_call_resolves_to_the_callee_function_symbol() {
    let code = r#"-module(bank).
-export([entry/0]).

entry() ->
    helper(1).

helper(X) -> X.
"#;
    let (symbols, relationships, _) = extract_with_relationships(code);
    let entry = find(&symbols, "entry");
    let helper = find(&symbols, "helper");

    assert_eq!(
        relationships.len(),
        1,
        "expected one call edge; got {}",
        relationship_inventory(&relationships)
    );
    let edge = &relationships[0];
    assert_eq!(edge.from_symbol_id, entry.id);
    assert_eq!(edge.to_symbol_id, helper.id);
    assert_eq!(edge.kind, RelationshipKind::Calls);
    assert_eq!(edge.line_number, 5);
    assert!(edge.reference_site_is_exact);
}

#[test]
fn call_arity_selects_between_same_named_functions() {
    let code = r#"-module(bank).
-export([entry/0]).

entry() ->
    helper(1, 2).

helper(X) -> X.
helper(X, Y) -> X + Y.
"#;
    let (symbols, relationships, _) = extract_with_relationships(code);
    let helper_2 = symbols
        .iter()
        .find(|symbol| symbol.name == "helper" && symbol.signature.as_deref().unwrap().contains("/2"))
        .expect("helper/2 symbol");

    assert_eq!(relationships.len(), 1);
    assert_eq!(relationships[0].to_symbol_id, helper_2.id);
}

#[test]
fn auto_imported_bif_call_emits_no_edge_and_no_pending() {
    let code = r#"-module(bank).
-export([entry/1]).

entry(List) ->
    length(List).
"#;
    let (_, relationships, pending) = extract_with_relationships(code);

    assert!(
        relationships.is_empty(),
        "BIF call must not resolve: {}",
        relationship_inventory(&relationships)
    );
    assert!(
        pending.is_empty(),
        "BIF call must not become a pending edge; got {pending:#?}"
    );
}

#[test]
fn dynamic_call_through_a_variable_emits_nothing() {
    let code = r#"-module(bank).
-export([entry/1]).

entry(Fun) ->
    Fun(1).
"#;
    let (_, relationships, pending) = extract_with_relationships(code);

    assert!(relationships.is_empty());
    assert!(pending.is_empty());
}

#[test]
fn type_signatures_emit_no_relationship_or_pending_rows() {
    let code = r#"-module(bank).
-export([entry/1]).

-type token() :: binary().
-opaque handle() :: reference().
-callback init(Args :: term()) -> {ok, term()}.
-record(account, {id :: integer()}).
-spec entry(integer()) -> list().
entry(X) -> X.
"#;
    let (_, relationships, pending) = extract_with_relationships(code);

    assert!(
        relationships.is_empty(),
        "type signatures leaked relationship rows: {}",
        relationship_inventory(&relationships)
    );
    assert!(
        pending.is_empty(),
        "type signatures leaked pending rows: {pending:#?}"
    );
}

#[test]
fn remote_call_becomes_a_structured_pending_call_with_module_namespace() {
    let code = r#"-module(bank).
-export([entry/0]).

entry() ->
    ledger:record(1).
"#;
    let (symbols, relationships, pending) = extract_with_relationships(code);
    let entry = find(&symbols, "entry");

    assert!(
        relationships.is_empty(),
        "remote call must not resolve in-file: {}",
        relationship_inventory(&relationships)
    );
    let edge = pending_named(&pending, "record");
    assert_eq!(edge.pending.kind, RelationshipKind::Calls);
    assert_eq!(edge.pending.from_symbol_id, entry.id);
    assert_eq!(edge.caller_scope_symbol_id.as_deref(), Some(entry.id.as_str()));
    assert_eq!(edge.pending.file_path, "bank.erl");
    assert_eq!(edge.pending.line_number, 5);
    assert_eq!(edge.target.display_name, "ledger:record");
    assert_eq!(edge.target.terminal_name, "record");
    assert_eq!(edge.target.namespace_path, vec!["ledger".to_string()]);
    assert_eq!(edge.target.receiver, None);
    assert_eq!(edge.target.import_context, None);
    assert!(edge.reference_site_is_exact);
}

#[test]
fn module_qualified_call_through_a_macro_emits_no_pending_module() {
    let code = r#"-module(bank).
-export([entry/0]).

entry() ->
    ?MODULE:helper(1).

helper(X) -> X.
"#;
    let (_, _, pending) = extract_with_relationships(code);

    assert!(
        pending.is_empty(),
        "?MODULE: has no spelled module atom; got {pending:#?}"
    );
}

#[test]
fn imported_call_carries_the_declaring_module_and_import_context() {
    let code = r#"-module(bank).
-import(lists, [reverse/1]).
-export([entry/1]).

entry(List) ->
    reverse(List).
"#;
    let (_, relationships, pending) = extract_with_relationships(code);

    assert!(relationships.is_empty());
    let edge = pending_named(&pending, "reverse");
    assert_eq!(edge.pending.kind, RelationshipKind::Calls);
    assert_eq!(edge.target.display_name, "reverse");
    assert_eq!(edge.target.terminal_name, "reverse");
    assert_eq!(edge.target.namespace_path, vec!["lists".to_string()]);
    assert_eq!(edge.target.import_context.as_deref(), Some("import"));
}

#[test]
fn import_attribution_is_arity_sensitive() {
    let code = r#"-module(bank).
-import(lists, [reverse/1]).
-export([entry/2]).

entry(A, B) ->
    reverse(A, B).
"#;
    let (_, _, pending) = extract_with_relationships(code);

    assert!(
        pending.iter().all(|edge| edge.target.terminal_name != "reverse"),
        "reverse/2 is not the imported reverse/1; got {pending:#?}"
    );
}

#[test]
fn behaviour_attribute_emits_a_pending_implements_edge_from_the_module() {
    let code = r#"-module(bank).
-behaviour(gen_server).
-export([entry/0]).

entry() -> ok.
"#;
    let (symbols, _, pending) = extract_with_relationships(code);
    let module = find(&symbols, "bank");

    let edge = pending_named(&pending, "gen_server");
    assert_eq!(edge.pending.kind, RelationshipKind::Implements);
    assert_eq!(edge.pending.from_symbol_id, module.id);
    assert_eq!(
        edge.caller_scope_symbol_id.as_deref(),
        Some(module.id.as_str())
    );
    assert_eq!(edge.pending.line_number, 2);
    assert_eq!(edge.target.display_name, "gen_server");
    assert_eq!(edge.target.terminal_name, "gen_server");
    assert!(edge.target.namespace_path.is_empty());
}

#[test]
fn include_and_include_lib_emit_pending_import_edges_with_path_structure() {
    let code = r#"-module(bank).
-include("bank_records.hrl").
-include_lib("stdlib/include/assert.hrl").
-export([entry/0]).

entry() -> ok.
"#;
    let (symbols, _, pending) = extract_with_relationships(code);
    let module = find(&symbols, "bank");

    let local = pending_named(&pending, "bank_records.hrl");
    assert_eq!(local.pending.kind, RelationshipKind::Imports);
    assert_eq!(local.pending.from_symbol_id, module.id);
    assert_eq!(local.target.display_name, "bank_records.hrl");
    assert!(local.target.namespace_path.is_empty());
    assert_eq!(local.target.import_context.as_deref(), Some("include"));

    let lib = pending_named(&pending, "assert.hrl");
    assert_eq!(lib.pending.kind, RelationshipKind::Imports);
    assert_eq!(lib.target.display_name, "stdlib/include/assert.hrl");
    assert_eq!(
        lib.target.namespace_path,
        vec!["stdlib".to_string(), "include".to_string()]
    );
    assert_eq!(lib.target.import_context.as_deref(), Some("include_lib"));
}

#[test]
fn import_attribute_emits_a_pending_module_import_edge() {
    let code = r#"-module(bank).
-import(lists, [reverse/1]).
-export([entry/0]).

entry() -> ok.
"#;
    let (symbols, _, pending) = extract_with_relationships(code);
    let module = find(&symbols, "bank");

    let edge = pending_named(&pending, "lists");
    assert_eq!(edge.pending.kind, RelationshipKind::Imports);
    assert_eq!(edge.pending.from_symbol_id, module.id);
    assert_eq!(edge.target.display_name, "lists");
    assert_eq!(edge.target.import_context.as_deref(), Some("import"));
}

#[test]
fn macro_body_calls_bind_to_the_macro_symbol() {
    let code = r#"-module(bank).
-define(LOG(Msg), io:format("~p~n", [Msg])).
-export([entry/0]).

entry() -> ok.
"#;
    let (symbols, _, pending) = extract_with_relationships(code);
    let macro_symbol = find(&symbols, "LOG");

    let edge = pending_named(&pending, "format");
    assert_eq!(edge.pending.from_symbol_id, macro_symbol.id);
    assert_eq!(edge.target.namespace_path, vec!["io".to_string()]);
}

#[test]
fn later_clauses_bind_call_edges_to_the_same_function_symbol() {
    let code = r#"-module(bank).
-export([entry/1]).

entry(0) ->
    helper(0);
entry(N) ->
    helper(N).

helper(X) -> X.
"#;
    let (symbols, relationships, _) = extract_with_relationships(code);
    let entry = find(&symbols, "entry");

    assert_eq!(relationships.len(), 2);
    assert!(
        relationships
            .iter()
            .all(|edge| edge.from_symbol_id == entry.id),
        "later clauses must bind to the first-clause symbol: {}",
        relationship_inventory(&relationships)
    );
}

#[test]
fn a_call_named_like_the_behaviour_does_not_resolve_to_the_behaviour_target() {
    let code = r#"-module(bank).
-behaviour(gen_server).
-export([entry/0]).

entry() ->
    gen_server:call(self(), ping).
"#;
    let (_, relationships, pending) = extract_with_relationships(code);

    assert!(
        relationships.is_empty(),
        "no same-file target exists: {}",
        relationship_inventory(&relationships)
    );
    let implements = pending_named(&pending, "gen_server");
    assert_eq!(implements.pending.kind, RelationshipKind::Implements);
    let call = pending_named(&pending, "call");
    assert_eq!(call.pending.kind, RelationshipKind::Calls);
    assert_eq!(call.target.namespace_path, vec!["gen_server".to_string()]);
}

#[test]
fn fun_references_emit_no_call_edges() {
    let code = r#"-module(bank).
-export([entry/0]).

entry() ->
    {fun helper/1, fun lists:reverse/1}.

helper(X) -> X.
"#;
    let (_, relationships, pending) = extract_with_relationships(code);

    assert!(
        relationships.is_empty(),
        "a fun reference names a value, not a call: {}",
        relationship_inventory(&relationships)
    );
    assert!(pending.is_empty(), "got {pending:#?}");
}

#[test]
fn header_attributes_emit_no_module_anchored_edges_but_macro_bodies_still_do() {
    let code = r#"-behaviour(gen_server).
-include("shared.hrl").
-import(lists, [reverse/1]).
-define(LOG(Msg), io:format("~p~n", [Msg])).
"#;
    let (_, _, pending) = extract_from_with_relationships("include/account.hrl", code);

    assert_eq!(
        pending.len(),
        1,
        "a header has no module symbol to anchor attribute edges on; got {}",
        pending_inventory(&pending)
    );
    assert_eq!(pending[0].target.display_name, "io:format");
}

#[test]
fn degraded_pending_view_mirrors_the_structured_rows() {
    let code = r#"-module(bank).
-behaviour(gen_server).
-export([entry/0]).

entry() ->
    ledger:record(1).
"#;
    let (_, _, structured) = extract_with_relationships(code);
    let degraded: Vec<_> = structured
        .iter()
        .map(|edge| edge.pending.clone())
        .collect();

    assert_eq!(degraded.len(), structured.len());
    assert!(
        degraded
            .iter()
            .all(|pending| !pending.callee_name.is_empty() && pending.line_number > 0)
    );
}
