use tree_sitter::Tree;

use super::super::go_http::collect_go_imports;
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
            language,
            tree,
            file_path,
            content,
            &mask,
            &http_alias,
            &needle,
            verb,
            url_arg,
            &mut facts,
        );
    }
    facts
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
