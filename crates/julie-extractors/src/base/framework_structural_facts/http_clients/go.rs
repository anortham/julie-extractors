use tree_sitter::Tree;

use std::collections::HashSet;

use super::super::go_http::{collect_assignment_names, collect_go_imports};
use super::super::helpers::{is_identifier_boundary, skip_ascii_whitespace_until};
use super::super::scan::{
    MaskLanguage, SourceMask, find_matching_paren, find_top_level_comma_or_end,
    parse_go_string_literal,
};
use super::client_fact;
use crate::base::types::StructuralFact;

pub(super) fn collect_go_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let Some(http_alias) = collect_go_imports(content).net_http else {
        return Vec::new();
    };
    let mask = SourceMask::new(content, MaskLanguage::Go);
    let mut facts = Vec::new();
    let package_calls = [("NewRequest", "", 1), ("NewRequestWithContext", "", 2)];
    for (method, verb, url_arg) in CLIENT_CALLS.iter().chain(package_calls.iter()) {
        let needle = format!("{http_alias}.{method}");
        collect_calls(
            language,
            tree,
            file_path,
            content,
            &mask,
            &http_alias,
            &needle,
            &http_alias,
            verb,
            *url_arg,
            &mut facts,
        );
    }
    let mut clients = HashSet::from([format!("{http_alias}.DefaultClient")]);
    for literal in [
        format!("&{http_alias}.Client{{"),
        format!("{http_alias}.Client{{"),
    ] {
        collect_assignment_names(content, &mask, &literal, &mut clients);
    }
    for client in clients {
        for (method, verb, url_arg) in CLIENT_CALLS {
            let needle = format!("{client}.{method}");
            collect_calls(
                language,
                tree,
                file_path,
                content,
                &mask,
                &client,
                &needle,
                &http_alias,
                verb,
                *url_arg,
                &mut facts,
            );
        }
    }
    facts
}

/// Request methods on the `net/http` package and on an `*http.Client`: the
/// verb they send and the index of their URL argument.
const CLIENT_CALLS: &[(&str, &str, usize)] = &[
    ("Get", "GET", 0),
    ("Head", "HEAD", 0),
    ("Post", "POST", 0),
    ("PostForm", "POST", 0),
];

/// The verb a `NewRequest` method argument names: a string literal, or a
/// `net/http` method constant (`http.MethodPost`).
fn request_method_verb(
    content: &str,
    http_alias: &str,
    start: usize,
    end: usize,
) -> Option<String> {
    if let Some((method, literal_end)) = parse_go_string_literal(content, start) {
        return (skip_ascii_whitespace_until(content, literal_end, end) == end)
            .then(|| method.to_uppercase());
    }
    let constant = content.get(start..end)?.trim();
    let verb = constant
        .strip_prefix(http_alias)?
        .strip_prefix(".Method")?
        .to_uppercase();
    matches!(
        verb.as_str(),
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "CONNECT" | "OPTIONS" | "TRACE"
    )
    .then_some(verb)
}

#[allow(clippy::too_many_arguments)]
fn collect_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    receiver: &str,
    needle: &str,
    http_alias: &str,
    fixed_verb: &str,
    url_arg_index: usize,
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if !is_identifier_boundary(content, call_start, receiver.len())
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
        let args = call_arguments(content, mask, open + 1, close);
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
            let Some(verb) = request_method_verb(content, http_alias, method_start, method_end)
            else {
                continue;
            };
            verb
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
            "net/http",
            &target_path,
            &verb,
            "attested",
            Some("net/http"),
        ) {
            facts.push(fact);
        }
    }
}

fn call_arguments(
    content: &str,
    mask: &SourceMask,
    mut cursor: usize,
    end: usize,
) -> Vec<(usize, usize)> {
    let mut args = Vec::new();
    while cursor < end {
        cursor = skip_ascii_whitespace_until(content, cursor, end);
        if cursor >= end {
            break;
        }
        let arg_end = find_top_level_comma_or_end(content, mask, cursor, end);
        args.push((cursor, arg_end));
        cursor = arg_end.saturating_add(1);
    }
    args
}
