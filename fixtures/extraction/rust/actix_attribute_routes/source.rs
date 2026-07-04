// actix-web attribute-macro route fixture: static `#[get]`/`#[post]`/`#[route]`
// registrations plus silent dynamic cases. Backs the `actix.attribute_route.v1`
// capability row.
use actix_web::{delete, get, post, route, web, HttpResponse, Responder};

/// A braced-param GET handler → verb GET, `{id}` normalizes to `:id`.
#[get("/users/{id}")]
async fn show(path: web::Path<u32>) -> impl Responder {
    HttpResponse::Ok().json(path.into_inner())
}

/// A POST handler with a plain static path.
#[post("/users")]
async fn create() -> impl Responder {
    HttpResponse::Created().finish()
}

/// A DELETE handler decorated with an extra (non-route) attribute: the route
/// fact still binds to the handler fn, skipping intervening attributes.
#[delete("/users/{id}")]
#[allow(unused_variables)]
async fn destroy(path: web::Path<u32>) -> impl Responder {
    HttpResponse::NoContent().finish()
}

/// A `#[route]` macro with two methods → one fact per verb (GET and POST).
#[route("/thing", method = "GET", method = "POST")]
async fn thing() -> impl Responder {
    HttpResponse::Ok().finish()
}

// Attribute routes also bind to handler methods inside an impl block.
struct Api;

impl Api {
    #[get("/health")]
    async fn health(&self) -> impl Responder {
        HttpResponse::Ok().finish()
    }
}

// A `const`-referenced macro argument is not a plain literal, so it stays silent (M2).
const USERS: &str = "/users";

#[get(USERS)]
async fn list() -> impl Responder {
    HttpResponse::Ok().finish()
}
