use std::path::Path;

use crate::base::StructuralFact;

const EXPRESS_ROUTE_PATTERN_ID: &str = "express.route.v1";
const EXPRESS_ROUTER_MOUNT_PATTERN_ID: &str = "express.router_mount.v1";

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

fn binding_symbol_name<'a>(
    results: &'a crate::ExtractionResults,
    fact: &StructuralFact,
) -> Option<&'a str> {
    let id = fact.containing_symbol_id.as_deref()?;
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.as_str())
}

#[test]
fn express_routes_and_same_file_router_mounts_emit_boundary_facts() {
    let source = r#"
import express from "express";

export function registerRoutes() {
  const app = express();
  const usersRouter = express.Router();

  app.get("/health", health);
  app.all("/any", any);
  app.route("/chain/:id").get(read).post(write);
  app.use("/api", usersRouter);
  usersRouter.get("/users/:id", showUser);
}
"#;
    let results = extract("src/server.js", source);
    let routes = facts_with_pattern(&results, EXPRESS_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 5, "{routes:#?}");

    let health = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/health"))
        .expect("health route");
    assert_eq!(metadata_str(health, "framework"), Some("express"));
    assert_eq!(metadata_str(health, "api_style"), Some("call_routing"));
    assert_eq!(metadata_str(health, "verb"), Some("GET"));
    assert_eq!(metadata_str(health, "verb_source"), Some("attested"));
    assert_eq!(
        metadata_str(health, "normalized_route_template"),
        Some("/health")
    );
    assert_eq!(
        binding_symbol_name(&results, health),
        Some("registerRoutes")
    );

    let any = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/any"))
        .expect("all route");
    assert_eq!(metadata_str(any, "verb"), None);
    assert_eq!(metadata_str(any, "verb_source"), None);

    let chain_verbs = routes
        .iter()
        .filter(|fact| metadata_str(fact, "route_template") == Some("/chain/:id"))
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect::<Vec<_>>();
    assert_eq!(chain_verbs, vec!["GET", "POST"]);

    let mounted = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/users/:id"))
        .expect("mounted router route");
    assert_eq!(metadata_str(mounted, "route_group_prefix"), Some("/api"));
    assert_eq!(
        metadata_str(mounted, "effective_route_template"),
        Some("/api/users/:id")
    );
    assert_eq!(
        metadata_str(mounted, "normalized_route_template"),
        Some("/api/users/:id")
    );
    assert_eq!(metadata_array(mounted, "dynamic_segments"), vec!["id"]);

    let mounts = facts_with_pattern(&results, EXPRESS_ROUTER_MOUNT_PATTERN_ID);
    assert_eq!(mounts.len(), 1, "{mounts:#?}");
    assert_eq!(metadata_str(mounts[0], "mount_path"), Some("/api"));
    assert_eq!(
        metadata_str(mounts[0], "normalized_mount_path"),
        Some("/api")
    );
    assert_eq!(metadata_str(mounts[0], "mount_target"), Some("usersRouter"));
}

#[test]
fn express_settings_getter_calls_stay_silent() {
    let source = r#"
import express from "express";

const app = express();
const port = app.get("port");
const engine = app.get("view engine");
app.get("/real", handler);
"#;
    let results = extract("src/server.js", source);
    let routes = facts_with_pattern(&results, EXPRESS_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/real"));
}

#[test]
fn express_multi_line_route_chains_emit_per_verb_facts() {
    let source = r#"
import express from "express";

const app = express();
app.route("/users")
  .get(list)
  .post(create);
"#;
    let results = extract("src/server.js", source);
    let routes = facts_with_pattern(&results, EXPRESS_ROUTE_PATTERN_ID);
    let verbs = routes
        .iter()
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect::<Vec<_>>();
    assert_eq!(verbs, vec!["GET", "POST"], "{routes:#?}");
}

#[test]
fn express_named_router_imports_and_inline_require_receivers_emit_routes() {
    let esm = r#"
import { Router as R } from "express";

const router = R();
router.get("/users", handler);
"#;
    let esm_results = extract("src/router.js", esm);
    let esm_routes = facts_with_pattern(&esm_results, EXPRESS_ROUTE_PATTERN_ID);
    assert_eq!(esm_routes.len(), 1, "{esm_routes:#?}");
    assert_eq!(
        metadata_str(esm_routes[0], "route_template"),
        Some("/users")
    );

    let cjs_router = r#"
const router = require("express").Router();
router.post("/users", handler);
"#;
    let cjs_router_results = extract("src/router.js", cjs_router);
    let cjs_router_routes = facts_with_pattern(&cjs_router_results, EXPRESS_ROUTE_PATTERN_ID);
    assert_eq!(cjs_router_routes.len(), 1, "{cjs_router_routes:#?}");
    assert_eq!(
        metadata_str(cjs_router_routes[0], "route_template"),
        Some("/users")
    );
    assert_eq!(metadata_str(cjs_router_routes[0], "verb"), Some("POST"));

    let cjs_app = r#"
const app = require("express")();
app.get("/health", handler);
"#;
    let cjs_app_results = extract("src/app.js", cjs_app);
    let cjs_app_routes = facts_with_pattern(&cjs_app_results, EXPRESS_ROUTE_PATTERN_ID);
    assert_eq!(cjs_app_routes.len(), 1, "{cjs_app_routes:#?}");
    assert_eq!(
        metadata_str(cjs_app_routes[0], "route_template"),
        Some("/health")
    );
}

#[test]
fn express_regex_literals_do_not_mask_later_routes() {
    let source = r#"
const express = require("express");
const app = express();
const quote = /['"]/;
app.get("/health", handler);
"#;
    let results = extract("src/app.js", source);
    let routes = facts_with_pattern(&results, EXPRESS_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/health"));
}

#[test]
fn express_route_chain_all_emits_verbless_route() {
    let source = r#"
import express from "express";

const app = express();
app.route("/any").all(handler);
"#;
    let results = extract("src/server.js", source);
    let routes = facts_with_pattern(&results, EXPRESS_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/any"));
    assert_eq!(metadata_str(routes[0], "verb"), None);
    assert_eq!(metadata_str(routes[0], "verb_source"), None);
}

#[test]
fn express_multi_parameter_segments_record_each_dynamic_segment() {
    let source = r#"
import express from "express";

const app = express();
app.get("/flights/:from-:to", handler);
"#;
    let results = extract("src/server.js", source);
    let routes = facts_with_pattern(&results, EXPRESS_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(
        metadata_str(routes[0], "normalized_route_template"),
        Some("/flights/:from-:to")
    );
    assert_eq!(
        metadata_array(routes[0], "dynamic_segments"),
        vec!["from", "to"]
    );
}

#[test]
fn express_chain_scan_ignores_calls_inside_handler_bodies() {
    let source = r#"
import express from "express";

const app = express();
app.route("/x").get((req, res) => res.json(cache.get(key)));
"#;
    let results = extract("src/server.js", source);
    let routes = facts_with_pattern(&results, EXPRESS_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "verb"), Some("GET"));
}

#[test]
fn express_dynamic_or_untraceable_routes_stay_silent() {
    let source = r#"
import express from "express";

export function registerRoutes(router, path) {
  const app = express();
  router.get("/untraceable", handler);
  app.get(`/dynamic`, handler);
  app.post(path, handler);
  app.use(handler);
}
"#;
    let results = extract("src/server.ts", source);
    assert!(facts_with_pattern(&results, EXPRESS_ROUTE_PATTERN_ID).is_empty());
    assert!(facts_with_pattern(&results, EXPRESS_ROUTER_MOUNT_PATTERN_ID).is_empty());
}
