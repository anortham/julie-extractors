use tree_sitter::Tree;

use super::super::HTTP_CLIENT_REQUEST_PATTERN_ID;
use super::super::helpers::{
    fact_for_span, is_comment_or_string_node, skip_ascii_whitespace_until,
    smallest_node_covering_range,
};
use crate::base::http_boundary::client_request_metadata;
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

pub(super) fn collect_go_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let Some(http_alias) = net_http_alias(content) else {
        return Vec::new();
    };
    let mut facts = Vec::new();
    for (method, verb, url_arg) in [
        ("Get", "GET", 0usize),
        ("Head", "HEAD", 0),
        ("Post", "POST", 0),
        ("PostForm", "POST", 0),
        ("NewRequest", "", 1),
        ("NewRequestWithContext", "", 2),
    ] {
        let needle = format!("{http_alias}.{method}");
        collect_calls(
            language, tree, file_path, content, &needle, verb, url_arg, &mut facts,
        );
    }
    facts
}

fn net_http_alias(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim().trim_start_matches("import ").trim();
        if trimmed == "\"net/http\"" {
            return Some("http".to_string());
        }
        if trimmed.ends_with("\"net/http\"") {
            let alias = trimmed.trim_end_matches("\"net/http\"").trim();
            if !alias.is_empty() && alias != "_" && alias != "." {
                return Some(alias.to_string());
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn collect_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    needle: &str,
    fixed_verb: &str,
    url_arg_index: usize,
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if is_in_go_string_or_comment(content, call_start) {
            continue;
        }
        let open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, open) else {
            continue;
        };
        let args = call_arguments(content, open + 1, close);
        let Some((url_start, url_end)) = args.get(url_arg_index).copied() else {
            continue;
        };
        let Some((target_path, literal_end)) = parse_go_string_literal(content, url_start) else {
            continue;
        };
        if skip_ascii_whitespace_until(content, literal_end, url_end) != url_end {
            continue;
        }
        let verb = if fixed_verb.is_empty() {
            let Some((method_start, method_end)) = args.get(url_arg_index - 1).copied() else {
                continue;
            };
            let Some((method, method_literal_end)) = parse_go_string_literal(content, method_start)
            else {
                continue;
            };
            if skip_ascii_whitespace_until(content, method_literal_end, method_end) != method_end {
                continue;
            }
            method.to_uppercase()
        } else {
            fixed_verb.to_string()
        };
        if let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            call_start,
            close + 1,
            &target_path,
            &verb,
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
        client_request_metadata("net/http", target_path, verb, "attested", Some("net/http")),
    ))
}

fn call_arguments(content: &str, mut cursor: usize, end: usize) -> Vec<(usize, usize)> {
    let mut args = Vec::new();
    while cursor < end {
        cursor = skip_ascii_whitespace_until(content, cursor, end);
        if cursor >= end {
            break;
        }
        let arg_end = find_top_level_comma_or_end(content, cursor, end);
        args.push((cursor, arg_end));
        cursor = arg_end.saturating_add(1);
    }
    args
}

fn parse_go_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    if content.as_bytes().get(start) != Some(&b'"') {
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
        } else if byte == b'"' {
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
    let mut quote = false;
    let mut escaped = false;
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        if quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quote = false;
            }
        } else if byte == b'"' {
            quote = true;
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

fn find_top_level_comma_or_end(content: &str, start: usize, end: usize) -> usize {
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut quote = false;
    let mut escaped = false;
    while cursor < end {
        let byte = content.as_bytes()[cursor];
        if quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quote = false;
            }
        } else if byte == b'"' {
            quote = true;
        } else if byte == b'(' {
            paren_depth += 1;
        } else if byte == b')' {
            paren_depth = paren_depth.saturating_sub(1);
        } else if byte == b',' && paren_depth == 0 {
            return cursor;
        }
        cursor += 1;
    }
    end
}

fn is_in_go_string_or_comment(content: &str, target: usize) -> bool {
    let bytes = content.as_bytes();
    let mut cursor = 0;
    let mut line_comment = false;
    let mut block_comment = false;
    let mut quote = false;
    let mut escaped = false;
    while cursor < target {
        let byte = bytes[cursor];
        let next = bytes.get(cursor + 1).copied();
        if line_comment {
            if byte == b'\n' {
                line_comment = false;
            }
        } else if block_comment {
            if byte == b'*' && next == Some(b'/') {
                block_comment = false;
                cursor += 1;
            }
        } else if quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quote = false;
            }
        } else if byte == b'/' && next == Some(b'/') {
            line_comment = true;
            cursor += 1;
        } else if byte == b'/' && next == Some(b'*') {
            block_comment = true;
            cursor += 1;
        } else if byte == b'"' {
            quote = true;
        }
        cursor += 1;
    }
    line_comment || block_comment || quote
}
