//! Scala HTTP client-request facts (`http.client_request.v1`).
//!
//! - requests-scala: `requests.get("…")` and the other verb functions.
//! - sttp: `basicRequest.get(uri"…")` (also `quickRequest`, `emptyRequest`),
//!   gated on an `sttp.client` import.
//! - Play WS: `ws.url("…").get()`, gated on a `play.api.libs.ws` import, where
//!   `ws` is declared in the file with type `WSClient`.
//!
//! Only a static URL produces a fact: a plain string, or a `uri"…"` string
//! with no interpolation.

use std::collections::HashSet;

use tree_sitter::{Node, Tree};

use super::super::helpers::node_text;
use super::{client_fact, verb_for_lower_method};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const STTP_REQUEST_BUILDERS: &[&str] = &["basicRequest", "quickRequest", "emptyRequest"];

struct Gates {
    sttp: bool,
    play_ws_clients: HashSet<String>,
}

pub(super) fn collect_scala_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let has_requests = content.contains("requests.");
    let sttp = content.contains("sttp.client");
    let play_ws = content.contains("play.api.libs.ws");
    if !has_requests && !sttp && !play_ws {
        return Vec::new();
    }
    let mut play_ws_clients = HashSet::new();
    if play_ws {
        collect_typed_names(
            tree.root_node(),
            content,
            "WSClient",
            0,
            &mut play_ws_clients,
        );
    }
    let gates = Gates {
        sttp,
        play_ws_clients,
    };
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        &gates,
        language,
        tree,
        file_path,
        content,
        0,
        &mut facts,
    );
    facts
}

#[allow(clippy::too_many_arguments)]
fn walk(
    node: Node,
    gates: &Gates,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "call_expression"
        && let Some((client, verb, url)) = classify(node, gates, content)
        && let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            node.start_byte(),
            node.end_byte(),
            client,
            url,
            verb,
            "attested",
            None,
        )
    {
        facts.push(fact);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk(
            child,
            gates,
            language,
            tree,
            file_path,
            content,
            child_depth,
            facts,
        );
    }
}

/// `(client, verb, url)` for a recognized client call.
fn classify<'a>(
    call: Node,
    gates: &Gates,
    content: &'a str,
) -> Option<(&'static str, &'static str, &'a str)> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "field_expression" {
        return None;
    }
    let method = node_text(content, function.child_by_field_name("field")?)?;
    let verb = verb_for_lower_method(method)?;
    let receiver = function.child_by_field_name("value")?;
    match receiver.kind() {
        "identifier" => {
            let receiver_name = node_text(content, receiver)?;
            let client = if receiver_name == "requests" {
                "requests_scala"
            } else if gates.sttp && STTP_REQUEST_BUILDERS.contains(&receiver_name) {
                "sttp"
            } else {
                return None;
            };
            let url = static_url(content, first_argument(call)?, client == "sttp")?;
            Some((client, verb, url))
        }
        // `ws.url("…").get()`
        "call_expression" => {
            let url_call = receiver.child_by_field_name("function")?;
            if url_call.kind() != "field_expression"
                || node_text(content, url_call.child_by_field_name("field")?)? != "url"
            {
                return None;
            }
            let client_name = node_text(content, url_call.child_by_field_name("value")?)?;
            if !gates.play_ws_clients.contains(client_name) {
                return None;
            }
            let url = static_url(content, first_argument(receiver)?, false)?;
            Some(("play_ws", verb, url))
        }
        _ => None,
    }
}

fn first_argument(call: Node) -> Option<Node> {
    let arguments = call.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    arguments.named_children(&mut cursor).next()
}

/// A plain `"…"` string, or with `allow_uri` a `uri"…"` string with no
/// interpolation.
fn static_url<'a>(content: &'a str, node: Node, allow_uri: bool) -> Option<&'a str> {
    let string = match node.kind() {
        "string" => node,
        "interpolated_string_expression" if allow_uri => {
            if node_text(content, node.child_by_field_name("interpolator")?)? != "uri" {
                return None;
            }
            let mut cursor = node.walk();
            let string = node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "interpolated_string")?;
            if string.named_child_count() > 0 {
                return None;
            }
            string
        }
        _ => return None,
    };
    let text = node_text(content, string)?;
    if text.starts_with("\"\"\"") {
        return None;
    }
    text.strip_prefix('"')?
        .strip_suffix('"')
        .filter(|url| !url.is_empty())
}

/// Names declared in the file with exactly `type_name`: parameters, class
/// parameters and `val`/`var` members.
fn collect_typed_names(
    node: Node,
    content: &str,
    type_name: &str,
    depth: u32,
    names: &mut HashSet<String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if matches!(
        node.kind(),
        "parameter" | "class_parameter" | "val_definition" | "var_definition" | "val_declaration"
    ) && let Some(declared) = node.child_by_field_name("type")
        && node_text(content, declared)
            .is_some_and(|text| text == type_name || text.rsplit('.').next() == Some(type_name))
        && let Some(name) = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("pattern"))
            .filter(|name| name.kind() == "identifier")
            .and_then(|name| node_text(content, name))
    {
        names.insert(name.to_string());
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_typed_names(child, content, type_name, child_depth, names);
    }
}
