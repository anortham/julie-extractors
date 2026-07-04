use serde_json::Value;
use tree_sitter::Tree;

use super::fact_builders::{base_metadata, fact_for_span, insert_string};
use super::js_imports::JsImportIndex;
use super::js_object_scan::{
    find_enclosing_object_range, find_js_array_initializer_range, find_matching_paren,
    find_object_property_value_start, find_top_level_comma_or_end, is_identifier_boundary,
    is_ignored_syntax_range, join_frontend_route_paths, object_or_ancestor_value_property_matches,
    parent_route_path_for_object, parse_js_identifier, parse_js_string_literal,
    parse_object_identifier_property, parse_object_string_property, skip_ascii_whitespace_until,
};
use super::jsx_scan::{
    jsx_boolean_attribute, jsx_element_component_attribute, jsx_identifier_expression_attribute,
    jsx_string_literal_attribute, next_markup_tag, parse_jsx_element_component_at,
};
use super::{REACT_ROUTE_DEFINITION_PATTERN_ID, REACT_ROUTE_REFERENCE_PATTERN_ID};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

pub(super) fn collect_react_router_route_references(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &JsImportIndex,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut cursor = 0;
    let mut route_stack: Vec<String> = Vec::new();

    while cursor < content.len() {
        let Some((tag_start, tag_end, tag_name)) = next_markup_tag(content, cursor, content.len())
        else {
            break;
        };
        pop_closed_jsx_route_tags(content, cursor, tag_start, imports, &mut route_stack);
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

pub(super) fn collect_react_router_route_definitions(
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
    let mut route_stack: Vec<String> = Vec::new();

    while cursor < content.len() {
        let Some((tag_start, tag_end, tag_name)) = next_markup_tag(content, cursor, content.len())
        else {
            break;
        };
        pop_closed_jsx_route_tags(content, cursor, tag_start, imports, &mut route_stack);
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
        let route_path = path.map(|(value, _)| value);
        let parent_route_path = route_stack.last().cloned();
        let effective_route_template = if index_route {
            parent_route_path.clone()
        } else {
            parent_route_path
                .as_ref()
                .zip(route_path.as_ref())
                .map(|(parent, child)| join_frontend_route_paths(parent, child))
        };
        let current_route_template = if index_route {
            parent_route_path.clone()
        } else {
            effective_route_template
                .clone()
                .or_else(|| route_path.clone())
        };

        facts.push(react_route_definition_fact(
            file_path,
            language,
            ReactRouteDefinitionFact {
                source_kind: "jsx_route",
                route_path,
                index_route,
                route_component,
                route_id: None,
                parent_route_path,
                effective_route_template,
                span,
                node_kind: "jsx_element",
            },
        ));
        if !jsx_tag_is_self_closing(content, tag_start, tag_end)
            && let Some(current_route_template) = current_route_template
        {
            route_stack.push(current_route_template);
        }
    }

    facts
}

fn pop_closed_jsx_route_tags(
    content: &str,
    start: usize,
    end: usize,
    imports: &JsImportIndex,
    route_stack: &mut Vec<String>,
) {
    let mut cursor = start;
    while cursor < end {
        let Some(relative_close) = content[cursor..end].find("</") else {
            break;
        };
        let name_start = cursor + relative_close + 2;
        let mut name_end = name_start;
        while name_end < end && is_jsx_tag_name_byte(content.as_bytes()[name_end]) {
            name_end += 1;
        }
        if name_end > name_start
            && let Some(tag_name) = content.get(name_start..name_end)
            && imports.react_router_routes.contains_key(tag_name)
        {
            route_stack.pop();
        }
        cursor = name_end.max(name_start + 1);
    }
}

fn jsx_tag_is_self_closing(content: &str, tag_start: usize, tag_end: usize) -> bool {
    content
        .as_bytes()
        .get(tag_start..tag_end)
        .and_then(|bytes| {
            bytes
                .iter()
                .rposition(|byte| !byte.is_ascii_whitespace())
                .map(|index| bytes[index])
        })
        == Some(b'/')
}

fn is_jsx_tag_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'.')
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
            if object_or_ancestor_value_property_matches(
                content,
                range_start,
                range_end,
                span_start,
                span_end,
                &["redirect", "meta"],
            ) {
                continue;
            }
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
                || !is_identifier_boundary(content, value_start, "true".len())
            {
                continue;
            }
            let Some((span_start, span_end)) =
                find_enclosing_object_range(content, range_start, range_end, index_start)
            else {
                continue;
            };
            if object_or_ancestor_value_property_matches(
                content,
                range_start,
                range_end,
                span_start,
                span_end,
                &["redirect", "meta"],
            ) {
                continue;
            }
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

fn is_static_react_route_path(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && !value.starts_with("//") && !value.contains("://")
}
