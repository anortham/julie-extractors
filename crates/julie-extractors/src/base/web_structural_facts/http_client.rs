use tree_sitter::Tree;

use super::HTTP_CLIENT_REQUEST_PATTERN_ID;
use super::fact_builders::{base_metadata, fact_for_span, insert_string};
use super::js_imports::JsImportIndex;
use super::js_object_scan::{
    find_matching_paren, find_top_level_comma_or_end, is_identifier_boundary,
    is_ignored_syntax_range, parse_js_identifier, parse_js_string_literal,
    skip_ascii_whitespace_until,
};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

const FETCH_IDENTIFIER: &str = "fetch";

const AXIOS_VERB_METHODS: &[&str] = &["get", "post", "put", "patch", "delete", "head", "options"];

/// Collects `http.client_request.v1` facts for global `fetch()` calls and
/// import-gated axios calls within `content[start..end]`.
///
/// Repo doctrine keeps dynamic requests silent: the first argument must be a
/// plain static string literal (`'...'` or `"..."`). Template literals stay
/// silent even without interpolation because they are not a plain string
/// literal, and a concatenated / expression first argument is silent too.
/// `fetch` is a global, so there is no import gate — but property calls
/// (`obj.fetch(...)`) and matches inside comments or strings are rejected.
/// Axios calls emit only when the scanned range imports axios (default or
/// namespace import; the call site is matched on the LOCAL binding, so
/// `import http from "axios"` gates `http.*`). When an options object carries
/// a `method:` property whose value is not a static string literal, the call
/// emits nothing rather than silently degrading to GET.
///
/// The `[start, end)` range covers the whole file for JS-family languages and
/// a single `<script>` section for Vue SFCs, keeping the import gate local to
/// the section that declares it.
pub(super) fn collect_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &JsImportIndex,
    start: usize,
    end: usize,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    collect_fetch_requests(language, tree, file_path, content, start, end, &mut facts);
    for (local, source) in &imports.axios_clients {
        collect_axios_requests(
            language, tree, file_path, content, local, source, start, end, &mut facts,
        );
    }
    facts
}

fn collect_fetch_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    start: usize,
    end: usize,
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = start;

    while cursor < end {
        let Some(relative_start) = content[cursor..end].find(FETCH_IDENTIFIER) else {
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

        let open_paren = skip_ascii_whitespace_until(content, cursor, end);
        if content.as_bytes().get(open_paren) != Some(&b'(') {
            continue;
        }

        push_client_request_fact(
            language, tree, file_path, content, name_start, open_paren, end, "fetch", None, facts,
        );
    }
}

/// Scans for `local.get/post/...("literal")`, `local.get<T>("literal")`, and
/// direct `local("literal", ...)` calls, where `local` is the range's axios
/// binding.
#[allow(clippy::too_many_arguments)]
fn collect_axios_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    local: &str,
    import_source: &str,
    start: usize,
    end: usize,
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = start;

    while cursor < end {
        let Some(relative_start) = content[cursor..end].find(local) else {
            break;
        };
        let name_start = cursor + relative_start;
        cursor = name_start + local.len();

        if !is_identifier_boundary(content, name_start, local.len()) {
            continue;
        }
        if preceding_non_whitespace_is_dot(content, name_start) {
            continue;
        }

        let after_name = skip_ascii_whitespace_until(content, cursor, end);
        let (method_verb, open_paren) = match content.as_bytes().get(after_name) {
            Some(&b'.') => {
                let method_start = skip_ascii_whitespace_until(content, after_name + 1, end);
                let Some((method, method_end)) = parse_js_identifier(content, method_start, end)
                else {
                    continue;
                };
                if !AXIOS_VERB_METHODS.contains(&method.as_str()) {
                    continue;
                }
                let mut paren = skip_ascii_whitespace_until(content, method_end, end);
                // TS call sites may carry generic type arguments between the
                // method name and the argument list: `axios.get<Msg[]>("/x")`.
                if content.as_bytes().get(paren) == Some(&b'<') {
                    let Some(after_generics) = skip_generic_type_arguments(content, paren, end)
                    else {
                        continue;
                    };
                    paren = skip_ascii_whitespace_until(content, after_generics, end);
                }
                if content.as_bytes().get(paren) != Some(&b'(') {
                    continue;
                }
                (Some(method), paren)
            }
            Some(&b'(') => (None, after_name),
            _ => continue,
        };

        push_client_request_fact(
            language,
            tree,
            file_path,
            content,
            name_start,
            open_paren,
            end,
            "axios",
            Some((method_verb, import_source)),
            facts,
        );
    }
}

/// Shared tail of the fetch/axios scans: validates the syntax range and the
/// static-string first argument, resolves the verb, and emits the fact.
///
/// `axios` is `None` for fetch calls; for axios calls it carries the optional
/// verb method (`.post(...)` → `Some("post")`, direct `axios(...)` → `None`)
/// and the import source.
#[allow(clippy::too_many_arguments)]
fn push_client_request_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    name_start: usize,
    open_paren: usize,
    end: usize,
    client: &str,
    axios: Option<(Option<String>, &str)>,
    facts: &mut Vec<StructuralFact>,
) {
    if is_ignored_syntax_range(tree, name_start, open_paren + 1) {
        return;
    }
    let Some(close_paren) = find_matching_paren(content, open_paren, end) else {
        return;
    };

    let first_arg_start = skip_ascii_whitespace_until(content, open_paren + 1, close_paren);
    let first_arg_end = find_top_level_comma_or_end(content, first_arg_start, close_paren);
    let Some((target_path, url_end)) = parse_js_string_literal(content, first_arg_start) else {
        return;
    };
    // Reject anything other than a plain string literal spanning the whole
    // first argument (e.g. `"/api" + suffix`).
    if skip_ascii_whitespace_until(content, url_end, first_arg_end) != first_arg_end {
        return;
    }

    let method_verb = axios.as_ref().and_then(|(method, _)| method.clone());
    let verb = match method_verb {
        Some(method) => Verb::attested(method),
        None => match resolve_verb(content, first_arg_end, close_paren) {
            VerbResolution::Get => Verb::default_get(),
            VerbResolution::Attested(method) => Verb::attested(method),
            VerbResolution::Silent => return,
        },
    };

    let Some(span) = NormalizedSpan::from_content_range(content, name_start, close_paren + 1)
    else {
        return;
    };

    let mut metadata = base_metadata("web.http_client");
    insert_string(&mut metadata, "framework", client);
    insert_string(&mut metadata, "client", client);
    insert_string(&mut metadata, "target_path", &target_path);
    insert_string(&mut metadata, "url_kind", classify_url_kind(&target_path));
    insert_string(&mut metadata, "verb", &verb.name);
    insert_string(&mut metadata, "verb_source", verb.source);
    if let Some((_, import_source)) = axios {
        insert_string(&mut metadata, "import_source", import_source);
    }

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

/// Skips a balanced `<...>` generic-type-argument run starting at `<`. Returns
/// the index just past the matching `>`, or `None` when the run is unbalanced
/// (the candidate call then stays silent).
fn skip_generic_type_arguments(content: &str, open_angle: usize, end: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut index = open_angle;
    while index < end {
        match bytes[index] {
            b'<' => depth += 1,
            b'>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            b'(' | b')' | b';' => return None,
            _ => {}
        }
        index += 1;
    }
    None
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
    if content.as_bytes().get(options_start) != Some(&b'{') {
        return VerbResolution::Get;
    }
    let Some(options_end) = find_matching_brace(content, options_start, close_paren) else {
        return VerbResolution::Silent;
    };
    let Some((method_value_start, method_value_end)) =
        find_top_level_method_property_value(content, options_start + 1, options_end)
    else {
        return VerbResolution::Get;
    };
    match parse_js_string_literal(content, method_value_start) {
        Some((method, method_end))
            if skip_ascii_whitespace_until(content, method_end, method_value_end)
                == method_value_end =>
        {
            VerbResolution::Attested(method)
        }
        None => VerbResolution::Silent,
        Some(_) => VerbResolution::Silent,
    }
}

fn find_top_level_method_property_value(
    content: &str,
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let mut cursor = start;
    while cursor < end {
        cursor = skip_ascii_whitespace_until(content, cursor, end);
        if cursor >= end {
            break;
        }
        let Some((property_name, after_key)) = parse_js_string_literal(content, cursor)
            .or_else(|| parse_js_identifier(content, cursor, end))
        else {
            let next = find_top_level_comma_or_end(content, cursor, end);
            cursor = next.saturating_add(1);
            continue;
        };
        let colon = skip_ascii_whitespace_until(content, after_key, end);
        if content.as_bytes().get(colon) != Some(&b':') {
            let next = find_top_level_comma_or_end(content, cursor, end);
            cursor = next.saturating_add(1);
            continue;
        }
        let value_start = skip_ascii_whitespace_until(content, colon + 1, end);
        let value_end = find_top_level_comma_or_end(content, value_start, end);
        if property_name == "method" {
            return Some((value_start, value_end));
        }
        cursor = value_end.saturating_add(1);
    }
    None
}

fn find_matching_brace(content: &str, open_brace: usize, end: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut cursor = open_brace;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    while cursor < end {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'"' | b'`') {
            quote = Some(byte);
        } else {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(cursor);
                    }
                }
                _ => {}
            }
        }
        cursor += 1;
    }

    None
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
