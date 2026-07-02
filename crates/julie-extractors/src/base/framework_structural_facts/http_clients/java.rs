use tree_sitter::Tree;

use super::super::HTTP_CLIENT_REQUEST_PATTERN_ID;
use super::super::helpers::{
    fact_for_span, is_comment_or_string_node, skip_ascii_whitespace_until,
    smallest_node_covering_range,
};
use crate::base::http_boundary::client_request_metadata;
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

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
    let mut facts = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find("HttpRequest.newBuilder") {
        let start = cursor + relative;
        cursor = start + "HttpRequest.newBuilder".len();
        if is_in_java_string_or_comment(content, start) {
            continue;
        }
        let end = statement_end(content, start);
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
            &target_path,
            &verb,
            source,
        ) {
            facts.push(fact);
        }
    }
    facts
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
    verb_source: &str,
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
        client_request_metadata("java.net.http", target_path, verb, verb_source, None),
    ))
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

fn parse_java_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    if content.as_bytes().get(start) != Some(&b'"') {
        return None;
    }
    let mut cursor = start + 1;
    let mut value = String::new();
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        if byte == b'\\' {
            let escaped_start = cursor + 1;
            let escaped = content.get(escaped_start..)?.chars().next()?;
            value.push(escaped);
            cursor = escaped_start + escaped.len_utf8();
        } else if byte == b'"' {
            return Some((value, cursor + 1));
        } else {
            let ch = content.get(cursor..)?.chars().next()?;
            value.push(ch);
            cursor += ch.len_utf8();
        }
    }
    None
}

fn statement_end(content: &str, start: usize) -> usize {
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut quote = false;
    let mut escaped = false;
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        if quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quote = false;
            }
        } else if byte == b'"' {
            quote = true;
        } else if byte == b'(' {
            paren_depth += 1;
        } else if byte == b')' {
            paren_depth = paren_depth.saturating_sub(1);
        } else if byte == b';' && paren_depth == 0 {
            return cursor + 1;
        }
        cursor += 1;
    }
    content.len()
}

fn is_in_java_string_or_comment(content: &str, target: usize) -> bool {
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
