//! Phoenix router structural facts (`phoenix.route.v1`,
//! `phoenix.resource_route.v1`, `phoenix.forward.v1`).

use std::path::Path;

use crate::base::{StructuralFact, Symbol, SymbolKind};
use crate::tests::helpers::{facts_with_pattern, metadata_str};

const PHOENIX_ROUTE_PATTERN_ID: &str = "phoenix.route.v1";
const PHOENIX_RESOURCE_ROUTE_PATTERN_ID: &str = "phoenix.resource_route.v1";
const PHOENIX_FORWARD_PATTERN_ID: &str = "phoenix.forward.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
}

fn metadata_array<'a>(fact: &'a StructuralFact, key: &str) -> Vec<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_array())
        .map(|values| values.iter().filter_map(|value| value.as_str()).collect())
        .unwrap_or_default()
}

fn find_route<'a>(facts: &[&'a StructuralFact], route_template: &str) -> &'a StructuralFact {
    facts
        .iter()
        .copied()
        .find(|fact| metadata_str(fact, "route_template") == Some(route_template))
        .unwrap_or_else(|| panic!("route_template {route_template:?} not found in {facts:#?}"))
}

fn module_symbol<'a>(results: &'a crate::ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == SymbolKind::Module)
        .unwrap_or_else(|| panic!("module symbol {name:?} not found"))
}

#[test]
fn phoenix_verb_routes_carry_verb_template_and_handler() {
    let source = r#"defmodule MyAppWeb.Router do
  use Phoenix.Router

  get "/users/:id", UserController, :show
  post "/users", UserController, :create
  delete "/users/:id", UserController, :destroy
  put "/users/:id", UserController, :update
end
"#;
    let results = extract("lib/my_app_web/router.ex", source);
    let routes = facts_with_pattern(&results, PHOENIX_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 4, "{routes:#?}");

    let show = find_route(&routes, "/users/:id");
    assert_eq!(metadata_str(show, "framework"), Some("phoenix"));
    assert_eq!(metadata_str(show, "api_style"), Some("dsl_routing"));
    assert_eq!(metadata_str(show, "verb"), Some("GET"));
    assert_eq!(metadata_str(show, "verb_source"), Some("attested"));
    assert_eq!(
        metadata_str(show, "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(metadata_array(show, "dynamic_segments"), vec!["id"]);
    assert_eq!(metadata_str(show, "controller"), Some("UserController"));
    assert_eq!(metadata_str(show, "action"), Some("show"));
    // No enclosing scope → no group prefix and no effective template.
    assert_eq!(metadata_str(show, "route_group_prefix"), None);
    assert_eq!(metadata_str(show, "effective_route_template"), None);

    let create = routes
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("POST"))
        .expect("post route");
    assert_eq!(
        metadata_str(create, "normalized_route_template"),
        Some("/users")
    );
    assert_eq!(metadata_str(create, "action"), Some("create"));
}

#[test]
fn phoenix_route_binds_to_enclosing_router_module() {
    // Phoenix routes are not inside a `def`; the innermost scope-bearing symbol
    // is the router module, so the binding resolves to it (LANE LEARNING:
    // anchor so containing_symbol_id binds to the enclosing handler symbol).
    let source = r#"defmodule MyAppWeb.Router do
  use Phoenix.Router

  get "/health", HealthController, :index
end
"#;
    let results = extract("lib/my_app_web/router.ex", source);
    let router = module_symbol(&results, "MyAppWeb.Router");
    let route = find_route(
        &facts_with_pattern(&results, PHOENIX_ROUTE_PATTERN_ID),
        "/health",
    );
    assert_eq!(
        route.containing_symbol_id.as_deref(),
        Some(router.id.as_str()),
        "route must bind to the enclosing router module symbol"
    );
}

#[test]
fn phoenix_scope_prefix_joins_and_records_group_prefix() {
    let source = r#"defmodule MyAppWeb.Router do
  use Phoenix.Router

  scope "/api", MyAppWeb do
    get "/users/:id", UserController, :show
    post "/users", UserController, :create
  end
end
"#;
    let results = extract("lib/my_app_web/router.ex", source);
    let routes = facts_with_pattern(&results, PHOENIX_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 2, "{routes:#?}");

    let show = find_route(&routes, "/users/:id");
    assert_eq!(metadata_str(show, "route_group_prefix"), Some("/api"));
    assert_eq!(
        metadata_str(show, "effective_route_template"),
        Some("/api/users/:id")
    );
    assert_eq!(
        metadata_str(show, "normalized_route_template"),
        Some("/api/users/:id")
    );
    // The scope alias is a controller namespace, not a path segment — the
    // controller is recorded as written at the route.
    assert_eq!(metadata_str(show, "controller"), Some("UserController"));
}

#[test]
fn phoenix_nested_scopes_accumulate() {
    let source = r#"defmodule MyAppWeb.Router do
  use Phoenix.Router

  scope "/api", MyAppWeb do
    scope "/v1" do
      get "/users/:id", UserController, :show
    end
  end
end
"#;
    let results = extract("lib/my_app_web/router.ex", source);
    let show = find_route(
        &facts_with_pattern(&results, PHOENIX_ROUTE_PATTERN_ID),
        "/users/:id",
    );
    assert_eq!(metadata_str(show, "route_group_prefix"), Some("/api/v1"));
    assert_eq!(
        metadata_str(show, "normalized_route_template"),
        Some("/api/v1/users/:id")
    );
}

#[test]
fn phoenix_options_only_scope_adds_no_path_segment() {
    // A `scope host: "…" do` (or alias-only scope) has no positional string
    // arg0: it bounds routes lexically but contributes no prefix.
    let source = r#"defmodule MyAppWeb.Router do
  use Phoenix.Router

  scope host: "admin." do
    get "/dashboard", AdminController, :index
  end
end
"#;
    let results = extract("lib/my_app_web/router.ex", source);
    let dash = find_route(
        &facts_with_pattern(&results, PHOENIX_ROUTE_PATTERN_ID),
        "/dashboard",
    );
    assert_eq!(metadata_str(dash, "route_group_prefix"), None);
    assert_eq!(
        metadata_str(dash, "normalized_route_template"),
        Some("/dashboard")
    );
}

#[test]
fn phoenix_resources_emit_resource_route() {
    let source = r#"defmodule MyAppWeb.Router do
  use Phoenix.Router

  scope "/api" do
    resources "/photos", PhotoController
  end
end
"#;
    let results = extract("lib/my_app_web/router.ex", source);
    let resources = facts_with_pattern(&results, PHOENIX_RESOURCE_ROUTE_PATTERN_ID);
    assert_eq!(resources.len(), 1, "{resources:#?}");
    let photos = resources[0];
    assert_eq!(metadata_str(photos, "framework"), Some("phoenix"));
    assert_eq!(metadata_str(photos, "api_style"), Some("resource_routing"));
    assert_eq!(metadata_str(photos, "resource_path"), Some("/photos"));
    assert_eq!(
        metadata_str(photos, "normalized_resource_path"),
        Some("/api/photos")
    );
    assert_eq!(metadata_str(photos, "controller"), Some("PhotoController"));
    assert_eq!(metadata_str(photos, "route_group_prefix"), Some("/api"));
}

#[test]
fn phoenix_forward_emits_mount_family_fact() {
    let source = r#"defmodule MyAppWeb.Router do
  use Phoenix.Router

  scope "/api" do
    forward "/health", HealthPlug
  end
end
"#;
    let results = extract("lib/my_app_web/router.ex", source);
    let forwards = facts_with_pattern(&results, PHOENIX_FORWARD_PATTERN_ID);
    assert_eq!(forwards.len(), 1, "{forwards:#?}");
    let forward = forwards[0];
    assert_eq!(metadata_str(forward, "mount_path"), Some("/health"));
    assert_eq!(
        metadata_str(forward, "normalized_mount_path"),
        Some("/api/health")
    );
    assert_eq!(metadata_str(forward, "mount_target"), Some("HealthPlug"));
    assert_eq!(metadata_str(forward, "route_group_prefix"), Some("/api"));
}

#[test]
fn phoenix_dynamic_paths_stay_silent() {
    // Interpolated, concatenated, ~r regex-sigil, and module-attribute paths
    // must all emit nothing (M2 silence).
    let source = r#"defmodule MyAppWeb.Router do
  use Phoenix.Router

  get "/u/#{tenant}", UserController, :show
  get "/u/" <> suffix, UserController, :show
  get ~r"/regex", UserController, :show
  get @dynamic_path, UserController, :show
  get ~S"/u/#{tenant}", UserController, :show
end
"#;
    let results = extract("lib/my_app_web/router.ex", source);
    assert!(
        facts_with_pattern(&results, PHOENIX_ROUTE_PATTERN_ID).is_empty(),
        "dynamic route args must stay silent: {:#?}",
        facts_with_pattern(&results, PHOENIX_ROUTE_PATTERN_ID)
    );
}

#[test]
fn phoenix_interpolated_scope_poisons_but_routes_still_emit_own_template() {
    let source = r#"defmodule MyAppWeb.Router do
  use Phoenix.Router

  scope "/#{tenant}" do
    get "/dashboard", DashController, :index
  end
end
"#;
    let results = extract("lib/my_app_web/router.ex", source);
    let routes = facts_with_pattern(&results, PHOENIX_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    // Poisoned scope → no group prefix / effective template, own template only.
    assert_eq!(metadata_str(routes[0], "route_group_prefix"), None);
    assert_eq!(metadata_str(routes[0], "effective_route_template"), None);
    assert_eq!(
        metadata_str(routes[0], "route_template"),
        Some("/dashboard")
    );
    assert_eq!(
        metadata_str(routes[0], "normalized_route_template"),
        Some("/dashboard")
    );
}

#[test]
fn phoenix_excluded_macros_stay_silent() {
    // pipe_through / live / socket / channel are documented exclusions.
    let source = r#"defmodule MyAppWeb.Router do
  use Phoenix.Router

  scope "/api" do
    pipe_through :browser
    live "/dashboard", DashboardLive
    forward "/health", HealthPlug
  end

  socket "/socket", UserSocket
  channel "room:*", RoomChannel
end
"#;
    let results = extract("lib/my_app_web/router.ex", source);
    // Only forward emits (a mount fact); no route/resource facts from the
    // excluded macros.
    assert!(
        facts_with_pattern(&results, PHOENIX_ROUTE_PATTERN_ID).is_empty(),
        "no route facts from excluded macros"
    );
    assert!(
        facts_with_pattern(&results, PHOENIX_RESOURCE_ROUTE_PATTERN_ID).is_empty(),
        "no resource facts from excluded macros"
    );
    assert_eq!(
        facts_with_pattern(&results, PHOENIX_FORWARD_PATTERN_ID).len(),
        1,
        "forward still emits"
    );
}

#[test]
fn phoenix_use_myappweb_router_gate() {
    // The generated `use MyAppWeb, :router` form (no literal `Phoenix.Router`)
    // still passes the import gate via the `:router` marker.
    let source = r#"defmodule MyAppWeb.Router do
  use MyAppWeb, :router

  get "/status", StatusController, :index
end
"#;
    let results = extract("lib/my_app_web/router.ex", source);
    let routes = facts_with_pattern(&results, PHOENIX_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(
        metadata_str(routes[0], "normalized_route_template"),
        Some("/status")
    );
}

#[test]
fn phoenix_verb_named_function_head_is_not_a_route() {
    // Even in a router file (gate passes), a helper `def get("/x")` parses its
    // `get("/x")` head as a nested call under the def's `arguments`, not as a
    // block statement — it must not be mistaken for a route (M2 silence).
    let source = r#"defmodule MyAppWeb.Router do
  use Phoenix.Router

  get "/real", RealController, :index

  def get("/decoy"), do: :not_a_route
end
"#;
    let results = extract("lib/my_app_web/router.ex", source);
    let routes = facts_with_pattern(&results, PHOENIX_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "only the real route emits: {routes:#?}");
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/real"));
}

#[test]
fn phoenix_requires_router_gate() {
    // A non-router Elixir module (no Phoenix.Router / :router marker) registers
    // no routes, even if it happens to have a bare `get` call.
    let source = r#"defmodule MyApp.Cache do
  def get("/x"), do: :ok
end
"#;
    let results = extract("lib/my_app/cache.ex", source);
    assert!(
        facts_with_pattern(&results, PHOENIX_ROUTE_PATTERN_ID).is_empty(),
        "non-router file must register no routes"
    );
}
