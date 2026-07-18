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

/// Resolve a fact's `containing_symbol_id` to the bound symbol's `(name, kind)`.
fn binding_symbol<'a>(
    results: &'a crate::ExtractionResults,
    fact: &StructuralFact,
) -> Option<(&'a str, &'a crate::base::SymbolKind)> {
    let id = fact.containing_symbol_id.as_deref()?;
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| (symbol.name.as_str(), &symbol.kind))
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
fn fetch_method_shorthand_emits_nothing() {
    let source = r#"
export async function send(method) {
  return fetch("/api/users", { method });
}
"#;
    let results = extract("src/send.js", source);
    assert!(
        client_requests(&results).is_empty(),
        "a shorthand method property is dynamic and must not degrade to GET"
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
fn vue_script_comments_and_strings_do_not_emit_client_requests() {
    let source = r#"<template>
  <button @click="load">load</button>
</template>
<script>
import axios from "axios";

// fetch("/api/commented-fetch");
// axios.get("/api/commented-axios");
const sample = 'fetch("/api/string-fetch")';
const docs = "axios.get('/api/string-axios')";

export async function load() {
  await fetch("/api/real-fetch");
  await axios.get("/api/real-axios");
}
</script>
"#;
    let results = extract("src/Comments.vue", source);
    let facts = client_requests(&results);
    assert_eq!(
        facts.len(),
        2,
        "only executable fetch and axios calls in vue scripts should emit"
    );
    let paths: Vec<_> = facts
        .iter()
        .filter_map(|fact| metadata_str(fact, "target_path"))
        .collect();
    assert_eq!(paths, ["/api/real-fetch", "/api/real-axios"]);
    let executable_start = source.find("export async function").unwrap() as u32;
    assert!(facts.iter().all(|fact| fact.start_byte > executable_start));
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

#[test]
fn fetch_assigned_to_const_binds_enclosing_function_not_variable() {
    // Repro (2026-07-02): `const res = await fetch(...)` bound the `res` variable
    // symbol (the narrowest byte-containing symbol) instead of the enclosing
    // function — useless for call-graph joining. The kind filter excludes the
    // `variable` candidate so the enclosing function wins.
    let source = r#"
export async function load() {
  const res = await fetch("/api/widgets");
  return res;
}
"#;
    let results = extract("src/load.js", source);
    let fact = single_request(&results);

    let (name, kind) =
        binding_symbol(&results, fact).expect("fetch fact must bind a containing symbol");
    assert_eq!(
        name, "load",
        "fetch assigned to a const must bind the enclosing function, not the `res` variable"
    );
    assert_eq!(kind, &crate::base::SymbolKind::Function);
}

#[test]
fn fetch_bare_call_binds_enclosing_function() {
    // Lock: a bare `await fetch(...)` (no assignment) already binds the enclosing
    // function; the kind-filter + line-fallback change must preserve this.
    let source = r#"
export async function load() {
  await fetch("/api/widgets");
}
"#;
    let results = extract("src/load.js", source);
    let fact = single_request(&results);

    let (name, kind) =
        binding_symbol(&results, fact).expect("fetch fact must bind a containing symbol");
    assert_eq!(name, "load");
    assert_eq!(kind, &crate::base::SymbolKind::Function);
}

#[test]
fn python_requests_and_httpx_imported_module_calls_emit_client_requests() {
    let source = r#"
import requests as req
import httpx

def load():
    req.get("https://api.example.com/users")
    httpx.post("/items")
    req.request("PATCH", "/users/1")
"#;
    let results = extract("src/client.py", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 3, "{facts:#?}");

    let requests_get = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("https://api.example.com/users"))
        .expect("requests get");
    assert_eq!(metadata_str(requests_get, "client"), Some("requests"));
    assert_eq!(
        metadata_str(requests_get, "import_source"),
        Some("requests")
    );
    assert_eq!(metadata_str(requests_get, "verb"), Some("GET"));
    assert_eq!(metadata_str(requests_get, "url_kind"), Some("absolute"));

    let httpx_post = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/items"))
        .expect("httpx post");
    assert_eq!(metadata_str(httpx_post, "client"), Some("httpx"));
    assert_eq!(metadata_str(httpx_post, "verb"), Some("POST"));
    assert_eq!(metadata_str(httpx_post, "url_kind"), Some("path"));

    let request_call = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/users/1"))
        .expect("request call");
    assert_eq!(metadata_str(request_call, "verb"), Some("PATCH"));
}

#[test]
fn python_requests_and_httpx_import_lines_tolerate_comments_and_commas() {
    let source = r#"
import requests, httpx  # shared client imports

def load():
    requests.get("/users")
    httpx.post("/items")
"#;
    let results = extract("src/client.py", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 2, "{facts:#?}");
    assert!(
        facts
            .iter()
            .any(|fact| metadata_str(fact, "client") == Some("requests"))
    );
    assert!(
        facts
            .iter()
            .any(|fact| metadata_str(fact, "client") == Some("httpx"))
    );
}

#[test]
fn python_unimported_or_instance_client_calls_stay_silent() {
    let source = r#"
def load(session, path):
    requests.get("/unimported")
    session.get("/session")
    httpx.get(path)
"#;
    let results = extract("src/client.py", source);
    assert!(client_requests(&results).is_empty());
}

#[test]
fn csharp_httpclient_methods_and_request_messages_emit_client_requests() {
    let source = r#"
using System.Net.Http;
using System.Net.Http.Json;

public class Api {
    public async Task Load(HttpClient client) {
        await client.GetFromJsonAsync<User>("/api/users/1");
        await client.PostAsJsonAsync("https://api.example.com/items", payload);
        var req = new HttpRequestMessage(HttpMethod.Patch, @"/api/users/1");
    }
}
"#;
    let results = extract("src/Api.cs", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 3, "{facts:#?}");

    let get = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/api/users/1"))
        .expect("get request");
    assert_eq!(metadata_str(get, "client"), Some("httpclient"));
    assert_eq!(metadata_str(get, "verb"), Some("GET"));
    assert_eq!(metadata_str(get, "verb_source"), Some("attested"));
    assert_eq!(metadata_str(get, "url_kind"), Some("path"));

    let post = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("https://api.example.com/items"))
        .expect("post request");
    assert_eq!(metadata_str(post, "verb"), Some("POST"));
    assert_eq!(metadata_str(post, "url_kind"), Some("absolute"));

    let patch = facts
        .iter()
        .filter(|fact| metadata_str(fact, "target_path") == Some("/api/users/1"))
        .find(|fact| metadata_str(fact, "verb") == Some("PATCH"))
        .expect("request message patch");
    assert_eq!(metadata_str(patch, "client"), Some("httpclient"));
}

#[test]
fn csharp_raw_and_at_dollar_verbatim_strings_do_not_drop_later_client_requests() {
    let raw = r#"
using System.Net.Http;

public class Api {
    public async Task Load(HttpClient client) {
        await client.GetAsync("""/api/raw""");
    }
}
"#;
    let raw_results = extract("src/Api.cs", raw);
    let raw_fact = single_request(&raw_results);
    assert_eq!(metadata_str(raw_fact, "target_path"), Some("/api/raw"));
    assert_eq!(metadata_str(raw_fact, "verb"), Some("GET"));

    let at_dollar = r#"
using System.Net.Http;

public class Api {
    public async Task Load(HttpClient client, string root) {
        var path = @$"{root}\bin\";
        await client.GetAsync("/api/after");
    }
}
"#;
    let at_dollar_results = extract("src/Api.cs", at_dollar);
    let at_dollar_fact = single_request(&at_dollar_results);
    assert_eq!(
        metadata_str(at_dollar_fact, "target_path"),
        Some("/api/after")
    );
}

#[test]
fn csharp_httpclient_non_url_or_interpolated_literals_stay_silent() {
    let source = r#"
public class Api {
    public async Task Load(HttpClient client, string id) {
        await cache.GetAsync("user-key");
        await client.GetAsync($"https://api.example.com/{id}");
        await client.PostAsync("relative/path", body);
    }
}
"#;
    let results = extract("src/Api.cs", source);
    assert!(client_requests(&results).is_empty());
}

#[test]
fn csharp_httpclient_methods_require_proven_httpclient_receiver() {
    let source = r#"
using System.Net.Http;

public class Api {
    public async Task Load(HttpClient client, Cache cache) {
        await cache.GetAsync("https://cache.example.com/users");
        await client.GetAsync("https://api.example.com/users");

        var local = new HttpClient();
        await local.PostAsync("/items", body);
    }
}
"#;
    let results = extract("src/Api.cs", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 2, "{facts:#?}");
    assert!(
        facts.iter().all(
            |fact| metadata_str(fact, "target_path") != Some("https://cache.example.com/users")
        ),
        "{facts:#?}"
    );
    assert!(
        facts
            .iter()
            .any(|fact| metadata_str(fact, "target_path") == Some("https://api.example.com/users")),
        "{facts:#?}"
    );
    assert!(
        facts
            .iter()
            .any(|fact| metadata_str(fact, "target_path") == Some("/items")),
        "{facts:#?}"
    );
}

#[test]
fn java_http_request_builder_chains_emit_client_requests() {
    let source = r#"
import java.net.URI;
import java.net.http.HttpRequest;

class Api {
    void load() {
        HttpRequest get = HttpRequest.newBuilder(URI.create("https://api.example.com/users")).build();
        HttpRequest post = HttpRequest.newBuilder().uri(URI.create("/items")).POST(body).build();
        HttpRequest custom = HttpRequest.newBuilder().uri(URI.create("/users/1")).method("PATCH", body).build();
    }
}
"#;
    let results = extract("src/Api.java", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 3, "{facts:#?}");

    let get = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("https://api.example.com/users"))
        .expect("default get");
    assert_eq!(metadata_str(get, "client"), Some("java.net.http"));
    assert_eq!(metadata_str(get, "verb"), Some("GET"));
    assert_eq!(metadata_str(get, "verb_source"), Some("default"));

    let post = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/items"))
        .expect("post");
    assert_eq!(metadata_str(post, "verb"), Some("POST"));

    let custom = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/users/1"))
        .expect("custom");
    assert_eq!(metadata_str(custom, "verb"), Some("PATCH"));
}

#[test]
fn java_http_request_dynamic_uri_create_stays_silent() {
    let source = r#"
import java.net.URI;
import java.net.http.HttpRequest;

class Api {
    void load(String path) {
        HttpRequest skipped = HttpRequest.newBuilder(URI.create("https://api.example.com" + path)).build();
        HttpRequest kept = HttpRequest.newBuilder(URI.create("https://api.example.com/static")).build();
    }
}
"#;
    let results = extract("src/Api.java", source);
    let facts = client_requests(&results);

    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(
        metadata_str(facts[0], "target_path"),
        Some("https://api.example.com/static")
    );
}

#[test]
fn go_net_http_client_calls_emit_client_requests() {
    let source = r#"
package main

import "net/http"

func load() {
    http.Get("https://api.example.com/users")
    http.Post("/items", "application/json", body)
    http.NewRequest("PATCH", "/users/1", nil)
    http.NewRequestWithContext(ctx, "DELETE", "/users/2", nil)
}
"#;
    let results = extract("client.go", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 4, "{facts:#?}");
    assert!(
        facts
            .iter()
            .any(|fact| metadata_str(fact, "verb") == Some("GET"))
    );
    assert!(
        facts
            .iter()
            .any(|fact| metadata_str(fact, "verb") == Some("POST"))
    );
    assert!(
        facts
            .iter()
            .any(|fact| metadata_str(fact, "verb") == Some("PATCH"))
    );
    assert!(
        facts
            .iter()
            .any(|fact| metadata_str(fact, "verb") == Some("DELETE"))
    );
}

#[test]
fn go_rune_literal_does_not_mask_later_client_requests() {
    let source = r#"
package main

import "net/http"

func load() {
    _ = '"'
    http.Get("/after")
}
"#;
    let results = extract("client.go", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "target_path"), Some("/after"));
}

#[test]
fn go_backtick_raw_string_urls_emit_client_requests() {
    let source = r#"
package main

import "net/http"

func load() {
    http.Get(`https://api.example.com/users`)
}
"#;
    let results = extract("client.go", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(
        metadata_str(facts[0], "target_path"),
        Some("https://api.example.com/users")
    );
    assert_eq!(metadata_str(facts[0], "verb"), Some("GET"));
}

#[test]
fn go_client_calls_on_longer_identifiers_stay_silent() {
    let source = r#"
package main

import "net/http"

func load() {
    myhttp.Get("/not-a-real-client-call")
}
"#;
    let results = extract("client.go", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "{facts:#?}");
}

#[test]
fn java_builder_on_longer_type_names_stays_silent() {
    let source = r#"
import java.net.http.*;

class Client {
    void load() {
        MyHttpRequest.newBuilder(URI.create("/not-a-real-request")).GET();
    }
}
"#;
    let results = extract("Client.java", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "{facts:#?}");
}

#[test]
fn ruby_net_http_uri_calls_emit_client_requests() {
    let source = r#"
require "net/http"
require "uri"

def load
  Net::HTTP.get(URI("https://api.example.com/users"))
  Net::HTTP.post_form(URI.parse("/items"), { "name" => "x" })
end
"#;
    let results = extract("client.rb", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 2, "{facts:#?}");
    assert!(
        facts
            .iter()
            .any(|fact| metadata_str(fact, "verb") == Some("GET"))
    );
    assert!(
        facts
            .iter()
            .any(|fact| metadata_str(fact, "verb") == Some("POST"))
    );
    assert!(
        facts
            .iter()
            .any(|fact| metadata_str(fact, "client") == Some("net::http"))
    );
}

#[test]
fn ruby_net_http_interpolated_uri_stays_silent() {
    let source = r#"
require "net/http"
require "uri"

def load(host)
  Net::HTTP.get(URI.parse("https://#{host}/users"))
  Net::HTTP.get(URI.parse("https://api.example.com/static"))
end
"#;
    let results = extract("client.rb", source);
    let facts = client_requests(&results);

    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(
        metadata_str(facts[0], "target_path"),
        Some("https://api.example.com/static")
    );
}

#[test]
fn ruby_regex_literal_does_not_mask_later_net_http_calls() {
    let source = r#"
require "net/http"
require "uri"

def load
  quote = /["']/
  Net::HTTP.get(URI("/after"))
end
"#;
    let results = extract("client.rb", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 1, "{facts:#?}");
    assert_eq!(metadata_str(facts[0], "target_path"), Some("/after"));
}

#[test]
fn kotlin_ktor_client_verb_calls_emit_client_requests() {
    let source = r#"
import io.ktor.client.HttpClient
import io.ktor.client.request.get
import io.ktor.client.request.post
import io.ktor.client.request.delete

suspend fun load(client: HttpClient) {
    client.get("https://api.example.com/users")
    client.post("/items")
    client.delete("/users/1")
}
"#;
    let results = extract("src/Client.kt", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 3, "{facts:#?}");

    let get = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("https://api.example.com/users"))
        .expect("get");
    assert_eq!(metadata_str(get, "client"), Some("ktor"));
    assert_eq!(metadata_str(get, "verb"), Some("GET"));
    assert_eq!(metadata_str(get, "verb_source"), Some("attested"));
    assert_eq!(metadata_str(get, "url_kind"), Some("absolute"));
    assert_eq!(metadata_str(get, "query_family"), Some("web.http_client"));
    assert_eq!(
        binding_symbol(&results, get).map(|(name, _)| name),
        Some("load")
    );

    let post = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/items"))
        .expect("post");
    assert_eq!(metadata_str(post, "verb"), Some("POST"));
    assert_eq!(metadata_str(post, "url_kind"), Some("path"));

    let delete = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/users/1"))
        .expect("delete");
    assert_eq!(metadata_str(delete, "verb"), Some("DELETE"));
}

#[test]
fn kotlin_ktor_dynamic_urls_stay_silent() {
    let source = r#"
import io.ktor.client.HttpClient
import io.ktor.client.request.get

suspend fun load(client: HttpClient, id: String) {
    client.get("$base/users")
    client.get("${base}/users")
    client.get("/users/" + id)
    client.get(endpoint)
}
"#;
    let results = extract("src/Client.kt", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "expected silence, got {facts:#?}");
}

#[test]
fn kotlin_ktor_requires_import() {
    // The `client.get("...")` shape exists but without the Ktor import the
    // collector stays silent (import gate keeps `.get()` from misfiring).
    let source = r#"
suspend fun load(client: Any) {
    client.get("/users")
}
"#;
    let results = extract("src/Client.kt", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "expected silence, got {facts:#?}");
}

#[test]
fn kotlin_ktor_bare_identifier_get_is_not_a_client_request() {
    // A bare `get("/x")` (no receiver) is the server-side routing DSL, not a
    // client call — only a `receiver.verb(...)` navigation callee qualifies.
    let source = r#"
import io.ktor.client.HttpClient
import io.ktor.client.request.get

fun routes() {
    get("/status")
}
"#;
    let results = extract("src/Routes.kt", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "expected silence, got {facts:#?}");
}

#[test]
fn kotlin_deferred_clients_emit_static_requests() {
    let source = r#"
import okhttp3.Request
import retrofit2.http.GET
import retrofit2.http.POST
import org.springframework.web.reactive.function.client.WebClient
import org.springframework.web.client.RestTemplate

interface Api {
    @GET("/users")
    suspend fun users(): List<String>

    @POST("/items")
    suspend fun create(): Unit
}

fun load(web: WebClient, rest: RestTemplate, body: okhttp3.RequestBody) {
    Request.Builder().url("https://api.example.com/health").build()
    Request.Builder().url("/items").post(body).build()
    web.get().uri("/web/users").retrieve()
    web.delete().uri("https://api.example.com/web/1").retrieve()
    rest.getForObject("/legacy/users", String::class.java)
    rest.postForObject("/legacy/items", body, String::class.java)
}
"#;
    let results = extract("src/Client.kt", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 8, "{facts:#?}");
    let tuples: Vec<_> = facts
        .iter()
        .map(|fact| {
            (
                metadata_str(fact, "client").unwrap(),
                metadata_str(fact, "target_path").unwrap(),
                metadata_str(fact, "verb").unwrap(),
                metadata_str(fact, "verb_source").unwrap(),
            )
        })
        .collect();
    assert!(tuples.contains(&(
        "okhttp",
        "https://api.example.com/health",
        "GET",
        "default"
    )));
    assert!(tuples.contains(&("okhttp", "/items", "POST", "attested")));
    assert!(tuples.contains(&("retrofit", "/users", "GET", "attested")));
    assert!(tuples.contains(&("retrofit", "/items", "POST", "attested")));
    assert!(tuples.contains(&("spring_webclient", "/web/users", "GET", "attested")));
    assert!(tuples.contains(&(
        "spring_webclient",
        "https://api.example.com/web/1",
        "DELETE",
        "attested"
    )));
    assert!(tuples.contains(&(
        "spring_resttemplate",
        "/legacy/users",
        "GET",
        "attested"
    )));
    assert!(tuples.contains(&(
        "spring_resttemplate",
        "/legacy/items",
        "POST",
        "attested"
    )));
}

#[test]
fn kotlin_deferred_clients_keep_ambiguous_shapes_silent() {
    let source = r#"
import okhttp3.Request
import retrofit2.http.GET
import org.springframework.web.reactive.function.client.WebClient
import org.springframework.web.client.RestTemplate

interface Api {
    @GET
    suspend fun bare(): Unit
}

fun load(web: WebClient, rest: RestTemplate, url: String) {
    Request.Builder().url(url).build()
    web.get().uri(url).retrieve()
    rest.getForObject(url, String::class.java)
}
"#;
    assert!(client_requests(&extract("src/Client.kt", source)).is_empty());
}

#[test]
fn php_guzzle_client_verb_calls_emit_client_requests() {
    let source = r#"<?php
use GuzzleHttp\Client;

function load(Client $client) {
    $client->get('https://api.example.com/users');
    $client->post('/items');
    $client->delete('/users/1');
}
"#;
    let results = extract("src/Client.php", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 3, "{facts:#?}");

    let get = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("https://api.example.com/users"))
        .expect("get");
    assert_eq!(metadata_str(get, "client"), Some("guzzle"));
    assert_eq!(metadata_str(get, "verb"), Some("GET"));
    assert_eq!(metadata_str(get, "verb_source"), Some("attested"));
    assert_eq!(metadata_str(get, "url_kind"), Some("absolute"));
    assert_eq!(metadata_str(get, "query_family"), Some("web.http_client"));
    assert_eq!(
        binding_symbol(&results, get).map(|(name, _)| name),
        Some("load")
    );

    let post = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/items"))
        .expect("post");
    assert_eq!(metadata_str(post, "verb"), Some("POST"));
    assert_eq!(metadata_str(post, "url_kind"), Some("path"));
}

#[test]
fn php_laravel_http_facade_calls_emit_client_requests() {
    let source = r#"<?php
use Illuminate\Support\Facades\Http;

function load() {
    Http::get('https://api.example.com/users');
    Http::withToken('t')->post('/items');
}
"#;
    let results = extract("src/Client.php", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 2, "{facts:#?}");

    let get = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("https://api.example.com/users"))
        .expect("get");
    assert_eq!(metadata_str(get, "client"), Some("laravel_http"));
    assert_eq!(metadata_str(get, "verb"), Some("GET"));
    assert_eq!(metadata_str(get, "url_kind"), Some("absolute"));

    // Chained `Http::withToken(...)->post(...)` still roots at the Http facade.
    let post = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/items"))
        .expect("post");
    assert_eq!(metadata_str(post, "client"), Some("laravel_http"));
    assert_eq!(metadata_str(post, "verb"), Some("POST"));
}

#[test]
fn php_dynamic_urls_stay_silent() {
    let source = r#"<?php
use GuzzleHttp\Client;
use Illuminate\Support\Facades\Http;

function load(Client $client, $id) {
    $client->get("https://api.example.com/$id");
    $client->get('/users/' . $id);
    $client->get($endpoint);
    Http::get(self::BASE);
}
"#;
    let results = extract("src/Client.php", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "expected silence, got {facts:#?}");
}

#[test]
fn php_guzzle_requires_import() {
    // The `$client->get('...')` shape is highly ambiguous; without the GuzzleHttp
    // import the collector stays silent.
    let source = r#"<?php
function load($client) {
    $client->get('/users');
}
"#;
    let results = extract("src/Client.php", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "expected silence, got {facts:#?}");
}

#[test]
fn php_http_facade_bare_scoped_call_requires_import() {
    // `Http::get('...')` without the facade import stays silent.
    let source = r#"<?php
function load() {
    Http::get('/users');
}
"#;
    let results = extract("src/Client.php", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "expected silence, got {facts:#?}");
}

#[test]
fn php_deferred_clients_emit_static_requests() {
    let source = r#"<?php
use Symfony\Contracts\HttpClient\HttpClientInterface;
use GuzzleHttp\Client;

function load(HttpClientInterface $symfony, Client $guzzle) {
    $symfony->request('PATCH', 'https://api.example.com/items/1');
    $guzzle->request('GET', '/users');
    $guzzle->requestAsync('POST', '/items');

    $direct = curl_init('https://api.example.com/health');
    $ch = curl_init();
    curl_setopt($ch, CURLOPT_URL, '/curl/items');
    curl_setopt($ch, CURLOPT_CUSTOMREQUEST, 'DELETE');
}
"#;
    let results = extract("src/Client.php", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 5, "{facts:#?}");

    let tuples: Vec<_> = facts
        .iter()
        .map(|fact| {
            (
                metadata_str(fact, "client").unwrap(),
                metadata_str(fact, "target_path").unwrap(),
                metadata_str(fact, "verb").unwrap(),
                metadata_str(fact, "verb_source").unwrap(),
            )
        })
        .collect();
    assert!(tuples.contains(&(
        "symfony_http_client",
        "https://api.example.com/items/1",
        "PATCH",
        "attested"
    )));
    assert!(tuples.contains(&("guzzle", "/users", "GET", "attested")));
    assert!(tuples.contains(&("guzzle", "/items", "POST", "attested")));
    assert!(tuples.contains(&(
        "curl",
        "https://api.example.com/health",
        "GET",
        "default"
    )));
    assert!(tuples.contains(&("curl", "/curl/items", "DELETE", "attested")));
}

#[test]
fn php_deferred_clients_keep_ambiguous_shapes_silent() {
    let source = r#"<?php
function load($client, $url) {
    $client->request('GET', '/no-type');
    curl_setopt($ch, CURLOPT_CUSTOMREQUEST, 'DELETE');
    $ch = curl_init();
    curl_setopt($ch, CURLOPT_CUSTOMREQUEST, 'DELETE');
}
"#;
    assert!(client_requests(&extract("src/Client.php", source)).is_empty());
}

#[test]
fn elixir_req_client_verb_calls_emit_client_requests() {
    let source = r#"defmodule MyApp.Client do
  def load do
    Req.get("https://api.example.com/users")
    Req.post("/items")
    Req.delete("/users/1")
    Req.get!("/health")
  end
end
"#;
    let results = extract("lib/my_app/client.ex", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 4, "{facts:#?}");

    let get = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("https://api.example.com/users"))
        .expect("get");
    assert_eq!(metadata_str(get, "client"), Some("req"));
    assert_eq!(metadata_str(get, "verb"), Some("GET"));
    assert_eq!(metadata_str(get, "verb_source"), Some("attested"));
    assert_eq!(metadata_str(get, "url_kind"), Some("absolute"));
    assert_eq!(metadata_str(get, "query_family"), Some("web.http_client"));
    // The request sits inside its enclosing function, so it binds to `load`.
    assert_eq!(
        binding_symbol(&results, get).map(|(name, _)| name),
        Some("load")
    );

    let post = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/items"))
        .expect("post");
    assert_eq!(metadata_str(post, "verb"), Some("POST"));
    assert_eq!(metadata_str(post, "url_kind"), Some("path"));

    // The bang variant `Req.get!` shares the GET verb.
    let health = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/health"))
        .expect("get! bang variant");
    assert_eq!(metadata_str(health, "verb"), Some("GET"));
}

#[test]
fn elixir_req_dynamic_and_keyword_urls_stay_silent() {
    let source = r#"defmodule MyApp.Client do
  def load(id) do
    Req.get("/users/#{id}")
    Req.get("/users/" <> id)
    Req.get(endpoint)
    Req.get(url: "/keyword-form")
  end
end
"#;
    let results = extract("lib/my_app/client.ex", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "expected silence, got {facts:#?}");
}

#[test]
fn elixir_non_req_clients_stay_silent() {
    // Only Req ships in v2.8.0; HTTPoison/Tesla/Finch/:httpc are deferred
    // open_gaps — a qualified call on another module alias stays silent.
    let source = r#"defmodule MyApp.Client do
  def load do
    HTTPoison.get("https://api.example.com/users")
    Tesla.get("/items")
  end
end
"#;
    let results = extract("lib/my_app/client.ex", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "expected silence, got {facts:#?}");
}

#[test]
fn rust_reqwest_scoped_and_builder_calls_emit_client_requests() {
    let source = r#"use reqwest::Client;

async fn load() {
    reqwest::get("https://api.example.com/users").await;
    let client = Client::new();
    client.post("https://api.example.com/items").await;
    reqwest::Client::new().delete("/users/1").await;
}
"#;
    let results = extract("src/client.rs", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 3, "{facts:#?}");

    let get = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("https://api.example.com/users"))
        .expect("scoped reqwest::get");
    assert_eq!(metadata_str(get, "client"), Some("reqwest"));
    assert_eq!(metadata_str(get, "verb"), Some("GET"));
    assert_eq!(metadata_str(get, "verb_source"), Some("attested"));
    assert_eq!(metadata_str(get, "url_kind"), Some("absolute"));
    assert_eq!(metadata_str(get, "query_family"), Some("web.http_client"));
    // ktor-style: no import_source key.
    assert_eq!(metadata_str(get, "import_source"), None);
    // The request sits inside its enclosing function, so it binds to `load`.
    assert_eq!(
        binding_symbol(&results, get).map(|(name, _)| name),
        Some("load")
    );

    let post = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("https://api.example.com/items"))
        .expect("builder client.post");
    assert_eq!(metadata_str(post, "verb"), Some("POST"));

    let delete = facts
        .iter()
        .find(|fact| metadata_str(fact, "target_path") == Some("/users/1"))
        .expect("Client::new().delete");
    assert_eq!(metadata_str(delete, "verb"), Some("DELETE"));
    assert_eq!(metadata_str(delete, "url_kind"), Some("path"));
}

#[test]
fn rust_reqwest_dynamic_urls_stay_silent() {
    let source = r#"use reqwest;

async fn load(id: u32) {
    reqwest::get(format!("https://api.example.com/users/{id}")).await;
    reqwest::get(&("https://x/".to_owned() + "y")).await;
    let url = endpoint();
    reqwest::get(url).await;
}
"#;
    let results = extract("src/client.rs", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "expected silence, got {facts:#?}");
}

#[test]
fn rust_reqwest_builder_map_get_collision_stays_silent() {
    // Rust's `HashMap::get(&str)` shares the `x.get("k")` builder shape. The
    // url-like guard keeps a non-url map lookup ("session-token", `relative`)
    // silent while a real reqwest builder call with a `/`-rooted URL still emits.
    let source = r#"use reqwest::Client;

async fn load(headers: std::collections::HashMap<String, String>) {
    let _token = headers.get("session-token");
    let client = Client::new();
    client.get("/health").await;
}
"#;
    let results = extract("src/client.rs", source);
    let facts = client_requests(&results);
    assert_eq!(
        facts.len(),
        1,
        "only the real reqwest call emits: {facts:#?}"
    );
    assert_eq!(metadata_str(facts[0], "target_path"), Some("/health"));
    assert_eq!(metadata_str(facts[0], "verb"), Some("GET"));
}

#[test]
fn rust_reqwest_requires_import() {
    // The `reqwest::get("...")` / `client.get("...")` shapes without the reqwest
    // import register nothing (import gate).
    let source = r#"async fn load() {
    http::get("https://api.example.com/users").await;
}
"#;
    let results = extract("src/client.rs", source);
    let facts = client_requests(&results);
    assert!(facts.is_empty(), "no reqwest import → no client requests");
}

#[test]
fn rust_reqwest_builder_unproven_receiver_stays_silent() {
    // F2 regression (codex): the builder verb arm must PROVE its receiver is a
    // reqwest client before emitting. `store` is single-assigned from a
    // non-reqwest constructor, so a `/`-rooted `store.delete("/cache/key")` /
    // `store.get("/ns/key")` (which classify as "path", not "relative", and so
    // slip past the url-like guard) must stay silent in a reqwest-import file.
    let source = r#"use reqwest::Client;

async fn load() {
    let store = SomeStore::new();
    store.delete("/cache/key");
    store.get("/ns/key");
}
"#;
    let results = extract("src/client.rs", source);
    let facts = client_requests(&results);
    assert!(
        facts.is_empty(),
        "unproven builder receiver must stay silent, got {facts:#?}"
    );
}

#[test]
fn rust_reqwest_proven_receivers_still_emit() {
    // F2 no-regression: every PROVEN reqwest receiver keeps emitting — the scoped
    // free function, an inline `reqwest::Client::new()` ctor chain, a local var
    // single-assigned from a client ctor, and a `&reqwest::Client` typed
    // parameter (the common injected-client idiom — guards against over-suppression).
    let source = r#"use reqwest::Client;

async fn load(shared: &reqwest::Client) {
    reqwest::get("https://api.example.com/a").await;
    reqwest::Client::new().post("https://api.example.com/b").await;
    let client = reqwest::Client::new();
    client.get("https://api.example.com/c").await;
    shared.delete("https://api.example.com/d").await;
}
"#;
    let results = extract("src/client.rs", source);
    let facts = client_requests(&results);
    let paths: std::collections::HashSet<_> = facts
        .iter()
        .filter_map(|fact| metadata_str(fact, "target_path"))
        .collect();
    assert!(
        paths.contains("https://api.example.com/a"),
        "scoped reqwest::get must emit: {facts:#?}"
    );
    assert!(
        paths.contains("https://api.example.com/b"),
        "inline reqwest::Client::new() builder must emit: {facts:#?}"
    );
    assert!(
        paths.contains("https://api.example.com/c"),
        "var single-assigned from a client ctor must emit: {facts:#?}"
    );
    assert!(
        paths.contains("https://api.example.com/d"),
        "a `&reqwest::Client` typed parameter must emit: {facts:#?}"
    );
    assert_eq!(
        facts.len(),
        4,
        "exactly the four proven requests: {facts:#?}"
    );
}

#[test]
fn rust_hyper_and_ureq_emit_static_client_requests() {
    let source = r#"
use hyper::{Method, Request};
use ureq;

fn load() {
    let _ = Request::builder()
        .method(Method::POST)
        .uri("https://api.example.com/items")
        .body(());
    let _ = hyper::Request::builder()
        .uri("/health")
        .body(());
    let _ = ureq::get("https://api.example.com/users").call();
    let _ = ureq::delete("/users/1").call();
}
"#;
    let results = extract("src/client.rs", source);
    let facts = client_requests(&results);
    assert_eq!(facts.len(), 4, "{facts:#?}");

    let tuples: Vec<_> = facts
        .iter()
        .map(|fact| {
            (
                metadata_str(fact, "client").unwrap(),
                metadata_str(fact, "target_path").unwrap(),
                metadata_str(fact, "verb").unwrap(),
                metadata_str(fact, "verb_source").unwrap(),
            )
        })
        .collect();
    assert!(tuples.contains(&(
        "hyper",
        "https://api.example.com/items",
        "POST",
        "attested"
    )));
    assert!(tuples.contains(&("hyper", "/health", "GET", "default")));
    assert!(tuples.contains(&(
        "ureq",
        "https://api.example.com/users",
        "GET",
        "attested"
    )));
    assert!(tuples.contains(&("ureq", "/users/1", "DELETE", "attested")));
}

#[test]
fn rust_deferred_clients_keep_ambiguous_or_dynamic_shapes_silent() {
    let source = r#"
use hyper::Request;
use ureq;

fn load(url: &str, map: std::collections::HashMap<&str, &str>) {
    let _ = Request::builder().uri(url).body(());
    let _ = ureq::get(url).call();
    let _ = map.get("https://not-a-request.example");
    let _ = OtherRequest::builder().uri("https://not-hyper.example").body(());
}
"#;
    assert!(client_requests(&extract("src/client.rs", source)).is_empty());
}
