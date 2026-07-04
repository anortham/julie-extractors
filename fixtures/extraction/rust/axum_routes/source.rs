// axum router fixture: static `.route`/`.nest` registrations plus silent
// dynamic cases. Backs the `axum.route.v1` and `axum.nest.v1` capability rows.
use axum::{
    routing::{any, get},
    Router,
};

/// Static routes: a braced-param GET, a chained GET+POST, and an all-method
/// route (`any` → verb omitted), plus a same-file `.nest` mount.
pub fn app() -> Router {
    Router::new()
        .route("/users/{id}", get(show))
        .route("/users", get(list).post(create))
        .route("/health", any(health))
        .nest("/api/{version}", api_routes())
}

/// A `Router` passed in as a parameter (unknown receiver, not poisoned) still
/// registers its routes.
pub fn add_admin(app: Router) -> Router {
    app.route("/admin", get(dashboard))
}

/// Dynamic paths stay silent (M2): `format!`, concatenation, and a `const`
/// reference each emit nothing.
pub fn dynamic(id: u32) -> Router {
    const USERS: &str = "/users";
    Router::new()
        .route(format!("/u/{id}").as_str(), get(a))
        .route(&("/u/".to_owned() + "x"), get(b))
        .route(USERS, get(c))
}

async fn show() {}
async fn list() {}
async fn create() {}
async fn health() {}
async fn dashboard() {}
async fn a() {}
async fn b() {}
async fn c() {}

fn api_routes() -> Router {
    Router::new().route("/status", get(show))
}
