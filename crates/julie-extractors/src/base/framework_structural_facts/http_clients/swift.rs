//! Swift HTTP client requests: Alamofire `AF.request("url", method: .verb)` and
//! URLSession tasks whose request is `URL(string: "url")` or
//! `URLRequest(url: URL(string: "url"))`.
use tree_sitter::{Node, Tree};

use super::super::swift::{
    named_child_of_kind, swift_call_arguments, swift_method_call, swift_static_string,
};
use super::client_fact;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// URLSession methods that take a URL or request as their first argument.
const URLSESSION_METHODS: &[&str] = &[
    "bytes",
    "data",
    "dataTask",
    "download",
    "downloadTask",
    "upload",
    "uploadTask",
];

pub(super) fn collect_swift_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        language,
        tree,
        file_path,
        content,
        0,
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
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if let Some((client, target, verb, verb_source, import_source)) = client_request(node, content)
        && let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            node.start_byte(),
            node.end_byte(),
            client,
            target,
            verb,
            verb_source,
            Some(import_source),
        )
    {
        facts.push(fact);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            child,
            language,
            tree,
            file_path,
            content,
            child_depth,
            facts,
        );
    }
}

type ClientRequest<'a> = (
    &'static str,
    &'a str,
    &'static str,
    &'static str,
    &'static str,
);

fn client_request<'a>(call: Node<'_>, content: &'a str) -> Option<ClientRequest<'a>> {
    let (receiver, method) = swift_method_call(call, content)?;
    let receiver_text = content.get(receiver.start_byte()..receiver.end_byte())?;
    let arguments = swift_call_arguments(call, content);
    if method == "request" && matches!(receiver_text, "AF" | "Session.default") {
        let (label, url) = arguments.first()?;
        if label.is_some() {
            return None;
        }
        let target = swift_static_string(*url, content)?;
        let attested = arguments
            .iter()
            .find(|(label, _)| label.as_deref() == Some("method"))
            .and_then(|(_, value)| content.get(value.start_byte()..value.end_byte()))
            .and_then(|text| text.strip_prefix('.'))
            .and_then(verb_for_member);
        return Some(match attested {
            Some(verb) => ("alamofire", target, verb, "attested", "Alamofire"),
            None => ("alamofire", target, "GET", "default", "Alamofire"),
        });
    }
    if !URLSESSION_METHODS.contains(&method) || !is_url_session(receiver_text) {
        return None;
    }
    let (label, request) = arguments.first()?;
    if !matches!(label.as_deref(), Some("from" | "with" | "for")) {
        return None;
    }
    let target = url_string(*request, content).or_else(|| {
        let request = unwrapped(*request);
        (constructor_name(request, content)? == "URLRequest")
            .then(|| labeled_argument(request, "url", content))
            .flatten()
            .and_then(|url| url_string(url, content))
    })?;
    Some(("urlsession", target, "GET", "default", "Foundation"))
}

fn is_url_session(receiver: &str) -> bool {
    receiver.contains("URLSession") || receiver.to_ascii_lowercase().ends_with("session")
}

/// `URL(string: "...")`, optionally force-unwrapped.
fn url_string<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    let call = unwrapped(node);
    if constructor_name(call, content)? != "URL" {
        return None;
    }
    swift_static_string(labeled_argument(call, "string", content)?, content)
}

fn unwrapped(node: Node) -> Node {
    if node.kind() == "postfix_expression" {
        node.child_by_field_name("target").unwrap_or(node)
    } else {
        node
    }
}

fn constructor_name<'a>(call: Node<'_>, content: &'a str) -> Option<&'a str> {
    if call.kind() != "call_expression" {
        return None;
    }
    let callee = call
        .named_child(0)
        .filter(|callee| callee.kind() == "simple_identifier")?;
    named_child_of_kind(call, "call_suffix")?;
    content.get(callee.start_byte()..callee.end_byte())
}

fn labeled_argument<'t>(call: Node<'t>, label: &str, content: &str) -> Option<Node<'t>> {
    swift_call_arguments(call, content)
        .into_iter()
        .find(|(name, _)| name.as_deref() == Some(label))
        .map(|(_, value)| value)
}

fn verb_for_member(member: &str) -> Option<&'static str> {
    match member {
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
