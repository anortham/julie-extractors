use tree_sitter::Tree;

use super::fact_builders::{base_metadata, fact_for_span, insert_string, insert_string_array};
use super::js_imports::{JsImportIndex, js_import_statement_end, parse_import_source};
use super::js_object_scan::{
    is_identifier_boundary, is_ignored_syntax_range, parse_js_identifier,
    skip_ascii_whitespace_until,
};
use super::jsx_scan::{
    jsx_object_pathname_attribute, jsx_string_literal_attribute, next_markup_tag,
};
use super::{
    NEXTJS_FILE_ROUTE_PATTERN_ID, NEXTJS_ROUTE_HANDLER_PATTERN_ID,
    NEXTJS_ROUTE_REFERENCE_PATTERN_ID, NUXT_FILE_ROUTE_PATTERN_ID, NUXT_SERVER_ROUTE_PATTERN_ID,
};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

#[derive(Debug)]
struct NextFileRoute {
    router: &'static str,
    route_path: String,
    normalized_route_template: Option<String>,
    dynamic_segments: Vec<String>,
    route_group_segments: Vec<String>,
    parallel_route_segments: Vec<String>,
    intercepting_route_markers: Vec<String>,
    intercepted_route_segments: Vec<String>,
}

pub(super) fn collect_nextjs_route_references(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut cursor = 0;

    while cursor < content.len() {
        let Some((tag_start, tag_end, tag_name)) = next_markup_tag(content, cursor, content.len())
        else {
            break;
        };
        cursor = tag_end + 1;
        if is_ignored_syntax_range(tree, tag_start, tag_end + 1) {
            continue;
        }
        let Some(import_source) = imports.next_links.get(tag_name) else {
            continue;
        };
        let href = jsx_string_literal_attribute(content, tag_start, tag_end, "href")
            .filter(|(value, _)| is_static_route_path(value))
            .map(|(value, span)| (value, "string_literal", span))
            .or_else(|| {
                jsx_object_pathname_attribute(content, tag_start, tag_end, "href")
                    .filter(|(value, _)| is_static_route_path(value))
                    .map(|(value, span)| (value, "object_pathname_literal", span))
            });
        let Some((target_path, route_source, span)) = href else {
            continue;
        };

        let mut metadata = base_metadata("frontend_navigation");
        insert_string(&mut metadata, "framework", "nextjs");
        insert_string(&mut metadata, "target_path", &target_path);
        insert_string(&mut metadata, "attribute_name", "href");
        insert_string(&mut metadata, "component_name", tag_name);
        insert_string(&mut metadata, "import_source", import_source);
        insert_string(&mut metadata, "route_source", route_source);
        insert_string(&mut metadata, "source_kind", "next_link");
        insert_string(&mut metadata, "verb", "GET");

        facts.push(fact_for_span(
            file_path,
            language,
            NEXTJS_ROUTE_REFERENCE_PATTERN_ID,
            "route_reference",
            "jsx_attribute",
            span,
            metadata,
        ));
    }

    facts
}

pub(super) fn nextjs_file_route_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let route = nextjs_file_route(file_path)?;
    if has_nuxt_page_signal(tree, content)
        && (route.router == "pages" || has_nuxt_app_pages_route(file_path))
    {
        return None;
    }
    if route.router == "pages" && !has_nextjs_page_signal(tree, content) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, 0, content.len())?;
    let mut metadata = base_metadata("frontend_navigation");
    insert_string(&mut metadata, "framework", "nextjs");
    insert_string(&mut metadata, "router", route.router);
    insert_string(&mut metadata, "file_convention", "page");
    insert_string(&mut metadata, "route_path", &route.route_path);
    insert_string(&mut metadata, "source_kind", "nextjs_file_route");
    if let Some(normalized) = route.normalized_route_template {
        insert_string(&mut metadata, "normalized_route_template", &normalized);
    }
    if !route.dynamic_segments.is_empty() {
        insert_string_array(&mut metadata, "dynamic_segments", route.dynamic_segments);
    }
    if !route.route_group_segments.is_empty() {
        insert_string_array(
            &mut metadata,
            "route_group_segments",
            route.route_group_segments,
        );
    }
    if !route.parallel_route_segments.is_empty() {
        insert_string_array(
            &mut metadata,
            "parallel_route_segments",
            route.parallel_route_segments,
        );
    }
    if !route.intercepting_route_markers.is_empty() {
        insert_string_array(
            &mut metadata,
            "intercepting_route_markers",
            route.intercepting_route_markers,
        );
    }
    if !route.intercepted_route_segments.is_empty() {
        insert_string_array(
            &mut metadata,
            "intercepted_route_segments",
            route.intercepted_route_segments,
        );
    }

    Some(fact_for_span(
        file_path,
        language,
        NEXTJS_FILE_ROUTE_PATTERN_ID,
        "file_route",
        "file",
        span,
        metadata,
    ))
}

pub(super) fn nuxt_file_route_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let route = nuxt_file_route(file_path)?;
    if route.router == "pages"
        && is_non_vue_file_path(file_path)
        && !has_nuxt_page_signal(tree, content)
        && (!has_nuxt_app_pages_route(file_path) || has_app_pages_page_file_route(file_path))
    {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, 0, content.len())?;
    let mut metadata = base_metadata("frontend_navigation");
    insert_string(&mut metadata, "framework", "nuxt");
    insert_string(&mut metadata, "router", route.router);
    insert_string(&mut metadata, "file_convention", "page");
    insert_string(&mut metadata, "route_path", &route.route_path);
    insert_string(&mut metadata, "source_kind", "nuxt_file_route");
    if let Some(normalized) = route.normalized_route_template {
        insert_string(&mut metadata, "normalized_route_template", &normalized);
    }
    if !route.dynamic_segments.is_empty() {
        insert_string_array(&mut metadata, "dynamic_segments", route.dynamic_segments);
    }
    if !route.route_group_segments.is_empty() {
        insert_string_array(
            &mut metadata,
            "route_group_segments",
            route.route_group_segments,
        );
    }
    if !route.parallel_route_segments.is_empty() {
        insert_string_array(
            &mut metadata,
            "parallel_route_segments",
            route.parallel_route_segments,
        );
    }
    if !route.intercepting_route_markers.is_empty() {
        insert_string_array(
            &mut metadata,
            "intercepting_route_markers",
            route.intercepting_route_markers,
        );
    }
    if !route.intercepted_route_segments.is_empty() {
        insert_string_array(
            &mut metadata,
            "intercepted_route_segments",
            route.intercepted_route_segments,
        );
    }

    Some(fact_for_span(
        file_path,
        language,
        NUXT_FILE_ROUTE_PATTERN_ID,
        "file_route",
        "file",
        span,
        metadata,
    ))
}

#[derive(Debug)]
struct NuxtServerRoute {
    route_path: String,
    normalized_route_template: Option<String>,
    dynamic_segments: Vec<String>,
    verb: Option<String>,
}

/// HTTP method suffixes Nuxt/Nitro recognizes in a server-route filename
/// (`users.get.ts` -> GET). Lowercase per the Nuxt file-naming convention.
const NUXT_SERVER_ROUTE_METHODS: &[&str] =
    &["get", "post", "put", "patch", "delete", "head", "options"];

/// Emits one `nuxt.server_route.v1` fact for a Nitro server-route file under
/// `server/api/**` (prefixed `/api`) or `server/routes/**` (no prefix).
///
/// The route path is derived from the file path; the method verb, when present,
/// comes from the filename suffix (`.get`, `.post`, ...). Emission requires a
/// handler signal — a `defineEventHandler`/`eventHandler` identifier — OR a
/// method suffix in the filename. A wrapped custom handler with neither signal
/// is a documented residual miss. `server/middleware`, `server/plugins`, and
/// `server/utils` are not routes and stay silent.
pub(super) fn nuxt_server_route_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let route = nuxt_server_route(file_path)?;
    let has_handler_signal = has_executable_identifier_signal(tree, content, "defineEventHandler")
        || has_executable_identifier_signal(tree, content, "eventHandler");
    if route.verb.is_none() && !has_handler_signal {
        return None;
    }

    let span = NormalizedSpan::from_content_range(content, 0, content.len())?;
    let mut metadata = base_metadata("framework");
    insert_string(&mut metadata, "framework", "nuxt");
    insert_string(&mut metadata, "router", "server");
    insert_string(&mut metadata, "route_path", &route.route_path);
    insert_string(&mut metadata, "source_kind", "nuxt_server_route");
    if let Some(normalized) = route.normalized_route_template {
        insert_string(&mut metadata, "normalized_route_template", &normalized);
    }
    if !route.dynamic_segments.is_empty() {
        insert_string_array(&mut metadata, "dynamic_segments", route.dynamic_segments);
    }
    if let Some(verb) = route.verb {
        insert_string(&mut metadata, "verb", &verb);
        insert_string(&mut metadata, "verb_source", "attested");
    }

    Some(fact_for_span(
        file_path,
        language,
        NUXT_SERVER_ROUTE_PATTERN_ID,
        "server_route",
        "file",
        span,
        metadata,
    ))
}

fn nuxt_server_route(file_path: &str) -> Option<NuxtServerRoute> {
    let normalized = file_path.replace('\\', "/");
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let server_index = segments
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, segment)| (*segment == "server").then_some(index))?;
    // The segment after `server` selects the route family. Only `api`
    // (prefixed `/api`) and `routes` (no prefix) are routes; `middleware`,
    // `plugins`, and `utils` are excluded.
    let (prefix, base_index) = match segments.get(server_index + 1) {
        Some(&"api") => (vec!["api".to_string()], server_index + 2),
        Some(&"routes") => (Vec::new(), server_index + 2),
        _ => return None,
    };

    let file_name = *segments.last()?;
    let (stem, extension) = split_file_name(file_name)?;
    if !is_route_handler_extension(extension) {
        return None;
    }
    let (route_stem, verb) = parse_nuxt_server_route_method(stem);

    let mut route_segments = prefix;
    if base_index < segments.len() {
        for segment in &segments[base_index..segments.len() - 1] {
            route_segments.push((*segment).to_string());
        }
    }
    if route_stem != "index" {
        route_segments.push(route_stem.to_string());
    }

    let (route_path, normalized_route_template, dynamic_segments) =
        route_path_metadata(&route_segments, "nuxt");
    Some(NuxtServerRoute {
        route_path,
        normalized_route_template,
        dynamic_segments,
        verb,
    })
}

/// Splits a trailing HTTP-method suffix off a server-route file stem.
/// `users.get` -> (`users`, Some("GET")); `health` -> (`health`, None).
fn parse_nuxt_server_route_method(stem: &str) -> (&str, Option<String>) {
    if let Some((base, suffix)) = stem.rsplit_once('.')
        && NUXT_SERVER_ROUTE_METHODS.contains(&suffix)
    {
        return (base, Some(suffix.to_ascii_uppercase()));
    }
    (stem, None)
}

fn nextjs_file_route(file_path: &str) -> Option<NextFileRoute> {
    let normalized = file_path.replace('\\', "/");
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if let Some(route) = segments
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, segment)| match *segment {
            "app" => nextjs_app_file_route(&segments, index),
            "pages" if segments.get(index.wrapping_sub(1)) != Some(&"app") => {
                nextjs_pages_file_route(&segments, index)
            }
            _ => None,
        })
    {
        return Some(route);
    }
    None
}

fn nuxt_file_route(file_path: &str) -> Option<NextFileRoute> {
    let normalized = file_path.replace('\\', "/");
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if let Some(app_index) = segments
        .windows(2)
        .enumerate()
        .rev()
        .find_map(|(index, window)| (window == ["app", "pages"]).then_some(index))
    {
        return nuxt_pages_file_route(&segments, app_index + 1);
    }
    if let Some(pages_index) = segments
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, segment)| (*segment == "pages").then_some(index))
    {
        return nuxt_pages_file_route(&segments, pages_index);
    }
    None
}

fn nextjs_app_file_route(segments: &[&str], app_index: usize) -> Option<NextFileRoute> {
    let file_name = *segments.last()?;
    let (stem, extension) = split_file_name(file_name)?;
    if stem != "page" || !is_javascript_like_extension(extension) {
        return None;
    }
    Some(nextjs_app_route_metadata(segments, app_index))
}

/// Walks the App Router directory segments between `app` and the route file to
/// derive the route path plus Next-specific segment metadata. Shared by the
/// `page` file route (`nextjs.file_route.v1`) and the `route` handler
/// (`nextjs.route_handler.v1`) so both stems resolve identical route paths.
fn nextjs_app_route_metadata(segments: &[&str], app_index: usize) -> NextFileRoute {
    let mut route_segments = Vec::new();
    let mut route_group_segments = Vec::new();
    let mut parallel_route_segments = Vec::new();
    let mut intercepting_route_markers = Vec::new();
    let mut intercepted_route_segments = Vec::new();
    for segment in &segments[app_index + 1..segments.len().saturating_sub(1)] {
        if segment.starts_with('(') && segment.ends_with(')') && segment.len() > 2 {
            route_group_segments.push(segment[1..segment.len() - 1].to_string());
        } else if segment.starts_with('@') && segment.len() > 1 {
            parallel_route_segments.push(segment[1..].to_string());
        } else if let Some((marker, intercepted_segment)) =
            nextjs_intercepting_route_segment(segment)
        {
            intercepting_route_markers.push(marker);
            intercepted_route_segments.push(intercepted_segment.clone());
            route_segments.push(intercepted_segment);
        } else {
            route_segments.push((*segment).to_string());
        }
    }

    let (route_path, normalized_route_template, dynamic_segments) =
        route_path_metadata(&route_segments, "nextjs");
    NextFileRoute {
        router: "app",
        route_path,
        normalized_route_template,
        dynamic_segments,
        route_group_segments,
        parallel_route_segments,
        intercepting_route_markers,
        intercepted_route_segments,
    }
}

/// Resolves the App Router route metadata for a `route.{js,ts}` handler file.
/// Returns `None` unless the file is a `route` stem with a `.js`/`.ts`
/// extension nested under an `app` directory (App Router convention, including
/// `src/app`). Route handler files are `.js`/`.ts` only — `.jsx`/`.tsx` route
/// files are nonstandard and stay silent.
fn nextjs_route_handler_route(file_path: &str) -> Option<NextFileRoute> {
    let normalized = file_path.replace('\\', "/");
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let file_name = *segments.last()?;
    let (stem, extension) = split_file_name(file_name)?;
    if stem != "route" || !is_route_handler_extension(extension) {
        return None;
    }
    let app_index = segments
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, segment)| (*segment == "app").then_some(index))?;
    Some(nextjs_app_route_metadata(&segments, app_index))
}

fn is_route_handler_extension(extension: &str) -> bool {
    matches!(extension, "js" | "ts")
}

/// HTTP verb exports Next.js recognizes as App Router route handlers. Next.js
/// auto-implements `OPTIONS` when it is not exported, but that synthesized
/// handler is not attested source, so only a literal `export`ed `OPTIONS`
/// emits a fact.
const ROUTE_HANDLER_VERBS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// Emits one `nextjs.route_handler.v1` fact per exported HTTP-verb handler in an
/// App Router `route.{js,ts}` file. Recognized export forms are
/// `export [async] function GET(...)` and `export const|let|var GET = ...`.
/// Re-exports (`export { GET } from ...`), default exports, non-verb exports,
/// and lowercase names stay silent, as do matches inside comments or strings.
pub(super) fn collect_nextjs_route_handlers(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let Some(route) = nextjs_route_handler_route(file_path) else {
        return Vec::new();
    };

    let mut facts = Vec::new();
    for (verb, export_start, name_end) in exported_route_handler_verbs(tree, content) {
        let Some(span) = NormalizedSpan::from_content_range(content, export_start, name_end) else {
            continue;
        };

        let mut metadata = base_metadata("framework");
        insert_string(&mut metadata, "framework", "nextjs");
        insert_string(&mut metadata, "router", route.router);
        insert_string(&mut metadata, "file_convention", "route");
        insert_string(&mut metadata, "route_path", &route.route_path);
        insert_string(&mut metadata, "source_kind", "nextjs_route_handler");
        insert_string(&mut metadata, "verb", &verb);
        insert_string(&mut metadata, "verb_source", "attested");
        if let Some(normalized) = &route.normalized_route_template {
            insert_string(&mut metadata, "normalized_route_template", normalized);
        }
        if !route.dynamic_segments.is_empty() {
            insert_string_array(
                &mut metadata,
                "dynamic_segments",
                route.dynamic_segments.clone(),
            );
        }
        if !route.route_group_segments.is_empty() {
            insert_string_array(
                &mut metadata,
                "route_group_segments",
                route.route_group_segments.clone(),
            );
        }
        if !route.parallel_route_segments.is_empty() {
            insert_string_array(
                &mut metadata,
                "parallel_route_segments",
                route.parallel_route_segments.clone(),
            );
        }
        if !route.intercepting_route_markers.is_empty() {
            insert_string_array(
                &mut metadata,
                "intercepting_route_markers",
                route.intercepting_route_markers.clone(),
            );
        }
        if !route.intercepted_route_segments.is_empty() {
            insert_string_array(
                &mut metadata,
                "intercepted_route_segments",
                route.intercepted_route_segments.clone(),
            );
        }

        facts.push(fact_for_span(
            file_path,
            language,
            NEXTJS_ROUTE_HANDLER_PATTERN_ID,
            "route_handler",
            "export_statement",
            span,
            metadata,
        ));
    }

    facts
}

/// Scans `content` for exported HTTP-verb handler declarations, returning
/// `(verb, export_keyword_start, handler_name_end)` for each. The span runs
/// from the `export` keyword through the handler name so `containing_symbol_id`
/// binds to the handler symbol.
fn exported_route_handler_verbs(tree: &Tree, content: &str) -> Vec<(String, usize, usize)> {
    const EXPORT_KEYWORD: &str = "export";
    let end = content.len();
    let mut results = Vec::new();
    let mut cursor = 0;

    while cursor < end {
        let Some(relative_start) = content[cursor..].find(EXPORT_KEYWORD) else {
            break;
        };
        let export_start = cursor + relative_start;
        cursor = export_start + EXPORT_KEYWORD.len();

        if !is_identifier_boundary(content, export_start, EXPORT_KEYWORD.len()) {
            continue;
        }
        let Some((verb, name_end)) = parse_exported_handler_name(content, cursor, end) else {
            continue;
        };
        if !ROUTE_HANDLER_VERBS.contains(&verb.as_str()) {
            continue;
        }
        if is_ignored_syntax_range(tree, export_start, name_end) {
            continue;
        }
        results.push((verb, export_start, name_end));
    }

    results
}

/// Parses the exported binding name that follows an `export` keyword, when the
/// declaration is a recognized route-handler form: `[async] function NAME` or
/// `const|let|var NAME =`/`const NAME:` (type-annotated binding). Returns
/// `(name, name_end_byte)` or `None` for any other export shape (re-exports,
/// default exports, class/enum/etc.).
fn parse_exported_handler_name(content: &str, start: usize, end: usize) -> Option<(String, usize)> {
    let keyword_start = skip_ascii_whitespace_until(content, start, end);
    let (keyword, keyword_end) = parse_js_identifier(content, keyword_start, end)?;
    match keyword.as_str() {
        "async" => {
            let function_start = skip_ascii_whitespace_until(content, keyword_end, end);
            let (function_keyword, function_end) =
                parse_js_identifier(content, function_start, end)?;
            if function_keyword != "function" {
                return None;
            }
            parse_function_handler_name(content, function_end, end)
        }
        "function" => parse_function_handler_name(content, keyword_end, end),
        "const" | "let" | "var" => {
            let name_start = skip_ascii_whitespace_until(content, keyword_end, end);
            let (name, name_end) = parse_js_identifier(content, name_start, end)?;
            // Require a binding: the next token is `=` (assignment) or `:`
            // (TypeScript type annotation, e.g. `export const GET: Handler =`).
            let after_name = skip_ascii_whitespace_until(content, name_end, end);
            match content.as_bytes().get(after_name) {
                Some(&b'=') | Some(&b':') => Some((name, name_end)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Parses the function name following the `function` keyword, tolerating an
/// optional generator `*` (route handlers are not generators, but the marker is
/// skipped defensively).
fn parse_function_handler_name(content: &str, start: usize, end: usize) -> Option<(String, usize)> {
    let mut name_start = skip_ascii_whitespace_until(content, start, end);
    if content.as_bytes().get(name_start) == Some(&b'*') {
        name_start = skip_ascii_whitespace_until(content, name_start + 1, end);
    }
    parse_js_identifier(content, name_start, end)
}

fn nextjs_pages_file_route(segments: &[&str], pages_index: usize) -> Option<NextFileRoute> {
    let file_name = *segments.last()?;
    let (stem, extension) = split_file_name(file_name)?;
    if !is_javascript_like_extension(extension) || stem.starts_with('_') {
        return None;
    }
    if segments.get(pages_index + 1) == Some(&"api") {
        return None;
    }

    let mut route_segments = segments[pages_index + 1..segments.len().saturating_sub(1)]
        .iter()
        .map(|segment| (*segment).to_string())
        .collect::<Vec<_>>();
    if stem != "index" {
        route_segments.push(stem.to_string());
    }

    let (route_path, normalized_route_template, dynamic_segments) =
        route_path_metadata(&route_segments, "nextjs");
    Some(NextFileRoute {
        router: "pages",
        route_path,
        normalized_route_template,
        dynamic_segments,
        route_group_segments: Vec::new(),
        parallel_route_segments: Vec::new(),
        intercepting_route_markers: Vec::new(),
        intercepted_route_segments: Vec::new(),
    })
}

fn nuxt_pages_file_route(segments: &[&str], pages_index: usize) -> Option<NextFileRoute> {
    let file_name = *segments.last()?;
    let (stem, extension) = split_file_name(file_name)?;
    if !is_nuxt_page_extension(extension) || stem.starts_with('_') || stem.contains('@') {
        return None;
    }
    if segments.get(pages_index + 1) == Some(&"api") {
        return None;
    }

    let mut route_segments = Vec::new();
    let mut route_group_segments = Vec::new();
    for segment in &segments[pages_index + 1..segments.len().saturating_sub(1)] {
        if segment.starts_with('(') && segment.ends_with(')') && segment.len() > 2 {
            route_group_segments.push(segment[1..segment.len() - 1].to_string());
        } else {
            route_segments.push((*segment).to_string());
        }
    }
    if stem != "index" {
        route_segments.push(stem.to_string());
    }

    let (route_path, normalized_route_template, dynamic_segments) =
        route_path_metadata(&route_segments, "nuxt");
    Some(NextFileRoute {
        router: "pages",
        route_path,
        normalized_route_template,
        dynamic_segments,
        route_group_segments,
        parallel_route_segments: Vec::new(),
        intercepting_route_markers: Vec::new(),
        intercepted_route_segments: Vec::new(),
    })
}

fn route_path_metadata(
    route_segments: &[String],
    framework: &str,
) -> (String, Option<String>, Vec<String>) {
    let mut normalized_segments = Vec::new();
    let mut dynamic_segments = Vec::new();
    let mut has_dynamic = false;

    for segment in route_segments {
        if let Some((names, normalized)) = dynamic_segment_metadata(framework, segment) {
            has_dynamic = true;
            dynamic_segments.extend(names);
            normalized_segments.push(normalized);
        } else {
            normalized_segments.push(segment.clone());
        }
    }

    let route_path = if route_segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", route_segments.join("/"))
    };
    let normalized_route_template = has_dynamic.then(|| {
        if normalized_segments.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", normalized_segments.join("/"))
        }
    });

    (route_path, normalized_route_template, dynamic_segments)
}

fn dynamic_segment_metadata(framework: &str, segment: &str) -> Option<(Vec<String>, String)> {
    if framework == "nuxt" {
        return nuxt_dynamic_segment_metadata(segment);
    }
    nextjs_dynamic_segment_metadata(segment).map(|(name, normalized)| (vec![name], normalized))
}

fn nextjs_dynamic_segment_metadata(segment: &str) -> Option<(String, String)> {
    if segment.starts_with("[[...") && segment.ends_with("]]") {
        let name = segment
            .trim_start_matches("[[...")
            .trim_end_matches("]]")
            .to_string();
        return Some((name.clone(), format!(":{name}*?")));
    }
    if segment.starts_with("[...") && segment.ends_with(']') {
        let name = segment
            .trim_start_matches("[...")
            .trim_end_matches(']')
            .to_string();
        return Some((name.clone(), format!(":{name}*")));
    }
    if segment.starts_with('[') && segment.ends_with(']') {
        let name = segment
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        return Some((name.clone(), format!(":{name}")));
    }
    None
}

fn nuxt_dynamic_segment_metadata(segment: &str) -> Option<(Vec<String>, String)> {
    let mut cursor = 0usize;
    let mut names = Vec::new();
    let mut normalized = String::new();

    while cursor < segment.len() {
        if let Some((name, replacement, next_cursor)) =
            parse_nuxt_dynamic_part(segment, cursor, "[[...", "]]", "*?")
        {
            names.push(format!("{name}*?"));
            normalized.push_str(&replacement);
            cursor = next_cursor;
        } else if let Some((name, replacement, next_cursor)) =
            parse_nuxt_dynamic_part(segment, cursor, "[[", "]]", "?")
        {
            names.push(format!("{name}?"));
            normalized.push_str(&replacement);
            cursor = next_cursor;
        } else if let Some((name, replacement, next_cursor)) =
            parse_nuxt_dynamic_part(segment, cursor, "[...", "]", "*")
        {
            names.push(name);
            normalized.push_str(&replacement);
            cursor = next_cursor;
        } else if let Some((name, replacement, next_cursor)) =
            parse_nuxt_dynamic_part(segment, cursor, "[", "]", "")
        {
            names.push(name);
            normalized.push_str(&replacement);
            cursor = next_cursor;
        } else {
            let ch = segment.get(cursor..)?.chars().next()?;
            normalized.push(ch);
            cursor += ch.len_utf8();
        }
    }

    (!names.is_empty()).then_some((names, normalized))
}

fn parse_nuxt_dynamic_part(
    segment: &str,
    cursor: usize,
    open: &str,
    close: &str,
    suffix: &str,
) -> Option<(String, String, usize)> {
    let remaining = segment.get(cursor..)?;
    if !remaining.starts_with(open) {
        return None;
    }
    let name_start = cursor + open.len();
    let close_start = segment.get(name_start..)?.find(close)? + name_start;
    if close_start == name_start {
        return None;
    }
    let name = segment.get(name_start..close_start)?.to_string();
    let next_cursor = close_start + close.len();
    Some((name.clone(), format!(":{name}{suffix}"), next_cursor))
}

fn nextjs_intercepting_route_segment(segment: &str) -> Option<(String, String)> {
    ["(..)(..)", "(...)", "(..)", "(.)"]
        .iter()
        .find_map(|marker| {
            segment
                .strip_prefix(marker)
                .filter(|intercepted| !intercepted.is_empty())
                .map(|intercepted| ((*marker).to_string(), intercepted.to_string()))
        })
}

fn split_file_name(file_name: &str) -> Option<(&str, &str)> {
    let dot = file_name.rfind('.')?;
    Some((&file_name[..dot], &file_name[dot + 1..]))
}

fn is_javascript_like_extension(extension: &str) -> bool {
    matches!(extension, "js" | "jsx" | "ts" | "tsx")
}

fn is_nuxt_page_extension(extension: &str) -> bool {
    matches!(extension, "vue" | "js" | "jsx" | "mjs" | "ts" | "tsx")
}

fn is_non_vue_file_path(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/");
    let Some(file_name) = normalized.split('/').rfind(|segment| !segment.is_empty()) else {
        return false;
    };
    split_file_name(file_name).is_some_and(|(_, extension)| extension != "vue")
}

fn has_nuxt_app_pages_route(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/");
    normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .windows(2)
        .any(|window| window == ["app", "pages"])
}

fn has_app_pages_page_file_route(file_path: &str) -> bool {
    let normalized = file_path.replace('\\', "/");
    let segments = normalized
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if !segments.windows(2).any(|window| window == ["app", "pages"]) {
        return false;
    }
    let Some(file_name) = segments.last() else {
        return false;
    };
    split_file_name(file_name)
        .is_some_and(|(stem, extension)| stem == "page" && is_javascript_like_extension(extension))
}

fn has_nuxt_page_signal(tree: &Tree, content: &str) -> bool {
    [
        "defineNuxtComponent",
        "definePageMeta",
        "defineNuxtRouteMiddleware",
        "useNuxtApp",
    ]
    .iter()
    .any(|signal| has_executable_identifier_signal(tree, content, signal))
        || ["#app", "#imports", "nuxt/app"]
            .iter()
            .any(|source| has_static_import_source(tree, content, source))
}

fn has_nextjs_page_signal(tree: &Tree, content: &str) -> bool {
    [
        "getStaticProps",
        "getServerSideProps",
        "getStaticPaths",
        "NextPage",
    ]
    .iter()
    .any(|signal| has_executable_identifier_signal(tree, content, signal))
        || [
            "next",
            "next/head",
            "next/image",
            "next/link",
            "next/router",
            "next/navigation",
        ]
        .iter()
        .any(|source| has_static_import_source(tree, content, source))
}

fn has_executable_identifier_signal(tree: &Tree, content: &str, signal: &str) -> bool {
    let mut cursor = 0;
    while cursor < content.len() {
        let Some(relative_start) = content[cursor..].find(signal) else {
            break;
        };
        let signal_start = cursor + relative_start;
        cursor = signal_start + signal.len();
        if is_identifier_boundary(content, signal_start, signal.len())
            && !is_ignored_syntax_range(tree, signal_start, cursor)
        {
            return true;
        }
    }
    false
}

pub(super) fn has_static_import_source(tree: &Tree, content: &str, expected_source: &str) -> bool {
    let mut cursor = 0;
    while cursor < content.len() {
        let Some(relative_import) = content[cursor..].find("import") else {
            break;
        };
        let import_start = cursor + relative_import;
        cursor = import_start + "import".len();
        if !is_identifier_boundary(content, import_start, "import".len())
            || is_ignored_syntax_range(tree, import_start, cursor)
        {
            continue;
        }

        let statement_end = js_import_statement_end(content, import_start);
        let Some(statement) = content.get(import_start..statement_end) else {
            continue;
        };
        cursor = statement_end;

        if parse_import_source(statement).as_deref() == Some(expected_source) {
            return true;
        }
    }
    false
}

pub(super) fn is_static_route_definition_path(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && !value.starts_with("//") && !value.contains("://")
}

fn is_static_route_path(value: &str) -> bool {
    value.trim().starts_with('/')
}

pub(super) fn is_nuxt_route_path(value: &str) -> bool {
    let value = value.trim();
    value.starts_with('/') && !value.starts_with("//")
}

pub(super) fn is_nuxt_link_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "nuxtlink" | "nuxt-link"
    )
}

pub(super) fn is_nuxt_external_attribute(attribute_name: &str) -> bool {
    matches!(attribute_name, "external" | ":external" | "v-bind:external")
}
