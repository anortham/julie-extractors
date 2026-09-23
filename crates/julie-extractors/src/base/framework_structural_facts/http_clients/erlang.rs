//! Erlang HTTP clients: OTP `httpc`, `hackney`, and `gun`.
//!
//! - `httpc:request(Url)` is a GET; `httpc:request(Method, {Url, ...}, ...)`
//!   names its method atom.
//! - `hackney:request(Method, Url, ...)` names its method atom;
//!   `hackney:request(Url)` is a GET, and `hackney:get(Url, ...)` and its
//!   siblings name the method in the function.
//! - `gun:get(ConnPid, Path, ...)` and its siblings send `Path` on an open
//!   connection, so the target is a path.

use tree_sitter::{Node, Tree};

use super::super::cowboy::{named_children, remote_call, static_term_text};
use super::super::helpers::node_text;
use super::{client_fact, verb_for_lower_method};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const CLIENTS: [&str; 3] = ["httpc", "hackney", "gun"];

pub(super) fn collect_erlang_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !CLIENTS
        .iter()
        .any(|client| content.contains(&format!("{client}:")))
    {
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
        0,
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
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if let Some((client, target, verb, verb_source)) = client_request(node, content) {
        facts.extend(client_fact(
            language,
            tree,
            file_path,
            content,
            node.start_byte(),
            node.end_byte(),
            client,
            &target,
            verb,
            verb_source,
            None,
        ));
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in named_children(node) {
        walk(
            child,
            language,
            tree,
            file_path,
            content,
            facts,
            child_depth,
        );
    }
}

/// `(client, target, verb, verb_source)` of a client request call.
fn client_request(
    node: Node,
    content: &str,
) -> Option<(&'static str, String, &'static str, &'static str)> {
    CLIENTS.into_iter().find_map(|client| {
        let (function, args) = remote_call(node, content, client)?;
        let (target, verb, verb_source) = match (client, function.as_str(), args.as_slice()) {
            ("httpc" | "hackney", "request", [url]) => (url_text(*url, content)?, "GET", "default"),
            ("httpc", "request", [method, request, ..]) => {
                let url = named_children(*request)
                    .first()
                    .copied()
                    .filter(|_| request.kind() == "tuple")?;
                (
                    url_text(url, content)?,
                    atom_verb(*method, content)?,
                    "attested",
                )
            }
            ("hackney", "request", [method, url, ..]) => (
                url_text(*url, content)?,
                atom_verb(*method, content)?,
                "attested",
            ),
            ("hackney", method, [url, ..]) => (
                url_text(*url, content)?,
                verb_for_lower_method(method)?,
                "attested",
            ),
            ("gun", method, [_connection, path, ..]) => (
                url_text(*path, content)?,
                verb_for_lower_method(method)?,
                "attested",
            ),
            _ => return None,
        };
        Some((client, target, verb, verb_source))
    })
}

/// A static string or binary; an atom is never a URL.
fn url_text(node: Node, content: &str) -> Option<String> {
    (node.kind() != "atom")
        .then(|| static_term_text(node, content))
        .flatten()
}

fn atom_verb(node: Node, content: &str) -> Option<&'static str> {
    if node.kind() != "atom" {
        return None;
    }
    verb_for_lower_method(node_text(content, node)?)
}
