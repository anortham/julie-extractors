use tree_sitter::{Node, Tree};

use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

use super::super::helpers::{is_identifier_boundary, skip_ascii_whitespace_until};
use super::super::scan::{
    MaskLanguage, SourceMask, find_matching_paren, parse_ruby_string_literal,
};
use super::client_fact;
use crate::base::types::StructuralFact;

pub(super) fn collect_ruby_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    collect_client_calls(
        language,
        tree,
        file_path,
        content,
        tree.root_node(),
        0,
        &mut facts,
    );
    if !content.contains("Net::HTTP.") {
        return facts;
    }
    let mask = SourceMask::new(content, MaskLanguage::Ruby);
    for (method, verb) in [
        ("get", "GET"),
        ("get_response", "GET"),
        ("post", "POST"),
        ("post_form", "POST"),
    ] {
        collect_net_http_calls(
            language, tree, file_path, content, &mask, method, verb, &mut facts,
        );
    }
    facts
}

#[allow(clippy::too_many_arguments)]
fn collect_net_http_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    method: &str,
    verb: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let needle = format!("Net::HTTP.{method}");
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(&needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if !is_identifier_boundary(content, call_start, needle.len())
            || mask.is_string_or_comment(call_start)
        {
            continue;
        }
        let open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, mask, open) else {
            continue;
        };
        let first_start = skip_ascii_whitespace_until(content, open + 1, close);
        let Some(target_path) = uri_literal_arg(content, mask, first_start) else {
            continue;
        };
        if let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            call_start,
            close + 1,
            "net::http",
            &target_path,
            verb,
            "attested",
            None,
        ) {
            facts.push(fact);
        }
    }
}

fn uri_literal_arg(content: &str, mask: &SourceMask, start: usize) -> Option<String> {
    for prefix in ["URI.parse", "URI"] {
        if content[start..].starts_with(prefix) {
            let open = skip_ascii_whitespace_until(content, start + prefix.len(), content.len());
            if content.as_bytes().get(open) != Some(&b'(') {
                continue;
            }
            let close = find_matching_paren(content, mask, open)?;
            let arg_start = skip_ascii_whitespace_until(content, open + 1, content.len());
            return parse_ruby_string_literal(content, arg_start)
                .filter(|(value, literal_end)| {
                    skip_ascii_whitespace_until(content, *literal_end, close) == close
                        && (content.as_bytes().get(arg_start) != Some(&b'"')
                            || !value.contains("#{"))
                })
                .map(|(value, _)| value);
        }
    }
    None
}

/// AST-proven client calls: `HTTParty.get("...")`, `RestClient.post "..."`,
/// `Faraday.get("...")`; `conn.get("/items")` on a `conn = Faraday.new(...)`
/// bound in the same method; and `Net::HTTP.get(uri)` on a `uri =
/// URI("...")` bound in the same method.
#[allow(clippy::too_many_arguments)]
fn collect_client_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    node: Node<'_>,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "call"
        && let Some((client, verb, target_path)) = client_call(content, node)
        && let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            node.start_byte(),
            node.end_byte(),
            client,
            &target_path,
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
        collect_client_calls(
            language,
            tree,
            file_path,
            content,
            child,
            child_depth,
            facts,
        );
    }
}

fn client_call(content: &str, call: Node<'_>) -> Option<(&'static str, &'static str, String)> {
    let method = text(content, call.child_by_field_name("method")?);
    let receiver = call.child_by_field_name("receiver")?;
    let first = call
        .child_by_field_name("arguments")
        .and_then(|arguments| arguments.named_child(0));
    let receiver_text = text(content, receiver);
    if receiver_text == "Net::HTTP" {
        let verb = match method {
            "get" | "get_response" => "GET",
            "post" | "post_form" => "POST",
            _ => return None,
        };
        let first = first.filter(|first| first.kind() == "identifier")?;
        let value = bound_value(content, call, text(content, first))?;
        return Some(("net::http", verb, uri_literal(content, value)?));
    }
    let verb = super::verb_for_lower_method(method)?;
    let client = match receiver.kind() {
        "constant" => match receiver_text {
            "HTTParty" => "httparty",
            "RestClient" => "rest-client",
            "Faraday" => "faraday",
            _ => return None,
        },
        "identifier"
            if is_faraday_connection(content, bound_value(content, call, receiver_text)?) =>
        {
            "faraday"
        }
        "call" if is_faraday_connection(content, receiver) => "faraday",
        _ => return None,
    };
    Some((client, verb, static_string(content, first?)?))
}

/// `Faraday.new(...)`, the connection object whose verbs send requests.
fn is_faraday_connection(content: &str, value: Node<'_>) -> bool {
    value.kind() == "call"
        && value
            .child_by_field_name("receiver")
            .is_some_and(|receiver| text(content, receiver) == "Faraday")
        && value
            .child_by_field_name("method")
            .is_some_and(|method| text(content, method) == "new")
}

/// The right-hand side of the last `name = value` before `call` in the same
/// method (or file).
fn bound_value<'tree>(content: &str, call: Node<'tree>, name: &str) -> Option<Node<'tree>> {
    let mut scope = call.parent()?;
    while !matches!(scope.kind(), "method" | "singleton_method" | "program") {
        scope = scope.parent()?;
    }
    let mut found = None;
    find_assignment(content, scope, name, call.start_byte(), 0, &mut found);
    found
}

fn find_assignment<'tree>(
    content: &str,
    node: Node<'tree>,
    name: &str,
    before: usize,
    depth: u32,
    found: &mut Option<Node<'tree>>,
) {
    if !should_visit_tree_depth(depth) || node.start_byte() >= before {
        return;
    }
    if node.kind() == "assignment"
        && let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        )
        && left.kind() == "identifier"
        && text(content, left) == name
    {
        *found = Some(right);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if !matches!(
            child.kind(),
            "method" | "singleton_method" | "class" | "module"
        ) {
            find_assignment(content, child, name, before, child_depth, found);
        }
    }
}

/// The URL of `URI("...")` or `URI.parse("...")`.
fn uri_literal(content: &str, value: Node<'_>) -> Option<String> {
    if value.kind() != "call" {
        return None;
    }
    let method = text(content, value.child_by_field_name("method")?);
    let is_uri = match value.child_by_field_name("receiver") {
        None => method == "URI",
        Some(receiver) => text(content, receiver) == "URI" && method == "parse",
    };
    if !is_uri {
        return None;
    }
    let first = value.child_by_field_name("arguments")?.named_child(0)?;
    static_string(content, first)
}

/// The value of a string literal without interpolation.
fn static_string(content: &str, node: Node<'_>) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let (value, end) = parse_ruby_string_literal(content, node.start_byte())?;
    (end == node.end_byte() && !(text(content, node).starts_with('"') && value.contains("#{")))
        .then_some(value)
}

fn text<'a>(content: &'a str, node: Node<'_>) -> &'a str {
    content.get(node.byte_range()).unwrap_or("")
}
