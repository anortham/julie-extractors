use tree_sitter::Tree;

use super::super::helpers::{is_identifier_boundary, skip_ascii_whitespace_until};
use super::super::scan::{MaskLanguage, SourceMask, parse_java_string_literal, statement_end};
use super::client_fact;
use crate::base::types::StructuralFact;

const BUILDER_NEEDLE: &str = "HttpRequest.newBuilder";

pub(super) fn collect_java_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains("import java.net.http.HttpRequest")
        && !content.contains("import java.net.http.*")
    {
        return Vec::new();
    }
    let mask = SourceMask::new(content, MaskLanguage::Java);
    let mut facts = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(BUILDER_NEEDLE) {
        let start = cursor + relative;
        cursor = start + BUILDER_NEEDLE.len();
        if !is_identifier_boundary(content, start, BUILDER_NEEDLE.len())
            || mask.is_string_or_comment(start)
        {
            continue;
        }
        let end = statement_end(content, &mask, start, false);
        let statement = &content[start..end];
        let Some(target_path) = uri_create_literal(statement) else {
            continue;
        };
        let (verb, source) = request_builder_verb(statement);
        if let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            start,
            end,
            "java.net.http",
            &target_path,
            &verb,
            source,
            None,
        ) {
            facts.push(fact);
        }
    }
    facts
}

fn uri_create_literal(statement: &str) -> Option<String> {
    let needle = "URI.create";
    let start = statement.find(needle)? + needle.len();
    let open = skip_ascii_whitespace_until(statement, start, statement.len());
    if statement.as_bytes().get(open) != Some(&b'(') {
        return None;
    }
    let arg_start = skip_ascii_whitespace_until(statement, open + 1, statement.len());
    parse_java_string_literal(statement, arg_start).map(|(value, _)| value)
}

fn request_builder_verb(statement: &str) -> (String, &'static str) {
    for verb in ["GET", "POST", "PUT", "DELETE"] {
        if statement.contains(&format!(".{verb}(")) {
            return (verb.to_string(), "attested");
        }
    }
    if let Some(method_start) = statement.find(".method(") {
        let open = method_start + ".method".len();
        let arg_start = skip_ascii_whitespace_until(statement, open + 1, statement.len());
        if let Some((verb, _)) = parse_java_string_literal(statement, arg_start) {
            return (verb.to_uppercase(), "attested");
        }
    }
    ("GET".to_string(), "default")
}
