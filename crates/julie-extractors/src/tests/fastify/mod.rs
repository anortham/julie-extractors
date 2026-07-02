use std::path::Path;

use crate::base::StructuralFact;

const FASTIFY_ROUTE_PATTERN_ID: &str = "fastify.route.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn routes(results: &crate::ExtractionResults) -> Vec<&StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == FASTIFY_ROUTE_PATTERN_ID)
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
fn fastify_instance_and_route_object_forms_emit_boundary_facts() {
    let source = r#"
import fastify from "fastify";

export function registerRoutes() {
  const app = fastify();
  app.get("/users/:id", showUser);
  app.all("/any", any);
  app.route({ method: ["GET", "POST"], url: "/items/:id", handler });
}
"#;
    let results = extract("src/server.ts", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 4, "{facts:#?}");

    let user = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/users/:id"))
        .expect("user route");
    assert_eq!(metadata_str(user, "framework"), Some("fastify"));
    assert_eq!(metadata_str(user, "api_style"), Some("call_routing"));
    assert_eq!(metadata_str(user, "verb"), Some("GET"));
    assert_eq!(metadata_str(user, "verb_source"), Some("attested"));
    assert_eq!(
        metadata_str(user, "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(metadata_array(user, "dynamic_segments"), vec!["id"]);
    assert_eq!(binding_symbol_name(&results, user), Some("registerRoutes"));

    let any = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/any"))
        .expect("all route");
    assert_eq!(metadata_str(any, "verb"), None);

    let object_verbs = facts
        .iter()
        .filter(|fact| metadata_str(fact, "route_template") == Some("/items/:id"))
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect::<Vec<_>>();
    assert_eq!(object_verbs, vec!["GET", "POST"]);
}

#[test]
fn fastify_plugin_parameter_form_emits_without_import() {
    let source = r#"
export default async function routes(fastify) {
  fastify.post("/plugin/:id", handler);
}
"#;
    let results = extract("src/plugin.js", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "verb"), Some("POST"));
    assert_eq!(
        metadata_str(facts[0], "normalized_route_template"),
        Some("/plugin/:id")
    );
}

#[test]
fn module_exports_app_parameter_without_fastify_import_stays_silent() {
    let source = r#"
module.exports = function (app) {
  app.get("/health", handler);
};
"#;
    let results = extract("src/routes.js", source);
    let facts = routes(&results);
    assert!(facts.is_empty(), "{facts:#?}");
}

#[test]
fn fastify_plugin_app_parameter_with_import_emits() {
    let source = r#"
const fastify = require("fastify");

module.exports = async function routes(app) {
  app.get("/health", handler);
};
"#;
    let results = extract("src/routes.js", source);
    let facts = routes(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "verb"), Some("GET"));
}

#[test]
fn fastify_dynamic_or_untraceable_routes_stay_silent() {
    let source = r#"
import fastify from "fastify";

export function registerRoutes(app, path) {
  const server = fastify();
  app.get("/untraceable", handler);
  server.get(`/dynamic`, handler);
  server.post(path, handler);
  server.route({ method: ["GET"], url: path, handler });
}
"#;
    let results = extract("src/server.js", source);
    assert!(routes(&results).is_empty());
}
