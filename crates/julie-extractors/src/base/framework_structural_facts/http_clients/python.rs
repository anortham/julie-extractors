use std::collections::HashMap;

use tree_sitter::Tree;

use super::super::HTTP_CLIENT_REQUEST_PATTERN_ID;
use super::super::helpers::{
    fact_for_span, is_comment_or_string_node, is_identifier_boundary, skip_ascii_whitespace_until,
    smallest_node_covering_range,
};
use crate::base::http_boundary::client_request_metadata;
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

const CLIENT_METHODS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
    ("head", "HEAD"),
    ("options", "OPTIONS"),
];

pub(super) fn collect_python_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let imports = collect_python_http_client_imports(content);
    let mut facts = Vec::new();
    for (local, client) in imports {
        collect_method_requests(
            language, tree, file_path, content, &local, &client, &mut facts,
        );
        collect_request_calls(
            language, tree, file_path, content, &local, &client, &mut facts,
        );
    }
    facts
}

fn collect_python_http_client_imports(content: &str) -> HashMap<String, String> {
    let mut imports = HashMap::new();
    for line in content.lines() {
        let trimmed = line.trim();
        for module in ["requests", "httpx"] {
            if trimmed == format!("import {module}") {
                imports.insert(module.to_string(), module.to_string());
            } else if let Some(alias) = trimmed
                .strip_prefix(&format!("import {module} as "))
                .map(str::trim)
                .filter(|alias| is_python_identifier(alias))
            {
                imports.insert(alias.to_string(), module.to_string());
            }
        }
    }
    imports
}

fn collect_method_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    local: &str,
    client: &str,
    facts: &mut Vec<StructuralFact>,
) {
    for (method, verb) in CLIENT_METHODS {
        let needle = format!("{local}.{method}");
        let mut cursor = 0;
        while let Some(relative) = content[cursor..].find(&needle) {
            let call_start = cursor + relative;
            cursor = call_start + needle.len();
            if !is_identifier_boundary(content, call_start, local.len())
                || is_in_python_string_or_comment(content, call_start)
            {
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
            let first_end = find_top_level_comma_or_end(content, first_start, close);
            let Some((target_path, url_end)) = parse_python_string_literal(content, first_start)
            else {
                continue;
            };
            if skip_ascii_whitespace_until(content, url_end, first_end) != first_end {
                continue;
            }
            if let Some(fact) = client_fact(
                language,
                tree,
                file_path,
                content,
                call_start,
                close + 1,
                client,
                &target_path,
                verb,
                "attested",
                Some(client),
            ) {
                facts.push(fact);
            }
        }
    }
}

fn collect_request_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    local: &str,
    client: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let needle = format!("{local}.request");
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(&needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if !is_identifier_boundary(content, call_start, local.len())
            || is_in_python_string_or_comment(content, call_start)
        {
            continue;
        }
        let open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, open) else {
            continue;
        };
        let method_start = skip_ascii_whitespace_until(content, open + 1, close);
        let method_end = find_top_level_comma_or_end(content, method_start, close);
        let Some((method, method_literal_end)) = parse_python_string_literal(content, method_start)
        else {
            continue;
        };
        if skip_ascii_whitespace_until(content, method_literal_end, method_end) != method_end {
            continue;
        }
        let url_start = skip_ascii_whitespace_until(content, method_end + 1, close);
        let url_end = find_top_level_comma_or_end(content, url_start, close);
        let Some((target_path, target_literal_end)) =
            parse_python_string_literal(content, url_start)
        else {
            continue;
        };
        if skip_ascii_whitespace_until(content, target_literal_end, url_end) != url_end {
            continue;
        }
        if let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            call_start,
            close + 1,
            client,
            &target_path,
            &method.to_uppercase(),
            "attested",
            Some(client),
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
    client: &str,
    target_path: &str,
    verb: &str,
    verb_source: &str,
    import_source: Option<&str>,
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
        client_request_metadata(client, target_path, verb, verb_source, import_source),
    ))
}

fn parse_python_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    let bytes = content.as_bytes();
    let mut cursor = start;
    while matches!(bytes.get(cursor).copied(), Some(b'r' | b'R' | b'u' | b'U')) {
        cursor += 1;
    }
    if matches!(bytes.get(cursor).copied(), Some(b'f' | b'F' | b'b' | b'B')) {
        return None;
    }
    let quote = bytes
        .get(cursor)
        .copied()
        .filter(|byte| matches!(*byte, b'\'' | b'"'))?;
    let triple = bytes.get(cursor + 1) == Some(&quote) && bytes.get(cursor + 2) == Some(&quote);
    let content_start = if triple { cursor + 3 } else { cursor + 1 };
    let mut value = String::new();
    let mut index = content_start;
    while index < content.len() {
        let byte = bytes[index];
        if byte == b'\\' {
            let escaped_start = index + 1;
            let escaped = content.get(escaped_start..)?.chars().next()?;
            value.push(escaped);
            index = escaped_start + escaped.len_utf8();
        } else if triple
            && byte == quote
            && bytes.get(index + 1) == Some(&quote)
            && bytes.get(index + 2) == Some(&quote)
        {
            return Some((value, index + 3));
        } else if !triple && byte == quote {
            return Some((value, index + 1));
        } else {
            let ch = content.get(index..)?.chars().next()?;
            value.push(ch);
            index += ch.len_utf8();
        }
    }
    None
}

fn find_matching_paren(content: &str, open: usize) -> Option<usize> {
    find_matching_delimiter(content, open, b'(', b')')
}

fn find_matching_delimiter(content: &str, open: usize, left: u8, right: u8) -> Option<usize> {
    if content.as_bytes().get(open) != Some(&left) {
        return None;
    }
    let bytes = content.as_bytes();
    let mut cursor = open;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    while cursor < content.len() {
        let byte = bytes[cursor];
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
        } else if byte == left {
            depth += 1;
        } else if byte == right {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }
    None
}

fn find_top_level_comma_or_end(content: &str, start: usize, end: usize) -> usize {
    let bytes = content.as_bytes();
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    while cursor < end {
        let byte = bytes[cursor];
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
        } else {
            match byte {
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                    return cursor;
                }
                _ => {}
            }
        }
        cursor += 1;
    }
    end
}

fn is_python_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_in_python_string_or_comment(content: &str, target: usize) -> bool {
    let bytes = content.as_bytes();
    let mut cursor = 0;
    let mut quote = None;
    let mut triple_quote = false;
    let mut escaped = false;
    while cursor < target {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if triple_quote
                && byte == active_quote
                && bytes.get(cursor + 1) == Some(&active_quote)
                && bytes.get(cursor + 2) == Some(&active_quote)
            {
                quote = None;
                triple_quote = false;
                cursor += 2;
            } else if !triple_quote && byte == active_quote {
                quote = None;
            }
        } else if byte == b'#' {
            while cursor < target && bytes.get(cursor) != Some(&b'\n') {
                cursor += 1;
            }
        } else if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
            triple_quote =
                bytes.get(cursor + 1) == Some(&byte) && bytes.get(cursor + 2) == Some(&byte);
            if triple_quote {
                cursor += 2;
            }
        }
        cursor += 1;
    }
    quote.is_some()
}
