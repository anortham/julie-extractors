//! Shared markup attribute scanning.
//!
//! This is the single canonical copy of the byte-level markup attribute scanner
//! that used to be duplicated in `framework_structural_facts.rs` (htmx/Alpine)
//! and `web_structural_facts.rs` (Vue/HTML). The two copies drifted
//! independently; this module keeps the superset behavior so both collectors
//! emit identically.
//!
//! `MarkupAttribute` is the superset of the two former shapes: framework
//! consumers read `start_byte`/`end_byte`, web consumers read `tag_name`/`span`.
//! Every scan populates all six fields.

use super::span::NormalizedSpan;

#[derive(Debug)]
pub(crate) struct MarkupAttribute {
    pub(crate) tag_name: String,
    pub(crate) name: String,
    pub(crate) value: Option<String>,
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) span: NormalizedSpan,
}

pub(crate) fn scan_markup_attributes(
    content: &str,
    start: usize,
    end: usize,
) -> Vec<MarkupAttribute> {
    let mut attributes = Vec::new();
    let mut cursor = start;

    while cursor < end {
        let Some(relative_tag_start) = content[cursor..end].find('<') else {
            break;
        };
        let tag_start = cursor + relative_tag_start;
        let Some(tag_end) = find_tag_end(content, tag_start).filter(|tag_end| *tag_end <= end)
        else {
            break;
        };
        if is_markup_tag_start(content.as_bytes(), tag_start) {
            scan_tag_attributes(content, tag_start, tag_end, &mut attributes);
        }
        cursor = tag_end + 1;
    }

    attributes
}

pub(crate) fn scan_tag_attributes(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attributes: &mut Vec<MarkupAttribute>,
) {
    let bytes = content.as_bytes();
    let mut cursor = tag_start + 1;
    while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
        cursor += 1;
    }
    let tag_name = content
        .get(tag_start + 1..cursor)
        .unwrap_or_default()
        .to_ascii_lowercase();

    while cursor < tag_end {
        cursor = skip_ascii_whitespace_until(content, cursor, tag_end);
        if cursor >= tag_end || bytes[cursor] == b'/' {
            cursor += 1;
            continue;
        }

        let name_start = cursor;
        while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
            cursor += 1;
        }
        if cursor == name_start {
            cursor += 1;
            continue;
        }

        let name_end = cursor;
        let mut value = None;
        let mut attr_end = name_end;
        cursor = skip_ascii_whitespace_until(content, cursor, tag_end);
        if cursor < tag_end && bytes[cursor] == b'=' {
            cursor = skip_ascii_whitespace_until(content, cursor + 1, tag_end);
            let (parsed_value, value_end) = parse_markup_attribute_value(content, cursor, tag_end);
            value = parsed_value;
            attr_end = value_end;
            cursor = value_end;
        }

        let Some(span) = NormalizedSpan::from_content_range(content, name_start, attr_end) else {
            continue;
        };
        let Some(name) = content.get(name_start..name_end) else {
            continue;
        };
        attributes.push(MarkupAttribute {
            tag_name: tag_name.clone(),
            name: name.to_string(),
            value,
            start_byte: name_start,
            end_byte: attr_end,
            span,
        });
    }
}

pub(crate) fn parse_markup_attribute_value(
    content: &str,
    value_start: usize,
    tag_end: usize,
) -> (Option<String>, usize) {
    let bytes = content.as_bytes();
    let Some(quote) = bytes
        .get(value_start)
        .copied()
        .filter(|byte| matches!(*byte, b'"' | b'\''))
    else {
        let mut value_end = value_start;
        while value_end < tag_end && !bytes[value_end].is_ascii_whitespace() {
            value_end += 1;
        }
        return (
            content.get(value_start..value_end).map(ToString::to_string),
            value_end,
        );
    };

    let mut value_end = value_start + 1;
    while value_end < tag_end && bytes[value_end] != quote {
        value_end += 1;
    }
    let value = content
        .get(value_start + 1..value_end)
        .map(ToString::to_string);
    let attr_end = if value_end < tag_end {
        value_end + 1
    } else {
        value_end
    };
    (value, attr_end)
}

// Superset of the two former `find_tag_end` copies. The web copy additionally
// tracks brace depth (so `>` inside a `{...}` JSX/Vue binding does not end the
// tag) and quote escapes (`\"`). Framework markup (htmx/Alpine on HTML) never
// contains unquoted braces or backslash-escaped quotes inside a tag, so this
// superset returns the same tag end for framework inputs while also handling
// the web collector's Vue/JSX shorthand ranges.
pub(crate) fn find_tag_end(content: &str, tag_start: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut cursor = tag_start + 1;
    let mut quote = None;
    let mut brace_depth = 0usize;
    let mut escaped = false;

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'{' {
            brace_depth += 1;
        } else if byte == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
        } else if byte == b'>' && brace_depth == 0 {
            return Some(cursor);
        }
        cursor += 1;
    }

    None
}

pub(crate) fn is_markup_tag_start(bytes: &[u8], tag_start: usize) -> bool {
    let Some(next) = bytes.get(tag_start + 1) else {
        return false;
    };
    !matches!(*next, b'!' | b'?' | b'/')
}

pub(crate) fn is_attr_name_byte(byte: u8) -> bool {
    !byte.is_ascii_whitespace() && !matches!(byte, b'=' | b'/' | b'>' | b'<')
}

pub(crate) fn split_argument_and_modifiers(value: &str) -> (Option<String>, Vec<String>) {
    let mut parts = value.split('.').filter(|part| !part.is_empty());
    let argument = parts.next().map(ToString::to_string);
    let modifiers = parts.map(ToString::to_string).collect();
    (argument, modifiers)
}

fn skip_ascii_whitespace_until(content: &str, mut cursor: usize, end: usize) -> usize {
    let bytes = content.as_bytes();
    while cursor < end && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}
