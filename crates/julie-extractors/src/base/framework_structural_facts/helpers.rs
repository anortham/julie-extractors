use std::collections::HashMap;

use serde_json::{Number, Value};
use tree_sitter::Node;

use crate::base::span::NormalizedSpan;
use crate::base::types::{StructuralFact, stable_location_id};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) fn fact_for_node(
    file_path: &str,
    language: &str,
    pattern_id: &str,
    capture_name: &str,
    node: Node<'_>,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    let span = NormalizedSpan::from_node(&node);
    fact_for_span(
        file_path,
        language,
        pattern_id,
        capture_name,
        node.kind(),
        span,
        metadata,
    )
}

pub(super) fn node_text<'a>(content: &'a str, node: Node<'_>) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}

pub(super) fn fact_for_span(
    file_path: &str,
    language: &str,
    pattern_id: &str,
    capture_name: &str,
    node_kind: &str,
    span: NormalizedSpan,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    StructuralFact {
        id: stable_location_id(file_path, &format!("{pattern_id}:{capture_name}"), span),
        file_path: file_path.to_string(),
        language: language.to_string(),
        pattern_id: pattern_id.to_string(),
        capture_name: capture_name.to_string(),
        node_kind: node_kind.to_string(),
        containing_symbol_id: None,
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        confidence: 1.0,
        metadata: Some(metadata),
    }
}

pub(super) fn base_metadata(query_family: &str, framework: &str) -> HashMap<String, Value> {
    HashMap::from([
        (
            "pattern_version".to_string(),
            Value::Number(Number::from(1)),
        ),
        (
            "query_family".to_string(),
            Value::String(query_family.to_string()),
        ),
        (
            "framework".to_string(),
            Value::String(framework.to_string()),
        ),
    ])
}

pub(super) fn insert_string(metadata: &mut HashMap<String, Value>, key: &str, value: &str) {
    metadata.insert(key.to_string(), Value::String(value.to_string()));
}

pub(super) fn insert_string_array(
    metadata: &mut HashMap<String, Value>,
    key: &str,
    values: Vec<String>,
) {
    metadata.insert(
        key.to_string(),
        Value::Array(values.into_iter().map(Value::String).collect()),
    );
}

pub(super) fn parse_first_route_argument(
    content: &str,
    args_start: usize,
    args_end: usize,
) -> Option<(String, usize, &'static str)> {
    let route_start = skip_ascii_whitespace_until(content, args_start, args_end);
    if route_start >= args_end {
        return None;
    }
    parse_csharp_string_literal(content, route_start)
        .filter(|(_, route_end, _)| *route_end <= args_end)
}

pub(super) fn parse_csharp_string_literal(
    content: &str,
    start: usize,
) -> Option<(String, usize, &'static str)> {
    let bytes = content.as_bytes();
    if bytes.get(start) == Some(&b'$')
        || (bytes.get(start) == Some(&b'@') && bytes.get(start + 1) == Some(&b'$'))
    {
        return None;
    }

    if bytes.get(start) == Some(&b'@') && bytes.get(start + 1) == Some(&b'"') {
        return parse_verbatim_csharp_string(content, start + 2)
            .map(|(value, end)| (value, end, "string_literal"));
    }

    if bytes.get(start) == Some(&b'"') {
        return parse_normal_csharp_string(content, start + 1)
            .map(|(value, end)| (value, end, "string_literal"));
    }

    None
}

fn parse_normal_csharp_string(content: &str, mut cursor: usize) -> Option<(String, usize)> {
    let mut value = String::new();
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        match byte {
            b'\\' => {
                let escaped_start = cursor + 1;
                let escaped = content.get(escaped_start..)?.chars().next()?;
                value.push(match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                });
                cursor = escaped_start + escaped.len_utf8();
            }
            b'"' => return Some((value, cursor + 1)),
            _ => {
                let ch = content.get(cursor..)?.chars().next()?;
                value.push(ch);
                cursor += ch.len_utf8();
            }
        }
    }
    None
}

fn parse_verbatim_csharp_string(content: &str, mut cursor: usize) -> Option<(String, usize)> {
    let mut value = String::new();
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        if byte == b'"' {
            if content.as_bytes().get(cursor + 1) == Some(&b'"') {
                value.push('"');
                cursor += 2;
            } else {
                return Some((value, cursor + 1));
            }
        } else {
            let ch = content.get(cursor..)?.chars().next()?;
            value.push(ch);
            cursor += ch.len_utf8();
        }
    }
    None
}

#[derive(Debug)]
pub(super) struct HandlerMetadata {
    pub(super) kind: &'static str,
    pub(super) name: Option<String>,
}

pub(super) fn parse_handler_argument(
    content: &str,
    route_arg_end: usize,
    args_end: usize,
) -> Option<HandlerMetadata> {
    let comma = skip_ascii_whitespace_until(content, route_arg_end, args_end);
    if content.as_bytes().get(comma) != Some(&b',') {
        return None;
    }
    let handler_start = skip_ascii_whitespace_until(content, comma + 1, args_end);
    if handler_start >= args_end {
        return None;
    }
    let handler_end = find_top_level_comma_or_end(content, handler_start, args_end);
    let expression = content.get(handler_start..handler_end)?.trim();

    if expression.contains("=>") {
        return Some(HandlerMetadata {
            kind: "lambda",
            name: None,
        });
    }

    parse_identifier_path(expression).map(|name| HandlerMetadata {
        kind: "method_group",
        name: Some(name),
    })
}

fn parse_identifier_path(expression: &str) -> Option<String> {
    let mut segments = expression.split('.');
    let first = segments.next()?;
    if !is_csharp_identifier(first) {
        return None;
    }
    for segment in segments {
        if !is_csharp_identifier(segment) {
            return None;
        }
    }
    Some(expression.to_string())
}

pub(super) fn is_csharp_identifier(value: &str) -> bool {
    is_ascii_identifier(value)
}

/// The identifier shape shared by the C#, Python, Go, Java, and Ruby scanners.
/// (JavaScript identifiers additionally allow `$` — see `js_object_scan`.)
pub(super) fn is_ascii_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}
pub(super) fn find_matching_paren(content: &str, open_paren: usize) -> Option<usize> {
    find_matching_delimiter(content, open_paren, b'(', b')')
}

pub(super) fn find_matching_paren_backwards(content: &str, close_paren: usize) -> Option<usize> {
    if content.as_bytes().get(close_paren) != Some(&b')') {
        return None;
    }
    let mut candidates = Vec::new();
    let mut cursor = 0;
    while cursor <= close_paren {
        if content.as_bytes().get(cursor) == Some(&b'(')
            && let Some(candidate_close) = find_matching_paren(content, cursor)
            && candidate_close == close_paren
        {
            candidates.push(cursor);
        }
        cursor += 1;
    }
    candidates.pop()
}

fn find_matching_delimiter(content: &str, open_byte: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut cursor = open_byte;
    let mut depth = 0usize;
    let mut normal_string = false;
    let mut verbatim_string = false;
    let mut line_comment = false;
    let mut block_comment = false;

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        let next = bytes.get(cursor + 1).copied();

        if line_comment {
            if byte == b'\n' {
                line_comment = false;
            }
            cursor += 1;
            continue;
        }
        if block_comment {
            if byte == b'*' && next == Some(b'/') {
                block_comment = false;
                cursor += 2;
            } else {
                cursor += 1;
            }
            continue;
        }
        if normal_string {
            if byte == b'\\' {
                cursor += 2;
            } else {
                normal_string = byte != b'"';
                cursor += 1;
            }
            continue;
        }
        if verbatim_string {
            if byte == b'"' && next == Some(b'"') {
                cursor += 2;
            } else {
                verbatim_string = byte != b'"';
                cursor += 1;
            }
            continue;
        }

        if byte == b'/' && next == Some(b'/') {
            line_comment = true;
            cursor += 2;
            continue;
        }
        if byte == b'/' && next == Some(b'*') {
            block_comment = true;
            cursor += 2;
            continue;
        }
        if byte == b'@' && next == Some(b'"') {
            verbatim_string = true;
            cursor += 2;
            continue;
        }
        if byte == b'@' && next == Some(b'$') && bytes.get(cursor + 2) == Some(&b'"') {
            verbatim_string = true;
            cursor += 3;
            continue;
        }
        if byte == b'$' && next == Some(b'@') && bytes.get(cursor + 2) == Some(&b'"') {
            verbatim_string = true;
            cursor += 3;
            continue;
        }
        if byte == b'"' {
            normal_string = true;
            cursor += 1;
            continue;
        }

        if byte == open {
            depth += 1;
        } else if byte == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }

    None
}

pub(super) fn find_top_level_comma_or_end(content: &str, start: usize, end: usize) -> usize {
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut normal_string = false;

    while cursor < end {
        let byte = content.as_bytes()[cursor];
        if normal_string {
            if byte == b'\\' {
                cursor += 2;
            } else {
                normal_string = byte != b'"';
                cursor += 1;
            }
            continue;
        }
        match byte {
            b'"' => normal_string = true,
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b',' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => return cursor,
            _ => {}
        }
        cursor += 1;
    }

    end
}

pub(super) fn smallest_node_covering_range<'tree>(
    node: Node<'tree>,
    start_byte: usize,
    end_byte: usize,
) -> Option<Node<'tree>> {
    smallest_node_covering_range_at_depth(node, start_byte, end_byte, 0)
}

fn smallest_node_covering_range_at_depth<'tree>(
    node: Node<'tree>,
    start_byte: usize,
    end_byte: usize,
    depth: u32,
) -> Option<Node<'tree>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.start_byte() > start_byte || node.end_byte() < end_byte {
        return None;
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return Some(node);
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(descendant) =
            smallest_node_covering_range_at_depth(child, start_byte, end_byte, child_depth)
        {
            return Some(descendant);
        }
    }

    Some(node)
}

pub(super) fn is_comment_or_string_node(node_kind: &str) -> bool {
    node_kind.contains("comment") || node_kind.contains("string")
}

pub(super) fn is_ignored_markup_node(mut node: Node<'_>) -> bool {
    loop {
        let kind = node.kind();
        if is_comment_or_string_node(kind)
            || matches!(
                kind,
                "raw_text" | "text" | "script_element" | "style_element"
            )
        {
            return true;
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
}

pub(super) fn is_identifier_boundary(content: &str, start: usize, len: usize) -> bool {
    let bytes = content.as_bytes();
    let before = start.checked_sub(1).and_then(|index| bytes.get(index));
    let after = bytes.get(start + len);
    !before.is_some_and(|byte| is_identifier_byte(*byte))
        && !after.is_some_and(|byte| is_identifier_byte(*byte))
}

fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

pub(super) fn skip_ascii_whitespace(content: &str, mut cursor: usize) -> usize {
    while content
        .as_bytes()
        .get(cursor)
        .is_some_and(u8::is_ascii_whitespace)
    {
        cursor += 1;
    }
    cursor
}

pub(super) fn skip_ascii_whitespace_until(content: &str, mut cursor: usize, end: usize) -> usize {
    while cursor < end
        && content
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
    {
        cursor += 1;
    }
    cursor
}
