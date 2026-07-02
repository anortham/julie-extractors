//! Focused emission tests for the `http.client_request.v1` structural-fact
//! family (Task 1: global `fetch()` calls in the JS/TS language family;
//! Task 2: import-gated axios calls and Vue SFC script-section coverage).

use std::path::Path;

use crate::base::StructuralFact;

const HTTP_CLIENT_REQUEST_PATTERN_ID: &str = "http.client_request.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn client_requests(results: &crate::ExtractionResults) -> Vec<&StructuralFact> {
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

fn single_request(results: &crate::ExtractionResults) -> &StructuralFact {
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
fn fetch_nested_method_property_does_not_set_http_verb() {
    let source = r#"
export async function load() {
  return fetch("/api/users", { headers: { method: "POST" } });
}
"#;
    let results = extract("src/load.js", source);
    let fact = single_request(&results);

    assert_eq!(metadata_str(fact, "target_path"), Some("/api/users"));
    assert_eq!(metadata_str(fact, "verb"), Some("GET"));
    assert_eq!(metadata_str(fact, "verb_source"), Some("default"));
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
fn axios_verb_method_emits_attested_with_generic_type_args() {
    let cases = [("src/api.ts", "typescript"), ("src/api.tsx", "tsx")];
    for (file_path, label) in cases {
        let source = r#"
import axios from "axios";

export async function getActiveMessages() {
  return await axios.get<Msg[]>("/api/messages/active");
}
"#;
        let results = extract(file_path, source);
        let fact = single_request(&results);

        assert_eq!(fact.language, label);
        assert_eq!(metadata_str(fact, "client"), Some("axios"));
        assert_eq!(metadata_str(fact, "import_source"), Some("axios"));
        assert_eq!(metadata_str(fact, "framework"), Some("axios"));
        assert_eq!(
            metadata_str(fact, "target_path"),
            Some("/api/messages/active")
        );
        assert_eq!(metadata_str(fact, "verb"), Some("GET"));
        assert_eq!(metadata_str(fact, "verb_source"), Some("attested"));
    }
}

#[test]
fn axios_verb_methods_cover_all_verbs() {
    let source = r#"
import axios from "axios";

export async function all() {
  await axios.get("/g");
  await axios.post("/p");
  await axios.put("/u");
  await axios.patch("/a");
  await axios.delete("/d");
  await axios.head("/h");
  await axios.options("/o");
}
"#;
    let results = extract("src/api.js", source);
    let facts = client_requests(&results);
    let verbs: Vec<_> = facts
        .iter()
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect();
    assert_eq!(
        verbs,
        ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"],
        "each axios verb method should emit its attested verb in source order"
    );
    assert!(
        facts
            .iter()
            .all(|fact| metadata_str(fact, "verb_source") == Some("attested"))
    );
}

#[test]
fn axios_direct_call_with_options_marks_attested_verb() {
    let source = r#"
import axios from "axios";

export async function save() {
  return axios("/api/users", { method: "post", data: payload });
}
"#;
    let results = extract("src/save.js", source);
    let fact = single_request(&results);

    assert_eq!(metadata_str(fact, "client"), Some("axios"));
    assert_eq!(metadata_str(fact, "verb"), Some("POST"));
    assert_eq!(metadata_str(fact, "verb_source"), Some("attested"));
}

#[test]
fn axios_direct_call_defaults_to_get() {
    let source = r#"
import axios from "axios";

export async function load() {
  return axios("/api/users");
}
"#;
    let results = extract("src/load.js", source);
    let fact = single_request(&results);

    assert_eq!(metadata_str(fact, "verb"), Some("GET"));
    assert_eq!(metadata_str(fact, "verb_source"), Some("default"));
}

#[test]
fn axios_non_static_method_emits_nothing() {
    let source = r#"
import axios from "axios";

export async function send(verb) {
  return axios("/api/users", { method: verb });
}
"#;
    let results = extract("src/send.js", source);
    assert!(
        client_requests(&results).is_empty(),
        "an attested-but-unreadable axios method must not degrade to GET"
    );
}

#[test]
fn axios_without_import_stays_silent() {
    let source = r#"
export async function load() {
  return axios.get("/api/users");
}
"#;
    let results = extract("src/load.js", source);
    assert!(
        client_requests(&results).is_empty(),
        "axios calls without an axios import must stay silent"
    );
}

#[test]
fn axios_comment_import_does_not_gate_client_requests() {
    let source = r#"
// import axios from "axios";
export async function load() {
  return axios.get("/api/users");
}
"#;
    let results = extract("src/load.js", source);
    assert!(
        client_requests(&results).is_empty(),
        "comment-only imports must not gate axios client-request facts"
    );
}

#[test]
fn axios_renamed_default_import_matches_local_name() {
    let source = r#"
import http from "axios";

export async function save() {
  await http.post("/api/users");
  await axios.get("/api/ignored");
}
"#;
    let results = extract("src/save.js", source);
    let fact = single_request(&results);

    assert_eq!(metadata_str(fact, "target_path"), Some("/api/users"));
    assert_eq!(metadata_str(fact, "verb"), Some("POST"));
    assert_eq!(metadata_str(fact, "import_source"), Some("axios"));
}

#[test]
fn axios_namespace_import_matches_local_name() {
    let source = r#"
import * as axios from "axios";

export async function load() {
  return axios.get("/api/users");
}
"#;
    let results = extract("src/load.js", source);
    let fact = single_request(&results);

    assert_eq!(metadata_str(fact, "verb"), Some("GET"));
    assert_eq!(metadata_str(fact, "verb_source"), Some("attested"));
}

#[test]
fn axios_dynamic_url_stays_silent() {
    let source = r#"
import axios from "axios";

export async function load(id) {
  await axios.get(`/api/users/${id}`);
  await axios.get(url);
}
"#;
    let results = extract("src/load.js", source);
    assert!(
        client_requests(&results).is_empty(),
        "dynamic axios URLs must stay silent"
    );
}

#[test]
fn vue_script_setup_fetch_and_axios_emit() {
    let source = r#"<template>
  <button @click="load">load</button>
</template>
<script setup lang="ts">
import axios from "axios";

async function load() {
  await fetch("/api/plain");
  await axios.get("/api/messages");
}
</script>
"#;
    let results = extract("src/Messages.vue", source);
    let facts = client_requests(&results);
    assert_eq!(
        facts.len(),
        2,
        "vue script setup should emit one fetch and one axios fact"
    );
    assert!(facts.iter().all(|fact| fact.language == "vue"));
    let clients: Vec<_> = facts
        .iter()
        .filter_map(|fact| metadata_str(fact, "client"))
        .collect();
    assert_eq!(clients, ["fetch", "axios"]);
}

#[test]
fn vue_template_content_stays_silent() {
    let source = r#"<template>
  <pre>fetch("/api/only-in-template")</pre>
</template>
<script>
export default {
  name: "Docs",
};
</script>
"#;
    let results = extract("src/Docs.vue", source);
    assert!(
        client_requests(&results).is_empty(),
        "template section content must not produce client-request facts"
    );
}

#[test]
fn vue_axios_import_gate_is_section_local() {
    let source = r#"<template>
  <div>x</div>
</template>
<script>
export default {
  methods: {
    load() {
      return axios.get("/api/users");
    },
  },
};
</script>
"#;
    let results = extract("src/NoImport.vue", source);
    assert!(
        client_requests(&results).is_empty(),
        "axios in a vue script without an axios import must stay silent"
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
