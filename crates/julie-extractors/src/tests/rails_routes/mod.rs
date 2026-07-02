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

    let search_verbs = routes
        .iter()
        .filter(|fact| metadata_str(fact, "route_template") == Some("/search"))
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect::<Vec<_>>();
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
