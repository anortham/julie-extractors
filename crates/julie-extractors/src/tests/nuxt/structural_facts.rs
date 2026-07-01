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
        "export default defineComponent({ render: () => h('h1', 'About') });",
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
