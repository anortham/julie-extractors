use std::path::Path;

use crate::base::StructuralFact;

const GO_NET_HTTP_ROUTE_PATTERN_ID: &str = "go.net_http.route.v1";
const GIN_ROUTE_PATTERN_ID: &str = "gin.route.v1";
const ECHO_ROUTE_PATTERN_ID: &str = "echo.route.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn facts_with_pattern<'a>(
    results: &'a crate::ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
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

#[test]
fn go_net_http_routes_parse_method_prefixed_patterns() {
    let source = r#"
package main

import "net/http"

func routes() {
    http.HandleFunc("GET /users/{id}", show)
    mux := http.NewServeMux()
    mux.Handle("POST /files/{path...}", handler)
}
"#;
    let results = extract("server.go", source);
    let facts = facts_with_pattern(&results, GO_NET_HTTP_ROUTE_PATTERN_ID);
    assert_eq!(facts.len(), 2, "{facts:#?}");

    let users = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/users/{id}"))
        .expect("users route");
    assert_eq!(metadata_str(users, "framework"), Some("net/http"));
    assert_eq!(metadata_str(users, "verb"), Some("GET"));
    assert_eq!(
        metadata_str(users, "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(metadata_array(users, "dynamic_segments"), vec!["id"]);

    let files = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/files/{path...}"))
        .expect("files route");
    assert_eq!(metadata_str(files, "verb"), Some("POST"));
    assert_eq!(
        metadata_str(files, "normalized_route_template"),
        Some("/files/:path")
    );
}

#[test]
fn gin_and_echo_routes_resolve_same_file_groups() {
    let source = r#"
package main

import (
    "github.com/gin-gonic/gin"
    "github.com/labstack/echo/v4"
)

func routes() {
    r := gin.Default()
    api := r.Group("/api")
    api.GET("/users/:id", show)

    e := echo.New()
    v1 := e.Group("/v1")
    v1.POST("/items/:id", create)
}
"#;
    let results = extract("server.go", source);
    let gin_facts = facts_with_pattern(&results, GIN_ROUTE_PATTERN_ID);
    assert_eq!(gin_facts.len(), 1, "{gin_facts:#?}");
    assert_eq!(metadata_str(gin_facts[0], "verb"), Some("GET"));
    assert_eq!(
        metadata_str(gin_facts[0], "route_group_prefix"),
        Some("/api")
    );
    assert_eq!(
        metadata_str(gin_facts[0], "normalized_route_template"),
        Some("/api/users/:id")
    );

    let echo_facts = facts_with_pattern(&results, ECHO_ROUTE_PATTERN_ID);
    assert_eq!(echo_facts.len(), 1, "{echo_facts:#?}");
    assert_eq!(metadata_str(echo_facts[0], "verb"), Some("POST"));
    assert_eq!(
        metadata_str(echo_facts[0], "route_group_prefix"),
        Some("/v1")
    );
    assert_eq!(
        metadata_str(echo_facts[0], "normalized_route_template"),
        Some("/v1/items/:id")
    );
}
