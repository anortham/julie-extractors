//! Kotlin HTTP client-request facts (`http.client_request.v1`) for the Ktor
//! client.
//!
//! Ktor is the dominant Kotlin HTTP client and has the cleanest grammar shape:
//! `client.get("https://…")` parses as a `call_expression` whose `function` is a
//! `navigation_expression` (`client` `.` `get`) and whose first `value_argument`
//! is the URL string — the same receiver-`.verb("url")` shape the shipped Go and
//! Java client collectors detect. (OkHttp's fluent `Request.Builder().url("…")`
//! chain is unlike any shipped collector and is deferred — recorded as a kotlin
//! `open_gaps` entry.)
//!
//! Silence (design §4.4, M2): only a lone static string literal URL (via the
//! shared Kotlin static guard) produces a fact; interpolated / concatenated /
//! variable URLs emit nothing. The `io.ktor.client` import gate keeps the
//! receiver-agnostic `.verb("…")` match from firing outside a Ktor file.

use tree_sitter::{Node, Tree};

use super::super::helpers::node_text;
use super::super::static_arg::{StaticArgLang, static_route_arg};
use super::client_fact;
use crate::base::types::StructuralFact;

/// Import gate: the Ktor client verb functions (`get`/`post`/…) come from
/// `io.ktor.client.request`; the whole client lives under `io.ktor.client`.
const IMPORT_NEEDLE: &str = "io.ktor.client";

/// The Ktor request verb functions this collector recognises as a
/// `receiver.verb("url")` call.
fn verb_for_method(method: &str) -> Option<&'static str> {
    match method {
        "get" => Some("GET"),
        "post" => Some("POST"),
        "put" => Some("PUT"),
        "patch" => Some("PATCH"),
        "delete" => Some("DELETE"),
        "head" => Some("HEAD"),
        "options" => Some("OPTIONS"),
        _ => None,
    }
}

pub(super) fn collect_kotlin_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains(IMPORT_NEEDLE) {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        language,
        tree,
        file_path,
        content,
        &mut facts,
    );
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

/// Build a `http.client_request.v1` fact for a `receiver.verb("url")` Ktor call,
/// or `None` when the call is not a recognised verb call with a static URL.
fn client_request_fact(
    call: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    // Only a `navigation_expression` callee (`client.get`) — a bare
    // `simple_identifier` callee is the server-side routing DSL (`get("/x")`),
    // not a client request.
    let callee = call
        .child_by_field_name("function")
        .or_else(|| first_child(call))?;
    if callee.kind() != "navigation_expression" {
        return None;
    }
    let method = last_identifier_text(callee, content)?;
    let verb = verb_for_method(method)?;

    let value_arguments = child_of_kind(call, "value_arguments")?;
    let url_argument = first_named_argument_value(value_arguments)?;
    let target_path = static_route_arg(url_argument, content, StaticArgLang::Kotlin)?;

    client_fact(
        language,
        tree,
        file_path,
        content,
        call.start_byte(),
        call.end_byte(),
        "ktor",
        target_path,
        verb,
        "attested",
        None,
    )
}

/// The value node of the first positional `value_argument`.
fn first_named_argument_value(value_arguments: Node) -> Option<Node> {
    let mut cursor = value_arguments.walk();
    for value_argument in value_arguments.children(&mut cursor) {
        if value_argument.kind() != "value_argument" {
            continue;
        }
        let mut arg_cursor = value_argument.walk();
        return value_argument
            .children(&mut arg_cursor)
            .find(|child| child.is_named());
    }
    None
}

fn last_identifier_text<'a>(node: Node, content: &'a str) -> Option<&'a str> {
    let mut cursor = node.walk();
    let mut last = None;
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            last = Some(child);
        }
    }
    last.and_then(|child| node_text(content, child))
}

fn child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn first_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor).next()
}
