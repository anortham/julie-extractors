use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.css", source, Path::new("/repo"))
        .expect("canonical CSS extraction should succeed")
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

fn metadata_u64(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_u64())
}

#[test]
fn css_emits_stylesheet_structural_facts() {
    let source = r#":root {
  --accent: #0f766e;
}

.button {
  color: var(--accent);
}

@media (min-width: 40rem) {
  .button {
    display: inline-flex;
  }
}

@keyframes spin {
  from { opacity: 0; }
  to { opacity: 1; }
}
"#;

    let results = extract(source);

    let selectors = facts_with_pattern(&results, "css.selector_rule.v1");
    assert_eq!(
        selectors
            .iter()
            .filter_map(|fact| metadata_str(fact, "selector"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([":root", ".button"])
    );
    assert!(
        selectors
            .iter()
            .any(|fact| metadata_str(fact, "selector_kind") == Some("class"))
    );
    assert!(
        selectors
            .iter()
            .all(|fact| metadata_str(fact, "query_family") == Some("stylesheet_structure"))
    );

    let custom_property = facts_with_pattern(&results, "css.custom_property.v1")
        .into_iter()
        .next()
        .expect("expected custom property fact");
    assert_eq!(custom_property.capture_name, "custom_property");
    assert_eq!(custom_property.node_kind, "property_name");
    assert_eq!(
        metadata_str(custom_property, "property_name"),
        Some("--accent")
    );

    let media = facts_with_pattern(&results, "css.media_query.v1")
        .into_iter()
        .next()
        .expect("expected media query fact");
    assert_eq!(media.capture_name, "media_query");
    assert_eq!(
        metadata_str(media, "query_family"),
        Some("responsive_design")
    );
    assert_eq!(metadata_str(media, "query"), Some("(min-width: 40rem)"));

    let keyframes = facts_with_pattern(&results, "css.keyframes.v1")
        .into_iter()
        .next()
        .expect("expected keyframes fact");
    assert_eq!(keyframes.capture_name, "keyframes");
    assert_eq!(metadata_str(keyframes, "query_family"), Some("animation"));
    assert_eq!(metadata_str(keyframes, "animation_name"), Some("spin"));

    let root_selector = selectors
        .iter()
        .find(|fact| metadata_str(fact, "selector") == Some(":root"))
        .expect("expected :root selector fact");
    assert_eq!(metadata_u64(root_selector, "declaration_count"), Some(1));
    assert!(
        results
            .structural_facts
            .iter()
            .all(|fact| fact.end_byte > fact.start_byte)
    );
}

#[test]
fn css_selector_kind_ignores_commas_inside_attribute_selectors() {
    let source = r#"
a[href*=","] {
  color: red;
}

.primary, .secondary {
  color: blue;
}
"#;

    let results = extract(source);
    let selectors = facts_with_pattern(&results, "css.selector_rule.v1");

    let attribute_selector = selectors
        .iter()
        .find(|fact| metadata_str(fact, "selector") == Some("a[href*=\",\"]"))
        .expect("expected attribute selector fact");
    assert_eq!(
        metadata_str(attribute_selector, "selector_kind"),
        Some("compound")
    );

    let selector_list = selectors
        .iter()
        .find(|fact| metadata_str(fact, "selector") == Some(".primary, .secondary"))
        .expect("expected selector list fact");
    assert_eq!(
        metadata_str(selector_list, "selector_kind"),
        Some("selector_list")
    );
}

#[test]
fn css_emits_additional_at_rule_structural_facts() {
    let source = r#"
@charset "UTF-8";
@namespace url(http://www.w3.org/1999/xhtml);
@supports (display: grid) {
  .grid { display: grid; }
}
@container (min-width: 20rem) {
  .card { color: red; }
}
@font-face {
  font-family: "Worker";
  src: url("/fonts/worker.woff2");
}
@layer utilities {
  .m-0 { margin: 0; }
}
"#;
    let results = extract(source);

    let charset = facts_with_pattern(&results, "css.charset.v1")
        .into_iter()
        .next()
        .expect("expected charset fact");
    assert_eq!(metadata_str(charset, "encoding"), Some("\"UTF-8\""));

    let namespace = facts_with_pattern(&results, "css.namespace.v1")
        .into_iter()
        .next()
        .expect("expected namespace fact");
    assert_eq!(
        metadata_str(namespace, "namespace"),
        Some("url(http://www.w3.org/1999/xhtml)")
    );

    let supports = facts_with_pattern(&results, "css.supports.v1")
        .into_iter()
        .next()
        .expect("expected supports fact");
    assert_eq!(metadata_str(supports, "condition"), Some("(display: grid)"));

    let container = facts_with_pattern(&results, "css.container.v1")
        .into_iter()
        .next()
        .expect("expected container fact");
    assert_eq!(
        metadata_str(container, "condition"),
        Some("(min-width: 20rem)")
    );

    let font_face = facts_with_pattern(&results, "css.font_face.v1")
        .into_iter()
        .next()
        .expect("expected font-face fact");
    assert_eq!(metadata_str(font_face, "at_rule"), Some("@font-face"));

    let layer = facts_with_pattern(&results, "css.layer.v1")
        .into_iter()
        .next()
        .expect("expected layer fact");
    assert_eq!(metadata_str(layer, "layer_name"), Some("utilities"));
}

#[test]
fn css_statement_form_layer_drops_trailing_semicolon() {
    let source = r#"
@layer reset;
"#;
    let results = extract(source);
    let layer = facts_with_pattern(&results, "css.layer.v1")
        .into_iter()
        .next()
        .expect("expected layer fact");
    assert_eq!(metadata_str(layer, "layer_name"), Some("reset"));
}
