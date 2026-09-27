use std::path::Path;

use julie_extractors::{ExtractionLevel, extract_canonical_for_language_at};

#[test]
fn pages_router_requires_framework_evidence_in_each_ecmascript_language() {
    for (language, extension) in [
        ("javascript", "js"),
        ("jsx", "jsx"),
        ("typescript", "ts"),
        ("tsx", "tsx"),
    ] {
        let path = format!("src/pages/Home.{extension}");
        for (source, expected_routes) in [
            ("export default function Home() { return null; }", 0),
            (
                "export const note = \"next/router getStaticProps\"; export default function Home() { return null; }",
                0,
            ),
            (
                "export function getStaticProps() { return { props: {} }; } export default function Home() { return null; }",
                1,
            ),
        ] {
            let results = extract_canonical_for_language_at(
                language,
                &path,
                source,
                Path::new("."),
                ExtractionLevel::Full,
            )
            .unwrap();
            let routes: Vec<_> = results
                .structural_facts
                .iter()
                .filter(|fact| fact.pattern_id == "nextjs.file_route.v1")
                .collect();
            assert_eq!(routes.len(), expected_routes, "{language}: {source}");
            if let Some(route) = routes.first() {
                assert_eq!(
                    route.metadata.as_ref().unwrap().get("route_path"),
                    Some(&serde_json::json!("/Home"))
                );
            }
        }
    }
}

#[test]
fn axum_preserves_colon_literals_without_claiming_dynamic_parameters() {
    let source = r#"use axum::{routing::get, Router};
fn routes() {
    Router::new()
        .without_v07_checks()
        .route("/:literal", get(handler))
        .route("/users/{id}", get(handler));
}
async fn handler() {}
"#;
    let results = extract_canonical_for_language_at(
        "rust",
        "routes.rs",
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap();
    let routes: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "axum.route.v1")
        .collect();
    assert_eq!(routes.len(), 2);
    let literal = routes
        .iter()
        .find(|fact| fact.metadata.as_ref().unwrap()["route_template"] == "/:literal")
        .unwrap();
    assert_eq!(
        literal.metadata.as_ref().unwrap()["normalized_route_template"],
        "/:literal"
    );
    assert!(
        !literal
            .metadata
            .as_ref()
            .unwrap()
            .contains_key("dynamic_segments")
    );
    let captured = routes
        .iter()
        .find(|fact| fact.metadata.as_ref().unwrap()["route_template"] == "/users/{id}")
        .unwrap();
    assert_eq!(
        captured.metadata.as_ref().unwrap()["dynamic_segments"],
        serde_json::json!(["id"])
    );
}
