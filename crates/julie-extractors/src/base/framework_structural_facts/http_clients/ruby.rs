use tree_sitter::Tree;

use super::super::HTTP_CLIENT_REQUEST_PATTERN_ID;
use super::super::helpers::{
    fact_for_span, is_comment_or_string_node, skip_ascii_whitespace_until,
    smallest_node_covering_range,
};
use crate::base::http_boundary::client_request_metadata;
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

pub(super) fn collect_ruby_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for (method, verb) in [
        ("get", "GET"),
        ("get_response", "GET"),
        ("post", "POST"),
        ("post_form", "POST"),
    ] {
        collect_net_http_calls(language, tree, file_path, content, method, verb, &mut facts);
    }
    facts
}

fn collect_net_http_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    method: &str,
    verb: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let needle = format!("Net::HTTP.{method}");
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(&needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if is_in_ruby_string_or_comment(content, call_start) {
            continue;
        }
        let open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, open) else {
            continue;
        };
        let first_start = skip_ascii_whitespace_until(content, open + 1, close);
        let Some(target_path) = uri_literal_arg(content, first_start) else {
            continue;
        };
        if let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            call_start,
            close + 1,
            &target_path,
            verb,
        ) {
            facts.push(fact);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn client_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    start: usize,
    end: usize,
    target_path: &str,
    verb: &str,
) -> Option<StructuralFact> {
    let node = smallest_node_covering_range(tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    Some(fact_for_span(
        file_path,
        language,
        HTTP_CLIENT_REQUEST_PATTERN_ID,
        "client_request",
        node.kind(),
        span,
        client_request_metadata("net::http", target_path, verb, "attested", None),
    ))
}

fn uri_literal_arg(content: &str, start: usize) -> Option<String> {
    for prefix in ["URI.parse", "URI"] {
        if content[start..].starts_with(prefix) {
            let open = skip_ascii_whitespace_until(content, start + prefix.len(), content.len());
            if content.as_bytes().get(open) != Some(&b'(') {
                continue;
            }
            let arg_start = skip_ascii_whitespace_until(content, open + 1, content.len());
            return parse_ruby_string_literal(content, arg_start).map(|(value, _)| value);
        }
    }
    None
}

fn parse_ruby_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    let quote = content.as_bytes().get(start).copied()?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    let mut cursor = start + 1;
    let mut value = String::new();
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        if byte == b'\\' {
            let escaped_start = cursor + 1;
            let escaped = content.get(escaped_start..)?.chars().next()?;
            value.push(escaped);
            cursor = escaped_start + escaped.len_utf8();
        } else if byte == quote {
            return Some((value, cursor + 1));
        } else {
            let ch = content.get(cursor..)?.chars().next()?;
            value.push(ch);
            cursor += ch.len_utf8();
        }
    }
    None
}

fn find_matching_paren(content: &str, open: usize) -> Option<usize> {
    if content.as_bytes().get(open) != Some(&b'(') {
        return None;
    }
    let mut cursor = open;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        } else if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }
    None
}

fn is_in_ruby_string_or_comment(content: &str, target: usize) -> bool {
    let bytes = content.as_bytes();
    let mut cursor = 0;
    let mut quote = None;
    let mut escaped = false;
    while cursor < target {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'#' {
            while cursor < target && bytes.get(cursor) != Some(&b'\n') {
                cursor += 1;
            }
        } else if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        }
        cursor += 1;
    }
    quote.is_some()
}
