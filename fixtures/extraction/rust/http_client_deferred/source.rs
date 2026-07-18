// Deferred rust HTTP clients: hyper builder chains and scoped ureq verbs.
// Backs closure of rust.http_client.deferred.
use hyper::{Method, Request};
use ureq;

/// Static hyper and ureq requests emit `http.client_request.v1`.
pub fn load() {
    let _ = Request::builder()
        .method(Method::POST)
        .uri("https://api.example.com/items")
        .body(());
    let _ = hyper::Request::builder().uri("/health").body(());
    let _ = ureq::get("https://api.example.com/users").call();
    let _ = ureq::delete("/users/1").call();
}

/// Dynamic URLs, map lookups, and non-hyper builders stay silent (M2).
pub fn silent(url: &str, map: std::collections::HashMap<&str, &str>) {
    let _ = Request::builder().uri(url).body(());
    let _ = ureq::get(url).call();
    let _ = map.get("https://not-a-request.example");
    let _ = OtherRequest::builder()
        .uri("https://not-hyper.example")
        .body(());
}

struct OtherRequest;
impl OtherRequest {
    fn builder() -> OtherBuilder {
        OtherBuilder
    }
}

struct OtherBuilder;
impl OtherBuilder {
    fn uri(self, _: &str) -> Self {
        self
    }
    fn body(self, _: ()) -> Self {
        self
    }
}
