//! Laravel facade-route structural facts (`laravel.route.v1`,
//! `laravel.resource_route.v1`, `laravel.route_prefix.v1`).

use std::path::Path;

use crate::base::StructuralFact;

const LARAVEL_ROUTE_PATTERN_ID: &str = "laravel.route.v1";
const LARAVEL_RESOURCE_ROUTE_PATTERN_ID: &str = "laravel.resource_route.v1";
const LARAVEL_ROUTE_PREFIX_PATTERN_ID: &str = "laravel.route_prefix.v1";

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

fn find_route<'a>(facts: &[&'a StructuralFact], route_template: &str) -> &'a StructuralFact {
    facts
        .iter()
        .copied()
        .find(|fact| metadata_str(fact, "route_template") == Some(route_template))
        .unwrap_or_else(|| panic!("route_template {route_template:?} not found in {facts:#?}"))
}

#[test]
fn laravel_verb_routes_carry_verb_template_and_controller_action() {
    let source = r#"<?php
use Illuminate\Support\Facades\Route;

Route::get('/users/{id}', [UserController::class, 'show']);
Route::post('/users', [UserController::class, 'store']);
Route::delete('/users/{id}', [UserController::class, 'destroy']);
Route::any('/webhook', [WebhookController::class, 'handle']);
"#;
    let results = extract("routes/web.php", source);
    let routes = facts_with_pattern(&results, LARAVEL_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 4, "{routes:#?}");

    let show = find_route(&routes, "/users/{id}");
    assert_eq!(metadata_str(show, "framework"), Some("laravel"));
    assert_eq!(metadata_str(show, "api_style"), Some("call_routing"));
    assert_eq!(metadata_str(show, "verb"), Some("GET"));
    assert_eq!(metadata_str(show, "verb_source"), Some("attested"));
    assert_eq!(
        metadata_str(show, "normalized_route_template"),
        Some("/users/:id")
    );
    assert_eq!(metadata_array(show, "dynamic_segments"), vec!["id"]);
    assert_eq!(
        metadata_str(show, "controller_action"),
        Some("UserController@show")
    );
    // Top-level route: no enclosing handler symbol (the controller lives
    // cross-file), so the binding is honestly None — unlike Spring/NestJS where
    // the route sits inside its handler method.
    assert_eq!(show.containing_symbol_id, None);

    let store = routes
        .iter()
        .find(|fact| metadata_str(fact, "verb") == Some("POST"))
        .expect("post route");
    assert_eq!(
        metadata_str(store, "normalized_route_template"),
        Some("/users")
    );

    // Route::any is not verb-restricted → verb omitted.
    let webhook = find_route(&routes, "/webhook");
    assert_eq!(metadata_str(webhook, "verb"), None);
    assert_eq!(metadata_str(webhook, "verb_source"), None);
}

#[test]
fn laravel_match_emits_one_route_per_static_verb() {
    let source = r#"<?php
use Illuminate\Support\Facades\Route;

Route::match(['get', 'post'], '/search', [SearchController::class, 'index']);
"#;
    let results = extract("routes/web.php", source);
    let routes = facts_with_pattern(&results, LARAVEL_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 2, "{routes:#?}");
    let mut verbs: Vec<&str> = routes
        .iter()
        .filter_map(|fact| metadata_str(fact, "verb"))
        .collect();
    verbs.sort_unstable();
    assert_eq!(verbs, vec!["GET", "POST"]);
    for fact in &routes {
        assert_eq!(metadata_str(fact, "route_template"), Some("/search"));
        assert_eq!(
            metadata_str(fact, "normalized_route_template"),
            Some("/search")
        );
    }
}

#[test]
fn laravel_chain_prefix_group_joins_and_emits_prefix_fact() {
    let source = r#"<?php
use Illuminate\Support\Facades\Route;

Route::prefix('admin')->group(function () {
    Route::get('/users/{id}', [AdminController::class, 'show']);
    Route::post('/users', [AdminController::class, 'store']);
});
"#;
    let results = extract("routes/web.php", source);
    let routes = facts_with_pattern(&results, LARAVEL_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 2, "{routes:#?}");

    let show = find_route(&routes, "/users/{id}");
    assert_eq!(metadata_str(show, "route_group_prefix"), Some("/admin"));
    assert_eq!(
        metadata_str(show, "effective_route_template"),
        Some("/admin/users/{id}")
    );
    assert_eq!(
        metadata_str(show, "normalized_route_template"),
        Some("/admin/users/:id")
    );

    // The prefix emits its own mount-family fact at the Route::prefix() site.
    let prefixes = facts_with_pattern(&results, LARAVEL_ROUTE_PREFIX_PATTERN_ID);
    assert_eq!(prefixes.len(), 1, "{prefixes:#?}");
    assert_eq!(metadata_str(prefixes[0], "mount_path"), Some("admin"));
    assert_eq!(
        metadata_str(prefixes[0], "normalized_mount_path"),
        Some("/admin")
    );
}

#[test]
fn laravel_array_config_prefix_group_joins_and_emits_prefix_fact() {
    let source = r#"<?php
use Illuminate\Support\Facades\Route;

Route::group(['prefix' => 'api', 'middleware' => 'auth'], function () {
    Route::get('/status', [StatusController::class, 'index']);
});
"#;
    let results = extract("routes/web.php", source);
    let status = find_route(
        &facts_with_pattern(&results, LARAVEL_ROUTE_PATTERN_ID),
        "/status",
    );
    assert_eq!(metadata_str(status, "route_group_prefix"), Some("/api"));
    assert_eq!(
        metadata_str(status, "normalized_route_template"),
        Some("/api/status")
    );

    let prefixes = facts_with_pattern(&results, LARAVEL_ROUTE_PREFIX_PATTERN_ID);
    assert_eq!(prefixes.len(), 1, "{prefixes:#?}");
    assert_eq!(metadata_str(prefixes[0], "mount_path"), Some("api"));
    assert_eq!(
        metadata_str(prefixes[0], "normalized_mount_path"),
        Some("/api")
    );
}

#[test]
fn laravel_nested_prefix_groups_accumulate() {
    let source = r#"<?php
use Illuminate\Support\Facades\Route;

Route::prefix('admin')->group(function () {
    Route::prefix('users')->group(function () {
        Route::get('/{id}', [AdminUserController::class, 'show']);
    });
});
"#;
    let results = extract("routes/web.php", source);
    let show = find_route(
        &facts_with_pattern(&results, LARAVEL_ROUTE_PATTERN_ID),
        "/{id}",
    );
    assert_eq!(
        metadata_str(show, "route_group_prefix"),
        Some("/admin/users")
    );
    assert_eq!(
        metadata_str(show, "normalized_route_template"),
        Some("/admin/users/:id")
    );

    // Inner prefix fact records its own literal plus the accumulated scope.
    let prefixes = facts_with_pattern(&results, LARAVEL_ROUTE_PREFIX_PATTERN_ID);
    let inner = prefixes
        .iter()
        .find(|fact| metadata_str(fact, "mount_path") == Some("users"))
        .expect("inner prefix fact");
    assert_eq!(
        metadata_str(inner, "normalized_mount_path"),
        Some("/admin/users")
    );
}

#[test]
fn laravel_resource_and_api_resource_routes() {
    let source = r#"<?php
use Illuminate\Support\Facades\Route;

Route::resource('photos', PhotoController::class);
Route::apiResource('books', BookController::class);
"#;
    let results = extract("routes/web.php", source);
    let resources = facts_with_pattern(&results, LARAVEL_RESOURCE_ROUTE_PATTERN_ID);
    assert_eq!(resources.len(), 2, "{resources:#?}");

    let photos = resources
        .iter()
        .find(|fact| metadata_str(fact, "resource_name") == Some("photos"))
        .expect("photos resource");
    assert_eq!(metadata_str(photos, "api_style"), Some("resource_routing"));
    assert_eq!(metadata_str(photos, "resource_kind"), Some("resource"));
    assert_eq!(metadata_str(photos, "controller"), Some("PhotoController"));

    let books = resources
        .iter()
        .find(|fact| metadata_str(fact, "resource_name") == Some("books"))
        .expect("books resource");
    assert_eq!(metadata_str(books, "resource_kind"), Some("api_resource"));
    assert_eq!(metadata_str(books, "controller"), Some("BookController"));
}

#[test]
fn laravel_dynamic_paths_stay_silent() {
    let source = r#"<?php
use Illuminate\Support\Facades\Route;

Route::get("/users/$id", [UserController::class, 'show']);
Route::get('/users/' . $id, [UserController::class, 'show']);
Route::get(self::PREFIX, [UserController::class, 'index']);
Route::get($path, [UserController::class, 'index']);
Route::match([$verb], '/x', [C::class, 'm']);
"#;
    let results = extract("routes/web.php", source);
    assert!(
        facts_with_pattern(&results, LARAVEL_ROUTE_PATTERN_ID).is_empty(),
        "dynamic route args must stay silent: {:#?}",
        facts_with_pattern(&results, LARAVEL_ROUTE_PATTERN_ID)
    );
}

#[test]
fn laravel_dynamic_prefix_poisons_group_but_routes_still_emit_own_template() {
    let source = r#"<?php
use Illuminate\Support\Facades\Route;

Route::prefix($tenant)->group(function () {
    Route::get('/dashboard', [DashController::class, 'index']);
});
"#;
    let results = extract("routes/web.php", source);
    // No prefix fact for a non-literal prefix (M2 silence).
    assert!(
        facts_with_pattern(&results, LARAVEL_ROUTE_PREFIX_PATTERN_ID).is_empty(),
        "dynamic prefix must not emit a route_prefix fact"
    );
    // The contained route still emits, but with no group prefix (poisoned).
    let routes = facts_with_pattern(&results, LARAVEL_ROUTE_PATTERN_ID);
    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/dashboard"));
    assert_eq!(metadata_str(routes[0], "route_group_prefix"), None);
    assert_eq!(metadata_str(routes[0], "effective_route_template"), None);
    assert_eq!(
        metadata_str(routes[0], "normalized_route_template"),
        Some("/dashboard")
    );
}

#[test]
fn laravel_requires_route_facade() {
    // A PHP file that never references the Route facade registers no routes.
    let source = r#"<?php
class UserController {
    public function show($id) {
        return $id;
    }
}
"#;
    let results = extract("app/Http/Controllers/UserController.php", source);
    assert!(facts_with_pattern(&results, LARAVEL_ROUTE_PATTERN_ID).is_empty());
}

#[test]
fn laravel_symfony_route_attributes_stay_silent() {
    // PHP `#[Route]` attributes are a Symfony idiom (future symfony.route.v1),
    // not Laravel — documented open_gap, emits nothing here.
    let source = r#"<?php
use Symfony\Component\Routing\Annotation\Route;

class UserController {
    #[Route('/users/{id}', methods: ['GET'])]
    public function show($id) {
        return $id;
    }
}
"#;
    let results = extract("src/Controller/UserController.php", source);
    assert!(
        facts_with_pattern(&results, LARAVEL_ROUTE_PATTERN_ID).is_empty(),
        "Symfony #[Route] attributes are out of scope for laravel.route.v1"
    );
}
