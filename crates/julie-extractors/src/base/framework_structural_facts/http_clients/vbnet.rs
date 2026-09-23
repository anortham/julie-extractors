use std::collections::HashSet;

use tree_sitter::{Node, Tree};

use super::super::helpers::node_text;
use super::client_fact;
use super::csharp::HTTPCLIENT_METHODS;
use crate::base::http_boundary::classify_url;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// `http.client_request.v1` facts for VB.NET `HttpClient` calls. A receiver
/// counts when the file declares it `As HttpClient` / `As New HttpClient` or
/// assigns it `New HttpClient()`. VB names are case-insensitive, so receivers
/// and method names compare without case.
pub(super) fn collect_vbnet_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut receivers = HashSet::new();
    collect_receivers(tree.root_node(), content, &mut receivers, 0);
    let mut facts = Vec::new();
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
    let declared_name = match node.kind() {
        "variable_declarator" | "parameter" | "property_declaration" => {
            node.child_by_field_name("name")
        }
        "dim_statement" => node.child_by_field_name("name"),
        _ => None,
    };
    if let Some(name) = declared_name
        && declares_http_client(node, content)
        && let Some(text) = node_text(content, name)
    {
        receivers.insert(text.to_ascii_lowercase());
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_receivers(child, content, receivers, child_depth);
    }
}

fn declares_http_client(declaration: Node<'_>, content: &str) -> bool {
    let mut cursor = declaration.walk();
    declaration.children(&mut cursor).any(|child| {
        let type_node = match child.kind() {
            "as_clause" => child.child_by_field_name("type"),
            "new_expression" => child.child_by_field_name("type"),
            "element_access" => child
                .child_by_field_name("object")
                .filter(|object| object.kind() == "new_expression")
                .and_then(|object| object.child_by_field_name("type")),
            _ => None,
        };
        type_node.is_some_and(|type_node| {
            let type_node = type_node
                .child_by_field_name("element")
                .unwrap_or(type_node);
            node_text(content, type_node)
                .is_some_and(|text| last_segment(text).eq_ignore_ascii_case("HttpClient"))
        })
    })
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
    let request = match node.kind() {
        "invocation" => client_method_request(node, content, receivers),
        "new_expression" | "element_access" => request_message(node, content),
        _ => None,
    };
    if let Some((verb, target_path)) = request
        && classify_url(&target_path) != "relative"
        && let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            node.start_byte(),
            node.end_byte(),
            "httpclient",
            &target_path,
            &verb,
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

fn client_method_request(
    node: Node<'_>,
    content: &str,
    receivers: &HashSet<String>,
) -> Option<(String, String)> {
    let target = node.child_by_field_name("target")?;
    if target.kind() != "member_access" {
        return None;
    }
    let receiver = node_text(content, target.child_by_field_name("object")?)?;
    if !receivers.contains(&receiver.to_ascii_lowercase()) {
        return None;
    }
    let method = node_text(content, target.child_by_field_name("member")?)?;
    let verb = HTTPCLIENT_METHODS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(method))
        .map(|(_, verb)| *verb)?;
    let arguments = node.child_by_field_name("arguments")?;
    let first = first_argument_value(arguments)?;
    Some((verb.to_string(), string_literal_value(first, content)?))
}

/// `New HttpRequestMessage(HttpMethod.Get, "https://...")`.
fn request_message(node: Node<'_>, content: &str) -> Option<(String, String)> {
    let (type_node, arguments) = crate::vbnet::constructor_parts(node)?;
    if !node_text(content, type_node)
        .is_some_and(|text| last_segment(text).eq_ignore_ascii_case("HttpRequestMessage"))
    {
        return None;
    }
    let [method, url, ..] = arguments.as_slice() else {
        return None;
    };
    let verb = node_text(content, *method)?
        .trim()
        .strip_prefix("HttpMethod.")?
        .to_ascii_uppercase();
    Some((verb, string_literal_value(*url, content)?))
}

fn first_argument_value(arguments: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = arguments.walk();
    let argument = arguments.named_children(&mut cursor).next()?;
    if argument.kind() != "argument" || argument.child_by_field_name("name").is_some() {
        return None;
    }
    argument.named_child(0)
}

fn string_literal_value(node: Node<'_>, content: &str) -> Option<String> {
    if node.kind() != "string_literal" {
        return None;
    }
    let text = node_text(content, node)?;
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.replace("\"\"", "\""))
}
