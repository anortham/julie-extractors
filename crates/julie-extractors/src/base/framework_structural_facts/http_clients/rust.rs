//! Rust HTTP client-request facts (`http.client_request.v1`) for the reqwest
//! client.
//!
//! reqwest is the dominant Rust HTTP client and has two clean grammar shapes the
//! shipped Go/Java collectors already prove:
//!
//! - the scoped convenience free function `reqwest::get("https://…")` — a
//!   `call_expression` whose function is a `scoped_identifier` (`reqwest` `::`
//!   `get`); and
//! - the builder verb `client.get("https://…")` /
//!   `reqwest::Client::new().get("…")` — a `receiver.verb("url")` call, the same
//!   `field_expression`-callee shape the Ktor and Go collectors detect.
//!
//! Silence (design §4.4, M2): only a lone static string literal URL (via the
//! shared Rust static guard) produces a fact; `format!(...)`, concatenated, and
//! variable URLs emit nothing. The `reqwest` import gate keeps the match from
//! firing outside a reqwest file.
//!
//! Collision guard: Rust's `HashMap::get(&str)` shares the bare `x.get("k")`
//! shape. For the receiver form only, the URL must be *url-like* (absolute
//! `scheme://…` or a `/`-rooted path) so a map lookup `map.get("key")` (a
//! `relative` literal) stays silent. reqwest requests in practice use absolute or
//! `/`-rooted URLs, so this trims false positives at negligible recall cost (M2 —
//! a false positive is worse than a miss). The scoped `reqwest::get(...)` form is
//! unambiguous and needs no such guard.
//!
//! Deferred as documented `open_gaps` (each an unlike-any-shipped detection shape
//! plus its own fixture): hyper (low-level, request-builder + body-future) and
//! ureq (blocking `ureq::get("url").call()`).

use tree_sitter::{Node, Tree};

use super::super::helpers::node_text;
use super::super::static_arg::{StaticArgLang, static_route_arg};
use super::client_fact;
use crate::base::http_boundary::classify_url;
use crate::base::types::StructuralFact;

/// Import gate: a reqwest request goes through the `reqwest` crate, so a file
/// that never names it issues no reqwest requests. Precision comes from the exact
/// `reqwest`-scoped / verb match below; this is the fast bail.
const IMPORT_NEEDLE: &str = "reqwest";

/// The reqwest request verb methods this collector recognises.
fn verb_for_method(method: &str) -> Option<&'static str> {
    match method {
        "get" => Some("GET"),
        "post" => Some("POST"),
        "put" => Some("PUT"),
        "patch" => Some("PATCH"),
        "delete" => Some("DELETE"),
        "head" => Some("HEAD"),
        _ => None,
    }
}

pub(super) fn collect_rust_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains(IMPORT_NEEDLE) {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(tree.root_node(), language, tree, file_path, content, &mut facts);
    facts
}

fn walk(
    node: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if node.kind() == "call_expression"
        && let Some(fact) = client_request_fact(node, language, tree, file_path, content)
    {
        facts.push(fact);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, language, tree, file_path, content, facts);
    }
}

/// Build a `http.client_request.v1` fact for a reqwest verb call, or `None` when
/// the call is not a recognised reqwest verb call with a static URL.
fn client_request_fact(
    call: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let function = call.child_by_field_name("function")?;
    let (verb, require_url_like) = match function.kind() {
        // `reqwest::get("url")` — scoped convenience free function. The path must
        // end in the `reqwest` alias so `Foo::get(...)` on another type is not a
        // request.
        "scoped_identifier" => {
            if !scoped_path_is_reqwest(function, content) {
                return None;
            }
            let method = node_text(content, function.child_by_field_name("name")?)?;
            (verb_for_method(method)?, false)
        }
        // `client.get("url")` / `reqwest::Client::new().get("url")` — builder
        // verb. Import-gated + url-like guard against `HashMap::get("key")`.
        "field_expression" => {
            let method = node_text(content, function.child_by_field_name("field")?)?;
            (verb_for_method(method)?, true)
        }
        _ => return None,
    };

    let url_argument = first_positional_arg(call)?;
    let target_path = static_route_arg(url_argument, content, StaticArgLang::Rust)?;
    if require_url_like && classify_url(target_path) == "relative" {
        return None;
    }

    client_fact(
        language,
        tree,
        file_path,
        content,
        call.start_byte(),
        call.end_byte(),
        "reqwest",
        target_path,
        verb,
        "attested",
        None,
    )
}

/// Whether a `scoped_identifier`'s path is (or ends in) the `reqwest` crate
/// alias: `reqwest::get` (path is `identifier "reqwest"`).
fn scoped_path_is_reqwest(scoped: Node, content: &str) -> bool {
    let Some(path) = scoped.child_by_field_name("path") else {
        return false;
    };
    match path.kind() {
        "identifier" => node_text(content, path) == Some("reqwest"),
        // `some::reqwest::get` — path ends in `reqwest`.
        "scoped_identifier" => path
            .child_by_field_name("name")
            .and_then(|name| node_text(content, name))
            == Some("reqwest"),
        _ => false,
    }
}

/// The first positional argument value of a `call_expression` — the first named
/// child of its `arguments` node.
fn first_positional_arg(call: Node) -> Option<Node> {
    let mut cursor = call.walk();
    let arguments = call
        .children(&mut cursor)
        .find(|child| child.kind() == "arguments")?;
    let mut arg_cursor = arguments.walk();
    arguments.named_children(&mut arg_cursor).next()
}
