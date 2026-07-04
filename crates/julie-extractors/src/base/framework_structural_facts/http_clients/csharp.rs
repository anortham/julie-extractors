use std::collections::HashSet;

use tree_sitter::Tree;

use super::super::helpers::{
    is_identifier_boundary, parse_csharp_string_literal, skip_ascii_whitespace_until,
};
use super::super::scan::{
    MaskLanguage, SourceMask, find_matching_angle_within, find_matching_paren,
    find_top_level_comma_or_end_with_angles,
};
use super::client_fact;
use crate::base::http_boundary::classify_url;
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
    let mask = SourceMask::new(content, MaskLanguage::CSharp);
    let receivers = collect_httpclient_receivers(content, &mask);
    let mut facts = Vec::new();
    for (method, verb) in HTTPCLIENT_METHODS {
        collect_method_calls(
            language, tree, file_path, content, &mask, &receivers, method, verb, &mut facts,
        );
    }
    collect_http_request_messages(language, tree, file_path, content, &mask, &mut facts);
    facts
}

#[allow(clippy::too_many_arguments)]
fn collect_method_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    receivers: &HashSet<String>,
    method: &str,
    verb: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(method) {
        let method_start = cursor + relative;
        cursor = method_start + method.len();
        if !is_identifier_boundary(content, method_start, method.len())
            || mask.is_string_or_comment(method_start)
            || !method_receiver_is_httpclient(content, method_start, receivers)
        {
            continue;
        }
        let mut open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) == Some(&b'<') {
            let Some(generics_close) =
                find_matching_angle_within(content, mask, open, content.len())
            else {
                continue;
            };
            open = skip_ascii_whitespace_until(content, generics_close + 1, content.len());
        }
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, mask, open) else {
            continue;
        };
        let first_start = skip_ascii_whitespace_until(content, open + 1, close);
        let first_end = find_top_level_comma_or_end_with_angles(content, mask, first_start, close);
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
            "httpclient",
            &target_path,
            verb,
            "attested",
            None,
        ) {
            facts.push(fact);
        }
    }
}

fn collect_httpclient_receivers(content: &str, mask: &SourceMask) -> HashSet<String> {
    let mut receivers = HashSet::new();
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find("HttpClient") {
        let type_start = cursor + relative;
        cursor = type_start + "HttpClient".len();
        if !is_identifier_boundary(content, type_start, "HttpClient".len())
            || mask.is_string_or_comment(type_start)
        {
            continue;
        }
        let name_start = skip_ascii_whitespace_until(content, cursor, content.len());
        if let Some((name, _)) = parse_csharp_identifier(content, name_start) {
            receivers.insert(name.to_string());
        }
    }

    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find("new HttpClient()") {
        let call_start = cursor + relative;
        cursor = call_start + "new HttpClient()".len();
        if mask.is_string_or_comment(call_start) {
            continue;
        }
        let statement_start = content[..call_start]
            .rfind(['\n', ';', '{'])
            .map(|index| index + 1)
            .unwrap_or(0);
        let before = content[statement_start..call_start].trim();
        if let Some(name) = csharp_assignment_name_before_call(before) {
            receivers.insert(name.to_string());
        }
    }

    receivers
}

fn method_receiver_is_httpclient(
    content: &str,
    method_start: usize,
    receivers: &HashSet<String>,
) -> bool {
    let Some(dot) = content[..method_start].trim_end().strip_suffix('.') else {
        return false;
    };
    let receiver_end = dot.len();
    let receiver_start = dot
        .char_indices()
        .rev()
        .find(|(_, ch)| !is_csharp_identifier_char(*ch))
        .map(|(index, ch)| index + ch.len_utf8())
        .unwrap_or(0);
    let receiver = dot[receiver_start..receiver_end].trim();
    receivers.contains(receiver)
}

fn csharp_assignment_name_before_call(before: &str) -> Option<&str> {
    let (left, _) = before.rsplit_once('=')?;
    let left = left.trim();
    let name = left
        .split(|ch: char| !is_csharp_identifier_char(ch))
        .rfind(|token| !token.is_empty())?;
    is_csharp_identifier(name).then_some(name)
}

fn parse_csharp_identifier(content: &str, start: usize) -> Option<(&str, usize)> {
    let mut chars = content[start..].char_indices();
    let (_, first) = chars.next()?;
    if !is_csharp_identifier_start(first) {
        return None;
    }
    let mut end = start + first.len_utf8();
    for (relative, ch) in chars {
        if !is_csharp_identifier_char(ch) {
            return Some((&content[start..end], end));
        }
        end = start + relative + ch.len_utf8();
    }
    Some((&content[start..end], end))
}

fn is_csharp_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    is_csharp_identifier_start(first) && chars.all(is_csharp_identifier_char)
}

fn is_csharp_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_csharp_identifier_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn collect_http_request_messages(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    facts: &mut Vec<StructuralFact>,
) {
    let needle = "new HttpRequestMessage";
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if mask.is_string_or_comment(call_start) {
            continue;
        }
        let open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, mask, open) else {
            continue;
        };
        let method_start = skip_ascii_whitespace_until(content, open + 1, close);
        let method_end =
            find_top_level_comma_or_end_with_angles(content, mask, method_start, close);
        let Some(verb) = content[method_start..method_end]
            .trim()
            .strip_prefix("HttpMethod.")
            .map(|value| value.to_uppercase())
        else {
            continue;
        };
        let url_start = skip_ascii_whitespace_until(content, method_end + 1, close);
        let url_end = find_top_level_comma_or_end_with_angles(content, mask, url_start, close);
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
            "httpclient",
            &target_path,
            &verb,
            "attested",
            None,
        ) {
            facts.push(fact);
        }
    }
}

fn parse_csharp_url_literal(content: &str, start: usize) -> Option<(String, usize)> {
    parse_csharp_raw_string_literal(content, start)
        .or_else(|| parse_csharp_string_literal(content, start).map(|(value, end, _)| (value, end)))
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
