//! `std.http.Client` requests in Zig.
//!
//! A receiver is proven in the same file: a name bound to a
//! `std.http.Client{ .. }` literal, or a parameter typed as `http.Client`.
//! `client.fetch(.{ .location = .{ .url = "lit" }, .method = .GET })` attests
//! its verb in `.method`; without it std sends POST when a `.payload` is set
//! and GET otherwise. `client.open(.GET, uri, ..)` and
//! `client.request(.GET, uri, ..)` take the verb first and a URI from
//! `std.Uri.parse("lit")`, inline or bound to a same-file name.

use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Tree};

use super::super::helpers::node_text;
use super::super::zig::{
    arguments, initializer, method_call, string_value, struct_field, unwrap_try,
};
use super::client_fact;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const CLIENT: &str = "std.http";

pub(super) fn collect_zig_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains("http.Client") {
        return Vec::new();
    }
    let mut bindings = Bindings::default();
    collect_bindings(tree.root_node(), content, 0, &mut bindings);
    if bindings.clients.is_empty() {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        &bindings,
        language,
        tree,
        file_path,
        content,
        0,
        &mut facts,
    );
    facts
}

#[derive(Default)]
struct Bindings<'a> {
    clients: HashSet<&'a str>,
    uris: HashMap<&'a str, &'a str>,
}

fn collect_bindings<'a>(node: Node, content: &'a str, depth: u32, bindings: &mut Bindings<'a>) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "variable_declaration" => {
            let name = node
                .children(&mut node.walk())
                .find(|child| child.kind() == "identifier")
                .and_then(|name| node_text(content, name));
            if let (Some(name), Some(value)) = (name, initializer(node).map(unwrap_try)) {
                if is_client_literal(value, content) {
                    bindings.clients.insert(name);
                } else if let Some(url) = parsed_uri(value, content) {
                    bindings.uris.insert(name, url);
                }
            }
        }
        "parameter" => {
            let name = node
                .child_by_field_name("name")
                .and_then(|name| node_text(content, name));
            let typed_client = node
                .child_by_field_name("type")
                .and_then(|ty| node_text(content, ty))
                .is_some_and(|ty| {
                    ty.trim_start_matches(['*', '?'])
                        .trim_start_matches("const ")
                        .ends_with("http.Client")
                });
            if let (Some(name), true) = (name, typed_client) {
                bindings.clients.insert(name);
            }
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.children(&mut node.walk()) {
        collect_bindings(child, content, child_depth, bindings);
    }
}

fn is_client_literal(value: Node, content: &str) -> bool {
    value.kind() == "struct_initializer"
        && value
            .named_child(0)
            .and_then(|ty| node_text(content, ty))
            .is_some_and(|ty| ty == "http.Client" || ty.ends_with(".http.Client"))
}

/// `std.Uri.parse("lit")` (with or without `try`): the literal.
fn parsed_uri<'a>(value: Node, content: &'a str) -> Option<&'a str> {
    let value = unwrap_try(value);
    let (receiver, method) = method_call(value, content)?;
    let receiver_text = node_text(content, receiver)?;
    if method != "parse" || !(receiver_text == "Uri" || receiver_text.ends_with(".Uri")) {
        return None;
    }
    string_value(*arguments(value).first()?, content)
}

#[allow(clippy::too_many_arguments)]
fn walk(
    node: Node,
    bindings: &Bindings,
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
    if let Some((target, verb, verb_source)) = client_request(node, bindings, content)
        && let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            node.start_byte(),
            node.end_byte(),
            CLIENT,
            target,
            verb,
            verb_source,
            Some("std"),
        )
    {
        facts.push(fact);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.children(&mut node.walk()) {
        walk(
            child,
            bindings,
            language,
            tree,
            file_path,
            content,
            child_depth,
            facts,
        );
    }
}

fn client_request<'a>(
    call: Node,
    bindings: &Bindings<'a>,
    content: &'a str,
) -> Option<(&'a str, &'a str, &'static str)> {
    let (receiver, method) = method_call(call, content)?;
    if receiver.kind() != "identifier" || !bindings.clients.contains(node_text(content, receiver)?)
    {
        return None;
    }
    let args = arguments(call);
    match method {
        "fetch" => {
            let options = *args.first()?;
            let location = struct_field(options, "location", content)?;
            let target = match struct_field(location, "url", content) {
                Some(url) => string_value(url, content)?,
                None => uri_value(struct_field(location, "uri", content)?, bindings, content)?,
            };
            match struct_field(options, "method", content) {
                Some(verb) => Some((target, enum_verb(verb, content)?, "attested")),
                None if struct_field(options, "payload", content).is_some() => {
                    Some((target, "POST", "default"))
                }
                None => Some((target, "GET", "default")),
            }
        }
        "open" | "request" => {
            let verb = enum_verb(*args.first()?, content)?;
            let target = uri_value(*args.get(1)?, bindings, content)?;
            Some((target, verb, "attested"))
        }
        _ => None,
    }
}

fn uri_value<'a>(node: Node, bindings: &Bindings<'a>, content: &'a str) -> Option<&'a str> {
    match node.kind() {
        "identifier" => bindings.uris.get(node_text(content, node)?).copied(),
        _ => parsed_uri(node, content),
    }
}

/// `.GET` (or `std.http.Method.GET`): the uppercase verb.
fn enum_verb<'a>(node: Node, content: &'a str) -> Option<&'a str> {
    if node.kind() != "field_expression" {
        return None;
    }
    let verb = node_text(content, node.child_by_field_name("member")?)?;
    matches!(
        verb,
        "GET" | "POST" | "PUT" | "DELETE" | "PATCH" | "HEAD" | "OPTIONS"
    )
    .then_some(verb)
}
