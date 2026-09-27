use std::path::Path;

use julie_extractors::{ExtractionLevel, StructuralFact, extract_canonical_for_language_at};

const PHOENIX_SOURCE: &str =
    include_str!("../../../fixtures/extraction/elixir/phoenix_websocket/source.ex");
const TESLA_SOURCE: &str =
    include_str!("../../../fixtures/extraction/elixir/tesla_local_clients/source.ex");

fn extract(path: &str, source: &str) -> julie_extractors::ExtractionResults {
    extract_canonical_for_language_at(
        "elixir",
        path,
        source,
        Path::new("/repo"),
        ExtractionLevel::Full,
    )
    .expect("canonical Elixir extraction should succeed")
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata.as_ref()?.get(key)?.as_str()
}

fn facts<'a>(
    results: &'a julie_extractors::ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .collect()
}

#[test]
fn phoenix_socket_and_channel_facts_keep_native_metadata_and_declaration_spans() {
    let results = extract("phoenix_websocket/source.ex", PHOENIX_SOURCE);
    let sockets = facts(&results, "phoenix.socket.v1");
    let channels = facts(&results, "phoenix.channel.v1");

    assert_eq!(sockets.len(), 2);
    assert_eq!(channels.len(), 3);

    let socket = sockets[0];
    assert_eq!(socket.capture_name, "socket");
    assert_eq!(metadata_str(socket, "framework"), Some("phoenix"));
    assert_eq!(metadata_str(socket, "query_family"), Some("websocket"));
    assert_eq!(metadata_str(socket, "path"), Some("/socket"));
    assert_eq!(metadata_str(socket, "handler"), Some("MyAppWeb.UserSocket"));
    assert!(
        !socket
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.contains_key("verb"))
    );
    assert_eq!(
        &PHOENIX_SOURCE[socket.start_byte as usize..socket.end_byte as usize],
        "socket \"/socket\", MyAppWeb.UserSocket,\n    websocket: true,\n    longpoll: false"
    );

    let channel = channels
        .iter()
        .find(|fact| metadata_str(fact, "topic") == Some("room:*"))
        .expect("static Phoenix channel should be emitted");
    assert_eq!(channel.capture_name, "channel");
    assert_eq!(metadata_str(channel, "framework"), Some("phoenix"));
    assert_eq!(metadata_str(channel, "query_family"), Some("websocket"));
    assert_eq!(
        metadata_str(channel, "handler"),
        Some("MyAppWeb.RoomChannel")
    );
    assert!(
        !channel
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.contains_key("verb"))
    );
    assert_eq!(
        &PHOENIX_SOURCE[channel.start_byte as usize..channel.end_byte as usize],
        "channel \"room:*\", MyAppWeb.RoomChannel, assigns: %{role: :member}"
    );

    let nested_channel = channels
        .iter()
        .find(|fact| metadata_str(fact, "topic") == Some("nested:*"))
        .expect("nested Phoenix channel should be emitted");
    let owner = results
        .symbols
        .iter()
        .find(|symbol| Some(symbol.id.as_str()) == nested_channel.containing_symbol_id.as_deref())
        .expect("nested channel should belong to its containing module");
    assert_eq!(owner.name, "MyAppWeb.UserSocket.NestedChannel");

    let router_facts = facts(&results, "phoenix.route.v1");
    assert_eq!(router_facts.len(), 1);
    assert_eq!(
        metadata_str(router_facts[0], "route_template"),
        Some("/health")
    );
}

#[test]
fn phoenix_websocket_facts_require_static_values_and_module_context() {
    let results = extract("phoenix_websocket/source.ex", PHOENIX_SOURCE);
    let socket_paths = facts(&results, "phoenix.socket.v1")
        .into_iter()
        .filter_map(|fact| metadata_str(fact, "path"))
        .collect::<Vec<_>>();
    let channel_topics = facts(&results, "phoenix.channel.v1")
        .into_iter()
        .filter_map(|fact| metadata_str(fact, "topic"))
        .collect::<Vec<_>>();

    assert_eq!(socket_paths, ["/socket", "/aliased"]);
    assert_eq!(channel_topics, ["room:*", "nested:*", "imported:*"]);
    assert!(
        facts(&results, "phoenix.route.v1").iter().all(|fact| ![
            "socket",
            "channel",
            "pipe_through"
        ]
        .contains(&fact.capture_name.as_str()))
    );
}

#[test]
fn tesla_local_base_urls_follow_same_scope_rebindings_and_dynamic_boundaries() {
    let results = extract("tesla_local_clients/source.ex", TESLA_SOURCE);
    let requests = facts(&results, "http.client_request.v1");
    let target = |call: &str| {
        let fact = requests
            .iter()
            .copied()
            .find(|fact| TESLA_SOURCE[fact.start_byte as usize..fact.end_byte as usize] == *call)
            .unwrap_or_else(|| panic!("missing request fact for {call}"));
        assert_eq!(metadata_str(fact, "client"), Some("tesla"));
        metadata_str(fact, "target_path").expect("request target path")
    };

    assert_eq!(
        target("Tesla.get(client, \"/users\")"),
        "https://api.example/users"
    );
    assert_eq!(
        target("Tesla.get(client, \"https://override.example/health\")"),
        "https://override.example/health"
    );
    assert_eq!(
        target("Tesla.get(client, \"/rebound\")"),
        "https://replacement.example/rebound"
    );
    assert_eq!(
        target("Tesla.get(client, \"/dynamic-base\")"),
        "/dynamic-base"
    );
    assert_eq!(
        target("Tesla.get(client, \"/inside-branch\")"),
        "https://branch.example/inside-branch"
    );
    assert_eq!(
        target("Tesla.get(client, \"/after-branch\")"),
        "https://before-branch.example/after-branch"
    );
    assert_eq!(
        target("Tesla.get(client, \"/after-outer-if\")"),
        "/after-outer-if"
    );
    assert_eq!(
        target("Tesla.get(client, \"/unknown-factory\")"),
        "/unknown-factory"
    );
    assert_eq!(
        target("Tesla.get(client, \"/after-recovery\")"),
        "https://recovered.example/after-recovery"
    );
    assert_eq!(
        target("Tesla.get(client, \"/shadowed-parameter\")"),
        "/shadowed-parameter"
    );
    assert_eq!(
        target("Tesla.get(client, \"/shadowed-case\")"),
        "/shadowed-case"
    );
    assert_eq!(
        target("Tesla.get(client, \"/after-shadowing\")"),
        "https://recovered.example/after-shadowing"
    );
    assert_eq!(
        target("Tesla.get(client, \"/after-nested-module\")"),
        "https://before-nested-module.example/after-nested-module"
    );
    assert_eq!(target("Tesla.get(client, \"/parameter\")"), "/parameter");
    assert_eq!(target("get \"/module\""), "https://module.example/module");
    assert_eq!(
        target("get \"https://module-override.example/health\""),
        "https://module-override.example/health"
    );
    assert!(!requests.iter().any(|fact| {
        &TESLA_SOURCE[fact.start_byte as usize..fact.end_byte as usize]
            == "Tesla.get(client, dynamic_path)"
    }));
    assert!(!requests.iter().any(|fact| {
        &TESLA_SOURCE[fact.start_byte as usize..fact.end_byte as usize]
            == "get \"/nested-without-tesla-context\""
    }));
}

#[test]
fn tesla_pinned_case_pattern_preserves_the_existing_client_binding() {
    let results = extract("tesla_local_clients/source.ex", TESLA_SOURCE);
    let requests = facts(&results, "http.client_request.v1");
    let target = |call: &str| {
        let fact = requests
            .iter()
            .copied()
            .find(|fact| TESLA_SOURCE[fact.start_byte as usize..fact.end_byte as usize] == *call)
            .unwrap_or_else(|| panic!("missing request fact for {call}"));
        metadata_str(fact, "target_path").expect("request target path")
    };

    assert_eq!(
        target("Tesla.get(client, \"/pinned-case\")"),
        "https://pinned.example/pinned-case"
    );
    assert_eq!(
        target("Tesla.get(client, \"/after-pinned-case\")"),
        "https://pinned.example/after-pinned-case"
    );
}

#[test]
fn tesla_base_url_static_options_control_relative_and_absolute_joining() {
    let results = extract("tesla_local_clients/source.ex", TESLA_SOURCE);
    let requests = facts(&results, "http.client_request.v1");
    let target = |call: &str| {
        let fact = requests
            .iter()
            .copied()
            .find(|fact| TESLA_SOURCE[fact.start_byte as usize..fact.end_byte as usize] == *call)
            .unwrap_or_else(|| panic!("missing request fact for {call}"));
        metadata_str(fact, "target_path").expect("request target path")
    };

    assert_eq!(
        target("Tesla.get(client, \"relative\")"),
        "https://api.example/relative"
    );
    assert_eq!(target("Tesla.get(client, \"\")"), "https://api.example");
    assert_eq!(
        target("Tesla.get(client, \"keyword-relative\")"),
        "https://keyword.example/root/keyword-relative"
    );
    assert_eq!(
        target("Tesla.get(client, \"https://keyword-override.example/health\")"),
        "https://keyword-override.example/health"
    );
    assert_eq!(
        target("Tesla.get(client, \"insecure-relative\")"),
        "https://insecure.example/root/insecure-relative"
    );
    assert_eq!(
        target("Tesla.get(client, \"http://insecure-override.example/health\")"),
        "http://insecure-override.example/health"
    );
    assert_eq!(
        target("Tesla.get(client, \"strict-relative\")"),
        "https://strict.example/root/strict-relative"
    );
    assert_eq!(
        target("Tesla.get(client, \"http://strict-override.example/health\")"),
        "https://strict.example/root/http://strict-override.example/health"
    );
    assert_eq!(
        target("Tesla.get(client, \"/dynamic-policy\")"),
        "/dynamic-policy"
    );
    assert_eq!(
        target("Tesla.get(client, \"/dynamic-keyword-base\")"),
        "/dynamic-keyword-base"
    );
    assert_eq!(
        target("get \"module-relative\""),
        "https://module-strict.example/root/module-relative"
    );
    assert_eq!(
        target("get \"http://module-strict-override.example/health\""),
        "https://module-strict.example/root/http://module-strict-override.example/health"
    );
}
