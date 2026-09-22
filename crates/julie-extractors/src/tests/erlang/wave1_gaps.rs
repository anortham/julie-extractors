use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("erlang extraction")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name && s.kind != SymbolKind::Variable)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn name_of(result: &ExtractionResults, id: &str) -> String {
    result
        .symbols
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn edges(result: &ExtractionResults) -> Vec<(String, String, u32)> {
    let mut edges: Vec<_> = result
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Calls)
        .map(|r| {
            (
                name_of(result, &r.from_symbol_id),
                name_of(result, &r.to_symbol_id),
                r.line_number,
            )
        })
        .collect();
    edges.sort();
    edges
}

fn edge(from: &str, to: &str, line: u32) -> (String, String, u32) {
    (from.to_string(), to.to_string(), line)
}

fn pending_targets(result: &ExtractionResults) -> Vec<(String, String)> {
    result
        .structured_pending_relationships
        .iter()
        .map(|p| {
            (
                name_of(result, &p.pending.from_symbol_id),
                p.target.display_name.clone(),
            )
        })
        .collect()
}

#[test]
fn comments_between_clauses_keep_the_clause_run() {
    let result = extract(
        "src/clause_comment.erl",
        "-module(clause_comment).\n-export([handle/2]).\n\n%% Handle a get request.\nhandle({get, Key}, State) ->\n    {lookup(Key), State};\n%% Handle a put request; validates first.\nhandle({put, Key, Value}, State) when is_binary(Key) ->\n    ok = validate(Value),\n    {ok, State};\n%% Anything else.\nhandle(_Other, State) ->\n    {error, State}.\n\nlookup(K) -> K.\nvalidate(V) -> V.\n",
    );
    let handle = symbol(&result, "handle");
    assert_eq!((handle.start_line, handle.end_line), (5, 13));
    let params: Vec<_> = result
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Variable && s.parent_id.as_ref() == Some(&handle.id))
        .map(|s| s.name.clone())
        .collect();
    assert!(params.contains(&"Value".to_string()), "{params:?}");
    assert!(params.contains(&"_Other".to_string()), "{params:?}");
    assert!(edges(&result).contains(&edge("handle", "validate", 9)));
}

#[test]
fn edoc_blocks_above_specs_and_paren_doc_attributes_document_functions() {
    let result = extract(
        "src/docs.erl",
        "-module(docs).\n-export([a/0, c/0, d/0, e/0, f/0]).\n\n%%--------------------------------------------------------------------\n%% @doc\n%% Starts the server.\n%% @end\n%%--------------------------------------------------------------------\n-spec a() -> ok.\na() -> ok.\n\n-doc(\"Parenthesised doc.\").\nc() -> ok.\n\n%% @doc EDoc directly above function.\nd() -> ok.\n\n%%%===================================================================\n%%% Internal functions\n%%%===================================================================\n\ne() -> ok.\n\n-doc(#{since => <<\"1.0\">>}).\n-doc(\"Doc with parens\").\nf() -> ok.\n",
    );
    let doc = |name: &str| symbol(&result, name).doc_comment.clone();
    assert_eq!(
        doc("a").as_deref(),
        Some("%% @doc\n%% Starts the server.\n%% @end")
    );
    assert_eq!(doc("c").as_deref(), Some("Parenthesised doc."));
    assert_eq!(
        doc("d").as_deref(),
        Some("%% @doc EDoc directly above function.")
    );
    assert_eq!(doc("e"), None);
    assert_eq!(doc("f").as_deref(), Some("Doc with parens"));
}

#[test]
fn moduledoc_outranks_a_license_header() {
    let result = extract(
        "src/licensed.erl",
        "%%\n%% %CopyrightBegin%\n%%\n%% SPDX-License-Identifier: Apache-2.0\n%%\n%% %CopyrightEnd%\n%%\n-module(licensed).\n-moduledoc(\"Utilities for working with ledgers.\").\n-export([f/0]).\nf() -> ok.\n",
    );
    assert_eq!(
        symbol(&result, "licensed").doc_comment.as_deref(),
        Some("Utilities for working with ledgers.")
    );
    let bare = extract(
        "src/bare.erl",
        "%% %CopyrightBegin%\n%% Copyright Example.\n%% %CopyrightEnd%\n-module(bare).\n",
    );
    assert_eq!(symbol(&bare, "bare").doc_comment, None);
}

#[test]
fn module_macro_and_self_qualified_calls_resolve_locally() {
    let result = extract(
        "src/module_macro.erl",
        "-module(module_macro).\n-export([loop/1, start/0]).\nstart() ->\n    spawn(fun() -> ?MODULE:loop(0) end),\n    Handler = fun ?MODULE:handle/2,\n    module_macro:loop(1),\n    Local = fun handle/2,\n    {Handler, Local}.\nloop(N) ->\n    receive\n        stop -> ok;\n        _ -> ?MODULE:loop(N + 1)\n    end.\nhandle(A, B) -> {A, B}.\n",
    );
    let edges = edges(&result);
    for expected in [
        edge("start", "loop", 4),
        edge("start", "loop", 6),
        edge("loop", "loop", 12),
    ] {
        assert!(
            edges.contains(&expected),
            "missing {expected:?} in {edges:?}"
        );
    }
    let references: Vec<_> = result
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::References)
        .map(|r| (name_of(&result, &r.to_symbol_id), r.line_number))
        .collect();
    assert_eq!(
        references,
        [("handle".to_string(), 5), ("handle".to_string(), 7)]
    );
    assert!(
        pending_targets(&result)
            .iter()
            .all(|(_, target)| !target.starts_with("module_macro:"))
    );
}

#[test]
fn macro_call_sites_emit_identifiers_and_edges_to_their_define() {
    let result = extract(
        "test/m_tests.erl",
        "-module(m_tests).\n-include_lib(\"eunit/include/eunit.hrl\").\n-define(is_ok(X), X =:= ok).\n-define(Double(X), X * 2).\nadd_test() ->\n    ?assertEqual(4, m:add(2, 2)),\n    ?assert(?is_ok(ok)),\n    ?Double(3).\n",
    );
    let calls: Vec<_> = result
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::Call)
        .map(|i| i.name.clone())
        .collect();
    for name in ["assertEqual", "assert", "is_ok", "Double", "add"] {
        assert!(
            calls.contains(&name.to_string()),
            "missing {name} in {calls:?}"
        );
    }
    let edges = edges(&result);
    assert!(edges.contains(&edge("add_test", "is_ok", 7)), "{edges:?}");
    assert!(edges.contains(&edge("add_test", "Double", 8)), "{edges:?}");
}

#[test]
fn common_test_cases_come_from_all_and_groups() {
    let result = extract(
        "test/store_SUITE.erl",
        "-module(store_SUITE).\n-compile([export_all, nowarn_export_all]).\nall() -> [put_get, {group, basic}].\ngroups() -> [{basic, [parallel], [delete_key, overwrite]}].\ngroup(basic) -> [{timetrap, {seconds, 10}}].\nput_get(Config) -> start_store(Config).\ndelete_key(_Config) -> ok.\noverwrite(_Config) -> ok.\nstart_store(Config) -> Config.\ninit_per_suite(Config) -> Config.\n",
    );
    let role = |name: &str| {
        symbol(&result, name)
            .metadata
            .as_ref()
            .and_then(|m| m.get("test_role"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    for case in ["put_get", "delete_key", "overwrite"] {
        assert_eq!(role(case).as_deref(), Some("test_case"), "{case}");
    }
    assert_eq!(role("group"), None);
    assert_eq!(role("start_store"), None);
    assert_eq!(role("init_per_suite").as_deref(), Some("fixture_setup"));
}

#[test]
fn recovered_declarations_keep_their_edges_and_bound_the_damaged_span() {
    let result = extract(
        "src/p.erl",
        "-module(p).\n-export([tail/0]).\n\nfirst() ->\n    try\n        risky()\n    catch\n        ?WITH_STACKTRACE(C, R, S)\n            io:format(\"~p\", [C]),\n            erlang:raise(C, R, S)\n    end.\n\ntail() ->\n    helper(),\n    lists:reverse([1, 2]).\n\nhelper() -> ok.\n",
    );
    let first = symbol(&result, "first");
    assert!(first.end_line < 13, "first spans to {}", first.end_line);
    assert!(edges(&result).contains(&edge("tail", "helper", 14)));
    assert!(
        pending_targets(&result).contains(&("tail".to_string(), "lists:reverse".to_string())),
        "{:?}",
        pending_targets(&result)
    );
    let tail = symbol(&result, "tail");
    assert!(
        result
            .identifiers
            .iter()
            .any(|i| i.name == "helper" && i.containing_symbol_id.as_ref() == Some(&tail.id))
    );
}

#[test]
fn remote_call_and_record_field_identifiers_carry_their_qualifier() {
    let result = extract(
        "src/fields.erl",
        "-module(fields).\ninit(Opts) -> maps:get(ttl, Opts, 60).\nf(#user{id = UserId} = U) -> O = #order{id = UserId, total = 0}, {O, U}.\n",
    );
    let receiver = |name: &str, line: u32, kind: IdentifierKind| {
        result
            .identifiers
            .iter()
            .filter(|i| i.name == name && i.start_line == line && i.kind == kind)
            .map(|i| {
                i.metadata
                    .as_ref()
                    .and_then(|m| m.get("receiver"))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(receiver("get", 2, IdentifierKind::Call), ["maps"]);
    assert_eq!(
        receiver("id", 3, IdentifierKind::MemberAccess),
        ["user", "order"]
    );
    assert_eq!(
        receiver("total", 3, IdentifierKind::MemberAccess),
        ["order"]
    );
}
