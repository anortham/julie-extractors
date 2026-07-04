use tree_sitter::Tree;

use super::super::helpers::{is_identifier_boundary, skip_ascii_whitespace_until};
use super::super::scan::{
    MaskLanguage, SourceMask, find_matching_paren, parse_ruby_string_literal,
};
use super::client_fact;
use crate::base::types::StructuralFact;

pub(super) fn collect_ruby_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains("Net::HTTP.") {
        return Vec::new();
    }
    let mask = SourceMask::new(content, MaskLanguage::Ruby);
    let mut facts = Vec::new();
    for (method, verb) in [
        ("get", "GET"),
        ("get_response", "GET"),
        ("post", "POST"),
        ("post_form", "POST"),
    ] {
        collect_net_http_calls(
            language, tree, file_path, content, &mask, method, verb, &mut facts,
        );
    }
    facts
}

#[allow(clippy::too_many_arguments)]
fn collect_net_http_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
    method: &str,
    verb: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let needle = format!("Net::HTTP.{method}");
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(&needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if !is_identifier_boundary(content, call_start, needle.len())
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
        let first_start = skip_ascii_whitespace_until(content, open + 1, close);
        let Some(target_path) = uri_literal_arg(content, mask, first_start) else {
            continue;
        };
        if let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            call_start,
            close + 1,
            "net::http",
            &target_path,
            verb,
            "attested",
            None,
        ) {
            facts.push(fact);
        }
    }
}

fn uri_literal_arg(content: &str, mask: &SourceMask, start: usize) -> Option<String> {
    for prefix in ["URI.parse", "URI"] {
        if content[start..].starts_with(prefix) {
            let open = skip_ascii_whitespace_until(content, start + prefix.len(), content.len());
            if content.as_bytes().get(open) != Some(&b'(') {
                continue;
            }
            let close = find_matching_paren(content, mask, open)?;
            let arg_start = skip_ascii_whitespace_until(content, open + 1, content.len());
            return parse_ruby_string_literal(content, arg_start)
                .filter(|(value, literal_end)| {
                    skip_ascii_whitespace_until(content, *literal_end, close) == close
                        && (content.as_bytes().get(arg_start) != Some(&b'"')
                            || !value.contains("#{"))
                })
                .map(|(value, _)| value);
        }
    }
    None
}
