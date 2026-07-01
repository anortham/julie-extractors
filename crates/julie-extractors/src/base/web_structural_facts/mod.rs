use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::markup_scan::{find_tag_end, is_attr_name_byte, is_markup_tag_start};
use super::span::NormalizedSpan;
use super::structural_facts::sort_structural_facts;
use super::types::{StructuralFact, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

mod css;
mod fact_builders;
mod html;
mod js_imports;
mod vue;

use css::collect_css_structural_facts;
use fact_builders::{
    attach_containing_symbols, base_metadata, fact_for_span, insert_string, insert_string_array,
};
use html::collect_html_structural_facts;
use js_imports::{JsImportIndex, collect_js_imports, js_import_statement_end, parse_import_source};
use vue::{collect_vue_router_route_definitions, collect_vue_structural_facts};

const CSS_SELECTOR_RULE_PATTERN_ID: &str = "css.selector_rule.v1";
const CSS_CUSTOM_PROPERTY_PATTERN_ID: &str = "css.custom_property.v1";
const CSS_MEDIA_QUERY_PATTERN_ID: &str = "css.media_query.v1";
const CSS_KEYFRAMES_PATTERN_ID: &str = "css.keyframes.v1";
const HTML_LINK_PATTERN_ID: &str = "html.link.v1";
const HTML_SCRIPT_PATTERN_ID: &str = "html.script.v1";
const HTML_FORM_PATTERN_ID: &str = "html.form.v1";
const HTML_FORM_CONTROL_PATTERN_ID: &str = "html.form_control.v1";
const VUE_SFC_SECTION_PATTERN_ID: &str = "vue.sfc_section.v1";
const VUE_TEMPLATE_DIRECTIVE_PATTERN_ID: &str = "vue.template_directive.v1";
const VUE_ROUTE_REFERENCE_PATTERN_ID: &str = "vue.route_reference.v1";
const VUE_ROUTE_DEFINITION_PATTERN_ID: &str = "vue.route_definition.v1";
const REACT_ROUTE_REFERENCE_PATTERN_ID: &str = "react.route_reference.v1";
const REACT_ROUTE_DEFINITION_PATTERN_ID: &str = "react.route_definition.v1";
const NEXTJS_ROUTE_REFERENCE_PATTERN_ID: &str = "nextjs.route_reference.v1";
const NEXTJS_FILE_ROUTE_PATTERN_ID: &str = "nextjs.file_route.v1";
const NUXT_ROUTE_REFERENCE_PATTERN_ID: &str = "nuxt.route_reference.v1";
const NUXT_FILE_ROUTE_PATTERN_ID: &str = "nuxt.file_route.v1";

#[cfg(all(test, feature = "test-capability-matrix"))]
const CSS_WEB_PATTERN_IDS: &[&str] = &[
    CSS_CUSTOM_PROPERTY_PATTERN_ID,
    CSS_KEYFRAMES_PATTERN_ID,
    CSS_MEDIA_QUERY_PATTERN_ID,
    CSS_SELECTOR_RULE_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const HTML_WEB_PATTERN_IDS: &[&str] = &[
    HTML_FORM_CONTROL_PATTERN_ID,
    HTML_FORM_PATTERN_ID,
    HTML_LINK_PATTERN_ID,
    HTML_SCRIPT_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const VUE_WEB_PATTERN_IDS: &[&str] = &[
    NUXT_FILE_ROUTE_PATTERN_ID,
    NUXT_ROUTE_REFERENCE_PATTERN_ID,
    VUE_ROUTE_DEFINITION_PATTERN_ID,
    VUE_ROUTE_REFERENCE_PATTERN_ID,
    VUE_SFC_SECTION_PATTERN_ID,
    VUE_TEMPLATE_DIRECTIVE_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const JS_FRAMEWORK_WEB_PATTERN_IDS: &[&str] = &[
    NEXTJS_FILE_ROUTE_PATTERN_ID,
    NEXTJS_ROUTE_REFERENCE_PATTERN_ID,
    NUXT_FILE_ROUTE_PATTERN_ID,
    REACT_ROUTE_DEFINITION_PATTERN_ID,
    REACT_ROUTE_REFERENCE_PATTERN_ID,
    VUE_ROUTE_DEFINITION_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const TS_FRAMEWORK_WEB_PATTERN_IDS: &[&str] = &[
    NEXTJS_FILE_ROUTE_PATTERN_ID,
    NUXT_FILE_ROUTE_PATTERN_ID,
    REACT_ROUTE_DEFINITION_PATTERN_ID,
    VUE_ROUTE_DEFINITION_PATTERN_ID,
];

pub fn collect_web_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let mut facts = match language {
        "css" => collect_css_structural_facts(tree, file_path, content),
        "html" => collect_html_structural_facts(tree, file_path, content),
        "vue" => collect_vue_structural_facts(tree, file_path, content),
        "javascript" | "jsx" | "typescript" | "tsx" => {
            collect_react_nextjs_structural_facts(language, tree, file_path, content)
        }
        _ => Vec::new(),
    };

    attach_containing_symbols(&mut facts, symbols);
    sort_structural_facts(&mut facts);
    facts
}

#[cfg(all(test, feature = "test-capability-matrix"))]
pub(crate) fn web_structural_fact_pattern_ids_for_language(
    language: &str,
) -> &'static [&'static str] {
    match language {
        "css" => CSS_WEB_PATTERN_IDS,
        "html" => HTML_WEB_PATTERN_IDS,
        "vue" => VUE_WEB_PATTERN_IDS,
        "javascript" | "jsx" | "tsx" => JS_FRAMEWORK_WEB_PATTERN_IDS,
        "typescript" => TS_FRAMEWORK_WEB_PATTERN_IDS,
        _ => &[],
    }
}

#[derive(Debug)]
struct JsxAttributeSpan {
    value_start: Option<usize>,
    value_end: usize,
    span: NormalizedSpan,
}

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

fn collect_react_nextjs_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let imports = collect_js_imports(content);
    let mut facts = Vec::new();
    facts.extend(collect_react_router_route_references(
        language, tree, file_path, content, &imports,
    ));
    facts.extend(collect_react_router_route_definitions(
        language, tree, file_path, content, &imports,
    ));
    facts.extend(collect_nextjs_route_references(
        language, tree, file_path, content, &imports,
    ));
    facts.extend(collect_vue_router_route_definitions(
        language, tree, file_path, content,
    ));
    if let Some(fact) = nextjs_file_route_fact(language, tree, file_path, content) {
        facts.push(fact);
    }
    if let Some(fact) = nuxt_file_route_fact(language, tree, file_path, content) {
        facts.push(fact);
    }
    facts
}

fn collect_react_router_route_references(
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
        let Some(import_source) = imports.react_router_links.get(tag_name) else {
            continue;
        };
        let Some((target_path, span)) =
            jsx_string_literal_attribute(content, tag_start, tag_end, "to")
                .filter(|(value, _)| is_static_react_route_path(value))
        else {
            continue;
        };

        let mut metadata = base_metadata("frontend_navigation");
        insert_string(&mut metadata, "framework", "react");
        insert_string(&mut metadata, "library", "react_router");
        insert_string(&mut metadata, "target_path", &target_path);
        insert_string(&mut metadata, "attribute_name", "to");
        insert_string(&mut metadata, "component_name", tag_name);
        insert_string(&mut metadata, "import_source", import_source);
        insert_string(&mut metadata, "route_source", "string_literal");
        insert_string(&mut metadata, "source_kind", "react_router_link");
        insert_string(&mut metadata, "verb", "GET");

        facts.push(fact_for_span(
            file_path,
            language,
            REACT_ROUTE_REFERENCE_PATTERN_ID,
            "route_reference",
            "jsx_attribute",
            span,
            metadata,
        ));
    }

    facts
}

fn collect_react_router_route_definitions(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<StructuralFact> {
    let mut facts =
        collect_react_router_jsx_route_definitions(language, tree, file_path, content, imports);
    facts.extend(collect_react_router_route_object_definitions(
        language, tree, file_path, content, imports,
    ));
    facts
}

fn collect_react_router_jsx_route_definitions(
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
        if !imports.react_router_routes.contains_key(tag_name) {
            continue;
        }

        let path = jsx_string_literal_attribute(content, tag_start, tag_end, "path")
            .filter(|(value, _)| is_static_react_route_path(value));
        let index_route = jsx_boolean_attribute(content, tag_start, tag_end, "index");
        if path.is_none() && !index_route {
            continue;
        }
        let Some(span) = NormalizedSpan::from_content_range(content, tag_start, tag_end + 1) else {
            continue;
        };
        let route_component =
            jsx_identifier_expression_attribute(content, tag_start, tag_end, "Component").or_else(
                || jsx_element_component_attribute(content, tag_start, tag_end, "element"),
            );

        facts.push(react_route_definition_fact(
            file_path,
            language,
            ReactRouteDefinitionFact {
                source_kind: "jsx_route",
                route_path: path.map(|(value, _)| value),
                index_route,
                route_component,
                route_id: None,
                parent_route_path: None,
                effective_route_template: None,
                span,
                node_kind: "jsx_element",
            },
        ));
    }

    facts
}

fn collect_react_router_route_object_definitions(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<StructuralFact> {
    if imports.react_router_route_apis.is_empty() {
        return Vec::new();
    }

    let mut facts = Vec::new();
    for (range_start, range_end) in react_router_route_api_argument_ranges(tree, content, imports) {
        let mut cursor = range_start;
        while cursor < range_end {
            let Some(relative_path_start) = content[cursor..range_end].find("path") else {
                break;
            };
            let path_start = cursor + relative_path_start;
            cursor = path_start + "path".len();
            if !is_identifier_boundary(content, path_start, "path".len()) {
                continue;
            }
            if is_ignored_syntax_range(tree, path_start, cursor) {
                continue;
            }
            let colon = skip_ascii_whitespace_until(content, cursor, range_end);
            if content.as_bytes().get(colon) != Some(&b':') {
                continue;
            }
            let value_start = skip_ascii_whitespace_until(content, colon + 1, range_end);
            let Some((route_path, path_end)) = parse_js_string_literal(content, value_start)
                .filter(|(value, _)| is_static_react_route_path(value))
            else {
                continue;
            };
            let Some((span_start, span_end)) =
                find_enclosing_object_range(content, range_start, range_end, path_start)
            else {
                continue;
            };
            let Some(span) = NormalizedSpan::from_content_range(content, span_start, span_end)
            else {
                continue;
            };
            if path_end > span_end {
                continue;
            }

            let parent_route_path = parent_route_path_for_object(
                tree,
                content,
                range_start,
                range_end,
                span_start,
                span_end,
            );
            let effective_route_template = parent_route_path
                .as_ref()
                .map(|parent| join_frontend_route_paths(parent, &route_path));

            facts.push(react_route_definition_fact(
                file_path,
                language,
                ReactRouteDefinitionFact {
                    source_kind: "route_object",
                    route_path: Some(route_path),
                    index_route: false,
                    route_component: react_route_object_component_name(
                        content, span_start, span_end,
                    ),
                    route_id: parse_object_string_property(content, span_start, span_end, "id"),
                    parent_route_path,
                    effective_route_template,
                    span,
                    node_kind: "object",
                },
            ));
        }

        let mut cursor = range_start;
        while cursor < range_end {
            let Some(relative_index_start) = content[cursor..range_end].find("index") else {
                break;
            };
            let index_start = cursor + relative_index_start;
            cursor = index_start + "index".len();
            if !is_identifier_boundary(content, index_start, "index".len()) {
                continue;
            }
            if is_ignored_syntax_range(tree, index_start, cursor) {
                continue;
            }
            let colon = skip_ascii_whitespace_until(content, cursor, range_end);
            if content.as_bytes().get(colon) != Some(&b':') {
                continue;
            }
            let value_start = skip_ascii_whitespace_until(content, colon + 1, range_end);
            if !content
                .get(value_start..)
                .is_some_and(|remaining| remaining.starts_with("true"))
            {
                continue;
            }
            let Some((span_start, span_end)) =
                find_enclosing_object_range(content, range_start, range_end, index_start)
            else {
                continue;
            };
            if parse_object_string_property(content, span_start, span_end, "path").is_some() {
                continue;
            }
            let Some(span) = NormalizedSpan::from_content_range(content, span_start, span_end)
            else {
                continue;
            };
            let parent_route_path = parent_route_path_for_object(
                tree,
                content,
                range_start,
                range_end,
                span_start,
                span_end,
            );
            let effective_route_template = parent_route_path.clone();
            facts.push(react_route_definition_fact(
                file_path,
                language,
                ReactRouteDefinitionFact {
                    source_kind: "route_object",
                    route_path: None,
                    index_route: true,
                    route_component: react_route_object_component_name(
                        content, span_start, span_end,
                    ),
                    route_id: parse_object_string_property(content, span_start, span_end, "id"),
                    parent_route_path,
                    effective_route_template,
                    span,
                    node_kind: "object",
                },
            ));
        }
    }

    facts
}

fn react_router_route_api_argument_ranges(
    tree: &Tree,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for api_name in imports.react_router_route_apis.keys() {
        let mut cursor = 0;
        while cursor < content.len() {
            let Some(relative_start) = content[cursor..].find(api_name) else {
                break;
            };
            let api_start = cursor + relative_start;
            cursor = api_start + api_name.len();
            if !is_identifier_boundary(content, api_start, api_name.len()) {
                continue;
            }
            if is_ignored_syntax_range(tree, api_start, cursor) {
                continue;
            }
            let open_paren = skip_ascii_whitespace_until(content, cursor, content.len());
            if content.as_bytes().get(open_paren) != Some(&b'(') {
                continue;
            }
            let Some(close_paren) = find_matching_paren(content, open_paren, content.len()) else {
                continue;
            };
            let first_arg_start = skip_ascii_whitespace_until(content, open_paren + 1, close_paren);
            let first_arg_end = find_top_level_comma_or_end(content, first_arg_start, close_paren);
            if let Some((identifier, identifier_end)) =
                parse_js_identifier(content, first_arg_start, first_arg_end)
            {
                let trailing = skip_ascii_whitespace_until(content, identifier_end, first_arg_end);
                if trailing == first_arg_end
                    && let Some(range) = find_js_array_initializer_range(content, &identifier)
                {
                    ranges.push(range);
                    continue;
                }
            }
            ranges.push((first_arg_start, first_arg_end));
        }
    }
    ranges.sort_unstable();
    ranges.dedup();
    ranges
}

struct ReactRouteDefinitionFact<'a> {
    source_kind: &'a str,
    route_path: Option<String>,
    index_route: bool,
    route_component: Option<String>,
    route_id: Option<String>,
    parent_route_path: Option<String>,
    effective_route_template: Option<String>,
    span: NormalizedSpan,
    node_kind: &'a str,
}

fn react_route_definition_fact(
    file_path: &str,
    language: &str,
    fact: ReactRouteDefinitionFact<'_>,
) -> StructuralFact {
    let mut metadata = base_metadata("frontend_navigation");
    insert_string(&mut metadata, "framework", "react");
    insert_string(&mut metadata, "library", "react_router");
    insert_string(&mut metadata, "source_kind", fact.source_kind);
    if let Some(route_path) = fact.route_path {
        insert_string(&mut metadata, "route_path", &route_path);
        insert_string(&mut metadata, "route_source", "string_literal");
    } else if fact.index_route {
        insert_string(&mut metadata, "route_source", "index_route");
    }
    if fact.index_route {
        metadata.insert("index_route".to_string(), Value::Bool(true));
    }
    if let Some(route_component) = fact.route_component {
        insert_string(&mut metadata, "route_component", &route_component);
    }
    if let Some(route_id) = fact.route_id {
        insert_string(&mut metadata, "route_id", &route_id);
    }
    if let Some(parent_route_path) = fact.parent_route_path {
        insert_string(&mut metadata, "parent_route_path", &parent_route_path);
    }
    if let Some(effective_route_template) = fact.effective_route_template {
        insert_string(
            &mut metadata,
            "effective_route_template",
            &effective_route_template,
        );
    }

    fact_for_span(
        file_path,
        language,
        REACT_ROUTE_DEFINITION_PATTERN_ID,
        "route_definition",
        fact.node_kind,
        fact.span,
        metadata,
    )
}

fn react_route_object_component_name(content: &str, start: usize, end: usize) -> Option<String> {
    parse_object_identifier_property(content, start, end, "Component")
        .or_else(|| parse_object_jsx_element_property_component(content, start, end, "element"))
}

fn parse_object_jsx_element_property_component(
    content: &str,
    start: usize,
    end: usize,
    property_name: &str,
) -> Option<String> {
    let value_start = find_object_property_value_start(content, start, end, property_name)?;
    parse_jsx_element_component_at(content, value_start, end)
}

fn collect_nextjs_route_references(
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

fn nextjs_file_route_fact(
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

fn nuxt_file_route_fact(
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

fn has_static_import_source(tree: &Tree, content: &str, expected_source: &str) -> bool {
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

fn next_markup_tag(content: &str, start: usize, end: usize) -> Option<(usize, usize, &str)> {
    let mut cursor = start;
    while cursor < end {
        let relative_tag_start = content.get(cursor..end)?.find('<')?;
        let tag_start = cursor + relative_tag_start;
        let tag_end = find_tag_end(content, tag_start).filter(|tag_end| *tag_end <= end)?;
        cursor = tag_end + 1;
        if !is_markup_tag_start(content.as_bytes(), tag_start) {
            continue;
        }
        let Some(tag_name) = markup_tag_name(content, tag_start, tag_end) else {
            continue;
        };
        return Some((tag_start, tag_end, tag_name));
    }
    None
}

fn jsx_string_literal_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<(String, NormalizedSpan)> {
    let attribute = find_jsx_attribute(content, tag_start, tag_end, attribute_name)?;
    let value_start = attribute.value_start?;
    let bytes = content.as_bytes();
    let value = if matches!(bytes.get(value_start), Some(b'\"') | Some(b'\'')) {
        parse_js_string_literal(content, value_start)?.0
    } else if bytes.get(value_start) == Some(&b'{') {
        let close = find_matching_brace(content, value_start, attribute.value_end)?;
        let literal_start = skip_ascii_whitespace_until(content, value_start + 1, close);
        let (value, literal_end) = parse_js_string_literal(content, literal_start)?;
        let trailing = skip_ascii_whitespace_until(content, literal_end, close);
        if trailing != close {
            return None;
        }
        value
    } else {
        return None;
    };
    Some((value, attribute.span))
}

fn jsx_object_pathname_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<(String, NormalizedSpan)> {
    let attribute = find_jsx_attribute(content, tag_start, tag_end, attribute_name)?;
    let value_start = attribute.value_start?;
    if content.as_bytes().get(value_start) != Some(&b'{') {
        return None;
    }
    let close = find_matching_brace(content, value_start, attribute.value_end)?;
    let object_start = skip_ascii_whitespace_until(content, value_start + 1, close);
    if content.as_bytes().get(object_start) != Some(&b'{') {
        return None;
    }
    let object_end = find_matching_brace(content, object_start, close)?;
    let value = parse_object_string_property(content, object_start, object_end + 1, "pathname")?;
    Some((value, attribute.span))
}

fn jsx_boolean_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> bool {
    find_jsx_attribute(content, tag_start, tag_end, attribute_name)
        .is_some_and(|attribute| attribute.value_start.is_none())
}

fn jsx_identifier_expression_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<String> {
    let attribute = find_jsx_attribute(content, tag_start, tag_end, attribute_name)?;
    let value_start = attribute.value_start?;
    if content.as_bytes().get(value_start) != Some(&b'{') {
        return None;
    }
    let close = find_matching_brace(content, value_start, attribute.value_end)?;
    let identifier_start = skip_ascii_whitespace_until(content, value_start + 1, close);
    let (identifier, identifier_end) = parse_js_identifier(content, identifier_start, close)?;
    let trailing = skip_ascii_whitespace_until(content, identifier_end, close);
    (trailing == close).then_some(identifier)
}

fn jsx_element_component_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<String> {
    let attribute = find_jsx_attribute(content, tag_start, tag_end, attribute_name)?;
    let value_start = attribute.value_start?;
    if content.as_bytes().get(value_start) != Some(&b'{') {
        return parse_jsx_element_component_at(content, value_start, attribute.value_end);
    }
    let close = find_matching_brace(content, value_start, attribute.value_end)?;
    let expression_start = skip_ascii_whitespace_until(content, value_start + 1, close);
    parse_jsx_element_component_at(content, expression_start, close)
}

fn parse_jsx_element_component_at(content: &str, value_start: usize, end: usize) -> Option<String> {
    if content.as_bytes().get(value_start) != Some(&b'<') {
        return None;
    }
    let component_start = value_start + 1;
    if matches!(
        content.as_bytes().get(component_start),
        Some(b'>') | Some(b'/')
    ) {
        return None;
    }
    parse_js_identifier(content, component_start, end).map(|(identifier, _)| identifier)
}

fn find_jsx_attribute(
    content: &str,
    tag_start: usize,
    tag_end: usize,
    attribute_name: &str,
) -> Option<JsxAttributeSpan> {
    let bytes = content.as_bytes();
    let mut cursor = tag_start + 1;
    while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
        cursor += 1;
    }

    while cursor < tag_end {
        cursor = skip_ascii_whitespace_until(content, cursor, tag_end);
        if cursor >= tag_end || bytes[cursor] == b'/' {
            cursor += 1;
            continue;
        }

        let attribute_start = cursor;
        while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
            cursor += 1;
        }
        if cursor == attribute_start {
            cursor += 1;
            continue;
        }

        let attribute_end = cursor;
        let Some(name) = content.get(attribute_start..attribute_end) else {
            continue;
        };
        let after_name = skip_ascii_whitespace_until(content, cursor, tag_end);
        if content.as_bytes().get(after_name) != Some(&b'=') {
            if name != attribute_name {
                continue;
            }
            let span = NormalizedSpan::from_content_range(content, attribute_start, cursor)?;
            return Some(JsxAttributeSpan {
                value_start: None,
                value_end: cursor,
                span,
            });
        }
        let value_start = skip_ascii_whitespace_until(content, after_name + 1, tag_end);
        let value_end = jsx_attribute_value_end(content, value_start, tag_end)?;
        cursor = value_end;
        if name != attribute_name {
            continue;
        }
        let span = NormalizedSpan::from_content_range(content, attribute_start, value_end)?;
        return Some(JsxAttributeSpan {
            value_start: Some(value_start),
            value_end,
            span,
        });
    }
    None
}

fn jsx_attribute_value_end(content: &str, value_start: usize, tag_end: usize) -> Option<usize> {
    let bytes = content.as_bytes();
    match bytes.get(value_start)? {
        b'\'' | b'\"' => {
            let (_, end) = parse_js_string_literal(content, value_start)?;
            Some(end)
        }
        b'{' => find_matching_brace(content, value_start, tag_end).map(|end| end + 1),
        _ => {
            let mut cursor = value_start;
            while cursor < tag_end
                && !bytes[cursor].is_ascii_whitespace()
                && !matches!(bytes[cursor], b'/' | b'>')
            {
                cursor += 1;
            }
            Some(cursor)
        }
    }
}

fn is_static_react_route_path(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && !value.starts_with("//") && !value.contains("://")
}

fn is_static_route_definition_path(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && !value.starts_with("//") && !value.contains("://")
}

fn markup_tag_name(content: &str, tag_start: usize, tag_end: usize) -> Option<&str> {
    let bytes = content.as_bytes();
    let mut cursor = tag_start + 1;
    let name_start = cursor;
    while cursor < tag_end && is_attr_name_byte(bytes[cursor]) {
        cursor += 1;
    }
    (cursor > name_start)
        .then(|| content.get(name_start..cursor))
        .flatten()
}

fn is_static_route_path(value: &str) -> bool {
    value.trim().starts_with('/')
}

fn is_nuxt_route_path(value: &str) -> bool {
    let value = value.trim();
    value.starts_with('/') && !value.starts_with("//")
}

fn is_nuxt_link_tag(tag_name: &str) -> bool {
    matches!(
        tag_name.to_ascii_lowercase().as_str(),
        "nuxtlink" | "nuxt-link"
    )
}

fn is_nuxt_external_attribute(attribute_name: &str) -> bool {
    matches!(attribute_name, "external" | ":external" | "v-bind:external")
}

fn parse_object_string_property(
    content: &str,
    start: usize,
    end: usize,
    property_name: &str,
) -> Option<String> {
    let value_start = find_object_property_value_start(content, start, end, property_name)?;
    let (value, value_end) = parse_js_string_literal(content, value_start)?;
    (value_end <= end).then_some(value)
}

fn parse_object_identifier_property(
    content: &str,
    start: usize,
    end: usize,
    property_name: &str,
) -> Option<String> {
    let value_start = find_object_property_value_start(content, start, end, property_name)?;
    let (identifier, _) = parse_js_identifier(content, value_start, end)?;
    Some(identifier)
}

fn find_object_property_value_start(
    content: &str,
    start: usize,
    end: usize,
    property_name: &str,
) -> Option<usize> {
    let mut cursor = start;
    while cursor < end {
        let Some(relative_start) = content[cursor..end].find(property_name) else {
            break;
        };
        let property_start = cursor + relative_start;
        cursor = property_start + property_name.len();
        if !is_identifier_boundary(content, property_start, property_name.len()) {
            continue;
        }
        let colon = skip_ascii_whitespace_until(content, cursor, end);
        if content.as_bytes().get(colon) != Some(&b':') {
            continue;
        }
        return Some(skip_ascii_whitespace_until(content, colon + 1, end));
    }
    None
}

fn parse_js_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    let bytes = content.as_bytes();
    let quote = bytes
        .get(start)
        .copied()
        .filter(|byte| matches!(*byte, b'\'' | b'"'))?;
    let mut cursor = start + 1;
    let mut value = String::new();

    while cursor < content.len() {
        let byte = bytes[cursor];
        if byte == b'\\' {
            let escaped_start = cursor + 1;
            let escaped = content.get(escaped_start..)?.chars().next()?;
            value.push(escaped);
            cursor = escaped_start + escaped.len_utf8();
        } else if byte == quote {
            return Some((value, cursor + 1));
        } else {
            let ch = content.get(cursor..)?.chars().next()?;
            value.push(ch);
            cursor += ch.len_utf8();
        }
    }

    None
}

fn parse_js_identifier(content: &str, start: usize, end: usize) -> Option<(String, usize)> {
    let bytes = content.as_bytes();
    let first = *bytes.get(start)?;
    if !is_js_identifier_start_byte(first) {
        return None;
    }
    let mut cursor = start + 1;
    while cursor < end
        && bytes
            .get(cursor)
            .is_some_and(|byte| is_js_identifier_byte(*byte))
    {
        cursor += 1;
    }
    Some((content.get(start..cursor)?.to_string(), cursor))
}

fn is_js_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    is_js_identifier_start_byte(first) && bytes.all(is_js_identifier_byte)
}

fn is_identifier_boundary(content: &str, start: usize, len: usize) -> bool {
    let bytes = content.as_bytes();
    let before = start.checked_sub(1).and_then(|index| bytes.get(index));
    let after = bytes.get(start + len);
    !before.is_some_and(|byte| is_js_identifier_byte(*byte))
        && !after.is_some_and(|byte| is_js_identifier_byte(*byte))
}

fn is_js_identifier_start_byte(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic()
}

fn is_js_identifier_byte(byte: u8) -> bool {
    is_js_identifier_start_byte(byte) || byte.is_ascii_digit()
}

fn is_ignored_syntax_range(tree: &Tree, start_byte: usize, end_byte: usize) -> bool {
    smallest_node_covering_range(tree.root_node(), start_byte, end_byte)
        .is_some_and(|node| node_or_parent_is_comment_or_string(node))
}

fn smallest_node_covering_range<'tree>(
    node: Node<'tree>,
    start_byte: usize,
    end_byte: usize,
) -> Option<Node<'tree>> {
    smallest_node_covering_range_at_depth(node, start_byte, end_byte, 0)
}

fn smallest_node_covering_range_at_depth<'tree>(
    node: Node<'tree>,
    start_byte: usize,
    end_byte: usize,
    depth: u32,
) -> Option<Node<'tree>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.start_byte() > start_byte || node.end_byte() < end_byte {
        return None;
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return Some(node);
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) =
            smallest_node_covering_range_at_depth(child, start_byte, end_byte, child_depth)
        {
            return Some(found);
        }
    }

    Some(node)
}

fn node_or_parent_is_comment_or_string(mut node: Node<'_>) -> bool {
    loop {
        if is_comment_or_string_node(node.kind()) {
            return true;
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
}

fn is_comment_or_string_node(node_kind: &str) -> bool {
    node_kind.contains("comment") || node_kind.contains("string")
}

fn parent_route_path_for_object(
    tree: &Tree,
    content: &str,
    range_start: usize,
    range_end: usize,
    object_start: usize,
    object_end: usize,
) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    let mut cursor = range_start;
    while cursor < range_end {
        let Some(relative_path_start) = content[cursor..range_end].find("path") else {
            break;
        };
        let path_start = cursor + relative_path_start;
        cursor = path_start + "path".len();
        if !is_identifier_boundary(content, path_start, "path".len()) {
            continue;
        }
        if is_ignored_syntax_range(tree, path_start, cursor) {
            continue;
        }
        let colon = skip_ascii_whitespace_until(content, cursor, range_end);
        if content.as_bytes().get(colon) != Some(&b':') {
            continue;
        }
        let value_start = skip_ascii_whitespace_until(content, colon + 1, range_end);
        let Some((route_path, _)) = parse_js_string_literal(content, value_start) else {
            continue;
        };
        let Some((candidate_start, candidate_end)) =
            find_enclosing_object_range(content, range_start, range_end, path_start)
        else {
            continue;
        };
        if candidate_start >= object_start || candidate_end < object_end {
            continue;
        }
        let candidate_len = candidate_end - candidate_start;
        if best
            .as_ref()
            .map(|(best_len, _)| candidate_len < *best_len)
            .unwrap_or(true)
        {
            best = Some((candidate_len, route_path));
        }
    }
    best.map(|(_, route_path)| route_path)
}

fn join_frontend_route_paths(parent: &str, child: &str) -> String {
    if child.starts_with('/') {
        return child.to_string();
    }
    let parent = parent.trim_end_matches('/');
    let child = child.trim_start_matches('/');
    if parent.is_empty() {
        format!("/{child}")
    } else if child.is_empty() {
        parent.to_string()
    } else {
        format!("{parent}/{child}")
    }
}

fn find_enclosing_object_range(
    content: &str,
    start: usize,
    end: usize,
    position: usize,
) -> Option<(usize, usize)> {
    let mut cursor = start;
    let mut candidate = None;
    while cursor < position {
        let Some(relative_open) = content[cursor..position].find('{') else {
            break;
        };
        let object_start = cursor + relative_open;
        cursor = object_start + 1;
        let Some(object_end) = find_matching_brace(content, object_start, end) else {
            continue;
        };
        if object_end >= position {
            candidate = Some((object_start, object_end + 1));
        }
    }
    candidate
}

fn find_matching_brace(content: &str, open_brace: usize, end: usize) -> Option<usize> {
    if content.as_bytes().get(open_brace) != Some(&b'{') {
        return None;
    }
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut cursor = open_brace;
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
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b'{' {
            depth += 1;
        } else if byte == b'}' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }

    None
}

fn find_matching_paren(content: &str, open_paren: usize, end: usize) -> Option<usize> {
    if content.as_bytes().get(open_paren) != Some(&b'(') {
        return None;
    }
    let bytes = content.as_bytes();
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut cursor = open_paren;
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
        } else if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
        } else if byte == b'{' {
            brace_depth += 1;
        } else if byte == b'}' {
            brace_depth = brace_depth.saturating_sub(1);
        } else if byte == b'[' {
            bracket_depth += 1;
        } else if byte == b']' {
            bracket_depth = bracket_depth.saturating_sub(1);
        } else if byte == b'(' {
            paren_depth += 1;
        } else if byte == b')' {
            paren_depth = paren_depth.saturating_sub(1);
            if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }

    None
}

fn find_matching_bracket(content: &str, open_bracket: usize, end: usize) -> Option<usize> {
    if content.as_bytes().get(open_bracket) != Some(&b'[') {
        return None;
    }
    let bytes = content.as_bytes();
    let mut depth = 0usize;
    let mut cursor = open_bracket;
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
        } else if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
        } else if byte == b'[' {
            depth += 1;
        } else if byte == b']' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(cursor);
            }
        }
        cursor += 1;
    }

    None
}

fn find_top_level_comma_or_end(content: &str, start: usize, end: usize) -> usize {
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    while cursor < end {
        let byte = content.as_bytes()[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' || byte == b'`' {
            quote = Some(byte);
        } else {
            match byte {
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b',' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => {
                    return cursor;
                }
                _ => {}
            }
        }
        cursor += 1;
    }

    end
}

fn find_js_array_initializer_range(content: &str, identifier: &str) -> Option<(usize, usize)> {
    find_js_array_initializer_range_in(content, identifier, 0, content.len())
}

fn find_js_array_initializer_range_in(
    content: &str,
    identifier: &str,
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    let mut cursor = start;
    while cursor < end {
        let Some(relative_start) = content[cursor..end].find(identifier) else {
            break;
        };
        let identifier_start = cursor + relative_start;
        cursor = identifier_start + identifier.len();
        if !is_identifier_boundary(content, identifier_start, identifier.len()) {
            continue;
        }
        let equals = skip_ascii_whitespace_until(content, cursor, end);
        if content.as_bytes().get(equals) != Some(&b'=') {
            continue;
        }
        let array_start = skip_ascii_whitespace_until(content, equals + 1, end);
        if content.as_bytes().get(array_start) != Some(&b'[') {
            continue;
        }
        let array_end = find_matching_bracket(content, array_start, end)?;
        return Some((array_start, array_end + 1));
    }
    None
}

fn skip_ascii_whitespace_until(content: &str, mut cursor: usize, end: usize) -> usize {
    let bytes = content.as_bytes();
    while cursor < end && bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    cursor
}

fn parse_attr_value(attrs: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let prefix = format!("{name}={quote}");
        if let Some(start) = attrs.find(&prefix) {
            let value_start = start + prefix.len();
            let value_end = attrs[value_start..].find(quote)? + value_start;
            return Some(attrs[value_start..value_end].to_string());
        }
    }
    None
}

fn has_boolean_attr(attrs: &str, name: &str) -> bool {
    attrs
        .split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, '<' | '>' | '/'))
        .any(|part| part == name)
}
