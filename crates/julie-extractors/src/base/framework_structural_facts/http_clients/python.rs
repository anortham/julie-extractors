use std::collections::HashMap;

use tree_sitter::Tree;

use super::super::helpers::{
    is_ascii_identifier, is_identifier_boundary, skip_ascii_whitespace_until,
};
use super::super::scan::{
    MaskLanguage, SourceMask, find_matching_paren, find_top_level_comma_or_end,
    parse_python_string_literal,
};
use super::client_fact;
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
    if imports.is_empty() {
        return Vec::new();
    }
    let mask = SourceMask::new(content, MaskLanguage::Python);
    let mut facts = Vec::new();
    for (local, client) in imports {
        collect_method_requests(
            language, tree, file_path, content, &mask, &local, &client, &mut facts,
        );
        collect_request_calls(
            language, tree, file_path, content, &mask, &local, &client, &mut facts,
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
                .filter(|alias| is_ascii_identifier(alias))
            {
                imports.insert(alias.to_string(), module.to_string());
            }
        }
    }
    imports
}

#[allow(clippy::too_many_arguments)]
fn collect_method_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
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
            let first_end = find_top_level_comma_or_end(content, mask, first_start, close);
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

#[allow(clippy::too_many_arguments)]
fn collect_request_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    mask: &SourceMask,
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
        let method_start = skip_ascii_whitespace_until(content, open + 1, close);
        let method_end = find_top_level_comma_or_end(content, mask, method_start, close);
        let Some((method, method_literal_end)) = parse_python_string_literal(content, method_start)
        else {
            continue;
        };
        if skip_ascii_whitespace_until(content, method_literal_end, method_end) != method_end {
            continue;
        }
        let url_start = skip_ascii_whitespace_until(content, method_end + 1, close);
        let url_end = find_top_level_comma_or_end(content, mask, url_start, close);
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
