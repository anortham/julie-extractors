//! actix-web structural facts (`actix.attribute_route.v1`, `actix.scope_route.v1`,
//! `actix.mount.v1`), plus the axum/actix no-double-emit coexistence guarantee.

use std::path::Path;

use crate::base::{StructuralFact, Symbol, SymbolKind};
use crate::tests::helpers::{facts_with_pattern, metadata_str};

const ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID: &str = "actix.attribute_route.v1";
const ACTIX_SCOPE_ROUTE_PATTERN_ID: &str = "actix.scope_route.v1";
const ACTIX_MOUNT_PATTERN_ID: &str = "actix.mount.v1";
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

// ---------------------------------------------------------------------------
// actix.attribute_route.v1
// ---------------------------------------------------------------------------

#[test]
fn actix_attribute_route_carries_verb_template_and_normalized_join_key() {
    let source = r#"use actix_web::{get, web, HttpResponse, Responder};

#[get("/users/{id}")]
async fn show(path: web::Path<u32>) -> impl Responder {
    HttpResponse::Ok().finish()
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");

    let show = routes[0];
    assert_eq!(metadata_str(show, "framework"), Some("actix"));
    assert_eq!(metadata_str(show, "api_style"), Some("attribute"));
    assert_eq!(metadata_str(show, "verb"), Some("GET"));
    assert_eq!(metadata_str(show, "verb_source"), Some("attested"));
    assert_eq!(metadata_str(show, "route_template"), Some("/users/{id}"));
    assert_eq!(
        metadata_str(show, "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(metadata_array(show, "dynamic_segments"), vec!["id"]);
    // Attribute registration is cross-file: no in-file prefix keys.
    assert_eq!(metadata_str(show, "route_group_prefix"), None);
    assert_eq!(metadata_str(show, "effective_route_template"), None);
}

#[test]
fn actix_attribute_route_binds_to_handler_function() {
    // LANE LEARNING: `#[get(...)]` is a *preceding sibling* of the handler
    // `function_item`, so the fact must anchor on the fn (not the attribute) for
    // its binding to resolve to the handler rather than the enclosing module.
    let source = r#"use actix_web::{post, HttpResponse, Responder};

#[post("/login")]
async fn login() -> impl Responder {
    HttpResponse::Ok().finish()
}
"#;
    let results = extract("src/main.rs", source);
    let login = function_symbol(&results, "login");
    let route = facts_with_pattern(&results, ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID)[0];
    assert_eq!(metadata_str(route, "verb"), Some("POST"));
    assert_eq!(
        route.containing_symbol_id.as_deref(),
        Some(login.id.as_str()),
        "attribute route must bind to the handler fn"
    );
}

#[test]
fn actix_route_macro_emits_one_fact_per_method() {
    // `#[route(path, method = "GET", method = "POST")]` registers both verbs.
    let source = r#"use actix_web::{route, HttpResponse, Responder};

#[route("/thing", method = "GET", method = "POST")]
async fn thing() -> impl Responder {
    HttpResponse::Ok().finish()
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 2, "one fact per method: {routes:#?}");

    let get = route_with_verb(&routes, "GET");
    assert_eq!(metadata_str(get, "api_style"), Some("attribute"));
    assert_eq!(metadata_str(get, "route_template"), Some("/thing"));
    let post = route_with_verb(&routes, "POST");
    assert_eq!(metadata_str(post, "route_template"), Some("/thing"));
}

#[test]
fn actix_attribute_route_binds_inside_impl_block() {
    let source = r#"use actix_web::{get, HttpResponse, Responder};

struct Api;

impl Api {
    #[get("/health")]
    async fn health(&self) -> impl Responder {
        HttpResponse::Ok().finish()
    }
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    let health = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "health")
        .expect("health method symbol");
    assert_eq!(metadata_str(routes[0], "verb"), Some("GET"));
    assert_eq!(
        routes[0].containing_symbol_id.as_deref(),
        Some(health.id.as_str()),
        "attribute route must bind to the handler method"
    );
}

#[test]
fn actix_attribute_route_dynamic_paths_stay_silent() {
    // A `#[get]` whose macro argument is not a plain string literal must stay
    // silent (M2). `const`-referenced macro args emit nothing.
    let source = r#"use actix_web::{get, HttpResponse, Responder};

const USERS: &str = "/users";

#[get(USERS)]
async fn list() -> impl Responder {
    HttpResponse::Ok().finish()
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID).is_empty(),
        "non-literal attribute path must stay silent: {:#?}",
        facts_with_pattern(&results, ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID)
    );
}

// ---------------------------------------------------------------------------
// actix.scope_route.v1
// ---------------------------------------------------------------------------

#[test]
fn actix_scope_route_joins_prefix_and_verb() {
    let source = r#"use actix_web::{web, App};

fn config() -> App<()> {
    App::new().service(web::scope("/api").route("/users/{id}", web::post().to(create)))
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");

    let route = routes[0];
    assert_eq!(metadata_str(route, "framework"), Some("actix"));
    assert_eq!(metadata_str(route, "api_style"), Some("call_routing"));
    assert_eq!(metadata_str(route, "verb"), Some("POST"));
    assert_eq!(metadata_str(route, "verb_source"), Some("attested"));
    assert_eq!(metadata_str(route, "route_template"), Some("/users/{id}"));
    assert_eq!(metadata_str(route, "route_group_prefix"), Some("/api"));
    assert_eq!(
        metadata_str(route, "effective_route_template"),
        Some("/api/users/{id}")
    );
    // The join key is computed from the effective (scope + route) template.
    assert_eq!(
        metadata_str(route, "normalized_route_template"),
        Some("/api/users/:id")
    );
    assert_eq!(metadata_array(route, "dynamic_segments"), vec!["id"]);
}

#[test]
fn actix_direct_app_route_emits_without_scope_prefix() {
    let source = r#"use actix_web::{web, App};

fn config() -> App<()> {
    App::new().route("/health", web::get().to(health))
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    let route = routes[0];
    assert_eq!(metadata_str(route, "route_template"), Some("/health"));
    assert_eq!(metadata_str(route, "verb"), Some("GET"));
    assert_eq!(metadata_str(route, "route_group_prefix"), None);
    assert_eq!(metadata_str(route, "effective_route_template"), None);
    assert_eq!(
        metadata_str(route, "normalized_route_template"),
        Some("/health")
    );
}

#[test]
fn actix_scope_route_method_agnostic_omits_verb() {
    // `web::route()` builds a method-agnostic route → verb omitted.
    let source = r#"use actix_web::{web, App};

fn config() -> App<()> {
    App::new().service(web::scope("/api").route("/health", web::route().to(health)))
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "verb"), None);
    assert_eq!(metadata_str(routes[0], "verb_source"), None);
    assert_eq!(metadata_str(routes[0], "route_group_prefix"), Some("/api"));
    assert_eq!(
        metadata_str(routes[0], "effective_route_template"),
        Some("/api/health")
    );
}

#[test]
fn actix_scope_route_chain_shares_prefix() {
    // Every `.route` in a scope chain carries the same scope prefix, resolved by
    // walking each `.route` receiver down to the base `web::scope`.
    let source = r#"use actix_web::{web, App};

fn config() -> App<()> {
    App::new().service(
        web::scope("/api")
            .route("/list", web::get().to(list))
            .route("/create", web::post().to(create)),
    )
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 2, "{routes:#?}");

    let get = route_with_verb(&routes, "GET");
    assert_eq!(metadata_str(get, "route_group_prefix"), Some("/api"));
    assert_eq!(
        metadata_str(get, "effective_route_template"),
        Some("/api/list")
    );
    let post = route_with_verb(&routes, "POST");
    assert_eq!(metadata_str(post, "route_group_prefix"), Some("/api"));
    assert_eq!(
        metadata_str(post, "effective_route_template"),
        Some("/api/create")
    );
}

#[test]
fn actix_scope_route_binds_to_enclosing_function() {
    let source = r#"use actix_web::{web, App};

fn build_app() -> App<()> {
    App::new().service(web::scope("/api").route("/x", web::get().to(handler)))
}
"#;
    let results = extract("src/main.rs", source);
    let build = function_symbol(&results, "build_app");
    let route = facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID)[0];
    assert_eq!(
        route.containing_symbol_id.as_deref(),
        Some(build.id.as_str()),
        "scope route must bind to the enclosing app-builder function"
    );
}

#[test]
fn actix_scope_route_non_static_prefix_stays_silent() {
    // A non-literal scope prefix makes the absolute path (and join key)
    // unknowable, so the route stays silent (M2) rather than emit a wrong key.
    let source = r#"use actix_web::{web, App};

fn config(prefix: String) -> App<()> {
    App::new().service(web::scope(&prefix).route("/x", web::get().to(handler)))
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID).is_empty(),
        "non-static scope prefix must stay silent: {:#?}",
        facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID)
    );
}

#[test]
fn actix_scope_route_dynamic_route_path_stays_silent() {
    let source = r#"use actix_web::{web, App};

fn config(id: u32) -> App<()> {
    App::new().service(
        web::scope("/api").route(format!("/u/{id}").as_str(), web::get().to(handler)),
    )
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID).is_empty(),
        "dynamic route path must stay silent: {:#?}",
        facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID)
    );
}

#[test]
fn actix_scope_route_method_guard_attests_verb() {
    // F3 regression (codex): `web::route().guard(guard::Get())` restricts the
    // route to GET, so the attested verb must be recorded. A verb-less fact would
    // let a non-GET client falsely join this GET-only handler.
    let source = r#"use actix_web::{guard, web, App};

fn config() -> App<()> {
    App::new().service(
        web::scope("/api").route("/x", web::route().guard(guard::Get()).to(handler)),
    )
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "verb"), Some("GET"));
    assert_eq!(metadata_str(routes[0], "verb_source"), Some("attested"));
    assert_eq!(metadata_str(routes[0], "route_group_prefix"), Some("/api"));
    assert_eq!(
        metadata_str(routes[0], "effective_route_template"),
        Some("/api/x")
    );
}

#[test]
fn actix_scope_route_qualified_method_guard_attests_verb() {
    // The qualified guard path `actix_web::guard::Post()` attests just as
    // `guard::Post()` does.
    let source = r#"use actix_web::{web, App};

fn config() -> App<()> {
    App::new().service(
        web::scope("/api").route("/y", web::route().guard(actix_web::guard::Post()).to(handler)),
    )
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "verb"), Some("POST"));
    assert_eq!(metadata_str(routes[0], "verb_source"), Some("attested"));
}

#[test]
fn actix_scope_route_non_method_guard_stays_verbless() {
    // A non-method guard (`guard::Header`) on `web::route()` does not attest a
    // verb, so the route stays genuinely method-agnostic (verb-less) — not a
    // guessed GET.
    let source = r#"use actix_web::{guard, web, App};

fn config() -> App<()> {
    App::new().service(
        web::scope("/api").route("/x", web::route().guard(guard::Header("x", "y")).to(handler)),
    )
}
"#;
    let results = extract("src/main.rs", source);
    let routes = facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "verb"), None);
    assert_eq!(metadata_str(routes[0], "verb_source"), None);
}

#[test]
fn actix_scope_route_unparsable_method_guard_stays_silent() {
    // `guard::Method(...)` restricts the method but its verb lives in an argument
    // we do not parse — emitting verb-less would wrongly claim any-method, so the
    // route stays silent (M2: a mis-attested join is worse than a miss).
    let source = r#"use actix_web::{guard, web, App};

fn config() -> App<()> {
    App::new().service(
        web::scope("/api").route("/x", web::route().guard(guard::Method(Method::GET)).to(handler)),
    )
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID).is_empty(),
        "unparsable method guard must stay silent: {:#?}",
        facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID)
    );
}

// ---------------------------------------------------------------------------
// actix.mount.v1
// ---------------------------------------------------------------------------

#[test]
fn actix_mount_records_scope_prefix_via_configure() {
    let source = r#"use actix_web::{web, App};

fn config() -> App<()> {
    App::new().service(web::scope("/api/{version}").configure(init))
}
"#;
    let results = extract("src/main.rs", source);
    let mounts = facts_with_pattern(&results, ACTIX_MOUNT_PATTERN_ID);
    assert_eq!(mounts.len(), 1, "{mounts:#?}");
    let mount = mounts[0];
    assert_eq!(metadata_str(mount, "framework"), Some("actix"));
    assert_eq!(metadata_str(mount, "mount_path"), Some("/api/{version}"));
    assert_eq!(
        metadata_str(mount, "normalized_mount_path"),
        Some("/api/:version")
    );
    assert_eq!(metadata_str(mount, "mount_target"), Some("init"));
}

#[test]
fn actix_mount_records_scope_prefix_via_service() {
    let source = r#"use actix_web::{web, App};

fn config() -> App<()> {
    App::new().service(web::scope("/admin").service(dashboard))
}
"#;
    let results = extract("src/main.rs", source);
    let mounts = facts_with_pattern(&results, ACTIX_MOUNT_PATTERN_ID);
    assert_eq!(mounts.len(), 1, "{mounts:#?}");
    assert_eq!(metadata_str(mounts[0], "mount_path"), Some("/admin"));
    assert_eq!(metadata_str(mounts[0], "mount_target"), Some("dashboard"));
}

#[test]
fn actix_mount_non_static_scope_stays_silent() {
    let source = r#"use actix_web::{web, App};

fn config(prefix: String) -> App<()> {
    App::new().service(web::scope(&prefix).configure(init))
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, ACTIX_MOUNT_PATTERN_ID).is_empty(),
        "non-static scope prefix must stay silent: {:#?}",
        facts_with_pattern(&results, ACTIX_MOUNT_PATTERN_ID)
    );
}

// ---------------------------------------------------------------------------
// import gate + axum/actix coexistence (no double-emit)
// ---------------------------------------------------------------------------

#[test]
fn actix_requires_import_gate() {
    // The attribute-macro and scope shapes without any actix_web import register
    // nothing (the macro could be any other crate's `#[get]`).
    let source = r#"#[get("/x")]
async fn show() {}

fn config() {
    web::scope("/api").route("/x", web::get().to(show));
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID).is_empty(),
        "no actix_web import → no attribute routes"
    );
    assert!(
        facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID).is_empty(),
        "no actix_web import → no scope routes"
    );
}

#[test]
fn actix_source_emits_no_axum_facts() {
    // A pure actix file (scope routes + attribute macros + mount) must not leak
    // any axum fact — the shared `rust` dispatch arm runs both collectors.
    let source = r#"use actix_web::{get, web, App, HttpResponse, Responder};

#[get("/ping")]
async fn ping() -> impl Responder {
    HttpResponse::Ok().finish()
}

fn config() -> App<()> {
    App::new().service(
        web::scope("/api")
            .route("/x", web::get().to(ping))
            .configure(init),
    )
}
"#;
    let results = extract("src/main.rs", source);
    assert!(
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID).is_empty(),
        "actix source must not emit axum.route.v1: {:#?}",
        facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID)
    );
    assert!(
        facts_with_pattern(&results, AXUM_NEST_PATTERN_ID).is_empty(),
        "actix source must not emit axum.nest.v1"
    );
    // ...while still emitting the actix facts.
    assert_eq!(
        facts_with_pattern(&results, ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID).len(),
        1
    );
    assert_eq!(
        facts_with_pattern(&results, ACTIX_SCOPE_ROUTE_PATTERN_ID).len(),
        1
    );
    assert_eq!(
        facts_with_pattern(&results, ACTIX_MOUNT_PATTERN_ID).len(),
        1
    );
}

#[test]
fn axum_source_emits_no_actix_facts() {
    // The mirror: a pure axum file must not leak any actix fact.
    let source = r#"use axum::{routing::get, Router};

fn app() -> Router {
    Router::new()
        .route("/users", get(list).post(create))
        .nest("/api", api_routes())
}
"#;
    let results = extract("src/main.rs", source);
    for pattern in [
        ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID,
        ACTIX_SCOPE_ROUTE_PATTERN_ID,
        ACTIX_MOUNT_PATTERN_ID,
    ] {
        assert!(
            facts_with_pattern(&results, pattern).is_empty(),
            "axum source must not emit {pattern}: {:#?}",
            facts_with_pattern(&results, pattern)
        );
    }
    // ...while still emitting axum facts (2 verbs + 1 nest).
    assert_eq!(facts_with_pattern(&results, AXUM_ROUTE_PATTERN_ID).len(), 2);
    assert_eq!(facts_with_pattern(&results, AXUM_NEST_PATTERN_ID).len(), 1);
}
