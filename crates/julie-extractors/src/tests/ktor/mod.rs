//! Ktor server routing structural facts (`ktor.route.v1`).
//!
//! Restricted lexical gate (design §4.6): bare verb identifier + trailing
//! lambda + static string_literal arg0, lexically inside `routing{}`/`route{}`.

use std::path::Path;

use crate::base::StructuralFact;

const KTOR_ROUTE_PATTERN_ID: &str = "ktor.route.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn routes(results: &crate::ExtractionResults) -> Vec<&StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == KTOR_ROUTE_PATTERN_ID)
        .collect()
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

fn metadata_array<'a>(fact: &'a StructuralFact, key: &str) -> Vec<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_array())
        .map(|values| values.iter().filter_map(|value| value.as_str()).collect())
        .unwrap_or_default()
}

fn find_route<'a>(facts: &[&'a StructuralFact], route_template: &str) -> &'a StructuralFact {
    facts
        .iter()
        .copied()
        .find(|fact| metadata_str(fact, "route_template") == Some(route_template))
        .unwrap_or_else(|| panic!("route_template {route_template:?} not found in {facts:#?}"))
}

#[test]
fn ktor_verb_calls_inside_routing_emit_route_facts() {
    let source = r#"
import io.ktor.server.routing.*
import io.ktor.server.application.*

fun Application.module() {
    routing {
        get("/users/{id}") {
            call.respondText("ok")
        }
        post("/users") {
            call.respondText("created")
        }
        delete("/users/{id}") {
            call.respondText("gone")
        }
    }
}
"#;
    let results = extract("src/Application.kt", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 3, "{facts:#?}");

    let get = find_route(&facts, "/users/{id}");
    assert_eq!(metadata_str(get, "framework"), Some("ktor"));
    assert_eq!(metadata_str(get, "api_style"), Some("call_routing"));
    assert_eq!(metadata_str(get, "verb"), Some("GET"));
    assert_eq!(metadata_str(get, "verb_source"), Some("attested"));
    assert_eq!(
        metadata_str(get, "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(metadata_array(get, "dynamic_segments"), vec!["id"]);

    let post = facts
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("POST"))
        .expect("post route");
    assert_eq!(metadata_str(post, "route_template"), Some("/users"));
    assert_eq!(
        metadata_str(post, "normalized_route_template"),
        Some("/users")
    );

    let delete = facts
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("DELETE"))
        .expect("delete route");
    assert_eq!(
        metadata_str(delete, "normalized_route_template"),
        Some("/users/:id")
    );
}

#[test]
fn ktor_nested_route_block_joins_prefix_with_verb_path() {
    let source = r#"
import io.ktor.server.routing.*

fun Application.module() {
    routing {
        route("/api") {
            get("/users/{id}") {
                call.respondText("ok")
            }
        }
    }
}
"#;
    let results = extract("src/Application.kt", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    let fact = facts[0];
    assert_eq!(metadata_str(fact, "route_template"), Some("/users/{id}"));
    assert_eq!(
        metadata_str(fact, "effective_route_template"),
        Some("/api/users/{id}")
    );
    assert_eq!(
        metadata_str(fact, "normalized_route_template"),
        Some("/api/users/:id")
    );
    assert_eq!(metadata_array(fact, "dynamic_segments"), vec!["id"]);
    assert_eq!(metadata_str(fact, "verb"), Some("GET"));
}

#[test]
fn ktor_multiple_nested_route_blocks_join_all_prefixes() {
    let source = r#"
import io.ktor.server.routing.*

fun Application.module() {
    routing {
        route("/api") {
            route("v1") {
                post("/users") {
                    call.respondText("created")
                }
            }
        }
    }
}
"#;
    let results = extract("src/Application.kt", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    let fact = facts[0];
    assert_eq!(metadata_str(fact, "route_template"), Some("/users"));
    assert_eq!(
        metadata_str(fact, "effective_route_template"),
        Some("/api/v1/users")
    );
    assert_eq!(
        metadata_str(fact, "normalized_route_template"),
        Some("/api/v1/users")
    );
    assert_eq!(metadata_str(fact, "verb"), Some("POST"));
}

#[test]
fn ktor_rejects_navigation_expression_callees() {
    let source = r#"
import io.ktor.server.routing.*
import io.ktor.client.request.*

fun Application.module() {
    routing {
        get("/ok") { call.respondText("ok") }
    }
    client.get("/elsewhere")
    map.get("/not-a-route")
}
"#;
    let results = extract("src/Application.kt", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "route_template"), Some("/ok"));
}

#[test]
fn ktor_verb_outside_routing_stays_silent() {
    let source = r#"
import io.ktor.server.routing.*

fun Application.module() {
    get("/orphan") {
        call.respondText("nope")
    }
}
"#;
    let results = extract("src/Application.kt", source);
    assert!(
        routes(&results).is_empty(),
        "verb calls outside routing{{}}/route{{}} must stay silent: {:#?}",
        routes(&results)
    );
}

#[test]
fn ktor_interpolated_path_stays_silent() {
    let source = r#"
import io.ktor.server.routing.*

fun Application.module() {
    routing {
        get("/users/$id") {
            call.respondText("nope")
        }
        get("/users/" + id) {
            call.respondText("nope")
        }
    }
}
"#;
    let results = extract("src/Application.kt", source);
    assert!(
        routes(&results).is_empty(),
        "interpolated/concat Ktor paths must stay silent: {:#?}",
        routes(&results)
    );
}

#[test]
fn ktor_requires_io_ktor_import() {
    let source = r#"
fun Application.module() {
    routing {
        get("/users") {
            call.respondText("ok")
        }
    }
}
"#;
    let results = extract("src/Application.kt", source);
    assert!(routes(&results).is_empty());
}

#[test]
fn ktor_client_only_import_with_local_routing_dsl_stays_silent() {
    let source = r#"
import io.ktor.client.request.get
import io.ktor.client.request.post
import io.ktor.http.HttpMethod

fun buildRoutes() {
    routing {
        get("/x") {
            handle()
        }
    }
}
"#;
    let results = extract("src/Client.kt", source);
    assert!(
        routes(&results).is_empty(),
        "client-only io.ktor imports must not activate server route facts: {:#?}",
        routes(&results)
    );
}

#[test]
fn ktor_1x_server_import_still_emits_routes() {
    let source = r#"
import io.ktor.routing.*
import io.ktor.application.*

fun Application.module() {
    routing {
        get("/legacy") {
            call.respondText("ok")
        }
    }
}
"#;
    let results = extract("src/Application.kt", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "route_template"), Some("/legacy"));
}

#[test]
fn ktor_names_in_comments_and_strings_do_not_activate_routes() {
    let source = r#"
// import io.ktor.server.routing.*
val frameworkHint = "io.ktor.server.routing"

fun Application.module() {
    routing {
        get("/users") {
            call.respondText("ok")
        }
    }
}
"#;
    let results = extract("src/Application.kt", source);
    assert!(routes(&results).is_empty(), "{:#?}", routes(&results));
}
