//! Focused emission tests for the `http.client_request.v1` structural-fact
//! family (Task 1: global `fetch()` calls in the JS/TS language family).

use std::path::Path;

use crate::base::StructuralFact;

const HTTP_CLIENT_REQUEST_PATTERN_ID: &str = "http.client_request.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn client_requests<'a>(results: &'a crate::ExtractionResults) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == HTTP_CLIENT_REQUEST_PATTERN_ID)
        .collect()
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

fn single_request<'a>(results: &'a crate::ExtractionResults) -> &'a StructuralFact {
    let facts = client_requests(results);
    assert_eq!(
        facts.len(),
        1,
        "expected exactly one http.client_request.v1 fact, got {}",
        facts.len()
    );
    facts[0]
}

#[test]
fn fetch_bare_call_defaults_to_get() {
    let source = r#"
export async function load() {
  return fetch("/api/users");
}
"#;
    let results = extract("src/load.js", source);
    let fact = single_request(&results);

    assert_eq!(metadata_str(fact, "client"), Some("fetch"));
    assert_eq!(metadata_str(fact, "target_path"), Some("/api/users"));
    assert_eq!(metadata_str(fact, "url_kind"), Some("path"));
    assert_eq!(metadata_str(fact, "verb"), Some("GET"));
    assert_eq!(metadata_str(fact, "verb_source"), Some("default"));
    assert_eq!(metadata_str(fact, "query_family"), Some("web.http_client"));
    assert_eq!(metadata_str(fact, "framework"), Some("fetch"));
    // fetch is a global — no import_source key (that is Task 2's axios key).
    assert_eq!(metadata_str(fact, "import_source"), None);
}

#[test]
fn fetch_options_object_marks_attested_verb() {
    let source = r#"
export async function save() {
  return fetch("/api/users", { method: "POST", body: payload });
}
"#;
    let results = extract("src/save.js", source);
    let fact = single_request(&results);

    assert_eq!(metadata_str(fact, "target_path"), Some("/api/users"));
    assert_eq!(metadata_str(fact, "url_kind"), Some("path"));
    assert_eq!(metadata_str(fact, "verb"), Some("POST"));
    assert_eq!(metadata_str(fact, "verb_source"), Some("attested"));
}

#[test]
fn fetch_lowercase_method_literal_is_upper_cased() {
    let source = r#"
export async function patch() {
  return fetch("/api/users/1", { method: "patch" });
}
"#;
    let results = extract("src/patch.js", source);
    let fact = single_request(&results);

    assert_eq!(metadata_str(fact, "verb"), Some("PATCH"));
    assert_eq!(metadata_str(fact, "verb_source"), Some("attested"));
}

#[test]
fn fetch_absolute_url_is_classified_absolute() {
    let source = r#"
export async function load() {
  return fetch("https://api.example.com/data");
}
"#;
    let results = extract("src/load.js", source);
    let fact = single_request(&results);

    assert_eq!(
        metadata_str(fact, "target_path"),
        Some("https://api.example.com/data")
    );
    assert_eq!(metadata_str(fact, "url_kind"), Some("absolute"));
    assert_eq!(metadata_str(fact, "verb_source"), Some("default"));
}

#[test]
fn fetch_relative_url_is_classified_relative() {
    let source = r#"
export async function load() {
  return fetch("api/users");
}
"#;
    let results = extract("src/load.js", source);
    let fact = single_request(&results);

    assert_eq!(metadata_str(fact, "target_path"), Some("api/users"));
    assert_eq!(metadata_str(fact, "url_kind"), Some("relative"));
}

#[test]
fn fetch_identifier_argument_stays_silent() {
    let source = r#"
export async function load(url) {
  return fetch(url);
}
"#;
    let results = extract("src/load.js", source);
    assert!(
        client_requests(&results).is_empty(),
        "dynamic identifier URL must not emit a client request"
    );
}

#[test]
fn fetch_template_literal_stays_silent() {
    let source = r#"
export async function load(id) {
  await fetch(`/api/users/${id}`);
  await fetch(`/api/users`);
}
"#;
    let results = extract("src/load.js", source);
    assert!(
        client_requests(&results).is_empty(),
        "template-literal URLs (interpolated or not) must stay silent"
    );
}

#[test]
fn property_fetch_call_stays_silent() {
    let source = r#"
export async function load(client) {
  return client.fetch("/api/users");
}
"#;
    let results = extract("src/load.js", source);
    assert!(
        client_requests(&results).is_empty(),
        "obj.fetch(...) property calls must not emit"
    );
}

#[test]
fn fetch_inside_comment_or_string_stays_silent() {
    let source = r#"
export function docs() {
  // fetch("/api/commented");
  const sample = 'fetch("/api/in-string")';
  return sample;
}
"#;
    let results = extract("src/docs.js", source);
    assert!(
        client_requests(&results).is_empty(),
        "matches inside comments or string literals must stay silent"
    );
}

#[test]
fn fetch_non_static_method_emits_nothing() {
    let source = r#"
export async function send(verb) {
  return fetch("/api/users", { method: verb });
}
"#;
    let results = extract("src/send.js", source);
    assert!(
        client_requests(&results).is_empty(),
        "an attested-but-unreadable method must not degrade to GET; emit nothing"
    );
}

#[test]
fn fetch_concatenated_url_stays_silent() {
    let source = r#"
export async function load(suffix) {
  return fetch("/api/" + suffix);
}
"#;
    let results = extract("src/load.js", source);
    assert!(
        client_requests(&results).is_empty(),
        "a concatenated (non-plain) first argument must stay silent"
    );
}

#[test]
fn fetch_emits_across_all_js_family_languages() {
    let cases = [
        ("src/load.js", "javascript"),
        ("src/load.jsx", "jsx"),
        ("src/load.ts", "typescript"),
        ("src/load.tsx", "tsx"),
    ];
    for (file_path, label) in cases {
        let source = r#"
export async function load() {
  return fetch("/api/users");
}
"#;
        let results = extract(file_path, source);
        let facts = client_requests(&results);
        assert_eq!(
            facts.len(),
            1,
            "{label} ({file_path}) should emit exactly one http.client_request.v1 fact"
        );
        assert_eq!(facts[0].language, label);
        assert_eq!(metadata_str(facts[0], "verb"), Some("GET"));
        assert_eq!(metadata_str(facts[0], "verb_source"), Some("default"));
    }
}
