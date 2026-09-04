//! axum router structural facts (`axum.route.v1`, `axum.nest.v1`).

use std::path::Path;

use crate::base::{StructuralFact, Symbol, SymbolKind};
use crate::tests::helpers::{facts_with_pattern, metadata_str};

const AXUM_ROUTE_PATTERN_ID: &str = "axum.route.v1";
const AXUM_NEST_PATTERN_ID: &str = "axum.nest.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}



fn metadata_array<'a>(fact: &'a StructuralFact, key: &str) -> Vec<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_array())
        .map(|values| values.iter().filter_map(|value| value.as_str()).collect())
        .unwrap_or_default()
}

fn route_with_verb<'a>(facts: &[&'a StructuralFact], verb: &str) -> &'a StructuralFact {
    facts
        .iter()
        .copied()
        .find(|fact| metadata_str(fact, "verb") == Some(verb))
        .unwrap_or_else(|| panic!("route with verb {verb:?} not found in {facts:#?}"))
}

fn function_symbol<'a>(results: &'a crate::ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == SymbolKind::Function)
        .unwrap_or_else(|| panic!("function symbol {name:?} not found"))
}

#[test]
fn axum_route_carries_verb_template_and_normalized_join_key() {
    let source = r#"use axum::{routing::get, Router};

fn app() -> Router {
    Router::new().route("/users/{id}", get(show))
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");

    let show = routes[0];
    assert_eq!(metadata_str(show, "framework"), Some("axum"));
    assert_eq!(metadata_str(show, "api_style"), Some("call_routing"));
    assert_eq!(metadata_str(show, "verb"), Some("GET"));
    assert_eq!(metadata_str(show, "verb_source"), Some("attested"));
    assert_eq!(metadata_str(show, "route_template"), Some("/users/{id}"));
    // axum 0.8 `{id}` brace captures normalize to the shared `:id` join key.
    assert_eq!(
        metadata_str(show, "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(metadata_array(show, "dynamic_segments"), vec!["id"]);
    // axum routes carry no receiver-derived prefix.
    assert_eq!(metadata_str(show, "route_group_prefix"), None);
    assert_eq!(metadata_str(show, "effective_route_template"), None);
}

#[test]
fn axum_chained_method_router_emits_one_fact_per_verb() {
    let source = r#"use axum::{routing::get, Router};

fn app() -> Router {
    Router::new().route("/users", get(list).post(create))
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 2, "one fact per verb: {routes:#?}");

    let get = route_with_verb(&routes, "GET");
    assert_eq!(metadata_str(get, "route_template"), Some("/users"));
    assert_eq!(
        metadata_str(get, "normalized_route_template"),
        Some("/users")
    );
    let post = route_with_verb(&routes, "POST");
    assert_eq!(metadata_str(post, "route_template"), Some("/users"));
}

#[test]
fn axum_all_method_router_omits_verb() {
    // `any`/`any_service` accept every method → verb omitted (not verb-restricted).
    let source = r#"use axum::{routing::any, Router};

fn app() -> Router {
    Router::new().route("/health", any(health))
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "verb"), None);
    assert_eq!(metadata_str(routes[0], "verb_source"), None);
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/health"));
}

#[test]
fn axum_method_router_middleware_is_transparent() {
    // A `.layer(...)` on the method router applies middleware; it is not a verb
    // and must not suppress the route or add a bogus verb.
    let source = r#"use axum::{routing::get, Router};

fn app() -> Router {
    Router::new().route("/x", get(handler).layer(trace_layer()))
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "verb"), Some("GET"));
}

#[test]
fn axum_route_binds_to_enclosing_function() {
    // LANE LEARNING: the route fact anchors on the `.route` call so its binding
    // resolves to the enclosing function that builds the router.
    let source = r#"use axum::{routing::get, Router};

fn build_router() -> Router {
    Router::new().route("/health", get(health))
}
"#;
    let results = extract("src/main.rs", source);
    let build = function_symbol(&results, "build_router");
    let route = facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID)[0];
    assert_eq!(
        route.containing_symbol_id.as_deref(),
        Some(build.id.as_str()),
        "route must bind to the enclosing router-builder function"
    );
}

#[test]
fn axum_nest_emits_mount_family_fact_without_guessed_join() {
    let source = r#"use axum::{routing::get, Router};

fn app() -> Router {
    Router::new()
        .route("/", get(root))
        .nest("/api/{version}", api_routes())
}
"#;
    let results = extract("src/main.rs", source);
    let nests = facts_with_pattern(&results, AXUM_NEST_PATTERN_ID);
    assert_eq!(nests.len(), 1, "{nests:#?}");
    let nest = nests[0];
    assert_eq!(metadata_str(nest, "framework"), Some("axum"));
    assert_eq!(metadata_str(nest, "mount_path"), Some("/api/{version}"));
    assert_eq!(
        metadata_str(nest, "normalized_mount_path"),
        Some("/api/:version")
    );
    // The target is a cross-file expression recorded verbatim; no route join is
    // guessed (Miller's job, decision 0004).
    assert_eq!(metadata_str(nest, "mount_target"), Some("api_routes()"));

    // The `/` route still emits and carries no `/api` prefix.
    let routes = facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/"));
    assert_eq!(metadata_str(routes[0], "effective_route_template"), None);
}

#[test]
fn axum_route_traces_router_variable_receiver() {
    let source = r#"use axum::{routing::get, Router};

fn app() -> Router {
    let router = Router::new();
    router.route("/status", get(status))
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/status"));
}

#[test]
fn axum_parameter_receiver_still_emits() {
    // A `Router` passed in as a parameter is never assigned `Router::new()`
    // in-file (unknown, not poisoned), so its routes still emit — the common
    // `fn add(app: Router) -> Router` builder idiom.
    let source = r#"use axum::{routing::post, Router};

fn add_routes(app: Router) -> Router {
    app.route("/login", post(login))
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "verb"), Some("POST"));
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/login"));
}

#[test]
fn axum_poisoned_receiver_suppresses_routes_and_nests() {
    // `app` is assigned `Router::new()` then reassigned a conflicting non-router
    // value — the receiver can no longer be confirmed as an axum Router, so its
    // `.route`/`.nest` calls stay silent (M2 — Go poison model, design §4.3).
    let source = r#"use axum::{routing::get, Router};

fn app() {
    let app = Router::new();
    let app = build_something_else();
    app.route("/ghost", get(ghost));
    app.nest("/ghost-api", api_routes());
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID).is_empty(),
        "poisoned receiver routes must stay silent: {:#?}",
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID)
    );
    assert!(
        facts_with_pattern(&results, AXUM_NEST_PATTERN_ID).is_empty(),
        "poisoned receiver nests must stay silent"
    );
}

#[test]
fn axum_local_non_router_receiver_suppresses_routes_and_nests() {
    // F1 regression (codex): `registry` is assigned ONLY from a non-router call
    // (`build_registry()`), so the in-file assignment PROVES it is not an axum
    // `Router`. Its `.route`/`.nest` calls must stay silent — a locally-known
    // non-router receiver is suppressed like a poisoned one, unlike an absent
    // (parameter) receiver which stays permissive.
    let source = r#"use axum::{routing::get, Router};

fn app() {
    let registry = build_registry();
    registry.route("/health", get(health));
    registry.nest("/api", api_routes());
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID).is_empty(),
        "locally-known non-router receiver must not emit routes: {:#?}",
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID)
    );
    assert!(
        facts_with_pattern(&results, AXUM_NEST_PATTERN_ID).is_empty(),
        "locally-known non-router receiver must not emit nests"
    );
}

#[test]
fn axum_alias_of_local_non_router_receiver_stays_silent() {
    // F1 regression: an alias chain to a proven non-router
    // (`let r2 = registry;`) is also suppressed — the proof follows aliases.
    let source = r#"use axum::{routing::get, Router};

fn app() {
    let registry = build_registry();
    let r2 = registry;
    r2.route("/health", get(health));
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID).is_empty(),
        "alias of a locally-known non-router receiver must stay silent: {:#?}",
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID)
    );
}

#[test]
fn axum_dynamic_paths_stay_silent() {
    // `format!`, concatenation, and const/identifier paths must all emit nothing
    // (M2 silence via the shared Rust static guard).
    let source = r#"use axum::{routing::get, Router};

const USERS: &str = "/users";

fn app(id: u32) -> Router {
    Router::new()
        .route(format!("/u/{id}").as_str(), get(a))
        .route(&("/u/".to_owned() + "x"), get(b))
        .route(USERS, get(c))
        .nest(format!("/api/{id}").as_str(), api())
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID).is_empty(),
        "dynamic route args must stay silent: {:#?}",
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID)
    );
    assert!(
        facts_with_pattern(&results, AXUM_NEST_PATTERN_ID).is_empty(),
        "dynamic nest path must stay silent"
    );
}

#[test]
fn axum_actix_method_router_shape_stays_silent() {
    // actix's `.route("/x", web::get().to(h))` argument bottoms out at the
    // `scoped_identifier` `web::get`, not a bare-identifier verb, so the axum
    // collector emits nothing (Task 6 owns this shape).
    let source = r#"use axum::Router;

fn app() -> Router {
    Router::new().route("/legacy", web::get().to(legacy))
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID).is_empty(),
        "actix-shaped method router must not emit an axum route: {:#?}",
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID)
    );
}

#[test]
fn axum_requires_import_gate() {
    // The `.route("/x", get(h))` shape without any axum import registers nothing.
    let source = r#"fn app() -> Router {
    Router::new().route("/x", get(handler))
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID).is_empty(),
        "no axum import → no routes"
    );
}
