use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Tree;

use super::fact_builders::{base_metadata, fact_for_span, insert_string};
use super::js_object_scan::{
    ScriptSyntaxMask, find_enclosing_object_range, find_js_array_initializer_range_in,
    find_matching_paren, find_object_property_value_start, is_identifier_boundary,
    is_ignored_syntax_range, is_js_identifier, join_frontend_route_paths,
    object_or_ancestor_value_property_matches, parent_route_path_for_object, parse_js_identifier,
    parse_js_string_literal, parse_object_identifier_property, parse_object_string_property,
    skip_ascii_whitespace_until,
};
use super::jsx_scan::{has_boolean_attr, next_markup_tag, parse_attr_value};
use super::nextjs_nuxt::{
    has_static_import_source, is_nuxt_external_attribute, is_nuxt_link_tag, is_nuxt_route_path,
    is_static_route_definition_path, nuxt_file_route_fact,
};
use super::{
    NUXT_ROUTE_REFERENCE_PATTERN_ID, VUE_ROUTE_DEFINITION_PATTERN_ID,
    VUE_ROUTE_REFERENCE_PATTERN_ID, VUE_SFC_SECTION_PATTERN_ID, VUE_TEMPLATE_DIRECTIVE_PATTERN_ID,
};
use crate::base::markup_scan::{
    MarkupAttribute, find_tag_end, scan_markup_attributes, scan_tag_attributes,
    split_argument_and_modifiers,
};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

#[derive(Debug)]
struct VueSectionSpan {
    section_type: &'static str,
    lang: Option<String>,
    setup: bool,
    scoped: bool,
    start_span: NormalizedSpan,
    content_start: usize,
    content_end: usize,
}

struct VueRouteReference {
    source_kind: &'static str,
    target_path: String,
    expression: Option<String>,
}

#[derive(Debug)]
struct VueDirective {
    name: &'static str,
    argument: Option<String>,
    modifiers: Vec<String>,
    shorthand: bool,
}

pub(super) fn collect_vue_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();

    if let Some(fact) = nuxt_file_route_fact("vue", tree, file_path, content) {
        facts.push(fact);
    }

    for section in scan_vue_sections(content) {
        facts.push(vue_section_fact(file_path, &section));

        if section.section_type == "template" {
            facts.extend(collect_nuxt_route_references(file_path, content, &section));
            for attribute in
                scan_markup_attributes(content, section.content_start, section.content_end)
            {
                let directive = parse_vue_directive(&attribute.name);
                if let Some(route_fact) =
                    vue_route_reference_fact(file_path, &attribute, directive.as_ref())
                {
                    facts.push(route_fact);
                }
                if let Some(directive) = directive {
                    facts.push(vue_template_directive_fact(
                        file_path, &attribute, directive,
                    ));
                }
            }
        } else if section.section_type == "script" {
            facts.extend(collect_vue_route_definitions(
                "vue", tree, file_path, content, &section,
            ));
        }
    }

    facts
}

fn vue_section_fact(file_path: &str, section: &VueSectionSpan) -> StructuralFact {
    let mut metadata = base_metadata("component_structure");
    insert_string(&mut metadata, "section_type", section.section_type);
    if let Some(lang) = section.lang.as_deref() {
        insert_string(&mut metadata, "lang", lang);
    }
    metadata.insert("setup".to_string(), Value::Bool(section.setup));
    metadata.insert("scoped".to_string(), Value::Bool(section.scoped));

    fact_for_span(
        file_path,
        "vue",
        VUE_SFC_SECTION_PATTERN_ID,
        "section",
        "sfc_section",
        section.start_span,
        metadata,
    )
}

fn vue_template_directive_fact(
    file_path: &str,
    attribute: &MarkupAttribute,
    directive: VueDirective,
) -> StructuralFact {
    let mut metadata = base_metadata("component_template");
    insert_string(&mut metadata, "directive", directive.name);
    insert_string(&mut metadata, "attribute_name", &attribute.name);
    metadata.insert("shorthand".to_string(), Value::Bool(directive.shorthand));
    if let Some(argument) = directive.argument {
        insert_string(&mut metadata, "argument", &argument);
    }
    if !directive.modifiers.is_empty() {
        metadata.insert(
            "modifiers".to_string(),
            Value::Array(directive.modifiers.into_iter().map(Value::String).collect()),
        );
    }
    if let Some(expression) = attribute.value.as_deref() {
        insert_string(&mut metadata, "expression", expression);
    }

    fact_for_span(
        file_path,
        "vue",
        VUE_TEMPLATE_DIRECTIVE_PATTERN_ID,
        "directive",
        "template_attribute",
        attribute.span,
        metadata,
    )
}

fn vue_route_reference_fact(
    file_path: &str,
    attribute: &MarkupAttribute,
    directive: Option<&VueDirective>,
) -> Option<StructuralFact> {
    let reference = vue_route_reference(attribute, directive)?;
    let mut metadata = base_metadata("frontend_navigation");
    insert_string(&mut metadata, "framework", "vue");
    insert_string(&mut metadata, "source_kind", reference.source_kind);
    insert_string(&mut metadata, "target_path", &reference.target_path);
    insert_string(&mut metadata, "verb", "GET");
    insert_string(&mut metadata, "attribute_name", &attribute.name);
    if let Some(expression) = reference.expression.as_deref() {
        insert_string(&mut metadata, "expression", expression);
    }

    Some(fact_for_span(
        file_path,
        "vue",
        VUE_ROUTE_REFERENCE_PATTERN_ID,
        "route_reference",
        "template_attribute",
        attribute.span,
        metadata,
    ))
}

fn collect_nuxt_route_references(
    file_path: &str,
    content: &str,
    section: &VueSectionSpan,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let mut cursor = section.content_start;

    while cursor < section.content_end {
        let Some((tag_start, tag_end, tag_name)) =
            next_markup_tag(content, cursor, section.content_end)
        else {
            break;
        };
        cursor = tag_end + 1;
        if !is_nuxt_link_tag(tag_name) {
            continue;
        }

        let mut attributes = Vec::new();
        scan_tag_attributes(content, tag_start, tag_end, &mut attributes);
        if attributes
            .iter()
            .any(|attribute| is_nuxt_external_attribute(&attribute.name))
        {
            continue;
        }

        let Some((attribute, target_path)) = attributes.iter().find_map(|attribute| {
            if attribute.name == "to" {
                return attribute
                    .value
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| is_nuxt_route_path(value))
                    .map(|value| (attribute, value.to_string()));
            }
            if matches!(attribute.name.as_str(), ":to" | "v-bind:to") {
                return attribute
                    .value
                    .as_deref()
                    .and_then(parse_vue_string_literal)
                    .filter(|value| is_nuxt_route_path(value))
                    .map(|value| (attribute, value));
            }
            None
        }) else {
            continue;
        };

        let mut metadata = base_metadata("frontend_navigation");
        insert_string(&mut metadata, "framework", "nuxt");
        insert_string(&mut metadata, "target_path", &target_path);
        insert_string(&mut metadata, "verb", "GET");
        insert_string(&mut metadata, "attribute_name", &attribute.name);
        insert_string(&mut metadata, "component_name", tag_name);
        insert_string(&mut metadata, "route_source", "string_literal");
        insert_string(&mut metadata, "source_kind", "nuxt_link");

        facts.push(fact_for_span(
            file_path,
            "vue",
            NUXT_ROUTE_REFERENCE_PATTERN_ID,
            "route_reference",
            "template_attribute",
            attribute.span,
            metadata,
        ));
    }

    facts
}

fn collect_vue_route_definitions(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    section: &VueSectionSpan,
) -> Vec<StructuralFact> {
    let imports = collect_vue_static_imports(content, section);
    let mut facts = Vec::new();
    let syntax_mask =
        ScriptSyntaxMask::for_js_ranges(content, &[(section.content_start, section.content_end)]);
    let ranges = vue_route_definition_ranges(content, section, language == "vue", &syntax_mask);

    for (range_start, range_end) in ranges {
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
            if syntax_mask.is_ignored(path_start) {
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
            let Some((target_path, path_end)) = parse_js_string_literal(content, value_start)
                .filter(|(value, end)| *end <= range_end && is_static_route_definition_path(value))
            else {
                continue;
            };

            let (span_start, span_end) =
                find_enclosing_object_range(content, range_start, range_end, path_start)
                    .unwrap_or((path_start, path_end));
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

            let route_name = parse_object_string_property(content, span_start, span_end, "name");
            let component_name =
                parse_object_identifier_property(content, span_start, span_end, "component");
            let component_path = component_name
                .as_ref()
                .and_then(|name| imports.get(name))
                .cloned();
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
                .map(|parent| join_frontend_route_paths(parent, &target_path));

            let mut metadata = base_metadata("frontend_navigation");
            insert_string(&mut metadata, "framework", "vue");
            insert_string(&mut metadata, "target_path", &target_path);
            insert_string(&mut metadata, "source_kind", "vue_router_route");
            insert_string(&mut metadata, "route_source", "string_literal");
            if let Some(parent_route_path) = parent_route_path {
                insert_string(&mut metadata, "parent_route_path", &parent_route_path);
            }
            if let Some(effective_route_template) = effective_route_template {
                insert_string(
                    &mut metadata,
                    "effective_route_template",
                    &effective_route_template,
                );
            }
            if let Some(route_name) = route_name {
                insert_string(&mut metadata, "route_name", &route_name);
            }
            if let Some(component_name) = component_name {
                insert_string(&mut metadata, "component_name", &component_name);
            }
            if let Some(component_path) = component_path {
                insert_string(&mut metadata, "component_path", &component_path);
            }

            facts.push(fact_for_span(
                file_path,
                language,
                VUE_ROUTE_DEFINITION_PATTERN_ID,
                "route_definition",
                "object",
                span,
                metadata,
            ));
        }
    }

    facts
}

fn vue_route_definition_ranges(
    content: &str,
    section: &VueSectionSpan,
    include_loose_routes_array: bool,
    syntax_mask: &ScriptSyntaxMask,
) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();

    if include_loose_routes_array
        && let Some(range) = find_js_array_initializer_range_in(
            content,
            "routes",
            section.content_start,
            section.content_end,
        )
        && !syntax_mask.is_ignored(range.0)
    {
        ranges.push(range);
    }

    let mut cursor = section.content_start;
    while cursor < section.content_end {
        let Some(relative_start) = content[cursor..section.content_end].find("createRouter") else {
            break;
        };
        let api_start = cursor + relative_start;
        cursor = api_start + "createRouter".len();
        if !is_identifier_boundary(content, api_start, "createRouter".len()) {
            continue;
        }
        if syntax_mask.is_ignored(api_start) {
            continue;
        }
        let open_paren = skip_ascii_whitespace_until(content, cursor, section.content_end);
        if content.as_bytes().get(open_paren) != Some(&b'(') {
            continue;
        }
        let Some(close_paren) = find_matching_paren(content, open_paren, section.content_end)
        else {
            continue;
        };
        ranges.push((open_paren + 1, close_paren));
        if let Some(routes_identifier) =
            create_router_routes_identifier(content, open_paren + 1, close_paren)
            && let Some(range) = find_js_array_initializer_range_in(
                content,
                &routes_identifier,
                section.content_start,
                section.content_end,
            )
            && !syntax_mask.is_ignored(range.0)
        {
            ranges.push(range);
        }
    }

    ranges.sort_unstable();
    ranges.dedup();
    ranges
}

fn create_router_routes_identifier(content: &str, start: usize, end: usize) -> Option<String> {
    let routes_value_start = find_object_property_value_start(content, start, end, "routes")?;
    parse_js_identifier(content, routes_value_start, end).map(|(identifier, _)| identifier)
}

pub(super) fn collect_vue_router_route_definitions(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !has_static_import_source(tree, content, "vue-router") {
        return Vec::new();
    }

    let Some(start_span) = NormalizedSpan::from_content_range(content, 0, content.len()) else {
        return Vec::new();
    };
    let section = VueSectionSpan {
        section_type: "script",
        lang: None,
        setup: false,
        scoped: false,
        start_span,
        content_start: 0,
        content_end: content.len(),
    };
    collect_vue_route_definitions(language, tree, file_path, content, &section)
}

fn collect_vue_static_imports(content: &str, section: &VueSectionSpan) -> HashMap<String, String> {
    let mut imports = HashMap::new();
    let Some(section_text) = content.get(section.content_start..section.content_end) else {
        return imports;
    };

    for line in section_text.lines() {
        if let Some((binding, specifier)) = parse_vue_static_import_line(line.trim()) {
            imports.insert(binding, specifier);
        }
    }

    imports
}

fn parse_vue_static_import_line(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("import ")?;
    let (binding, specifier) = rest.split_once(" from ")?;
    let binding = binding.trim();
    if !is_js_identifier(binding) {
        return None;
    }
    let specifier = specifier.trim().trim_end_matches(';').trim();
    let (specifier, _) = parse_js_string_literal(specifier, 0)?;
    Some((binding.to_string(), specifier))
}

/// Content ranges of `<script>` / `<script setup>` sections, for collectors
/// that scan script bodies only (e.g. the HTTP client-request scan).
pub(super) fn vue_script_section_ranges(content: &str) -> Vec<(usize, usize)> {
    scan_vue_sections(content)
        .into_iter()
        .filter(|section| section.section_type == "script")
        .map(|section| (section.content_start, section.content_end))
        .collect()
}

/// Content ranges of `<template>` sections, for collectors that scan template
/// markup only (e.g. the htmx attribute scan in `framework_structural_facts`).
/// Restricting to template ranges keeps htmx attributes embedded in `<script>`
/// strings silent.
pub(crate) fn vue_template_section_ranges(content: &str) -> Vec<(usize, usize)> {
    scan_vue_sections(content)
        .into_iter()
        .filter(|section| section.section_type == "template")
        .map(|section| (section.content_start, section.content_end))
        .collect()
}

fn scan_vue_sections(content: &str) -> Vec<VueSectionSpan> {
    let mut sections = Vec::new();
    let mut cursor = 0usize;

    while let Some((tag_start, section_type)) = next_vue_section_start(content, cursor) {
        let Some(open_tag_end) = find_tag_end(content, tag_start) else {
            break;
        };
        let content_start = open_tag_end + 1;
        let Some(content_end) = find_vue_section_content_end(content, section_type, content_start)
        else {
            cursor = content_start;
            continue;
        };
        let close_tag = format!("</{section_type}>");
        let tag_end = content_end + close_tag.len();
        let Some(span) = NormalizedSpan::from_content_range(content, tag_start, tag_end) else {
            cursor = tag_end;
            continue;
        };

        let attrs = content.get(tag_start..=open_tag_end).unwrap_or_default();
        sections.push(VueSectionSpan {
            section_type,
            lang: parse_attr_value(attrs, "lang"),
            setup: has_boolean_attr(attrs, "setup"),
            scoped: has_boolean_attr(attrs, "scoped"),
            start_span: span,
            content_start,
            content_end,
        });
        cursor = tag_end;
    }

    sections
}

fn find_vue_section_content_end(
    content: &str,
    section_type: &str,
    content_start: usize,
) -> Option<usize> {
    if section_type != "template" {
        let close_tag = format!("</{section_type}>");
        return content[content_start..]
            .find(&close_tag)
            .map(|relative| content_start + relative);
    }

    let mut cursor = content_start;
    let mut depth = 1usize;
    while let Some(relative_tag_start) = content[cursor..].find('<') {
        let tag_start = cursor + relative_tag_start;
        if vue_tag_name_matches(content, tag_start, "template", true) {
            let tag_end = find_tag_end(content, tag_start)?;
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(tag_start);
            }
            cursor = tag_end + 1;
        } else if vue_tag_name_matches(content, tag_start, "template", false) {
            let tag_end = find_tag_end(content, tag_start)?;
            depth += 1;
            cursor = tag_end + 1;
        } else {
            cursor = tag_start + 1;
        }
    }
    None
}

fn vue_tag_name_matches(
    content: &str,
    tag_start: usize,
    expected_name: &str,
    closing: bool,
) -> bool {
    let Some(after_open) = tag_start.checked_add(1) else {
        return false;
    };
    let name_start = after_open + usize::from(closing);
    if closing && content.as_bytes().get(after_open) != Some(&b'/') {
        return false;
    }
    let name_end = name_start + expected_name.len();
    if content.get(name_start..name_end) != Some(expected_name) {
        return false;
    }
    content
        .as_bytes()
        .get(name_end)
        .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(*byte, b'>' | b'/'))
}

fn next_vue_section_start(content: &str, cursor: usize) -> Option<(usize, &'static str)> {
    ["template", "script", "style"]
        .into_iter()
        .filter_map(|section| {
            content[cursor..]
                .find(&format!("<{section}"))
                .map(|relative| (cursor + relative, section))
        })
        .min_by_key(|(start, _)| *start)
}

fn parse_vue_directive(attribute_name: &str) -> Option<VueDirective> {
    if let Some(rest) = attribute_name.strip_prefix('@') {
        let (argument, modifiers) = split_argument_and_modifiers(rest);
        return Some(VueDirective {
            name: "v-on",
            argument,
            modifiers,
            shorthand: true,
        });
    }

    if let Some(rest) = attribute_name.strip_prefix(':') {
        let (argument, modifiers) = split_argument_and_modifiers(rest);
        return Some(VueDirective {
            name: "v-bind",
            argument,
            modifiers,
            shorthand: true,
        });
    }

    let rest = attribute_name.strip_prefix("v-")?;
    let base = rest
        .find(&[':', '.'][..])
        .map(|index| &rest[..index])
        .unwrap_or(rest);
    let directive_name = match base {
        "bind" => "v-bind",
        "on" => "v-on",
        "if" => "v-if",
        "else-if" => "v-else-if",
        "else" => "v-else",
        "for" => "v-for",
        "show" => "v-show",
        "model" => "v-model",
        "slot" => "v-slot",
        "text" => "v-text",
        "html" => "v-html",
        "pre" => "v-pre",
        "once" => "v-once",
        "memo" => "v-memo",
        "cloak" => "v-cloak",
        _ => return None,
    };

    let mut argument = None;
    let mut modifiers = Vec::new();
    let tail_start = "v-".len() + base.len();
    if let Some(separator) = attribute_name.as_bytes().get(tail_start).copied() {
        let tail = &attribute_name[tail_start + 1..];
        if separator == b':' {
            let (parsed_argument, parsed_modifiers) = split_argument_and_modifiers(tail);
            argument = parsed_argument;
            modifiers = parsed_modifiers;
        } else if separator == b'.' {
            modifiers = tail
                .split('.')
                .filter(|modifier| !modifier.is_empty())
                .map(ToString::to_string)
                .collect();
        }
    }

    Some(VueDirective {
        name: directive_name,
        argument,
        modifiers,
        shorthand: false,
    })
}

fn vue_route_reference(
    attribute: &MarkupAttribute,
    directive: Option<&VueDirective>,
) -> Option<VueRouteReference> {
    if is_vue_router_link_tag(&attribute.tag_name) {
        if attribute.name == "to" {
            let target_path = attribute.value.as_deref()?.trim();
            if is_vue_route_path(target_path) {
                return Some(VueRouteReference {
                    source_kind: "router_link",
                    target_path: target_path.to_string(),
                    expression: None,
                });
            }
        }

        if is_vue_to_binding(directive) {
            let expression = attribute.value.as_deref()?.trim();
            let target_path = parse_vue_string_literal(expression)?;
            if is_vue_route_path(&target_path) {
                return Some(VueRouteReference {
                    source_kind: "router_link",
                    target_path,
                    expression: Some(expression.to_string()),
                });
            }
        }
    }

    if directive.is_some_and(|directive| directive.name == "v-on") {
        let expression = attribute.value.as_deref()?.trim();
        let target_path = parse_vue_router_navigation_literal(expression)?;
        return Some(VueRouteReference {
            source_kind: "router_navigation_expression",
            target_path,
            expression: Some(expression.to_string()),
        });
    }

    None
}

fn is_vue_router_link_tag(tag_name: &str) -> bool {
    matches!(tag_name, "router-link" | "routerlink")
}

fn is_vue_to_binding(directive: Option<&VueDirective>) -> bool {
    directive.is_some_and(|directive| {
        directive.name == "v-bind" && directive.argument.as_deref() == Some("to")
    })
}

fn parse_vue_router_navigation_literal(expression: &str) -> Option<String> {
    let open_paren = expression.find('(')?;
    let receiver = expression[..open_paren].trim();
    if !matches!(
        receiver,
        "$router.push" | "$router.replace" | "router.push" | "router.replace"
    ) {
        return None;
    }

    let close_paren = expression.rfind(')')?;
    if !expression[close_paren + 1..].trim().is_empty() {
        return None;
    }

    let target_path = parse_vue_string_literal(expression[open_paren + 1..close_paren].trim())?;
    is_vue_route_path(&target_path).then_some(target_path)
}

fn parse_vue_string_literal(expression: &str) -> Option<String> {
    let trimmed = expression.trim();
    let bytes = trimmed.as_bytes();
    let quote = *bytes.first()?;
    if !matches!(quote, b'\'' | b'"') || bytes.last().copied() != Some(quote) {
        return None;
    }
    let inner = trimmed.get(1..trimmed.len().saturating_sub(1))?;
    if inner.as_bytes().contains(&b'\\') {
        return None;
    }
    Some(inner.to_string())
}

fn is_vue_route_path(value: &str) -> bool {
    value.starts_with('/') && !value.starts_with("//")
}
