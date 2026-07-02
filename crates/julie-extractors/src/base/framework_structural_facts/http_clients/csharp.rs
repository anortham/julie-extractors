use tree_sitter::Tree;

use super::super::HTTP_CLIENT_REQUEST_PATTERN_ID;
use super::super::helpers::{
    fact_for_span, is_comment_or_string_node, is_identifier_boundary, parse_csharp_string_literal,
    skip_ascii_whitespace_until, smallest_node_covering_range,
};
use crate::base::http_boundary::{classify_url, client_request_metadata};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

const HTTPCLIENT_METHODS: &[(&str, &str)] = &[
    ("GetAsync", "GET"),
    ("GetStringAsync", "GET"),
    ("GetByteArrayAsync", "GET"),
    ("GetStreamAsync", "GET"),
    ("GetFromJsonAsync", "GET"),
    ("PostAsync", "POST"),
    ("PostAsJsonAsync", "POST"),
    ("PutAsync", "PUT"),
    ("PutAsJsonAsync", "PUT"),
    ("PatchAsync", "PATCH"),
    ("PatchAsJsonAsync", "PATCH"),
    ("DeleteAsync", "DELETE"),
    ("DeleteFromJsonAsync", "DELETE"),
];

pub(super) fn collect_csharp_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for (method, verb) in HTTPCLIENT_METHODS {
        collect_method_calls(language, tree, file_path, content, method, verb, &mut facts);
    }
    collect_http_request_messages(language, tree, file_path, content, &mut facts);
    facts
}

fn collect_method_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    method: &str,
    verb: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(method) {
        let method_start = cursor + relative;
        cursor = method_start + method.len();
        if !is_identifier_boundary(content, method_start, method.len())
            || is_in_csharp_string_or_comment(content, method_start)
        {
            continue;
        }
        let mut open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) == Some(&b'<') {
            let Some(after_generics) = skip_generic_type_arguments(content, open) else {
                continue;
            };
            open = skip_ascii_whitespace_until(content, after_generics, content.len());
        }
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, open) else {
            continue;
        };
        let first_start = skip_ascii_whitespace_until(content, open + 1, close);
        let first_end = find_top_level_comma_or_end(content, first_start, close);
        let Some((target_path, literal_end)) = parse_csharp_url_literal(content, first_start)
        else {
            continue;
        };
        if skip_ascii_whitespace_until(content, literal_end, first_end) != first_end {
            continue;
        }
        if classify_url(&target_path) == "relative" {
            continue;
        }
        if let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            method_start,
            close + 1,
            &target_path,
            verb,
        ) {
            facts.push(fact);
        }
    }
}

fn collect_http_request_messages(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let needle = "new HttpRequestMessage";
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if is_in_csharp_string_or_comment(content, call_start) {
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
        let Some(verb) = content[method_start..method_end]
            .trim()
            .strip_prefix("HttpMethod.")
            .map(|value| value.to_uppercase())
        else {
            continue;
        };
        let url_start = skip_ascii_whitespace_until(content, method_end + 1, close);
        let url_end = find_top_level_comma_or_end(content, url_start, close);
        let Some((target_path, literal_end)) = parse_csharp_url_literal(content, url_start) else {
            continue;
        };
        if skip_ascii_whitespace_until(content, literal_end, url_end) != url_end {
            continue;
        }
        if classify_url(&target_path) == "relative" {
            continue;
        }
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
        client_request_metadata("httpclient", target_path, verb, "attested", None),
    ))
}

fn parse_csharp_url_literal(content: &str, start: usize) -> Option<(String, usize)> {
    parse_csharp_string_literal(content, start)
        .map(|(value, end, _)| (value, end))
        .or_else(|| parse_csharp_raw_string_literal(content, start))
}

fn parse_csharp_raw_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    let bytes = content.as_bytes();
    if bytes.get(start) == Some(&b'$') {
        return None;
    }
    if bytes.get(start) != Some(&b'"')
        || bytes.get(start + 1) != Some(&b'"')
        || bytes.get(start + 2) != Some(&b'"')
    {
        return None;
    }
    let mut cursor = start + 3;
    while cursor + 2 < content.len() {
        if bytes.get(cursor) == Some(&b'"')
            && bytes.get(cursor + 1) == Some(&b'"')
            && bytes.get(cursor + 2) == Some(&b'"')
        {
            let value = content[start + 3..cursor].trim_matches('\n').to_string();
            return Some((value, cursor + 3));
        }
        cursor += 1;
    }
    None
}

fn skip_generic_type_arguments(content: &str, open: usize) -> Option<usize> {
    find_matching_delimiter(content, open, b'<', b'>').map(|close| close + 1)
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
    let mut normal_string = false;
    let mut verbatim_string = false;
    let mut line_comment = false;
    let mut block_comment = false;
    while cursor < content.len() {
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
        } else if normal_string {
            if byte == b'\\' {
                cursor += 1;
            } else if byte == b'"' {
                normal_string = false;
            }
        } else if verbatim_string {
            if byte == b'"' && next == Some(b'"') {
                cursor += 1;
            } else if byte == b'"' {
                verbatim_string = false;
            }
        } else if byte == b'/' && next == Some(b'/') {
            line_comment = true;
            cursor += 1;
        } else if byte == b'/' && next == Some(b'*') {
            block_comment = true;
            cursor += 1;
        } else if byte == b'@' && next == Some(b'"') {
            verbatim_string = true;
            cursor += 1;
        } else if byte == b'"' {
            normal_string = true;
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
    let mut angle_depth = 0usize;
    let mut normal_string = false;
    while cursor < end {
        let byte = bytes[cursor];
        if normal_string {
            if byte == b'\\' {
                cursor += 1;
            } else if byte == b'"' {
                normal_string = false;
            }
        } else {
            match byte {
                b'"' => normal_string = true,
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'<' => angle_depth += 1,
                b'>' => angle_depth = angle_depth.saturating_sub(1),
                b',' if paren_depth == 0 && angle_depth == 0 => return cursor,
                _ => {}
            }
        }
        cursor += 1;
    }
    end
}

fn is_in_csharp_string_or_comment(content: &str, target: usize) -> bool {
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
