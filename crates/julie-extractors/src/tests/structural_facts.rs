use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

#[test]
fn rust_unsafe_blocks_emit_structural_facts_with_containing_symbol() {
    let source = r#"pub fn read_flag(value: &i32) -> i32 {
    unsafe {
        core::ptr::read_volatile(value)
    }
}
"#;

    let results = extract("src/lib.rs", source);
    let read_flag = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "read_flag")
        .expect("expected read_flag symbol");

    let fact = results
        .structural_facts
        .iter()
        .find(|fact| fact.pattern_id == "rust.unsafe_block.v1")
        .expect("expected unsafe-block structural fact");

    assert_eq!(fact.capture_name, "unsafe_block");
    assert_eq!(fact.node_kind, "unsafe_block");
    assert_eq!(
        fact.containing_symbol_id.as_deref(),
        Some(read_flag.id.as_str())
    );
    assert_eq!(fact.confidence, 1.0);
    assert_eq!(
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("query_family"))
            .and_then(|value| value.as_str()),
        Some("safety")
    );
    assert!(fact.end_byte > fact.start_byte);
}

#[test]
fn csharp_minimal_api_routes_emit_structural_facts() {
    let source = r#"using Microsoft.AspNetCore.Builder;

var builder = WebApplication.CreateBuilder(args);
var app = builder.Build();

app.MapGet("/todos", () => "ok");
app.MapPost("/todos", CreateTodo);
app.MapPut("/todos/{id}", (int id) => Results.Ok(id));
app.MapPatch("/todos/{id}", (int id) => Results.Ok(id));
app.MapDelete("/todos/{id}", DeleteTodo);

var dynamicRoute = "/dynamic";
app.MapGet(dynamicRoute, () => "skip");
app.MapGet($"/computed/{id}", () => "skip");

static IResult CreateTodo() => Results.Ok();
static IResult DeleteTodo(int id) => Results.Ok();
"#;

    let results = extract("src/Program.cs", source);
    let facts = facts_with_pattern(&results, "aspnet.minimal_api.route.v1");

    assert_eq!(facts.len(), 5);
    assert_eq!(
        facts
            .iter()
            .filter_map(|fact| metadata_str(fact, "verb"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["DELETE", "GET", "PATCH", "POST", "PUT"])
    );
    assert_eq!(
        facts
            .iter()
            .filter_map(|fact| metadata_str(fact, "route_template"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/todos", "/todos/{id}"])
    );

    for fact in &facts {
        assert_common_framework_fact(fact, "route_call", "framework");
        assert_eq!(metadata_str(fact, "framework"), Some("aspnet"));
        assert_eq!(metadata_str(fact, "api_style"), Some("minimal_api"));
        assert_eq!(metadata_str(fact, "route_source"), Some("string_literal"));
    }

    let post = facts
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("POST"))
        .expect("expected POST route fact");
    assert_eq!(metadata_str(post, "handler_kind"), Some("method_group"));
    assert_eq!(metadata_str(post, "handler_name"), Some("CreateTodo"));

    let put = facts
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("PUT"))
        .expect("expected PUT route fact");
    assert_eq!(metadata_str(put, "handler_kind"), Some("lambda"));
    assert_eq!(metadata_str(put, "handler_name"), None);
}

#[test]
fn csharp_minimal_api_route_groups_emit_group_and_effective_route_facts() {
    let source = r#"using Microsoft.AspNetCore.Builder;

var builder = WebApplication.CreateBuilder(args);
var app = builder.Build();

var admin = app.MapGroup("/admin/connectors");
admin.MapPost("/save", SaveAsync);
admin.MapGet("/preview-email", PreviewEmailAsync);

RouteGroupBuilder reports = app.MapGroup("/reports");
reports.MapGet("/daily", () => Results.Ok());

app.MapGet("/health", () => "ok");

// var skipped = app.MapGroup("/commented");
var text = "app.MapGroup(\"/string\")";

static IResult SaveAsync() => Results.Ok();
static IResult PreviewEmailAsync() => Results.Ok();
"#;

    let results = extract("src/Program.cs", source);
    let groups = facts_with_pattern(&results, "aspnet.minimal_api.route_group.v1");
    assert_eq!(groups.len(), 2);
    assert_eq!(
        groups
            .iter()
            .filter_map(|fact| metadata_str(fact, "route_prefix"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/admin/connectors", "/reports"])
    );
    let admin_group = groups
        .iter()
        .find(|fact| metadata_str(fact, "group_variable") == Some("admin"))
        .expect("expected admin route group fact");
    assert_common_framework_fact(admin_group, "route_group", "framework");
    assert_eq!(metadata_str(admin_group, "framework"), Some("aspnet"));
    assert_eq!(metadata_str(admin_group, "api_style"), Some("minimal_api"));
    assert_eq!(
        metadata_str(admin_group, "route_source"),
        Some("string_literal")
    );
    assert_eq!(metadata_str(admin_group, "source_kind"), Some("map_group"));

    let routes = facts_with_pattern(&results, "aspnet.minimal_api.route.v1");
    let save = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/save"))
        .expect("expected grouped save route fact");
    assert_eq!(
        metadata_str(save, "route_group_prefix"),
        Some("/admin/connectors")
    );
    assert_eq!(
        metadata_str(save, "effective_route_template"),
        Some("/admin/connectors/save")
    );
    assert_eq!(metadata_str(save, "route_group_source"), Some("map_group"));

    let health = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/health"))
        .expect("expected ungrouped health route fact");
    assert_eq!(metadata_str(health, "route_group_prefix"), None);
    assert_eq!(metadata_str(health, "effective_route_template"), None);
}

#[test]
fn csharp_minimal_api_chained_route_groups_emit_effective_route_facts() {
    let source = r#"using Microsoft.AspNetCore.Builder;

var builder = WebApplication.CreateBuilder(args);
var app = builder.Build();

app.MapGroup("/admin").MapGet("/users", () => "ok");
"#;

    let results = extract("src/Program.cs", source);
    let routes = facts_with_pattern(&results, "aspnet.minimal_api.route.v1");
    let users = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/users"))
        .expect("expected chained group route fact");

    assert_eq!(metadata_str(users, "route_group_prefix"), Some("/admin"));
    assert_eq!(
        metadata_str(users, "effective_route_template"),
        Some("/admin/users")
    );
    assert_eq!(metadata_str(users, "route_group_source"), Some("map_group"));
}

#[test]
fn csharp_attribute_routes_emit_controller_and_method_facts() {
    let source = r#"using Microsoft.AspNetCore.Mvc;

[ApiController]
[Route("api/[controller]")]
public class UsersController : ControllerBase
{
    [HttpGet("{id}")]
    public IActionResult Get(int id) => Ok();

    [HttpPost]
    public IActionResult Create() => Ok();

    [HttpGet("[action]")]
    public IActionResult List() => Ok();
}
"#;

    let results = extract("src/UsersController.cs", source);
    let facts = facts_with_pattern(&results, "aspnet.attribute_route.v1");

    for fact in &facts {
        assert_common_framework_fact(fact, "attribute_route", "framework");
        assert_eq!(metadata_str(fact, "framework"), Some("aspnet"));
        assert_eq!(metadata_str(fact, "api_style"), Some("attribute_routing"));
    }

    // Controller-level route fact.
    let controller = facts
        .iter()
        .find(|fact| metadata_str(fact, "attribute_kind") == Some("controller_route"))
        .expect("expected controller_route fact");
    assert_eq!(
        metadata_str(controller, "route_template"),
        Some("api/[controller]")
    );
    assert_eq!(
        metadata_str(controller, "effective_route_template"),
        Some("/api/users")
    );
    assert_eq!(
        metadata_array(controller, "route_tokens"),
        vec!["controller"]
    );
    assert_eq!(metadata_str(controller, "verb"), None);

    // GET /api/users/{id}
    let get = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("{id}"))
        .expect("expected HttpGet({id}) fact");
    assert_eq!(metadata_str(get, "attribute_kind"), Some("http_method"));
    assert_eq!(metadata_str(get, "verb"), Some("GET"));
    assert_eq!(
        metadata_str(get, "controller_route_template"),
        Some("api/[controller]")
    );
    assert_eq!(
        metadata_str(get, "effective_route_template"),
        Some("/api/users/{id}")
    );
    assert_eq!(metadata_array(get, "route_tokens"), vec!["controller"]);

    // Bare [HttpPost] inherits controller-level effective template.
    let post = facts
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("POST"))
        .expect("expected bare HttpPost fact");
    assert_eq!(metadata_str(post, "attribute_kind"), Some("http_method"));
    assert_eq!(metadata_str(post, "route_template"), None);
    assert_eq!(
        metadata_str(post, "controller_route_template"),
        Some("api/[controller]")
    );
    assert_eq!(
        metadata_str(post, "effective_route_template"),
        Some("/api/users")
    );
    assert_eq!(metadata_array(post, "route_tokens"), vec!["controller"]);

    // [action] substitution.
    let list = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("[action]"))
        .expect("expected HttpGet([action]) fact");
    assert_eq!(
        metadata_str(list, "effective_route_template"),
        Some("/api/users/list")
    );
    assert_eq!(
        metadata_array(list, "route_tokens"),
        vec!["controller", "action"]
    );
}

#[test]
fn csharp_attribute_route_named_http_argument_keeps_bare_method_fact() {
    let source = r#"using Microsoft.AspNetCore.Mvc;

[Route("api/[controller]")]
public class UsersController : ControllerBase
{
    [HttpGet(Name = "GetUsers")]
    public IActionResult Index() => Ok();
}
"#;

    let results = extract("src/UsersController.cs", source);
    let facts = facts_with_pattern(&results, "aspnet.attribute_route.v1");

    assert_eq!(facts.len(), 2);
    let get = facts
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("GET"))
        .expect("expected named-only HttpGet fact");
    assert_eq!(metadata_str(get, "attribute_kind"), Some("http_method"));
    assert_eq!(metadata_str(get, "route_template"), None);
    assert_eq!(
        metadata_str(get, "controller_route_template"),
        Some("api/[controller]")
    );
    assert_eq!(
        metadata_str(get, "effective_route_template"),
        Some("/api/users")
    );
    assert_eq!(metadata_array(get, "route_tokens"), vec!["controller"]);
}

#[test]
fn csharp_attribute_route_absolute_action_template_does_not_combine_controller_route() {
    let source = r#"using Microsoft.AspNetCore.Mvc;

[Route("api/[controller]")]
public class UsersController : ControllerBase
{
    [HttpGet("/health")]
    public IActionResult Health() => Ok();

    [HttpGet("~/status")]
    public IActionResult Status() => Ok();
}
"#;

    let results = extract("src/UsersController.cs", source);
    let facts = facts_with_pattern(&results, "aspnet.attribute_route.v1");

    let health = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/health"))
        .expect("expected absolute /health route fact");
    assert_eq!(
        metadata_str(health, "effective_route_template"),
        Some("/health")
    );
    assert!(metadata_array(health, "route_tokens").is_empty());

    let status = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("~/status"))
        .expect("expected absolute ~/status route fact");
    assert_eq!(
        metadata_str(status, "effective_route_template"),
        Some("/status")
    );
    assert!(metadata_array(status, "route_tokens").is_empty());
}

#[test]
fn csharp_attribute_route_non_literal_argument_is_silent() {
    let source = r#"using Microsoft.AspNetCore.Mvc;

[Route("api/[controller]")]
public class PingController : ControllerBase
{
    [HttpGet(Routes.Ping)]
    public IActionResult Ping() => Ok();

    [HttpGet($"/computed/{Version}")]
    public IActionResult Computed() => Ok();
}
"#;

    let results = extract("src/PingController.cs", source);
    let facts = facts_with_pattern(&results, "aspnet.attribute_route.v1");

    // Only the controller_route fact survives; both non-literal method attributes stay silent.
    assert_eq!(facts.len(), 1);
    assert_eq!(
        metadata_str(facts[0], "attribute_kind"),
        Some("controller_route")
    );
}

#[test]
fn csharp_attribute_route_without_controller_template() {
    let source = r#"using Microsoft.AspNetCore.Mvc;

[ApiController]
public class HealthController : ControllerBase
{
    [HttpGet("ping")]
    public IActionResult Ping() => Ok();
}
"#;

    let results = extract("src/HealthController.cs", source);
    let facts = facts_with_pattern(&results, "aspnet.attribute_route.v1");

    assert_eq!(facts.len(), 1);
    let ping = facts[0];
    assert_eq!(metadata_str(ping, "attribute_kind"), Some("http_method"));
    assert_eq!(metadata_str(ping, "verb"), Some("GET"));
    assert_eq!(metadata_str(ping, "route_template"), Some("ping"));
    assert_eq!(metadata_str(ping, "controller_route_template"), None);
    assert_eq!(
        metadata_str(ping, "effective_route_template"),
        Some("/ping")
    );
    assert!(metadata_array(ping, "route_tokens").is_empty());
}

#[test]
fn csharp_api_controller_without_route_attributes_is_silent() {
    let source = r#"using Microsoft.AspNetCore.Mvc;

[ApiController]
public class BareController : ControllerBase
{
    public IActionResult Index() => Ok();
}
"#;

    let results = extract("src/BareController.cs", source);
    assert!(facts_with_pattern(&results, "aspnet.attribute_route.v1").is_empty());
}

#[test]
fn csharp_method_route_attribute_without_verb_emits_route_fact() {
    let source = r#"using Microsoft.AspNetCore.Mvc;

[Route("api/[controller]")]
public class LegacyController : ControllerBase
{
    [Route("legacy")]
    public IActionResult Legacy() => Ok();

    [HttpGet]
    [Route("both")]
    public IActionResult Both() => Ok();
}
"#;

    let results = extract("src/LegacyController.cs", source);
    let facts = facts_with_pattern(&results, "aspnet.attribute_route.v1");

    let legacy = facts
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("legacy"))
        .expect("expected method Route fact");
    assert_eq!(metadata_str(legacy, "attribute_kind"), Some("route"));
    assert_eq!(metadata_str(legacy, "verb"), None);
    assert_eq!(
        metadata_str(legacy, "effective_route_template"),
        Some("/api/legacy/legacy")
    );

    // Method with both [HttpGet] and [Route("both")]: the Http* verb wins,
    // no separate `route` fact is emitted for the sibling [Route].
    let route_facts_for_both = facts
        .iter()
        .filter(|fact| metadata_str(fact, "route_template") == Some("both"))
        .collect::<Vec<_>>();
    assert!(
        route_facts_for_both
            .iter()
            .all(|fact| metadata_str(fact, "attribute_kind") != Some("route")),
        "sibling [Route] must not emit a route fact when an Http* attribute is present"
    );
    assert!(
        facts
            .iter()
            .any(|fact| metadata_str(fact, "verb") == Some("GET")
                && metadata_str(fact, "attribute_kind") == Some("http_method")),
        "expected the [HttpGet] http_method fact"
    );
}

#[test]
fn html_htmx_and_alpine_attributes_emit_structural_facts() {
    let source = r##"<div id="list"
    hx-get="/todos"
    hx-post="/todos"
    hx-target="#list"
    hx-trigger="click"
    x-data="{ open: false }">
    <button @click.prevent="open = !open" :class="{ active: open }" x-show="open">Toggle</button>
</div>
"##;

    let results = extract("src/index.html", source);

    let htmx = facts_with_pattern(&results, "htmx.attribute.v1");
    assert_eq!(htmx.len(), 4);
    assert_eq!(
        htmx.iter()
            .filter_map(|fact| metadata_str(fact, "attribute_name"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["hx-get", "hx-post", "hx-target", "hx-trigger"])
    );
    let get = htmx
        .iter()
        .find(|fact| metadata_str(fact, "attribute_name") == Some("hx-get"))
        .expect("expected hx-get fact");
    assert_common_framework_fact(get, "attribute", "frontend_interaction");
    assert_eq!(metadata_str(get, "framework"), Some("htmx"));
    assert_eq!(metadata_str(get, "verb"), Some("GET"));
    assert_eq!(metadata_str(get, "target_path"), Some("/todos"));

    let alpine = facts_with_pattern(&results, "alpine.directive.v1");
    assert_eq!(alpine.len(), 4);
    assert_eq!(
        alpine
            .iter()
            .filter_map(|fact| metadata_str(fact, "directive"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["x-bind", "x-data", "x-on", "x-show"])
    );
    let click = alpine
        .iter()
        .find(|fact| metadata_str(fact, "directive") == Some("x-on"))
        .expect("expected x-on shorthand fact");
    assert_common_framework_fact(click, "directive", "frontend_interaction");
    assert_eq!(metadata_str(click, "framework"), Some("alpine"));
    assert_eq!(metadata_str(click, "argument"), Some("click"));
    assert_eq!(metadata_bool(click, "shorthand"), Some(true));
    assert_eq!(metadata_array(click, "modifiers"), vec!["prevent"]);
    assert_eq!(metadata_str(click, "expression"), Some("open = !open"));
}

#[test]
fn html_framework_attribute_scanner_ignores_script_text() {
    let source = r#"<script>
const markup = "<div hx-get='/todos' x-data='{ open: true }'></div>";
</script>
"#;

    let results = extract("src/index.html", source);

    assert!(facts_with_pattern(&results, "htmx.attribute.v1").is_empty());
    assert!(facts_with_pattern(&results, "alpine.directive.v1").is_empty());
}

#[test]
fn data_hx_attributes_emit_canonical_htmx_facts() {
    let html = extract(
        "src/index.html",
        r#"<form data-hx-post="/todos" DATA-HX-GET="/todos">
  <button>Save</button>
</form>
"#,
    );
    let html_htmx = facts_with_pattern(&html, "htmx.attribute.v1");
    assert_eq!(html_htmx.len(), 2);
    assert_eq!(
        html_htmx
            .iter()
            .filter_map(|fact| metadata_str(fact, "attribute_name"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["hx-get", "hx-post"])
    );
    assert!(
        html_htmx
            .iter()
            .all(|fact| metadata_bool(fact, "data_prefix") == Some(true))
    );
    let post = html_htmx
        .iter()
        .find(|fact| metadata_str(fact, "attribute_name") == Some("hx-post"))
        .expect("expected data-hx-post fact");
    assert_eq!(metadata_str(post, "target_path"), Some("/todos"));
    assert_eq!(metadata_str(post, "verb"), Some("POST"));

    let razor = extract(
        "Components/Todos.razor",
        r#"<button data-hx-delete="/todos/1">Delete</button>"#,
    );
    let razor_htmx = facts_with_pattern(&razor, "htmx.attribute.v1");
    assert_eq!(razor_htmx.len(), 1);
    assert_eq!(
        metadata_str(razor_htmx[0], "attribute_name"),
        Some("hx-delete")
    );
    assert_eq!(metadata_bool(razor_htmx[0], "data_prefix"), Some(true));
    assert_eq!(metadata_str(razor_htmx[0], "verb"), Some("DELETE"));
    assert_eq!(metadata_str(razor_htmx[0], "target_path"), Some("/todos/1"));
}

#[test]
fn razor_htmx_and_alpine_attributes_emit_structural_facts() {
    let source = r##"@page "/todos"

<div hx-get="/todos" hx-target="#list" x-data="{ open: false }">
    <button x-on:click.prevent="open = !open" x-bind:class="{ active: open }">Toggle</button>
</div>
"##;

    let results = extract("Components/Todos.razor", source);

    let htmx = facts_with_pattern(&results, "htmx.attribute.v1");
    assert_eq!(htmx.len(), 2);
    assert_eq!(
        htmx.iter()
            .filter_map(|fact| metadata_str(fact, "attribute_name"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["hx-get", "hx-target"])
    );
    let get = htmx
        .iter()
        .find(|fact| metadata_str(fact, "attribute_name") == Some("hx-get"))
        .expect("expected Razor hx-get fact");
    assert_common_framework_fact(get, "attribute", "frontend_interaction");
    assert_eq!(metadata_str(get, "target_path"), Some("/todos"));

    let alpine = facts_with_pattern(&results, "alpine.directive.v1");
    assert_eq!(alpine.len(), 3);
    assert_eq!(
        alpine
            .iter()
            .filter_map(|fact| metadata_str(fact, "directive"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["x-bind", "x-data", "x-on"])
    );
    let click = alpine
        .iter()
        .find(|fact| metadata_str(fact, "directive") == Some("x-on"))
        .expect("expected Razor x-on directive fact");
    assert_common_framework_fact(click, "directive", "frontend_interaction");
    assert_eq!(metadata_str(click, "argument"), Some("click"));
    assert_eq!(metadata_array(click, "modifiers"), vec!["prevent"]);
}

#[test]
fn jsx_family_grammars_accept_jsx_but_typescript_does_not() {
    // Language-claim evidence (Task 6). Plain `.ts` cannot carry JSX because the
    // typescript grammar reads `<Ident ...>` as a type expression, so `typescript`
    // is NOT claimed for htmx. `tsx`, `jsx`, and `javascript` accept JSX and are
    // claimed.
    let jsx = r#"const view = <button hx-post="/clicked">Go</button>;"#;

    let parse_has_error = |language: &str| {
        let ts_language = crate::language_spec::get_tree_sitter_language(language)
            .expect("language should be registered");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&ts_language)
            .expect("grammar should load");
        parser
            .parse(jsx, None)
            .expect("parse should succeed")
            .root_node()
            .has_error()
    };

    assert!(
        parse_has_error("typescript"),
        "typescript grammar should error on JSX (type-expression ambiguity)"
    );
    assert!(!parse_has_error("tsx"), "tsx grammar should accept JSX");
    assert!(!parse_has_error("jsx"), "jsx grammar should accept JSX");
    assert!(
        !parse_has_error("javascript"),
        "javascript grammar should accept JSX"
    );
}

#[test]
fn tsx_htmx_attribute_matches_html_fact_shape() {
    let tsx = extract(
        "src/Button.tsx",
        r#"export function Button() {
  return <button hx-post="/clicked">Click</button>;
}
"#,
    );
    let html = extract(
        "src/index.html",
        r#"<button hx-post="/clicked">Click</button>"#,
    );

    let tsx_htmx = facts_with_pattern(&tsx, "htmx.attribute.v1");
    assert_eq!(tsx_htmx.len(), 1);
    let html_htmx = facts_with_pattern(&html, "htmx.attribute.v1");
    assert_eq!(html_htmx.len(), 1);

    assert_common_framework_fact(tsx_htmx[0], "attribute", "frontend_interaction");
    assert_eq!(metadata_str(tsx_htmx[0], "framework"), Some("htmx"));
    assert_eq!(metadata_str(tsx_htmx[0], "attribute_name"), Some("hx-post"));
    assert_eq!(metadata_str(tsx_htmx[0], "verb"), Some("POST"));
    assert_eq!(metadata_str(tsx_htmx[0], "target_path"), Some("/clicked"));
    assert_eq!(
        metadata_str(tsx_htmx[0], "attribute_value"),
        Some("/clicked")
    );
    assert_eq!(metadata_bool(tsx_htmx[0], "data_prefix"), None);
    assert_eq!(tsx_htmx[0].language, "tsx");

    // Metadata parity key-by-key against the documented html emission.
    assert_eq!(tsx_htmx[0].metadata, html_htmx[0].metadata);
    assert_eq!(tsx_htmx[0].capture_name, html_htmx[0].capture_name);
}

#[test]
fn jsx_data_hx_attribute_normalizes_like_html() {
    let jsx = extract(
        "src/List.jsx",
        r#"export const List = () => <ul data-hx-get="/todos"></ul>;
"#,
    );
    let htmx = facts_with_pattern(&jsx, "htmx.attribute.v1");
    assert_eq!(htmx.len(), 1);
    assert_eq!(metadata_str(htmx[0], "attribute_name"), Some("hx-get"));
    assert_eq!(metadata_bool(htmx[0], "data_prefix"), Some(true));
    assert_eq!(metadata_str(htmx[0], "verb"), Some("GET"));
    assert_eq!(metadata_str(htmx[0], "target_path"), Some("/todos"));
    assert_eq!(htmx[0].language, "jsx");
}

#[test]
fn javascript_jsx_htmx_attribute_emits_fact() {
    let js = extract(
        "src/App.js",
        r#"export const App = () => <button hx-get="/todos">Go</button>;
"#,
    );
    let htmx = facts_with_pattern(&js, "htmx.attribute.v1");
    assert_eq!(htmx.len(), 1);
    assert_eq!(metadata_str(htmx[0], "attribute_name"), Some("hx-get"));
    assert_eq!(metadata_str(htmx[0], "target_path"), Some("/todos"));
    assert_eq!(htmx[0].language, "javascript");
}

#[test]
fn jsx_brace_expression_htmx_value_stays_silent() {
    let jsx = extract(
        "src/Dyn.tsx",
        r#"export const Dyn = ({ url }: { url: string }) => (
  <button hx-post={url}>Go</button>
);
"#,
    );
    assert!(facts_with_pattern(&jsx, "htmx.attribute.v1").is_empty());
}

#[test]
fn vue_template_htmx_attribute_emits_fact() {
    let vue = extract(
        "src/Widget.vue",
        r#"<template>
  <button hx-post="/clicked">Click</button>
</template>
"#,
    );
    let htmx = facts_with_pattern(&vue, "htmx.attribute.v1");
    assert_eq!(htmx.len(), 1);
    assert_common_framework_fact(htmx[0], "attribute", "frontend_interaction");
    assert_eq!(metadata_str(htmx[0], "framework"), Some("htmx"));
    assert_eq!(metadata_str(htmx[0], "attribute_name"), Some("hx-post"));
    assert_eq!(metadata_str(htmx[0], "verb"), Some("POST"));
    assert_eq!(metadata_str(htmx[0], "target_path"), Some("/clicked"));
    assert_eq!(htmx[0].language, "vue");
}

#[test]
fn vue_dynamic_binding_htmx_value_stays_silent() {
    let vue = extract(
        "src/Dyn.vue",
        r#"<template>
  <button :hx-post="url" v-bind:hx-get="endpoint">Go</button>
</template>
"#,
    );
    assert!(facts_with_pattern(&vue, "htmx.attribute.v1").is_empty());
}

#[test]
fn vue_script_section_htmx_string_stays_silent() {
    let vue = extract(
        "src/Script.vue",
        r#"<script setup>
const markup = "<button hx-post='/clicked'></button>";
</script>

<template>
  <div>{{ markup }}</div>
</template>
"#,
    );
    assert!(facts_with_pattern(&vue, "htmx.attribute.v1").is_empty());
}

#[derive(Debug)]
struct StructuralFactCase {
    file_path: &'static str,
    source: &'static str,
    expected: &'static [ExpectedStructuralFact],
}

#[derive(Debug)]
struct ExpectedStructuralFact {
    pattern_id: &'static str,
    capture_name: &'static str,
    query_family: &'static str,
    node_kinds: &'static [&'static str],
}

#[test]
fn supported_structural_patterns_emit_parser_backed_facts() {
    let cases = [
        StructuralFactCase {
            file_path: "src/lib.rs",
            source: r#"pub fn read_flag(value: &i32) -> i32 {
    unsafe {
        core::ptr::read_volatile(value)
    }
}
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "rust.unsafe_block.v1",
                capture_name: "unsafe_block",
                query_family: "safety",
                node_kinds: &["unsafe_block"],
            }],
        },
        StructuralFactCase {
            file_path: "src/service.go",
            source: r#"package main

func worker() {}
func cleanup() {}

func run() {
    go worker()
    defer cleanup()
}
"#,
            expected: &[
                ExpectedStructuralFact {
                    pattern_id: "go.goroutine_launch.v1",
                    capture_name: "go_statement",
                    query_family: "concurrency",
                    node_kinds: &["go_statement"],
                },
                ExpectedStructuralFact {
                    pattern_id: "go.defer_statement.v1",
                    capture_name: "defer_statement",
                    query_family: "lifecycle",
                    node_kinds: &["defer_statement"],
                },
            ],
        },
        StructuralFactCase {
            file_path: "src/decorators.py",
            source: r#"def timed(fn):
    return fn

@timed
def run():
    return 1
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "python.decorated_definition.v1",
                capture_name: "decorated_definition",
                query_family: "metadata",
                node_kinds: &["decorated_definition"],
            }],
        },
        StructuralFactCase {
            file_path: "src/load.js",
            source: r#"export async function load() {
    return await fetch("/api");
}
"#,
            expected: &[
                ExpectedStructuralFact {
                    pattern_id: "javascript.await_expression.v1",
                    capture_name: "await_expression",
                    query_family: "async",
                    node_kinds: &["await_expression"],
                },
                ExpectedStructuralFact {
                    pattern_id: "http.client_request.v1",
                    capture_name: "client_request",
                    query_family: "web.http_client",
                    node_kinds: &["call_expression"],
                },
            ],
        },
        StructuralFactCase {
            file_path: "src/View.jsx",
            source: r#"export async function View() {
    const data = await load();
    return <div>{data}</div>;
}
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "jsx.await_expression.v1",
                capture_name: "await_expression",
                query_family: "async",
                node_kinds: &["await_expression"],
            }],
        },
        StructuralFactCase {
            file_path: "src/load.ts",
            source: r#"export async function load(): Promise<Response> {
    return await fetch("/api");
}
"#,
            expected: &[
                ExpectedStructuralFact {
                    pattern_id: "typescript.await_expression.v1",
                    capture_name: "await_expression",
                    query_family: "async",
                    node_kinds: &["await_expression"],
                },
                ExpectedStructuralFact {
                    pattern_id: "http.client_request.v1",
                    capture_name: "client_request",
                    query_family: "web.http_client",
                    node_kinds: &["call_expression"],
                },
            ],
        },
        StructuralFactCase {
            file_path: "src/View.tsx",
            source: r#"export async function View() {
    const data = await load();
    return <div>{data}</div>;
}
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "tsx.await_expression.v1",
                capture_name: "await_expression",
                query_family: "async",
                node_kinds: &["await_expression"],
            }],
        },
        StructuralFactCase {
            file_path: "src/config.c",
            source: r#"#define LIMIT 4
#define DOUBLE(x) ((x) * 2)

int read_value(void) {
    return DOUBLE(LIMIT);
}
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "c.preprocessor_definition.v1",
                capture_name: "preprocessor_definition",
                query_family: "preprocessor",
                node_kinds: &["preproc_def", "preproc_function_def"],
            }],
        },
        StructuralFactCase {
            file_path: "src/config.cpp",
            source: r#"#define LIMIT 4
#define DOUBLE(x) ((x) * 2)

int readValue() {
    return DOUBLE(LIMIT);
}
"#,
            expected: &[ExpectedStructuralFact {
                pattern_id: "cpp.preprocessor_definition.v1",
                capture_name: "preprocessor_definition",
                query_family: "preprocessor",
                node_kinds: &["preproc_def", "preproc_function_def"],
            }],
        },
    ];

    for case in cases {
        let results = extract(case.file_path, case.source);
        let expected_ids = case
            .expected
            .iter()
            .map(|expected| expected.pattern_id.to_string())
            .collect::<BTreeSet<_>>();
        let actual_ids = results
            .structural_facts
            .iter()
            .map(|fact| fact.pattern_id.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual_ids, expected_ids,
            "{} emitted unexpected structural pattern ids",
            case.file_path
        );

        for expected in case.expected {
            let facts = results
                .structural_facts
                .iter()
                .filter(|fact| fact.pattern_id == expected.pattern_id)
                .collect::<Vec<_>>();
            let actual_node_kinds = facts
                .iter()
                .map(|fact| fact.node_kind.clone())
                .collect::<BTreeSet<_>>();
            let expected_node_kinds = expected
                .node_kinds
                .iter()
                .map(|kind| (*kind).to_string())
                .collect::<BTreeSet<_>>();
            assert_eq!(
                actual_node_kinds, expected_node_kinds,
                "{} emitted wrong node kinds for {}",
                case.file_path, expected.pattern_id
            );
            for fact in facts {
                assert_eq!(fact.capture_name, expected.capture_name);
                assert_eq!(fact.confidence, 1.0);
                assert_eq!(
                    fact.metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("query_family"))
                        .and_then(|value| value.as_str()),
                    Some(expected.query_family)
                );
                assert_eq!(
                    fact.metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("pattern_version"))
                        .and_then(|value| value.as_u64()),
                    Some(1)
                );
                assert!(fact.end_byte > fact.start_byte);
            }
        }
    }
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

fn assert_common_framework_fact(fact: &StructuralFact, capture_name: &str, query_family: &str) {
    assert_eq!(fact.capture_name, capture_name);
    assert_eq!(fact.confidence, 1.0);
    assert!(fact.end_byte > fact.start_byte);
    assert_eq!(
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("pattern_version"))
            .and_then(|value| value.as_u64()),
        Some(1)
    );
    assert_eq!(metadata_str(fact, "query_family"), Some(query_family));
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

fn metadata_bool(fact: &StructuralFact, key: &str) -> Option<bool> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
}

fn metadata_array<'a>(fact: &'a StructuralFact, key: &str) -> Vec<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_array())
        .map(|values| values.iter().filter_map(|value| value.as_str()).collect())
        .unwrap_or_default()
}

#[test]
fn web_structural_facts_mod_does_not_own_extracted_submodule_helpers() {
    let mod_source = include_str!("../base/web_structural_facts/mod.rs");
    for forbidden_definition in [
        "fn collect_css_node",
        "fn html_form_fact",
        "fn scan_vue_sections",
        "fn collect_react_router_route_object_definitions",
        "fn nextjs_app_file_route",
        "fn find_enclosing_object_range",
    ] {
        assert!(
            !mod_source.contains(forbidden_definition),
            "web_structural_facts/mod.rs still owns extracted submodule helper {forbidden_definition}"
        );
    }
}

#[test]
fn framework_structural_facts_does_not_own_shared_markup_scanner() {
    let framework_source = include_str!("../base/framework_structural_facts.rs");
    for forbidden_definition in ["fn scan_markup_attributes(", "struct MarkupAttribute {"] {
        assert!(
            !framework_source.contains(forbidden_definition),
            "framework_structural_facts.rs still owns shared markup scanner {forbidden_definition}"
        );
    }
}

/// Emission-agreement pinning for the four HTTP boundary fact families
/// (2026-07-01 plan, Task 7): the exact metadata key sets asserted here must
/// match the rows documented in `docs/contracts/jsonl-v3.md` and
/// `docs/contracts/sqlite-schema-v3.md`. A failure here means either emission
/// or the contract docs changed without the other.
#[test]
fn http_boundary_families_emit_documented_metadata_keys() {
    fn metadata_keys(fact: &StructuralFact) -> Vec<&str> {
        let mut keys: Vec<&str> = fact
            .metadata
            .as_ref()
            .map(|metadata| metadata.keys().map(String::as_str).collect())
            .unwrap_or_default();
        keys.sort_unstable();
        keys
    }

    // http.client_request.v1 — fetch carries no import_source; axios adds it.
    let fetch = extract(
        "src/save.js",
        r#"export const save = (body) => fetch("/api/users", { method: "POST", body });"#,
    );
    let fetch_facts = facts_with_pattern(&fetch, "http.client_request.v1");
    assert_eq!(fetch_facts.len(), 1);
    assert_eq!(
        metadata_keys(fetch_facts[0]),
        [
            "client",
            "framework",
            "pattern_version",
            "query_family",
            "target_path",
            "url_kind",
            "verb",
            "verb_source",
        ]
    );

    let axios = extract(
        "src/load.ts",
        "import axios from \"axios\";\nexport const load = () => axios.get<User[]>(\"/api/users\");\n",
    );
    let axios_facts = facts_with_pattern(&axios, "http.client_request.v1");
    assert_eq!(axios_facts.len(), 1);
    assert_eq!(
        metadata_keys(axios_facts[0]),
        [
            "client",
            "framework",
            "import_source",
            "pattern_version",
            "query_family",
            "target_path",
            "url_kind",
            "verb",
            "verb_source",
        ]
    );

    // nextjs.route_handler.v1 — one fact per exported verb handler.
    let handler = extract(
        "app/api/users/[id]/route.ts",
        "export async function GET(request: Request) {\n  return Response.json({});\n}\n",
    );
    let handler_facts = facts_with_pattern(&handler, "nextjs.route_handler.v1");
    assert_eq!(handler_facts.len(), 1);
    assert_eq!(
        metadata_keys(handler_facts[0]),
        [
            "dynamic_segments",
            "file_convention",
            "framework",
            "normalized_route_template",
            "pattern_version",
            "query_family",
            "route_path",
            "router",
            "source_kind",
            "verb",
            "verb_source",
        ]
    );

    // nuxt.server_route.v1 — verb/normalization keys only when attested by
    // the filename; a suffix-less static route carries the minimal set.
    let verbed = extract(
        "server/api/users/[id].get.ts",
        "export default defineEventHandler((event) => ({}));\n",
    );
    let verbed_facts = facts_with_pattern(&verbed, "nuxt.server_route.v1");
    assert_eq!(verbed_facts.len(), 1);
    assert_eq!(
        metadata_keys(verbed_facts[0]),
        [
            "dynamic_segments",
            "framework",
            "normalized_route_template",
            "pattern_version",
            "query_family",
            "route_path",
            "router",
            "source_kind",
            "verb",
            "verb_source",
        ]
    );

    let suffixless = extract(
        "server/routes/health.ts",
        "export default defineEventHandler(() => \"ok\");\n",
    );
    let suffixless_facts = facts_with_pattern(&suffixless, "nuxt.server_route.v1");
    assert_eq!(suffixless_facts.len(), 1);
    assert_eq!(
        metadata_keys(suffixless_facts[0]),
        [
            "framework",
            "pattern_version",
            "query_family",
            "route_path",
            "router",
            "source_kind",
        ]
    );

    // aspnet.attribute_route.v1 — verb only on http_method facts;
    // controller_route_template only under a controller-level [Route].
    let controller = extract(
        "src/UsersController.cs",
        "[Route(\"api/[controller]\")]\npublic class UsersController\n{\n    [HttpGet(\"{id}\")]\n    public string Get(int id) => \"\";\n}\n",
    );
    let attribute_facts = facts_with_pattern(&controller, "aspnet.attribute_route.v1");
    assert_eq!(attribute_facts.len(), 2);
    let controller_route = attribute_facts
        .iter()
        .find(|fact| metadata_str(fact, "attribute_kind") == Some("controller_route"))
        .expect("controller_route fact");
    assert_eq!(
        metadata_keys(controller_route),
        [
            "api_style",
            "attribute_kind",
            "effective_route_template",
            "framework",
            "pattern_version",
            "query_family",
            "route_template",
            "route_tokens",
        ]
    );
    let http_method = attribute_facts
        .iter()
        .find(|fact| metadata_str(fact, "attribute_kind") == Some("http_method"))
        .expect("http_method fact");
    assert_eq!(
        metadata_keys(http_method),
        [
            "api_style",
            "attribute_kind",
            "controller_route_template",
            "effective_route_template",
            "framework",
            "pattern_version",
            "query_family",
            "route_template",
            "route_tokens",
            "verb",
        ]
    );
}
