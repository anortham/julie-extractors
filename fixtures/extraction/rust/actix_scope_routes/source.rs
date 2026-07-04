// actix-web scope-chained route + mount fixture: static `web::scope(...).route`
// registrations (same-file prefix join) and `web::scope(...).configure/service`
// mounts, plus silent dynamic cases. Backs the `actix.scope_route.v1` and
// `actix.mount.v1` capability rows.
use actix_web::{web, App, HttpResponse, Responder};

/// A scope whose routes chain directly off `web::scope("/api")`: each `.route`
/// carries the `/api` prefix (route_group_prefix + effective_route_template),
/// with the verb taken from the `web::<verb>()` method router.
pub fn api_scope() -> App<()> {
    App::new().service(
        web::scope("/api")
            .route("/users/{id}", web::get().to(show))
            .route("/users", web::post().to(create))
            // `web::route()` is method-agnostic → verb omitted.
            .route("/health", web::route().to(health)),
    )
}

/// A scope that delegates to a cross-file `configure` fn → an `actix.mount.v1`
/// recording the `/admin/{tenant}` prefix at its registration site.
pub fn admin_scope() -> App<()> {
    App::new().service(web::scope("/admin/{tenant}").configure(admin_config))
}

/// A scope that mounts a cross-file handler via `.service` → mount fact.
pub fn reports_scope() -> App<()> {
    App::new().service(web::scope("/reports").service(report_index))
}

/// Dynamic cases stay silent (M2): a non-literal scope prefix and a `format!`
/// route path each emit nothing.
pub fn dynamic(prefix: String, id: u32) -> App<()> {
    App::new()
        .service(web::scope(&prefix).route("/x", web::get().to(show)))
        .service(web::scope("/api").route(format!("/u/{id}").as_str(), web::get().to(show)))
}

async fn show() -> impl Responder {
    HttpResponse::Ok().finish()
}
async fn create() -> impl Responder {
    HttpResponse::Created().finish()
}
async fn health() -> impl Responder {
    HttpResponse::Ok().finish()
}
async fn report_index() -> impl Responder {
    HttpResponse::Ok().finish()
}

fn admin_config(_cfg: &mut web::ServiceConfig) {}
