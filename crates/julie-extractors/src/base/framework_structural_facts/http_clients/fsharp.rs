use std::collections::HashSet;

use tree_sitter::{Node, Tree};

use super::super::helpers::node_text;
use super::client_fact;
use super::csharp::HTTPCLIENT_METHODS;
use crate::base::http_boundary::classify_url;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// `http.client_request.v1` facts for F# `HttpClient` calls with a literal
/// URL. A receiver counts when the file types it `HttpClient` (a parameter
/// or annotated binding) or binds it to `new HttpClient()` / `HttpClient()`.
pub(super) fn collect_fsharp_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut receivers = HashSet::new();
    collect_receivers(tree.root_node(), content, &mut receivers, 0);
    let mut facts = Vec::new();
    if receivers.is_empty() {
        return facts;
    }
    collect_requests(
        tree.root_node(),
        language,
        tree,
        file_path,
        content,
        &receivers,
        &mut facts,
        0,
    );
    facts
}

fn collect_receivers(node: Node<'_>, content: &str, receivers: &mut HashSet<String>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "typed_pattern" => {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.named_children(&mut cursor).collect();
            if let [pattern, type_node] = children.as_slice()
                && names_http_client(*type_node, content)
                && let Some(name) = node_text(content, *pattern)
            {
                receivers.insert(name.trim().to_string());
            }
        }
        "function_or_value_defn" => {
            let name = child_of_kind(node, "value_declaration_left")
                .and_then(|left| node_text(content, left))
                .map(str::trim);
            let constructs_client = node
                .child_by_field_name("body")
                .and_then(|body| node_text(content, body))
                .map(|body| body.trim().trim_start_matches("new").trim_start())
                .is_some_and(|body| {
                    body.split('(')
                        .next()
                        .is_some_and(|head| last_segment(head) == "HttpClient")
                });
            if let Some(name) = name.filter(|_| constructs_client) {
                receivers.insert(name.to_string());
            }
        }
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_receivers(child, content, receivers, child_depth);
    }
}

fn names_http_client(type_node: Node<'_>, content: &str) -> bool {
    node_text(content, type_node).is_some_and(|text| last_segment(text) == "HttpClient")
}

fn last_segment(text: &str) -> &str {
    text.rsplit('.').next().unwrap_or(text).trim()
}

#[allow(clippy::too_many_arguments)]
fn collect_requests(
    node: Node<'_>,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    receivers: &HashSet<String>,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "application_expression"
        && let Some((verb, url)) = client_request(node, content, receivers)
        && classify_url(&url) != "relative"
        && let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            node.start_byte(),
            node.end_byte(),
            "httpclient",
            &url,
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
    for child in node.children(&mut cursor) {
        collect_requests(
            child,
            language,
            tree,
            file_path,
            content,
            receivers,
            facts,
            child_depth,
        );
    }
}

/// `client.GetAsync("https://...")` or `client.GetAsync "https://..."`.
fn client_request(
    node: Node<'_>,
    content: &str,
    receivers: &HashSet<String>,
) -> Option<(&'static str, String)> {
    let mut cursor = node.walk();
    let mut children = node.named_children(&mut cursor);
    let callee = children.next()?;
    let argument = children.next()?;
    let (receiver, method) = match callee.kind() {
        "dot_expression" => (
            node_text(content, callee.child_by_field_name("base")?)?,
            node_text(content, callee.child_by_field_name("field")?)?,
        ),
        "long_identifier_or_op" => node_text(content, callee)?.trim().rsplit_once('.')?,
        _ => return None,
    };
    if !receivers.contains(receiver.trim()) {
        return None;
    }
    let verb = HTTPCLIENT_METHODS
        .iter()
        .find(|(name, _)| *name == method.trim())
        .map(|(_, verb)| *verb)?;
    Some((verb, first_string(argument, content)?))
}

fn first_string(mut node: Node<'_>, content: &str) -> Option<String> {
    while matches!(node.kind(), "paren_expression" | "tuple_expression") {
        node = node.named_child(0)?;
    }
    if node.kind() != "const" {
        return None;
    }
    let string = child_of_kind(node, "string")?;
    let text = node_text(content, string)?;
    Some(text.strip_prefix('"')?.strip_suffix('"')?.to_string())
}

fn child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}
