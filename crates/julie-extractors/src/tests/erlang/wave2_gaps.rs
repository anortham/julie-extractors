use crate::base::{
    ExtractionResults, IdentifierKind, RelationshipKind, SourceRegionKind, Symbol, SymbolKind,
    Visibility,
};
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

fn edges(result: &ExtractionResults, kind: RelationshipKind) -> Vec<(String, String)> {
    let mut rows: Vec<_> = result
        .relationships
        .iter()
        .filter(|r| r.kind == kind)
        .map(|r| {
            (
                name_of(result, &r.from_symbol_id),
                name_of(result, &r.to_symbol_id),
            )
        })
        .collect();
    rows.sort();
    rows
}

fn pending(result: &ExtractionResults) -> Vec<(String, String, Option<u32>)> {
    result
        .structured_pending_relationships
        .iter()
        .map(|p| {
            (
                name_of(result, &p.pending.from_symbol_id),
                p.target.display_name.clone(),
                p.target_arity,
            )
        })
        .collect()
}

fn idents(result: &ExtractionResults, kind: IdentifierKind) -> Vec<(String, String)> {
    result
        .identifiers
        .iter()
        .filter(|i| i.kind == kind)
        .map(|i| {
            (
                i.name.clone(),
                name_of(result, i.containing_symbol_id.as_deref().unwrap_or("")),
            )
        })
        .collect()
}

fn role(symbol: &Symbol) -> Option<&str> {
    symbol.metadata.as_ref()?.get("test_role")?.as_str()
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.to_string(), b.to_string())
}

#[test]
fn pending_calls_carry_the_call_arity() {
    let result = extract(
        "src/client.erl",
        "-module(client).\n-import(ledger, [flush/1]).\ngo() ->\n    ledger:record(a, b),\n    ledger:record(a, b, c),\n    flush(x).\n",
    );

    let calls: Vec<_> = pending(&result)
        .into_iter()
        .filter(|(from, _, _)| from == "go")
        .collect();
    assert_eq!(
        calls,
        [
            ("go".to_string(), "ledger:record".to_string(), Some(2)),
            ("go".to_string(), "ledger:record".to_string(), Some(3)),
            ("go".to_string(), "flush".to_string(), Some(1)),
        ]
    );
}

#[test]
fn test_only_conditional_blocks_leave_production_visibility_alone() {
    let result = extract(
        "src/ifdef_test.erl",
        "-module(ifdef_test).\n-export([api/0]).\n-ifdef(TEST).\n-compile([export_all, nowarn_export_all]).\n-include_lib(\"eunit/include/eunit.hrl\").\n-endif.\napi() -> internal().\ninternal() -> ok.\n-ifdef(TEST).\napi_test() -> ok = api().\n-endif.\n",
    );

    assert_eq!(
        symbol(&result, "internal").visibility,
        Some(Visibility::Private)
    );
    assert_eq!(role(symbol(&result, "ifdef_test")), None);
    assert_eq!(role(symbol(&result, "api_test")), Some("test_case"));
}

#[test]
fn declared_types_are_scoped_and_walked() {
    let result = extract(
        "src/rtypes.erl",
        "-module(rtypes).\n-record(state, {req :: cowboy_req:req(), opts = #{} :: opts(), started = erlang:monotonic_time()}).\n-type opts() :: #{timeout => timeout()}.\n-type conn() :: gen_tcp:socket().\n-spec f(conn()) -> #state{}.\nf(S) -> #state{req = S}.\n",
    );

    let types = idents(&result, IdentifierKind::TypeUsage);
    for expected in [
        pair("req", "state"),
        pair("cowboy_req", "state"),
        pair("opts", "state"),
        pair("gen_tcp", "conn"),
        pair("socket", "conn"),
        pair("conn", "f"),
        pair("state", "f"),
    ] {
        assert!(types.contains(&expected), "{expected:?} in {types:?}");
    }
    assert!(
        idents(&result, IdentifierKind::Call).contains(&pair("monotonic_time", "state")),
        "{:?}",
        result.identifiers
    );
    assert!(
        pending(&result).contains(&(
            "state".to_string(),
            "erlang:monotonic_time".to_string(),
            Some(0)
        )),
        "{:?}",
        pending(&result)
    );
}

#[test]
fn mfa_carriers_call_the_named_function() {
    let result = extract(
        "src/calls.erl",
        "-module(calls).\nstart() ->\n    Pid = spawn(?MODULE, loop, [0]),\n    erlang:apply(calls, run, [x]),\n    rpc:call(node(), remote_mod, remote_fun, [1]),\n    timer:apply_after(10, ?MODULE, loop, [1]),\n    Pid.\nloop(N) -> N.\nrun(X) -> X.\ninit(Opts) ->\n    Children = [#{id => w, start => {worker, start_link, [Opts]}},\n                {h, {http, start_link, []}, permanent, 5000, worker, [http]}],\n    {ok, {#{}, Children}}.\n",
    );

    assert_eq!(
        edges(&result, RelationshipKind::Calls),
        [
            pair("start", "loop"),
            pair("start", "loop"),
            pair("start", "run")
        ]
    );
    let targets = pending(&result);
    for expected in [
        ("start", "remote_mod:remote_fun", Some(1)),
        ("init", "worker:start_link", Some(1)),
        ("init", "http:start_link", Some(0)),
    ] {
        let expected = (expected.0.to_string(), expected.1.to_string(), expected.2);
        assert!(targets.contains(&expected), "{expected:?} in {targets:?}");
    }
    assert!(idents(&result, IdentifierKind::Call).contains(&pair("loop", "start")));
}

#[test]
fn nominal_types_and_native_records() {
    let result = extract(
        "src/nom.erl",
        "-module(nom).\n-export_type([meters/0]).\n-nominal meters() :: integer().\n-nominal feet() :: integer().\n-export_record([point]).\n-import_record(geo, [line]).\n-record(point, {x = 0 :: integer()}).\n-spec f(meters()) -> feet().\nf(P) -> #geo:line{from = P}.\n",
    );

    let meters = symbol(&result, "meters");
    assert_eq!(meters.kind, SymbolKind::Type);
    assert_eq!(meters.visibility, Some(Visibility::Public));
    assert_eq!(
        meters.metadata.as_ref().and_then(|m| m.get("nominal")),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
        symbol(&result, "feet").visibility,
        Some(Visibility::Private)
    );
    assert_eq!(
        symbol(&result, "point").visibility,
        Some(Visibility::Public)
    );
    assert_eq!(
        result
            .types
            .get(&symbol(&result, "meters").id)
            .map(|t| t.resolved_type.as_str()),
        Some("integer")
    );

    let types = idents(&result, IdentifierKind::TypeUsage);
    assert!(types.contains(&pair("line", "f")), "{types:?}");
    assert!(types.contains(&pair("geo", "f")), "{types:?}");
    assert!(idents(&result, IdentifierKind::MemberAccess).contains(&pair("from", "f")));
    assert!(
        pending(&result)
            .iter()
            .any(|(from, target, _)| from == "nom" && target == "geo")
    );
}

#[test]
fn proper_properties_and_eunit_fixtures_have_roles() {
    let proper = extract(
        "test/prop_lists.erl",
        "-module(prop_lists).\n-include_lib(\"proper/include/proper.hrl\").\nprop_reverse_twice() ->\n    ?FORALL(L, list(integer()), lists:reverse(lists:reverse(L)) =:= L).\nhelper(L) -> L.\n",
    );
    assert_eq!(role(symbol(&proper, "prop_lists")), Some("test_container"));
    assert_eq!(
        role(symbol(&proper, "prop_reverse_twice")),
        Some("test_case")
    );
    assert_eq!(role(symbol(&proper, "helper")), None);

    let eunit = extract(
        "test/kv_tests.erl",
        "-module(kv_tests).\n-include_lib(\"eunit/include/eunit.hrl\").\nkv_test_() ->\n    {setup, fun setup/0, fun cleanup/1,\n     [fun put_then_get/0]}.\nloop_test_() -> {foreach, local, fun setup/0, fun cleanup/1, [fun missing/0]}.\nsetup() -> ok.\ncleanup(_) -> ok.\nput_then_get() -> ok.\nmissing() -> ok.\n",
    );
    assert_eq!(role(symbol(&eunit, "setup")), Some("fixture_setup"));
    assert_eq!(role(symbol(&eunit, "cleanup")), Some("fixture_teardown"));
    assert_eq!(role(symbol(&eunit, "put_then_get")), Some("test_case"));
    assert_eq!(role(symbol(&eunit, "missing")), Some("test_case"));
}

#[test]
fn record_field_and_spec_parameter_types_are_facts() {
    let result = extract(
        "src/facts.erl",
        "-module(facts).\n-record(state, {req :: cowboy_req:req(), opts = #{} :: opts()}).\n-spec c(Req :: cowboy_req:req(), role()) -> ok.\nc(Req, Role) -> {Req, Role}.\n",
    );

    let typed = |name: &str| {
        result
            .symbols
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| result.types.get(&s.id))
            .map(|t| t.resolved_type.clone())
    };
    assert_eq!(typed("req").as_deref(), Some("cowboy_req:req"));
    assert_eq!(typed("opts").as_deref(), Some("opts"));
    assert_eq!(typed("Req").as_deref(), Some("cowboy_req:req"));
    assert_eq!(typed("Role").as_deref(), Some("role"));
}

#[test]
fn type_applications_record_their_arguments() {
    let result = extract(
        "src/targs.erl",
        "-module(targs).\n-type result(T) :: {ok, T} | {error, term()}.\n-type user() :: map().\n-spec f() -> result(user()).\nf() -> ok.\n",
    );

    let usages: Vec<_> = result
        .type_argument_usages
        .iter()
        .filter_map(|usage| {
            let generic = result
                .identifiers
                .iter()
                .find(|i| i.id == usage.identifier_id)?;
            Some((generic.name.clone(), usage.arguments[0].type_name.clone()))
        })
        .collect();
    assert_eq!(usages, [pair("result", "user")]);
}

#[test]
fn doc_macros_document_the_next_form() {
    let result = extract(
        "src/docs2.erl",
        "-module(docs2).\n?MODULEDOC(\"Module doc from macro.\").\n-export([f/0, h/0, k/0]).\n-doc(\"Doc with parens\").\nf() -> ok.\n?DOC(\"Macro doc\").\nh() -> ok.\n?DOC(false).\nk() -> ok.\n",
    );

    assert_eq!(
        symbol(&result, "docs2").doc_comment.as_deref(),
        Some("Module doc from macro.")
    );
    assert_eq!(
        symbol(&result, "f").doc_comment.as_deref(),
        Some("Doc with parens")
    );
    assert_eq!(
        symbol(&result, "h").doc_comment.as_deref(),
        Some("Macro doc")
    );
    assert_eq!(symbol(&result, "k").doc_comment, None);
    assert!(
        result
            .identifiers
            .iter()
            .all(|i| i.name != "DOC" && i.name != "MODULEDOC"),
        "{:?}",
        result.identifiers
    );
}

#[test]
fn header_declarations_and_exported_records_are_public() {
    let result = extract(
        "include/shared.hrl",
        "-type user_id() :: pos_integer().\n-record(user, {id :: user_id()}).\n-define(ADMIN_ROLE, admin).\n",
    );
    for name in ["user_id", "user", "id", "ADMIN_ROLE"] {
        assert_eq!(
            symbol(&result, name).visibility,
            Some(Visibility::Public),
            "{name}"
        );
    }

    let local = extract(
        "src/local.erl",
        "-module(local).\n-record(user, {id}).\n-define(ADMIN_ROLE, admin).\n",
    );
    assert_eq!(symbol(&local, "user").visibility, Some(Visibility::Private));
    assert_eq!(
        symbol(&local, "ADMIN_ROLE").visibility,
        Some(Visibility::Private)
    );
}

#[test]
fn body_comments_are_plain_comments_of_their_function() {
    let result = extract(
        "src/c.erl",
        "-module(c).\na(X) ->\n    %% TODO: handle negative numbers\n    X.\n\n%% @doc Returns ok.\nb() -> ok.\n",
    );

    let regions: Vec<_> = result
        .source_regions
        .iter()
        .map(|r| {
            (
                r.start_line,
                r.kind.clone(),
                name_of(&result, r.containing_symbol_id.as_deref().unwrap_or("")),
            )
        })
        .collect();
    assert_eq!(
        regions,
        [
            (3, SourceRegionKind::Comment, "a".to_string()),
            (6, SourceRegionKind::DocComment, "b".to_string()),
        ]
    );
}

#[test]
fn binary_string_arguments_are_literals() {
    let result = extract(
        "src/lits.erl",
        "-module(lits).\nf() -> hackney:request(post, <<\"https://api.example.com/v1/orders\">>, [], <<>>, []).\n",
    );

    let mut literals = result.literals.clone();
    crate::language_policy::classify_literals_by_carrier(&mut literals);
    let literals: Vec<_> = literals
        .iter()
        .map(|l| (l.literal_text.clone(), l.kind.clone()))
        .collect();
    assert_eq!(
        literals,
        [(
            "https://api.example.com/v1/orders".to_string(),
            crate::LiteralKind::Url
        )]
    );
}

#[test]
fn escript_files_are_erlang_and_emulator_args_are_not_docs() {
    let result = extract(
        "bin/tool.escript",
        "#!/usr/bin/env escript\n%%! -smp enable\nmain(Args) ->\n    io:format(\"~p~n\", [Args]).\n",
    );

    let main = symbol(&result, "main");
    assert_eq!(main.kind, SymbolKind::Function);
    assert_eq!(main.doc_comment, None);
    assert!(
        result
            .source_regions
            .iter()
            .all(|region| region.kind != SourceRegionKind::DocComment),
        "{:?}",
        result.source_regions
    );
}

fn facts(result: &ExtractionResults, pattern_id: &str, keys: &[&str]) -> Vec<Vec<String>> {
    result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .map(|fact| {
            let metadata = fact.metadata.as_ref().expect("fact metadata");
            keys.iter()
                .map(|key| {
                    metadata
                        .get(*key)
                        .map(|value| value.as_str().map_or(value.to_string(), String::from))
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect()
}

#[test]
fn cowboy_dispatch_tables_are_routes() {
    let result = extract(
        "src/kv_app.erl",
        "-module(kv_app).\nstart(_, _) ->\n    Dispatch = cowboy_router:compile([\n        {'_', [\n            {\"/\", kv_root_handler, []},\n            {\"/keys/:key\", [{key, nonempty}], kv_http, #{mode => single}},\n            {\"/static/[...]\", cowboy_static, {priv_dir, kv, \"static\"}}\n        ]},\n        {\"api.example.com\", [{<<\"/v1/items\">>, items_handler, []}]}\n    ]),\n    cowboy:start_clear(http, [{port, 8080}], #{env => #{dispatch => Dispatch}}).\n",
    );

    assert_eq!(
        facts(
            &result,
            "cowboy.route.v1",
            &[
                "route_template",
                "normalized_route_template",
                "dynamic_segments",
                "host",
                "handler_module"
            ]
        ),
        [
            ["/", "/", "", "_", "kv_root_handler"],
            ["/keys/:key", "/keys/:key", "[\"key\"]", "_", "kv_http"],
            ["/static/[...]", "/static/[...]", "", "_", "cowboy_static"],
            [
                "/v1/items",
                "/v1/items",
                "",
                "api.example.com",
                "items_handler"
            ],
        ]
    );
}

#[test]
fn httpc_hackney_and_gun_requests_are_client_facts() {
    let result = extract(
        "src/client.erl",
        "-module(client).\nf(Conn, Body) ->\n    httpc:request(\"https://api.example.com/v1/health\"),\n    httpc:request(post, {\"http://audit.internal/events\", [], \"application/json\", Body}, [], []),\n    hackney:request(delete, <<\"https://api.example.com/v1/orders/1\">>, [], <<>>, []),\n    hackney:get(<<\"https://api.example.com/v1/orders\">>),\n    gun:post(Conn, \"/v1/events\", [], Body),\n    httpc:request(get, {Url, []}, [], []).\n",
    );

    assert_eq!(
        facts(
            &result,
            "http.client_request.v1",
            &["client", "verb", "verb_source", "target_path", "url_kind"]
        ),
        [
            [
                "httpc",
                "GET",
                "default",
                "https://api.example.com/v1/health",
                "absolute"
            ],
            [
                "httpc",
                "POST",
                "attested",
                "http://audit.internal/events",
                "absolute"
            ],
            [
                "hackney",
                "DELETE",
                "attested",
                "https://api.example.com/v1/orders/1",
                "absolute"
            ],
            [
                "hackney",
                "GET",
                "attested",
                "https://api.example.com/v1/orders",
                "absolute"
            ],
            ["gun", "POST", "attested", "/v1/events", "path"],
        ]
    );
}

fn symbol_rows(result: &ExtractionResults) -> Vec<(String, SymbolKind, String)> {
    result
        .symbols
        .iter()
        .map(|s| {
            (
                s.name.clone(),
                s.kind.clone(),
                name_of(result, s.parent_id.as_deref().unwrap_or("")),
            )
        })
        .collect()
}

#[test]
fn term_config_files_route_to_erlang() {
    for path in ["src/kv.app.src", "rebar.config", "config/sys.config"] {
        assert_eq!(
            crate::detect_language_for_source(path, ""),
            Some("erlang"),
            "{path}"
        );
    }
    assert_eq!(crate::detect_language_for_source("web.config", ""), None);
    assert_eq!(crate::detect_language_for_source("src/.app.src", ""), None);
}

#[test]
fn application_resources_have_symbols_callbacks_and_dependencies() {
    let result = extract(
        "src/kv.app.src",
        "%% kv application\n{application, kv,\n [{description, \"KV store\"},\n  {mod, {kv_app, []}},\n  {applications, [kernel, stdlib, cowboy]},\n  {env, [{http_port, 8080}, {pool, [{size, 10}]}]}]}.\n",
    );

    assert_eq!(
        symbol_rows(&result),
        [
            ("kv".to_string(), SymbolKind::Module, String::new()),
            (
                "http_port".to_string(),
                SymbolKind::Property,
                "kv".to_string()
            ),
            ("pool".to_string(), SymbolKind::Property, "kv".to_string()),
            ("size".to_string(), SymbolKind::Property, "pool".to_string()),
        ]
    );
    let callbacks: Vec<_> = result
        .structured_pending_relationships
        .iter()
        .map(|p| {
            (
                name_of(&result, &p.pending.from_symbol_id),
                p.target.terminal_name.clone(),
                p.pending.kind.clone(),
            )
        })
        .collect();
    assert_eq!(
        callbacks,
        [(
            "kv".to_string(),
            "kv_app".to_string(),
            RelationshipKind::References
        )]
    );
    assert_eq!(
        facts(
            &result,
            "manifest.dependency.v1",
            &["ecosystem", "group", "name"]
        ),
        [
            ["otp", "applications", "kernel"],
            ["otp", "applications", "stdlib"],
            ["otp", "applications", "cowboy"],
        ]
    );
}

#[test]
fn rebar_config_declares_hex_dependencies() {
    let result = extract(
        "rebar.config",
        "{erl_opts, [debug_info]}.\n{deps, [{cowboy, \"2.12.0\"}, jsx, {gun, {git, \"https://github.com/ninenines/gun\", {tag, \"2.1.0\"}}}]}.\n{profiles, [{test, [{deps, [{meck, \"0.9.2\"}]}]}]}.\n",
    );

    assert_eq!(
        symbol_rows(&result)
            .into_iter()
            .map(|(name, kind, _)| (name, kind))
            .collect::<Vec<_>>(),
        [
            ("erl_opts".to_string(), SymbolKind::Property),
            ("deps".to_string(), SymbolKind::Property),
            ("profiles".to_string(), SymbolKind::Property),
        ]
    );
    assert_eq!(
        facts(
            &result,
            "manifest.dependency.v1",
            &["ecosystem", "group", "name", "version"]
        ),
        [
            ["hex", "deps", "cowboy", "2.12.0"],
            ["hex", "deps", "jsx", ""],
            ["hex", "deps", "gun", ""],
            ["hex", "profile:test", "meck", "0.9.2"],
        ]
    );
}

#[test]
fn sys_config_keys_nest_under_their_application() {
    let result = extract(
        "config/sys.config",
        "[{kv, [{http_port, 8080}, {db, [{host, \"localhost\"}]}]},\n {kernel, [{logger_level, info}]}].\n",
    );

    assert_eq!(
        symbol_rows(&result),
        [
            ("kv".to_string(), SymbolKind::Module, String::new()),
            (
                "http_port".to_string(),
                SymbolKind::Property,
                "kv".to_string()
            ),
            ("db".to_string(), SymbolKind::Property, "kv".to_string()),
            ("host".to_string(), SymbolKind::Property, "db".to_string()),
            ("kernel".to_string(), SymbolKind::Module, String::new()),
            (
                "logger_level".to_string(),
                SymbolKind::Property,
                "kernel".to_string()
            ),
        ]
    );
    assert!(result.identifiers.is_empty());
}
