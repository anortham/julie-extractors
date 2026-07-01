use tree_sitter::Tree;

use super::fact_builders::{base_metadata, fact_for_span, insert_string, insert_string_array};
use super::js_imports::{JsImportIndex, js_import_statement_end, parse_import_source};
use super::js_object_scan::{is_identifier_boundary, is_ignored_syntax_range};
use super::jsx_scan::{
    jsx_object_pathname_attribute, jsx_string_literal_attribute, next_markup_tag,
};
use super::{
    NEXTJS_FILE_ROUTE_PATTERN_ID, NEXTJS_ROUTE_REFERENCE_PATTERN_ID, NUXT_FILE_ROUTE_PATTERN_ID,
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
    Some(NextFileRoute {
        router: "app",
        route_path,
        normalized_route_template,
        dynamic_segments,
        route_group_segments,
        parallel_route_segments,
        intercepting_route_markers,
        intercepted_route_segments,
    })
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
