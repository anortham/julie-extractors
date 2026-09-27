use std::path::Path;

use julie_extractors::{ExtractionResults, StructuralFact, extract_canonical};

const ACTIX_SCOPE_ROUTE: &str = "actix.scope_route.v1";
const ACTIX_RESOURCE_ROUTE: &str = "actix.resource_route.v1";
const ACTIX_MOUNT: &str = "actix.mount.v1";
const HTTP_CLIENT_REQUEST: &str = "http.client_request.v1";

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/repo")).expect("extraction should succeed")
}

fn facts<'a>(results: &'a ExtractionResults, pattern: &str) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern)
        .collect()
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata.as_ref()?.get(key)?.as_str()
}

#[test]
fn actix_local_scope_routes_keep_their_prefix() {
    let results = extract(
        "src/main.rs",
        r#"use actix_web::web;

fn configure(prefix: &str) {
    let scope = web::scope("/api");
    scope
        .route("/users", web::post().to(create))
        .route("/items", web::get().to(list));
    let dynamic = web::scope(prefix);
    dynamic.route("/unknown", web::get().to(unknown));
}
"#,
    );
    let routes = facts(&results, ACTIX_SCOPE_ROUTE);

    assert_eq!(routes.len(), 2, "{routes:#?}");
    assert!(routes.iter().any(|fact| {
        metadata_str(fact, "route_group_prefix") == Some("/api")
            && metadata_str(fact, "effective_route_template") == Some("/api/users")
    }));
    assert!(routes.iter().any(|fact| {
        metadata_str(fact, "route_group_prefix") == Some("/api")
            && metadata_str(fact, "effective_route_template") == Some("/api/items")
    }));
}

#[test]
fn actix_local_scope_bindings_respect_shadowing() {
    let results = extract(
        "src/main.rs",
        r#"use actix_web::web;

fn configure() {
    let scope = web::scope("/outer");
    {
        let scope = web::scope("/inner");
        scope.route("/inside", web::get().to(inside));
    }
    scope.route("/outside", web::get().to(outside));
}
"#,
    );
    let routes = facts(&results, ACTIX_SCOPE_ROUTE);

    assert_eq!(routes.len(), 2, "{routes:#?}");
    assert!(
        routes.iter().any(|fact| {
            metadata_str(fact, "effective_route_template") == Some("/inner/inside")
        })
    );
    assert!(
        routes.iter().any(|fact| {
            metadata_str(fact, "effective_route_template") == Some("/outer/outside")
        })
    );
}

#[test]
fn actix_branch_reassignment_does_not_keep_a_stale_prefix() {
    let results = extract(
        "src/main.rs",
        r#"use actix_web::web;

fn configure(prefix: &str, branch: bool) {
    let fixed = web::scope("/fixed");
    fixed.route("/health", web::get().to(health));
    let mut scope = web::scope("/api");
    if branch {
        scope = web::scope(prefix);
    }
    scope.route("/users", web::get().to(users));
}
"#,
    );

    let routes = facts(&results, ACTIX_SCOPE_ROUTE);

    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(
        metadata_str(routes[0], "effective_route_template"),
        Some("/fixed/health")
    );
}

#[test]
fn actix_service_config_routes_stay_local_to_the_configurer() {
    let results = extract(
        "src/main.rs",
        r#"use actix_web::{web, App};

fn configure(config: &mut web::ServiceConfig) {
    config.route("/status", web::get().to(status));
}

struct OtherConfig;

fn ignore_other(config: &mut OtherConfig) {
    config.route("/not-actix", web::get().to(not_actix));
}

fn app() {
    App::new()
        .service(web::scope("/api").configure(configure))
        .service(web::scope("/v2").configure(configure));
}
"#,
    );
    let routes = facts(&results, ACTIX_SCOPE_ROUTE);
    let mounts = facts(&results, ACTIX_MOUNT);

    assert_eq!(routes.len(), 1, "{routes:#?}");
    assert_eq!(metadata_str(routes[0], "route_template"), Some("/status"));
    assert_eq!(metadata_str(routes[0], "route_group_prefix"), None);
    assert_eq!(metadata_str(routes[0], "effective_route_template"), None);
    assert_eq!(mounts.len(), 2, "{mounts:#?}");
    assert!(
        mounts
            .iter()
            .any(|fact| metadata_str(fact, "mount_path") == Some("/api"))
    );
    assert!(
        mounts
            .iter()
            .any(|fact| metadata_str(fact, "mount_path") == Some("/v2"))
    );
}

#[test]
fn actix_resource_routes_emit_attested_verbs_without_duplicates() {
    let results = extract(
        "src/main.rs",
        r#"use actix_web::{guard, web, App};

fn configure() {
    App::new()
        .service(web::resource("/resource-guard").guard(guard::Post()).route(web::route().to(create)))
        .service(web::resource("/route-guard").route(web::route().guard(guard::Delete()).to(remove)))
        .service(web::resource("/header-guard").guard(guard::Header("x-mode", "fast")).route(web::route().to(read)));
}
"#,
    );
    let routes = facts(&results, ACTIX_RESOURCE_ROUTE);

    assert_eq!(routes.len(), 3, "{routes:#?}");
    assert!(facts(&results, ACTIX_SCOPE_ROUTE).is_empty());
    assert!(routes.iter().any(|fact| {
        metadata_str(fact, "route_template") == Some("/resource-guard")
            && metadata_str(fact, "verb") == Some("POST")
    }));
    assert!(routes.iter().any(|fact| {
        metadata_str(fact, "route_template") == Some("/route-guard")
            && metadata_str(fact, "verb") == Some("DELETE")
    }));
    assert!(routes.iter().any(|fact| {
        metadata_str(fact, "route_template") == Some("/header-guard")
            && metadata_str(fact, "verb").is_none()
    }));
}

#[test]
fn actix_resource_routes_keep_literal_paths_when_scope_prefixes_are_unknown() {
    let results = extract(
        "src/main.rs",
        r#"use actix_web::{web, App};

fn configure(prefix: &str) {
    App::new()
        .service(web::scope("/api").service(web::scope("/v2").service(web::resource("/users").route(web::get().to(users)))))
        .service(web::scope(prefix).service(web::resource("/dynamic").route(web::get().to(dynamic))));
}
"#,
    );
    let routes = facts(&results, ACTIX_RESOURCE_ROUTE);

    assert_eq!(routes.len(), 2, "{routes:#?}");
    let users = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/users"))
        .unwrap();
    assert_eq!(metadata_str(users, "route_group_prefix"), Some("/api/v2"));
    assert_eq!(
        metadata_str(users, "effective_route_template"),
        Some("/api/v2/users")
    );
    let dynamic = routes
        .iter()
        .find(|fact| metadata_str(fact, "route_template") == Some("/dynamic"))
        .unwrap();
    assert_eq!(metadata_str(dynamic, "verb"), Some("GET"));
    assert!(metadata_str(dynamic, "route_group_prefix").is_none());
    assert!(metadata_str(dynamic, "effective_route_template").is_none());
}

#[test]
fn rust_reqwest_fields_emit_only_with_same_impl_type_evidence() {
    let results = extract(
        "src/client.rs",
        r#"use reqwest::Client;

struct Service {
    client: reqwest::Client,
    bare_client: Client,
}

struct Other {
    client: reqwest::Client,
}

struct NonHttp {
    client: String,
}

fn local_service() {
    struct Service {
        client: String,
    }

    impl Service {
        async fn load(&self) {
            self.client.get("https://example.com/local-owner").await;
        }
    }
}

mod other {
    pub struct Service {
        pub client: String,
    }
}

impl Service {
    async fn load(&self, other: Other) {
        self.client.get("https://example.com/qualified").await;
        self.bare_client.post("https://example.com/bare").await;
        other.client.get("https://example.com/other-object").await;
    }
}

impl NonHttp {
    async fn load(&self) {
        self.client.get("https://example.com/sibling-impl").await;
    }
}

impl other::Service {
    async fn load(&self) {
        self.client.get("https://example.com/qualified-owner").await;
    }
}

impl Missing {
    async fn load(&self) {
        self.client.get("https://example.com/unknown-field").await;
    }
}
"#,
    );
    let requests = facts(&results, HTTP_CLIENT_REQUEST);

    assert_eq!(requests.len(), 2, "{requests:#?}");
    assert!(requests.iter().any(|fact| {
        metadata_str(fact, "target_path") == Some("https://example.com/qualified")
            && metadata_str(fact, "verb") == Some("GET")
    }));
    assert!(requests.iter().any(|fact| {
        metadata_str(fact, "target_path") == Some("https://example.com/bare")
            && metadata_str(fact, "verb") == Some("POST")
    }));
}

#[test]
fn rust_reqwest_field_proof_is_local_to_the_struct_import_scope() {
    let results = extract(
        "src/client.rs",
        r#"mod imported {
    use reqwest::Client;

    struct Service {
        client: Client,
    }

    impl Service {
        async fn load(&self) {
            self.client.get("https://example.com/reqwest").await;
        }
    }
}

mod local {
    struct Client;

    struct Service {
        client: Client,
    }

    impl Service {
        async fn load(&self) {
            self.client.get("https://example.com/local").await;
        }
    }
}

mod client_builder_only {
    use reqwest::ClientBuilder;

    struct Client;

    struct Service {
        client: Client,
    }

    impl Service {
        async fn load(&self) {
            self.client.get("https://example.com/builder").await;
        }
    }
}

mod generic_shadow {
    use reqwest::Client;

    struct Service<Client> {
        client: Client,
    }

    impl<T> Service<T> {
        async fn load(&self) {
            self.client.get("https://example.com/generic").await;
        }
    }
}
"#,
    );
    let requests = facts(&results, HTTP_CLIENT_REQUEST);

    assert_eq!(requests.len(), 1, "{requests:#?}");
    assert_eq!(
        metadata_str(requests[0], "target_path"),
        Some("https://example.com/reqwest")
    );
}
