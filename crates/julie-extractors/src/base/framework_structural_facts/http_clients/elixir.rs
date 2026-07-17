//! Elixir HTTP client-request facts (`http.client_request.v1`) for the Req
//! client.
//!
//! Req is the modern dominant Elixir HTTP client and has the cleanest grammar
//! shape: `Req.get("https://…")` parses as a `call` whose `target` is a `dot`
//! (`Req` `.` `get`) and whose first positional argument is the URL string — the
//! same qualified-`Module.verb("url")` shape the shipped Go/Java client
//! collectors detect. Bang variants (`Req.get!("…")`) are the same shape with a
//! trailing `!` on the verb identifier.
//!
//! Silence (design §4.4, M2): only a lone static string literal URL (via the
//! shared Elixir static guard) produces a fact; interpolated / concatenated /
//! `~r` / variable URLs, and the keyword-list form (`Req.get(url: "…")`, which
//! carries no positional string arg0), emit nothing. The `Req.` import gate keeps
//! the match from firing outside a Req file.
//!
//! Deferred as documented `open_gaps` (each an unlike-any-shipped detection shape
//! plus its own fixture): Tesla (`Tesla.get(client, "url")` two-arg + middleware
//! base URLs), HTTPoison (`HTTPoison.get("url")`), Finch (`Finch.build(:get,
//! "url")`), and `:httpc` (Erlang stdlib).

use tree_sitter::{Node, Tree};

use super::super::helpers::{child_of_kind, node_text};
use super::super::static_arg::{StaticArgLang, static_route_arg};
use super::client_fact;
use crate::base::types::StructuralFact;

/// Import gate: a Req request goes through the `Req` module (`Req.get(...)`), so a
/// file with no `Req.` reference issues no Req requests. Precision comes from the
/// exact `Req` alias match on the call target below; this is the fast bail.
const IMPORT_NEEDLE: &str = "Req.";

/// The Req request verb functions this collector recognises as a
/// `Req.verb("url")` call (bang variants share the same verb).
fn verb_for_method(method: &str) -> Option<&'static str> {
    match method.strip_suffix('!').unwrap_or(method) {
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

pub(super) fn collect_elixir_http_client_requests(
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
    if node.kind() == "call"
        && let Some(fact) = client_request_fact(node, language, tree, file_path, content)
    {
        facts.push(fact);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, language, tree, file_path, content, facts);
    }
}

/// Build a `http.client_request.v1` fact for a `Req.verb("url")` call, or `None`
/// when the call is not a recognised Req verb call with a static URL.
fn client_request_fact(
    call: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    // Only a `dot` target whose left is the bare `Req` alias — a bare
    // `identifier` callee (`get "/x"`) is the server-side Phoenix routing DSL,
    // and any other module alias is a different client.
    let target = call.child_by_field_name("target")?;
    if target.kind() != "dot" {
        return None;
    }
    let module = target.child_by_field_name("left")?;
    if module.kind() != "alias" || node_text(content, module)? != "Req" {
        return None;
    }
    let method = node_text(content, target.child_by_field_name("right")?)?;
    let verb = verb_for_method(method)?;

    let arguments = child_of_kind(call, "arguments")?;
    let url_argument = first_positional_arg(arguments)?;
    let target_path = static_route_arg(url_argument, content, StaticArgLang::Elixir)?;

    client_fact(
        language,
        tree,
        file_path,
        content,
        call.start_byte(),
        call.end_byte(),
        "req",
        target_path,
        verb,
        "attested",
        None,
    )
}

/// The first positional argument value of an `arguments` node — its first named
/// child, skipping a leading `keywords` list (the `Req.get(url: "…")` form has
/// no positional string arg0 and stays silent).
fn first_positional_arg(arguments: Node) -> Option<Node> {
    let mut cursor = arguments.walk();
    arguments
        .named_children(&mut cursor)
        .find(|child| child.kind() != "keywords")
}
