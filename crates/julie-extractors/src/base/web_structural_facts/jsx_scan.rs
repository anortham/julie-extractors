use super::js_object_scan::{
    find_matching_brace, parse_js_identifier, parse_js_string_literal,
    parse_object_string_property, skip_ascii_whitespace_until,
};
use crate::base::markup_scan::{find_tag_end, is_attr_name_byte, is_markup_tag_start};
use crate::base::span::NormalizedSpan;

#[derive(Debug)]
struct JsxAttributeSpan {
    value_start: Option<usize>,
    value_end: usize,
    span: NormalizedSpan,
}

pub(super) fn next_markup_tag(
    content: &str,
    start: usize,
    end: usize,
) -> Option<(usize, usize, &str)> {
    let mut cursor = start;
    while cursor < end {
        let relative_tag_start = content.get(cursor..end)?.find('<')?;
        let tag_start = cursor + relative_tag_start;
        let tag_end = find_tag_end(content, tag_start).filter(|tag_end| *tag_end <= end)?;
        cursor = tag_end + 1;
        if !is_markup_tag_start(content.as_bytes(), tag_start) {
            continue;
        }
        let Some(tag_name) = markup_tag_name(content, tag_start, tag_end) else {
            continue;
        };
        return Some((tag_start, tag_end, tag_name));
    }
    None
}

pub(super) fn jsx_string_literal_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<(String, NormalizedSpan)> {
    let attribute = find_jsx_attribute(content, tag_start, tag_end, attribute_name)?;
    let value_start = attribute.value_start?;
    let bytes = content.as_bytes();
    let value = if matches!(bytes.get(value_start), Some(b'\"') | Some(b'\'')) {
        parse_js_string_literal(content, value_start)?.0
    } else if bytes.get(value_start) == Some(&b'{') {
        let close = find_matching_brace(content, value_start, attribute.value_end)?;
        let literal_start = skip_ascii_whitespace_until(content, value_start + 1, close);
        let (value, literal_end) = parse_js_string_literal(content, literal_start)?;
        let trailing = skip_ascii_whitespace_until(content, literal_end, close);
        if trailing != close {
            return None;
        }
        value
    } else {
        return None;
    };
    Some((value, attribute.span))
}

pub(super) fn jsx_object_pathname_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<(String, NormalizedSpan)> {
    let attribute = find_jsx_attribute(content, tag_start, tag_end, attribute_name)?;
    let value_start = attribute.value_start?;
    if content.as_bytes().get(value_start) != Some(&b'{') {
        return None;
    }
    let close = find_matching_brace(content, value_start, attribute.value_end)?;
    let object_start = skip_ascii_whitespace_until(content, value_start + 1, close);
    if content.as_bytes().get(object_start) != Some(&b'{') {
        return None;
    }
    let object_end = find_matching_brace(content, object_start, close)?;
    let value = parse_object_string_property(content, object_start, object_end + 1, "pathname")?;
    Some((value, attribute.span))
}

pub(super) fn jsx_boolean_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> bool {
    find_jsx_attribute(content, tag_start, tag_end, attribute_name)
        .is_some_and(|attribute| attribute.value_start.is_none())
}

pub(super) fn jsx_identifier_expression_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<String> {
    let attribute = find_jsx_attribute(content, tag_start, tag_end, attribute_name)?;
    let value_start = attribute.value_start?;
    if content.as_bytes().get(value_start) != Some(&b'{') {
        return None;
    }
    let close = find_matching_brace(content, value_start, attribute.value_end)?;
    let identifier_start = skip_ascii_whitespace_until(content, value_start + 1, close);
    let (identifier, identifier_end) = parse_js_identifier(content, identifier_start, close)?;
    let trailing = skip_ascii_whitespace_until(content, identifier_end, close);
    (trailing == close).then_some(identifier)
}

pub(super) fn jsx_element_component_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<String> {
    let attribute = find_jsx_attribute(content, tag_start, tag_end, attribute_name)?;
    let value_start = attribute.value_start?;
    if content.as_bytes().get(value_start) != Some(&b'{') {
        return parse_jsx_element_component_at(content, value_start, attribute.value_end);
    }
    let close = find_matching_brace(content, value_start, attribute.value_end)?;
    let expression_start = skip_ascii_whitespace_until(content, value_start + 1, close);
    parse_jsx_element_component_at(content, expression_start, close)
}

pub(super) fn parse_jsx_element_component_at(
    content: &str,
    value_start: usize,
    end: usize,
) -> Option<String> {
    if content.as_bytes().get(value_start) != Some(&b'<') {
        return None;
    }
    let component_start = value_start + 1;
    if matches!(
        content.as_bytes().get(component_start),
        Some(b'>') | Some(b'/')
    ) {
        return None;
    }
    parse_js_identifier(content, component_start, end).map(|(identifier, _)| identifier)
}

fn find_jsx_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<JsxAttributeSpan> {
    let bytes = content.as_bytes();
    let mut cursor = tag_start + 1;
    while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
        cursor += 1;
    }

    while cursor < tag_end {
        cursor = skip_ascii_whitespace_until(content, cursor, tag_end);
        if cursor >= tag_end || bytes[cursor] == b'/' {
            cursor += 1;
            continue;
        }

        let attribute_start = cursor;
        while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
            cursor += 1;
        }
        if cursor == attribute_start {
            cursor += 1;
            continue;
        }

        let attribute_end = cursor;
        let Some(name) = content.get(attribute_start..attribute_end) else {
            continue;
        };
        let after_name = skip_ascii_whitespace_until(content, cursor, tag_end);
        if content.as_bytes().get(after_name) != Some(&b'=') {
            if name != attribute_name {
                continue;
            }
            let span = NormalizedSpan::from_content_range(content, attribute_start, cursor)?;
            return Some(JsxAttributeSpan {
                value_start: None,
                value_end: cursor,
                span,
            });
        }
        let value_start = skip_ascii_whitespace_until(content, after_name + 1, tag_end);
        let value_end = jsx_attribute_value_end(content, value_start, tag_end)?;
        cursor = value_end;
        if name != attribute_name {
            continue;
        }
        let span = NormalizedSpan::from_content_range(content, attribute_start, value_end)?;
        return Some(JsxAttributeSpan {
            value_start: Some(value_start),
            value_end,
            span,
        });
    }
    None
}

fn jsx_attribute_value_end(content: &str, value_start: usize, tag_end: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    match bytes.get(value_start)? {
        b'\'' | b'\"' => {
            let (_, end) = parse_js_string_literal(content, value_start)?;
            Some(end)
        }
        b'{' => find_matching_brace(content, value_start, tag_end).map(|end| end + 1),
        _ => {
            let mut cursor = value_start;
            while cursor < tag_end
                && !bytes[cursor].is_ascii_whitespace()
                && !matches!(bytes[cursor], b'/' | b'>')
            {
                cursor += 1;
            }
            Some(cursor)
        }
    }
}

fn markup_tag_name(content: &str, tag_start: usize, tag_end: usize) -> Option<&str> {
    let bytes = content.as_bytes();
    let mut cursor = tag_start + 1;
    let name_start = cursor;
    while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
        cursor += 1;
    }
    (cursor > name_start)
        .then(|| content.get(name_start..cursor))
        .flatten()
}

pub(super) fn parse_attr_value(attrs: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let prefix = format!("{name}={quote}");
        if let Some(start) = attrs.find(&prefix) {
            let value_start = start + prefix.len();
            let value_end = attrs[value_start..].find(quote)? + value_start;
            return Some(attrs[value_start..value_end].to_string());
        }
    }
    None
}

pub(super) fn has_boolean_attr(attrs: &str, name: &str) -> bool {
    attrs
        .split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '<' | '>' | '/'))
        .any(|part| part == name)
}
