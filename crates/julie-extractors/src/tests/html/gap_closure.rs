use crate::base::{ExtractionResults, IdentifierKind, RelationshipKind, Symbol};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("page.html", source, Path::new("/tmp/test")).expect("HTML extraction")
}

fn name_of(results: &ExtractionResults, id: &str) -> String {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map(|symbol| symbol.name.clone())
        .unwrap_or_else(|| id.to_string())
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

const PAGE: &str = r#"<html>
<head>
  <style>
    :root { --brand: #0af; }
    .card { color: var(--brand); }
  </style>
</head>
<body><script>
  function helper(value) {
    return value + 1;
  }
  function validate(form) { if (!form.email.value) { showError("email required"); return false; } return true; }
  function showError(message) { axios.post("/api/log", { message }); fetch("/api/todos", { method: "POST" }); }
  class Widget extends HTMLElement { connectedCallback() { this.render(); } render() { loadDashboard(1); } }
</script>
<script type="module">import { formatDate } from "./src/app/format.js"; formatDate(new Date());</script>
</body>
</html>
"#;

#[test]
fn inline_scripts_and_styles_publish_relationships_and_pending_rows() {
    let results = extract(PAGE);
    let edges: Vec<(String, RelationshipKind, String)> = results
        .relationships
        .iter()
        .map(|relationship| {
            (
                name_of(&results, &relationship.from_symbol_id),
                relationship.kind.clone(),
                name_of(&results, &relationship.to_symbol_id),
            )
        })
        .collect();
    for expected in [
        ("validate", RelationshipKind::Calls, "showError"),
        ("connectedCallback", RelationshipKind::Calls, "render"),
        (".card", RelationshipKind::References, "--brand"),
    ] {
        assert!(
            edges.contains(&(
                expected.0.to_string(),
                expected.1.clone(),
                expected.2.to_string()
            )),
            "missing {expected:?} in {edges:?}"
        );
    }

    let pending: Vec<(String, RelationshipKind, String)> = results
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            (
                name_of(&results, &pending.pending.from_symbol_id),
                pending.pending.kind.clone(),
                pending.target.display_name.clone(),
            )
        })
        .collect();
    for expected in [
        ("Widget", RelationshipKind::Extends, "HTMLElement"),
        ("render", RelationshipKind::Calls, "loadDashboard"),
    ] {
        assert!(
            pending.contains(&(
                expected.0.to_string(),
                expected.1.clone(),
                expected.2.to_string()
            )),
            "missing {expected:?} in {pending:?}"
        );
    }
    let import = symbol(&results, "formatDate");
    assert_eq!(
        import
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("source"))
            .and_then(|value| value.as_str()),
        Some("./src/app/format.js")
    );
}

#[test]
fn inline_scripts_publish_identifiers_literals_facts_and_complexity() {
    let results = extract(PAGE);
    let validate = symbol(&results, "validate");
    assert!(results.identifiers.iter().any(|identifier| {
        identifier.name == "showError"
            && identifier.kind == IdentifierKind::Call
            && identifier.containing_symbol_id.as_deref() == Some(validate.id.as_str())
    }));
    assert!(
        results
            .identifiers
            .iter()
            .any(|identifier| identifier.name == "--brand"
                && identifier.kind == IdentifierKind::VariableRef)
    );
    assert!(
        results
            .literals
            .iter()
            .any(|literal| literal.literal_text == "/api/log"
                && literal.carrier.as_deref() == Some("axios.post"))
    );
    assert!(
        results
            .structural_facts
            .iter()
            .any(|fact| { fact.pattern_id == "http.client_request.v1" && fact.language == "html" })
    );
    let callable_metrics: Vec<String> = results
        .complexity_metrics
        .iter()
        .filter_map(|metric| metric.symbol_id.as_deref())
        .map(|id| name_of(&results, id))
        .collect();
    assert!(callable_metrics.contains(&"validate".to_string()));
    assert!(callable_metrics.contains(&"render".to_string()));
    assert_eq!(
        results
            .complexity_metrics
            .iter()
            .filter(|metric| metric.scope == "file")
            .count(),
        1
    );
}

#[test]
fn embedded_body_spans_sit_inside_their_symbols_in_host_coordinates() {
    let results = extract(PAGE);
    for name in ["helper", "Widget", ":root", ".card"] {
        let symbol = symbol(&results, name);
        let body = symbol.body_span.expect("embedded symbol keeps a body span");
        assert!(
            symbol.start_byte <= body.start_byte && body.end_byte <= symbol.end_byte,
            "{name} body {body:?} lies outside its symbol"
        );
    }
    let helper = symbol(&results, "helper");
    let body = helper.body_span.unwrap();
    assert!(
        PAGE[body.start_byte as usize..body.end_byte as usize]
            .trim()
            .starts_with('{')
    );
    let root = symbol(&results, ":root");
    let body = root.body_span.unwrap();
    assert_eq!(
        &PAGE[body.start_byte as usize..body.end_byte as usize],
        "{ --brand: #0af; }"
    );
}

const HANDLERS: &str = r#"<section id="panel">
  <button onclick="saveForm(event)">Save</button>
  <button onclick="app.cart.add(42); return false;">Add</button>
  <form onsubmit="return validate(this)"><input name="q" oninput="search(this.value)"></form>
  <div data-controller="hello" data-action="click->hello#greet">Hi</div>
  <div x-data="{ open: false, toggle() { this.open = !this.open } }" x-init="loadItems()"><button @click="toggle()">Open</button></div>
  <button hx-post="/reset" hx-on::after-request="notifyReset(event)">Reset</button>
</section>
<script>
function saveForm(e) { return e; }
function toggle() {}
</script>
"#;

#[test]
fn handler_attributes_give_one_call_identifier_per_callee() {
    let results = extract(HANDLERS);
    let calls: Vec<&str> = results
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::Call)
        .map(|identifier| identifier.name.as_str())
        .collect();
    for name in [
        "saveForm",
        "add",
        "validate",
        "search",
        "greet",
        "loadItems",
        "toggle",
        "notifyReset",
    ] {
        assert!(calls.contains(&name), "missing call {name} in {calls:?}");
    }
    assert!(
        calls
            .iter()
            .all(|name| !name.contains('(') && !name.contains("->")),
        "call names must be callees: {calls:?}"
    );
    let add = results
        .identifiers
        .iter()
        .find(|identifier| identifier.name == "add")
        .unwrap();
    assert_eq!(
        &HANDLERS[add.start_byte as usize..add.end_byte as usize],
        "add"
    );
    let greet = results
        .identifiers
        .iter()
        .find(|identifier| identifier.name == "greet" && identifier.kind == IdentifierKind::Call)
        .unwrap();
    assert_eq!(
        greet
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("controller"))
            .and_then(|value| value.as_str()),
        Some("hello")
    );
}

#[test]
fn handler_calls_link_same_file_functions_or_stay_pending() {
    let results = extract(HANDLERS);
    let calls: Vec<String> = results
        .relationships
        .iter()
        .filter(|relationship| relationship.kind == RelationshipKind::Calls)
        .map(|relationship| name_of(&results, &relationship.to_symbol_id))
        .collect();
    assert!(calls.contains(&"saveForm".to_string()), "{calls:?}");
    assert!(calls.contains(&"toggle".to_string()), "{calls:?}");
    let pending: Vec<&str> = results
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.kind == RelationshipKind::Calls)
        .map(|pending| pending.target.display_name.as_str())
        .collect();
    for name in ["validate", "search", "loadItems", "notifyReset"] {
        assert!(
            pending.contains(&name),
            "missing pending {name} in {pending:?}"
        );
    }
    assert!(
        !pending.contains(&"add"),
        "member calls stay identifiers only"
    );
}

#[test]
fn handler_calls_skip_nested_and_module_script_functions() {
    let results = extract(
        r#"<html><body>
<script>
  function outer() { function save() {} save(); }
  function reset() {}
</script>
<script type="module">
  function publish() {}
</script>
<button onclick="save()">Save</button>
<button onclick="publish()">Publish</button>
<button onclick="reset()">Reset</button>
</body></html>
"#,
    );
    let handler_calls: Vec<String> = results
        .relationships
        .iter()
        .filter(|relationship| {
            relationship.kind == RelationshipKind::Calls
                && name_of(&results, &relationship.from_symbol_id) == "button"
        })
        .map(|relationship| name_of(&results, &relationship.to_symbol_id))
        .collect();
    assert_eq!(handler_calls, vec!["reset".to_string()]);
    let pending: Vec<&str> = results
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.kind == RelationshipKind::Calls)
        .map(|pending| pending.target.display_name.as_str())
        .collect();
    assert!(pending.contains(&"save"), "{pending:?}");
    assert!(pending.contains(&"publish"), "{pending:?}");
}
