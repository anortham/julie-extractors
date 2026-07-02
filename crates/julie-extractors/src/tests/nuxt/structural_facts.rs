use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical Nuxt extraction should succeed")
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
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .collect()
}

#[test]
fn nuxt_emits_static_route_reference_facts() {
    let source = r#"
<template>
  <nav>
    <NuxtLink to="/about">About</NuxtLink>
    <nuxt-link to="/contact">Contact</nuxt-link>
    <NuxtLink :to="{ name: 'posts-id', params: { id: post.id } }">Post</NuxtLink>
    <NuxtLink to="//cdn.example.com/file.pdf">CDN</NuxtLink>
    <NuxtLink to="/download.pdf" external>Download</NuxtLink>
    <NuxtLink to="https://nuxt.com">External</NuxtLink>
  </nav>
</template>
"#;

    let results = extract("app/components/Nav.vue", source);
    let references = facts_with_pattern(&results, "nuxt.route_reference.v1");

    assert_eq!(references.len(), 2);
    assert_eq!(
        references
            .iter()
            .filter_map(|fact| metadata_str(fact, "target_path"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/about", "/contact"])
    );

    let about_link = references
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/about"))
        .expect("expected static NuxtLink route reference");
    assert_eq!(
        metadata_str(about_link, "query_family"),
        Some("frontend_navigation")
    );
    assert_eq!(metadata_str(about_link, "framework"), Some("nuxt"));
    assert_eq!(metadata_str(about_link, "source_kind"), Some("nuxt_link"));
    assert_eq!(
        metadata_str(about_link, "route_source"),
        Some("string_literal")
    );
    assert_eq!(metadata_str(about_link, "attribute_name"), Some("to"));
    assert_eq!(metadata_str(about_link, "component_name"), Some("NuxtLink"));

    let contact_link = references
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/contact"))
        .expect("expected lowercase nuxt-link route reference");
    assert_eq!(
        metadata_str(contact_link, "component_name"),
        Some("nuxt-link")
    );
}

#[test]
fn nuxt_emits_file_route_facts() {
    let home = extract("app/pages/index.vue", "<template><h1>Home</h1></template>");
    let home_routes = facts_with_pattern(&home, "nuxt.file_route.v1");
    assert_eq!(home_routes.len(), 1);
    assert_eq!(metadata_str(home_routes[0], "route_path"), Some("/"));
    assert_eq!(metadata_str(home_routes[0], "framework"), Some("nuxt"));
    assert_eq!(metadata_str(home_routes[0], "router"), Some("pages"));
    assert_eq!(
        metadata_str(home_routes[0], "source_kind"),
        Some("nuxt_file_route")
    );
    assert_eq!(
        metadata_str(home_routes[0], "file_convention"),
        Some("page")
    );

    let blog = extract(
        "app/pages/(marketing)/blog/[slug].vue",
        "<template><h1>Post</h1></template>",
    );
    let blog_routes = facts_with_pattern(&blog, "nuxt.file_route.v1");
    assert_eq!(blog_routes.len(), 1);
    assert_eq!(
        metadata_str(blog_routes[0], "route_path"),
        Some("/blog/[slug]")
    );
    assert_eq!(
        metadata_str(blog_routes[0], "normalized_route_template"),
        Some("/blog/:slug")
    );
    assert_eq!(
        metadata_array(blog_routes[0], "dynamic_segments"),
        vec!["slug"]
    );
    assert_eq!(
        metadata_array(blog_routes[0], "route_group_segments"),
        vec!["marketing"]
    );

    let about = extract(
        "pages/about.ts",
        "export default defineNuxtComponent({ render: () => h('h1', 'About') });",
    );
    let about_routes = facts_with_pattern(&about, "nuxt.file_route.v1");
    assert_eq!(about_routes.len(), 1);
    assert_eq!(metadata_str(about_routes[0], "route_path"), Some("/about"));
    assert!(
        facts_with_pattern(&about, "nextjs.file_route.v1").is_empty(),
        "Nuxt page files must not emit Next.js file routes"
    );

    let server_api = extract(
        "server/api/status.ts",
        "export default defineEventHandler(() => ({}));",
    );
    assert!(facts_with_pattern(&server_api, "nuxt.file_route.v1").is_empty());

    let pages_api = extract(
        "pages/api/status.ts",
        "export async function handler(): Promise<Response> { return new Response('ok'); }",
    );
    assert!(facts_with_pattern(&pages_api, "nuxt.file_route.v1").is_empty());

    let named_view = extract(
        "app/pages/child@sidebar.vue",
        "<template><h1>Named view</h1></template>",
    );
    assert!(facts_with_pattern(&named_view, "nuxt.file_route.v1").is_empty());
}

#[test]
fn nuxt_emits_server_route_facts_with_method_suffix() {
    let user = extract(
        "server/api/users/[id].get.ts",
        "export default defineEventHandler((event) => ({ id: getRouterParam(event, 'id') }));",
    );
    let user_routes = facts_with_pattern(&user, "nuxt.server_route.v1");
    assert_eq!(user_routes.len(), 1);
    let fact = user_routes[0];
    assert_eq!(fact.node_kind, "file");
    assert_eq!(fact.capture_name, "server_route");
    assert_eq!(metadata_str(fact, "query_family"), Some("framework"));
    assert_eq!(metadata_str(fact, "framework"), Some("nuxt"));
    assert_eq!(metadata_str(fact, "router"), Some("server"));
    assert_eq!(metadata_str(fact, "source_kind"), Some("nuxt_server_route"));
    assert_eq!(metadata_str(fact, "route_path"), Some("/api/users/[id]"));
    assert_eq!(
        metadata_str(fact, "normalized_route_template"),
        Some("/api/users/:id")
    );
    assert_eq!(metadata_array(fact, "dynamic_segments"), vec!["id"]);
    assert_eq!(metadata_str(fact, "verb"), Some("GET"));
    assert_eq!(metadata_str(fact, "verb_source"), Some("attested"));

    // Method suffix alone is enough to emit, even without a defineEventHandler signal.
    let legacy = extract(
        "server/api/legacy.post.js",
        "export default (event) => ({ ok: true });",
    );
    let legacy_routes = facts_with_pattern(&legacy, "nuxt.server_route.v1");
    assert_eq!(legacy_routes.len(), 1);
    assert_eq!(
        metadata_str(legacy_routes[0], "route_path"),
        Some("/api/legacy")
    );
    assert_eq!(metadata_str(legacy_routes[0], "verb"), Some("POST"));
    assert_eq!(
        metadata_str(legacy_routes[0], "verb_source"),
        Some("attested")
    );

    // `index.<method>` maps to the directory route.
    let index = extract(
        "server/api/users/index.get.ts",
        "export default defineEventHandler(() => []);",
    );
    let index_routes = facts_with_pattern(&index, "nuxt.server_route.v1");
    assert_eq!(index_routes.len(), 1);
    assert_eq!(
        metadata_str(index_routes[0], "route_path"),
        Some("/api/users")
    );
    assert_eq!(metadata_str(index_routes[0], "verb"), Some("GET"));
}

#[test]
fn nuxt_server_routes_without_method_suffix_omit_verb() {
    let health = extract(
        "server/routes/health.ts",
        "export default defineEventHandler(() => ({ status: 'ok' }));",
    );
    let health_routes = facts_with_pattern(&health, "nuxt.server_route.v1");
    assert_eq!(health_routes.len(), 1);
    let fact = health_routes[0];
    assert_eq!(metadata_str(fact, "route_path"), Some("/health"));
    assert_eq!(metadata_str(fact, "router"), Some("server"));
    assert_eq!(metadata_str(fact, "source_kind"), Some("nuxt_server_route"));
    assert_eq!(
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("verb")),
        None,
        "handlers with no method suffix answer all verbs and must omit verb"
    );
    assert_eq!(
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("verb_source")),
        None
    );
    // `eventHandler` alias also counts as a handler signal.
    let alias = extract(
        "server/routes/ping.ts",
        "export default eventHandler(() => 'pong');",
    );
    assert_eq!(facts_with_pattern(&alias, "nuxt.server_route.v1").len(), 1);
}

#[test]
fn nuxt_server_routes_normalize_optional_and_catch_all_segments() {
    let optional = extract(
        "server/api/users/[[id]].ts",
        "export default defineEventHandler(() => ({}));",
    );
    let optional_routes = facts_with_pattern(&optional, "nuxt.server_route.v1");
    assert_eq!(optional_routes.len(), 1);
    assert_eq!(
        metadata_str(optional_routes[0], "route_path"),
        Some("/api/users/[[id]]")
    );
    assert_eq!(
        metadata_str(optional_routes[0], "normalized_route_template"),
        Some("/api/users/:id?")
    );
    assert_eq!(
        metadata_array(optional_routes[0], "dynamic_segments"),
        vec!["id?"]
    );

    let catch_all = extract(
        "server/api/[...slug].get.ts",
        "export default defineEventHandler(() => ({}));",
    );
    let catch_all_routes = facts_with_pattern(&catch_all, "nuxt.server_route.v1");
    assert_eq!(catch_all_routes.len(), 1);
    assert_eq!(
        metadata_str(catch_all_routes[0], "route_path"),
        Some("/api/[...slug]")
    );
    assert_eq!(
        metadata_str(catch_all_routes[0], "normalized_route_template"),
        Some("/api/:slug*")
    );
    assert_eq!(
        metadata_array(catch_all_routes[0], "dynamic_segments"),
        vec!["slug"]
    );
    assert_eq!(metadata_str(catch_all_routes[0], "verb"), Some("GET"));
}

#[test]
fn nuxt_server_route_requires_handler_signal_or_method_suffix() {
    // No defineEventHandler and no method suffix: documented residual miss, stays silent.
    let silent = extract("server/api/util.ts", "export const helper = () => 42;");
    assert!(
        facts_with_pattern(&silent, "nuxt.server_route.v1").is_empty(),
        "server files without a handler signal or method suffix must stay silent"
    );
}

#[test]
fn nuxt_non_server_files_emit_no_server_route_facts() {
    // server/middleware, server/plugins, server/utils are not routes.
    let middleware = extract(
        "server/middleware/log.ts",
        "export default defineEventHandler((event) => { console.log(event.path); });",
    );
    assert!(facts_with_pattern(&middleware, "nuxt.server_route.v1").is_empty());

    let plugin = extract(
        "server/plugins/setup.ts",
        "export default defineNitroPlugin(() => {});",
    );
    assert!(facts_with_pattern(&plugin, "nuxt.server_route.v1").is_empty());

    // Page files never emit server routes, and their file_route facts are unaffected.
    let page = extract("app/pages/index.vue", "<template><h1>Home</h1></template>");
    assert!(facts_with_pattern(&page, "nuxt.server_route.v1").is_empty());
    assert_eq!(facts_with_pattern(&page, "nuxt.file_route.v1").len(), 1);
}

#[test]
fn nuxt_file_routes_normalize_optional_and_partial_dynamic_segments() {
    let optional = extract(
        "pages/users/[[id]].vue",
        "<template><h1>User</h1></template>",
    );
    let optional_routes = facts_with_pattern(&optional, "nuxt.file_route.v1");
    assert_eq!(optional_routes.len(), 1);
    assert_eq!(
        metadata_str(optional_routes[0], "route_path"),
        Some("/users/[[id]]")
    );
    assert_eq!(
        metadata_str(optional_routes[0], "normalized_route_template"),
        Some("/users/:id?")
    );
    assert_eq!(
        metadata_array(optional_routes[0], "dynamic_segments"),
        vec!["id?"]
    );

    let partial = extract(
        "pages/users-[group]/[id].vue",
        "<template><h1>User</h1></template>",
    );
    let partial_routes = facts_with_pattern(&partial, "nuxt.file_route.v1");
    assert_eq!(partial_routes.len(), 1);
    assert_eq!(
        metadata_str(partial_routes[0], "route_path"),
        Some("/users-[group]/[id]")
    );
    assert_eq!(
        metadata_str(partial_routes[0], "normalized_route_template"),
        Some("/users-:group/:id")
    );
    assert_eq!(
        metadata_array(partial_routes[0], "dynamic_segments"),
        vec!["group", "id"]
    );
}
