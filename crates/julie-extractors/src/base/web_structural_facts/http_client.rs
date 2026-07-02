use tree_sitter::Tree;

use super::HTTP_CLIENT_REQUEST_PATTERN_ID;
use super::fact_builders::{base_metadata, fact_for_span, insert_string};
use super::js_object_scan::{
    find_matching_paren, find_object_property_value_start, find_top_level_comma_or_end,
    is_identifier_boundary, is_ignored_syntax_range, parse_js_string_literal,
    skip_ascii_whitespace_until,
};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

const FETCH_IDENTIFIER: &str = "fetch";

/// Collects `http.client_request.v1` facts for global `fetch()` calls.
///
/// Repo doctrine keeps dynamic requests silent: the first argument must be a
/// plain static string literal (`'...'` or `"..."`). Template literals stay
/// silent even without interpolation because they are not a plain string
/// literal, and a concatenated / expression first argument is silent too.
/// `fetch` is a global, so there is no import gate — but property calls
/// (`obj.fetch(...)`) and matches inside comments or strings are rejected. When
/// an options object carries a `method:` property whose value is not a static
/// string literal, the call emits nothing rather than silently degrading to
/// GET.
pub(super) fn collect_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut cursor = 0;

    while cursor < content.len() {
        let Some(relative_start) = content[cursor..].find(FETCH_IDENTIFIER) else {
            break;
        };
        let name_start = cursor + relative_start;
        cursor = name_start + FETCH_IDENTIFIER.len();

        if !is_identifier_boundary(content, name_start, FETCH_IDENTIFIER.len()) {
            continue;
        }
        if preceding_non_whitespace_is_dot(content, name_start) {
            continue;
        }

        let open_paren = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open_paren) != Some(&b'(') {
            continue;
        }
        if is_ignored_syntax_range(tree, name_start, open_paren + 1) {
            continue;
        }
        let Some(close_paren) = find_matching_paren(content, open_paren, content.len()) else {
            continue;
        };

        let first_arg_start = skip_ascii_whitespace_until(content, open_paren + 1, close_paren);
        let first_arg_end = find_top_level_comma_or_end(content, first_arg_start, close_paren);
        let Some((target_path, url_end)) = parse_js_string_literal(content, first_arg_start) else {
            continue;
        };
        // Reject anything other than a plain string literal spanning the whole
        // first argument (e.g. `"/api" + suffix`).
        if skip_ascii_whitespace_until(content, url_end, first_arg_end) != first_arg_end {
            continue;
        }

        let verb = match resolve_verb(content, first_arg_end, close_paren) {
            VerbResolution::Get => Verb::default_get(),
            VerbResolution::Attested(method) => Verb::attested(method),
            VerbResolution::Silent => continue,
        };

        let Some(span) = NormalizedSpan::from_content_range(content, name_start, close_paren + 1)
        else {
            continue;
        };

        let mut metadata = base_metadata("web.http_client");
        insert_string(&mut metadata, "framework", "fetch");
        insert_string(&mut metadata, "client", "fetch");
        insert_string(&mut metadata, "target_path", &target_path);
        insert_string(&mut metadata, "url_kind", classify_url_kind(&target_path));
        insert_string(&mut metadata, "verb", &verb.name);
        insert_string(&mut metadata, "verb_source", verb.source);

        facts.push(fact_for_span(
            file_path,
            language,
            HTTP_CLIENT_REQUEST_PATTERN_ID,
            "client_request",
            "call_expression",
            span,
            metadata,
        ));
    }

    facts
}

struct Verb {
    name: String,
    source: &'static str,
}

impl Verb {
    fn default_get() -> Self {
        Verb {
            name: "GET".to_string(),
            source: "default",
        }
    }

    fn attested(method: String) -> Self {
        Verb {
            name: method.to_uppercase(),
            source: "attested",
        }
    }
}

enum VerbResolution {
    Get,
    Attested(String),
    Silent,
}

/// Resolves the HTTP verb from the options object (the second argument).
///
/// - No options object or no `method:` property → GET by fetch's spec default.
/// - `method:` bound to a static string literal → attested verb.
/// - `method:` present but bound to an identifier/expression → `Silent`: the
///   call emits nothing so an attested-but-unreadable verb never degrades to
///   GET.
fn resolve_verb(content: &str, first_arg_end: usize, close_paren: usize) -> VerbResolution {
    if content.as_bytes().get(first_arg_end) != Some(&b',') {
        return VerbResolution::Get;
    }
    let options_start = skip_ascii_whitespace_until(content, first_arg_end + 1, close_paren);
    let Some(method_value_start) =
        find_object_property_value_start(content, options_start, close_paren, "method")
    else {
        return VerbResolution::Get;
    };
    match parse_js_string_literal(content, method_value_start) {
        Some((method, _)) => VerbResolution::Attested(method),
        None => VerbResolution::Silent,
    }
}

fn classify_url_kind(url: &str) -> &'static str {
    if url.starts_with('/') {
        "path"
    } else if url.contains("://") {
        "absolute"
    } else {
        "relative"
    }
}

/// Returns true when the nearest non-whitespace byte before `start` is a `.`,
/// which marks a property call such as `obj.fetch(...)` or `obj?.fetch(...)`.
fn preceding_non_whitespace_is_dot(content: &str, start: usize) -> bool {
    let bytes = content.as_bytes();
    let mut index = start;
    while index > 0 {
        index -= 1;
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            continue;
        }
        return byte == b'.';
    }
    false
}
