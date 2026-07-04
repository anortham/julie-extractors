use std::path::Path;

use crate::base::StructuralFact;

const RAILS_ROUTE_PATTERN_ID: &str = "rails.route.v1";
const RAILS_RESOURCE_ROUTE_PATTERN_ID: &str = "rails.resource_route.v1";
const RAILS_MOUNT_PATTERN_ID: &str = "rails.mount.v1";

fn extract(file_path: &str, source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical extraction should succeed")
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
        .map(|values| values.iter().filter_map(|value| value.as_str()).collect())
        .unwrap_or_default()
}

#[test]
fn rails_routes_resources_scopes_and_mounts_emit_boundary_facts() {
    let source = r#"
Rails.application.routes.draw do
  namespace :admin do
    get "/users/:id", to: "users#show", as: :user
    match "/search", via: [:get, :post], to: "search#index"
    resources :accounts, only: [:index, :show]
    mount Sidekiq::Web, at: "/jobs"
  end
  root "home#index"
end
"#;
    let results = extract("config/routes.rb", source);
    let routes = facts_with_pattern(&results, RAILS_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 4, "{routes:#?}");

    let user = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/users/:id"))
        .expect("user route");
    assert_eq!(metadata_str(user, "framework"), Some("rails"));
    assert_eq!(metadata_str(user, "verb"), Some("GET"));
    assert_eq!(metadata_str(user, "scope_path"), Some("/admin"));
    assert_eq!(
        metadata_str(user, "normalized_route_template"),
        Some("/admin/users/:id")
    );
    assert_eq!(metadata_array(user, "dynamic_segments"), vec!["id"]);
    assert_eq!(metadata_str(user, "controller_action"), Some("users#show"));
    assert_eq!(metadata_str(user, "route_name"), Some("user"));

    let mut search_verbs = routes
        .iter()
        .filter(|fact| metadata_str(fact, "route_template") == Some("/search"))
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect::<Vec<_>>();
    search_verbs.sort_unstable();
    assert_eq!(search_verbs, vec!["GET", "POST"]);

    let resources = facts_with_pattern(&results, RAILS_RESOURCE_ROUTE_PATTERN_ID);
    assert_eq!(resources.len(), 1, "{resources:#?}");
    assert_eq!(
        metadata_str(resources[0], "resource_name"),
        Some("accounts")
    );
    assert_eq!(
        metadata_str(resources[0], "resource_kind"),
        Some("collection")
    );
    assert_eq!(metadata_array(resources[0], "only"), vec!["index", "show"]);

    let mounts = facts_with_pattern(&results, RAILS_MOUNT_PATTERN_ID);
    assert_eq!(mounts.len(), 1, "{mounts:#?}");
    assert_eq!(
        metadata_str(mounts[0], "mount_target"),
        Some("Sidekiq::Web")
    );
    assert_eq!(metadata_str(mounts[0], "mount_path"), Some("/jobs"));
    assert_eq!(
        metadata_str(mounts[0], "normalized_mount_path"),
        Some("/admin/jobs")
    );
}

#[test]
fn rails_dynamic_interpolation_stays_silent_and_does_not_poison_nested_scope() {
    let source = r##"
Rails.application.routes.draw do
  get "/#{locale}/users", to: "users#index"
  scope "#{tenant}" do
    get "/dashboard", to: "dash#show"
  end
  get "/health", to: "health#show"
end
"##;
    let results = extract("config/routes.rb", source);
    let routes = facts_with_pattern(&results, RAILS_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/health"));
    assert_eq!(
        metadata_str(routes[0], "normalized_route_template"),
        Some("/health")
    );
}

#[test]
fn rails_scope_path_keyword_and_single_symbol_match_emit_routes() {
    let source = r#"
Rails.application.routes.draw do
  scope path: "/admin" do
    get "/users", to: "users#index"
  end
  match "/legacy", to: "legacy#show", via: :get
end
"#;
    let results = extract("config/routes.rb", source);
    let routes = facts_with_pattern(&results, RAILS_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 2, "{routes:#?}");

    let users = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/users"))
        .expect("scoped users route");
    assert_eq!(metadata_str(users, "scope_path"), Some("/admin"));
    assert_eq!(
        metadata_str(users, "normalized_route_template"),
        Some("/admin/users")
    );

    let legacy = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/legacy"))
        .expect("legacy match route");
    assert_eq!(metadata_str(legacy, "verb"), Some("GET"));
}

#[test]
fn rails_nested_non_scope_blocks_do_not_pop_namespace_scopes() {
    let source = r#"
Rails.application.routes.draw do
  namespace :api do
    resources :posts do
      member do
        get "activate"
      end
    end
    get "health", to: "health#show"
  end
end
"#;
    let results = extract("config/routes.rb", source);
    let routes = facts_with_pattern(&results, RAILS_ROUTE_PATTERN_ID);
    let health = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("health"))
        .expect("health route");
    assert_eq!(metadata_str(health, "scope_path"), Some("/api"));
    assert_eq!(
        metadata_str(health, "normalized_route_template"),
        Some("/api/health")
    );
}

#[test]
fn rails_dsl_outside_the_draw_block_stays_silent() {
    let source = r#"
get "/before", to: "legacy#before"

Rails.application.routes.draw do
  get "/inside", to: "pages#inside"
end

get "/after", to: "legacy#after"
"#;
    let results = extract("config/routes.rb", source);
    let routes = facts_with_pattern(&results, RAILS_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/inside"));
}

#[test]
fn rails_split_route_files_emit_top_level_dsl() {
    let source = r#"
namespace :admin do
  get "reports", to: "reports#index"
end
"#;
    let results = extract("config/routes/admin.rb", source);
    let routes = facts_with_pattern(&results, RAILS_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "scope_path"), Some("/admin"));
    assert_eq!(
        metadata_str(routes[0], "normalized_route_template"),
        Some("/admin/reports")
    );
}

#[test]
fn rails_controller_files_stay_silent() {
    let source = r#"
class UsersController < ApplicationController
  def show
    get "not-a-route"
  end
end
"#;
    let results = extract("app/controllers/users_controller.rb", source);
    assert!(facts_with_pattern(&results, RAILS_ROUTE_PATTERN_ID).is_empty());
}
