use axum::{routing::get, Router};

fn routes() {
    Router::new()
        .without_v07_checks()
        .route("/:literal", get(handler))
        .route("/users/{id}", get(handler));
}

async fn handler() {}
