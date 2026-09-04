use std::path::Path;

use crate::base::StructuralFact;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

const GO_NET_HTTP_ROUTE_PATTERN_ID: &str = "go.net_http.route.v1";
const GIN_ROUTE_PATTERN_ID: &str = "gin.route.v1";
const ECHO_ROUTE_PATTERN_ID: &str = "echo.route.v1";

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
fn go_net_http_var_mux_receiver_emits_route_fact() {
    let source = r#"
package main

import "net/http"

func routes() {
    var mux = http.NewServeMux()
    mux.HandleFunc("GET /ready", ready)
}
"#;
    let results = extract("server.go", source);
    let facts = facts_with_pattern(&results, GO_NET_HTTP_ROUTE_PATTERN_ID);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "route_template"), Some("/ready"));
    assert_eq!(metadata_str(facts[0], "verb"), Some("GET"));
    assert_eq!(
        metadata_str(facts[0], "normalized_route_template"),
        Some("/ready")
    );
}

#[test]
fn go_net_http_exact_anchor_normalizes_to_root_without_dynamic_segment() {
    let source = r#"
package main

import "net/http"

func routes() {
    http.HandleFunc("/{$}", home)
}
"#;
    let results = extract("server.go", source);
    let facts = facts_with_pattern(&results, GO_NET_HTTP_ROUTE_PATTERN_ID);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "route_template"), Some("/{$}"));
    assert_eq!(
        metadata_str(facts[0], "normalized_route_template"),
        Some("/")
    );
    assert!(metadata_array(facts[0], "dynamic_segments").is_empty());
}

#[test]
fn go_net_http_scoped_exact_anchor_strips_dollar_segment() {
    let source = r#"
package main

import "net/http"

func routes() {
    http.HandleFunc("/items/{$}", items)
}
"#;
    let results = extract("server.go", source);
    let facts = facts_with_pattern(&results, GO_NET_HTTP_ROUTE_PATTERN_ID);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "route_template"), Some("/items/{$}"));
    assert_eq!(
        metadata_str(facts[0], "normalized_route_template"),
        Some("/items/")
    );
    assert!(metadata_array(facts[0], "dynamic_segments").is_empty());
}

#[test]
fn go_net_http_host_patterns_record_host_separately() {
    let source = r#"
package main

import "net/http"

func routes() {
    http.HandleFunc("GET example.com/users/{id}", show)
    http.HandleFunc("admin.example.com/", admin)
}
"#;
    let results = extract("server.go", source);
    let facts = facts_with_pattern(&results, GO_NET_HTTP_ROUTE_PATTERN_ID);
    assert_eq!(facts.len(), 2, "{facts:#?}");

    let users = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/users/{id}"))
        .expect("host-scoped users route");
    assert_eq!(metadata_str(users, "verb"), Some("GET"));
    assert_eq!(metadata_str(users, "host"), Some("example.com"));
    assert_eq!(
        metadata_str(users, "normalized_route_template"),
        Some("/users/:id")
    );

    let admin = facts
        .iter()
        .find(|fact| metadata_str(fact, "host") == Some("admin.example.com"))
        .expect("verbless host route");
    assert_eq!(metadata_str(admin, "verb"), None);
    assert_eq!(metadata_str(admin, "route_template"), Some("/"));
}

#[test]
fn gin_any_handle_and_nested_groups_emit_boundary_facts() {
    let source = r#"
package main

import "github.com/gin-gonic/gin"

func routes() {
    r := gin.Default()
    r.Any("/ping", handler)
    r.Handle("PUT", "/manual", handler)
    v1 := r.Group("/v1")
    users := v1.Group("/users")
    users.GET("/:id", show)
}
"#;
    let results = extract("server.go", source);
    let facts = facts_with_pattern(&results, GIN_ROUTE_PATTERN_ID);
    assert_eq!(facts.len(), 3, "{facts:#?}");

    let any = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/ping"))
        .expect("any route");
    assert_eq!(metadata_str(any, "verb"), None);
    assert_eq!(metadata_str(any, "verb_source"), None);

    let manual = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/manual"))
        .expect("handle route");
    assert_eq!(metadata_str(manual, "verb"), Some("PUT"));
    assert_eq!(metadata_str(manual, "verb_source"), Some("attested"));

    let nested = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/:id"))
        .expect("nested group route");
    assert_eq!(
        metadata_str(nested, "route_group_prefix"),
        Some("/v1/users")
    );
    assert_eq!(
        metadata_str(nested, "effective_route_template"),
        Some("/v1/users/:id")
    );
    assert_eq!(
        metadata_str(nested, "normalized_route_template"),
        Some("/v1/users/:id")
    );
}

#[test]
fn gin_rune_literals_do_not_mask_later_routes() {
    let source = r#"
package main

import "github.com/gin-gonic/gin"

func routes() {
    r := gin.Default()
    _ = '"'
    r.GET("/health", handler)
}
"#;
    let results = extract("server.go", source);
    let facts = facts_with_pattern(&results, GIN_ROUTE_PATTERN_ID);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "route_template"), Some("/health"));
}

#[test]
fn gin_conflicting_group_receiver_names_terminate_and_keep_routes_unprefixed() {
    let source = r#"
package main

import "github.com/gin-gonic/gin"

func a() {
    r := gin.Default()
    v := r.Group("/v1")
    v.GET("/a", handler)
}

func b() {
    r := gin.Default()
    v := r.Group("/v2")
    v.GET("/b", handler)
}
"#;
    let results = extract("server.go", source);
    let facts = facts_with_pattern(&results, GIN_ROUTE_PATTERN_ID);
    assert_eq!(facts.len(), 2, "{facts:#?}");
    assert!(
        facts
            .iter()
            .all(|fact| metadata_str(fact, "route_group_prefix").is_none()),
        "conflicting same-name local group receivers should be treated as ambiguous, not oscillate: {facts:#?}"
    );
    let templates = facts
        .iter()
        .filter_map(|fact| metadata_str(fact, "route_template"))
        .collect::<Vec<_>>();
    assert_eq!(templates, vec!["/a", "/b"]);
}

#[test]
fn gin_and_echo_routes_carry_call_routing_api_style() {
    let source = r#"
package main

import (
    "net/http"
    "github.com/gin-gonic/gin"
    "github.com/labstack/echo/v4"
)

func routes() {
    http.HandleFunc("/plain", handler)
    r := gin.Default()
    r.GET("/gin", handler)
    e := echo.New()
    e.GET("/echo", handler)
}
"#;
    let results = extract("server.go", source);
    let net_http = facts_with_pattern(&results, GO_NET_HTTP_ROUTE_PATTERN_ID);
    assert_eq!(metadata_str(net_http[0], "api_style"), Some("mux_routing"));
    let gin_facts = facts_with_pattern(&results, GIN_ROUTE_PATTERN_ID);
    assert_eq!(
        metadata_str(gin_facts[0], "api_style"),
        Some("call_routing")
    );
    let echo_facts = facts_with_pattern(&results, ECHO_ROUTE_PATTERN_ID);
    assert_eq!(
        metadata_str(echo_facts[0], "api_style"),
        Some("call_routing")
    );
}

#[test]
fn echo_any_and_other_major_versions_emit() {
    let source = r#"
package main

import "github.com/labstack/echo/v5"

func routes() {
    e := echo.New()
    e.Any("/any", handler)
    e.GET("/x", handler)
}
"#;
    let results = extract("server.go", source);
    let facts = facts_with_pattern(&results, ECHO_ROUTE_PATTERN_ID);
    assert_eq!(facts.len(), 2, "{facts:#?}");
    let any = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/any"))
        .expect("any route");
    assert_eq!(metadata_str(any, "verb"), None);
}

#[test]
fn gin_routes_on_longer_identifiers_stay_silent() {
    let source = r#"
package main

import "github.com/gin-gonic/gin"

func routes() {
    r := gin.Default()
    r.GET("/real", handler)
    apiRouter.GET("/not-traced", handler)
}
"#;
    let results = extract("server.go", source);
    let facts = facts_with_pattern(&results, GIN_ROUTE_PATTERN_ID);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "route_template"), Some("/real"));
}

#[test]
fn gin_non_literal_group_prefixes_poison_the_prefix_chain() {
    let source = r#"
package main

import "github.com/gin-gonic/gin"

func routes() {
    r := gin.Default()
    dynamic := r.Group(prefixFor("tenant"))
    dynamic.GET("/records/:id", show)
}
"#;
    let results = extract("server.go", source);
    let facts = facts_with_pattern(&results, GIN_ROUTE_PATTERN_ID);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(
        metadata_str(facts[0], "route_template"),
        Some("/records/:id")
    );
    assert_eq!(metadata_str(facts[0], "route_group_prefix"), None);
    assert_eq!(metadata_str(facts[0], "effective_route_template"), None);
    assert_eq!(
        metadata_str(facts[0], "normalized_route_template"),
        Some("/records/:id")
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
