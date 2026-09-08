use serde_json::Value;
use tree_sitter::Tree;

use super::helpers::{
    base_metadata, fact_for_span, insert_string, is_ignored_markup_node,
    smallest_node_covering_range,
};
use super::{ALPINE_DIRECTIVE_PATTERN_ID, HTMX_ATTRIBUTE_PATTERN_ID};
use crate::base::markup_scan::{
    MarkupAttribute, scan_markup_attributes, split_argument_and_modifiers,
};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

pub(super) fn collect_markup_framework_attributes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();

    for attribute in scan_markup_attributes(content, 0, content.len()) {
        if let Some((attribute_name, data_prefix)) = canonical_htmx_attribute_name(&attribute.name)
            && let Some(fact) = htmx_attribute_fact(
                language,
                tree,
                file_path,
                content,
                &attribute,
                &attribute_name,
                data_prefix,
            )
        {
            facts.push(fact);
        }

        if is_alpine_attribute_name(language, &attribute.name)
            && let Some(fact) =
                alpine_directive_fact(language, tree, file_path, content, &attribute)
        {
            facts.push(fact);
        }
    }

    facts
}

fn htmx_attribute_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    attribute: &MarkupAttribute,
    attribute_name: &str,
    data_prefix: bool,
) -> Option<StructuralFact> {
    let node =
        smallest_node_covering_range(tree.root_node(), attribute.start_byte, attribute.end_byte)?;
    if is_ignored_markup_node(node) {
        return None;
    }
    let expression = if matches!(language, "javascript" | "jsx" | "tsx") {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .find(|child| child.kind() == "jsx_expression" && !child.has_error())
    } else {
        None
    };
    let value = expression
        .and_then(|expression| content.get(expression.byte_range()))
        .or(attribute.value.as_deref());
    let (start, end) = if expression.is_some() {
        (node.start_byte(), node.end_byte())
    } else {
        (attribute.start_byte, attribute.end_byte)
    };
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    let mut metadata = htmx_attribute_metadata(attribute_name, value, data_prefix);
    if expression.is_some() {
        metadata.insert(
            "value_source".into(),
            Value::String("dynamic_expression".into()),
        );
    }
    if language == "vue"
        && (attribute.name.starts_with(':') || attribute.name.starts_with("v-bind:"))
    {
        metadata.insert(
            "value_source".into(),
            Value::String("dynamic_expression".into()),
        );
    }
    if let Some(value) = value {
        super::htmx_templates::enrich_htmx_template(language, value, &mut metadata);
    }

    Some(fact_for_span(
        file_path,
        language,
        HTMX_ATTRIBUTE_PATTERN_ID,
        "attribute",
        node.kind(),
        span,
        metadata,
    ))
}

pub(super) fn canonical_htmx_attribute_name(attribute_name: &str) -> Option<(String, bool)> {
    let normalized = attribute_name.to_ascii_lowercase();
    if normalized.starts_with("hx-") {
        return Some((normalized, false));
    }
    normalized
        .strip_prefix("data-hx-")
        .map(|suffix| (format!("hx-{suffix}"), true))
}

/// htmx emission for JSX/TSX component markup. The javascript/jsx/tsx grammars
/// all accept JSX, so the shared byte-level scanner runs over the whole file;
/// only static-string values emit (see `component_htmx_attribute_fact`).
pub(super) fn htmx_attribute_metadata(
    attribute_name: &str,
    value: Option<&str>,
    data_prefix: bool,
) -> std::collections::HashMap<String, Value> {
    let mut metadata = base_metadata("frontend_interaction", "htmx");

    insert_string(&mut metadata, "attribute_name", attribute_name);
    if data_prefix {
        metadata.insert("data_prefix".to_string(), Value::Bool(true));
    }
    if let Some(value) = value {
        insert_string(&mut metadata, "attribute_value", value);
    }
    if let Some(verb) = htmx_request_verb(attribute_name) {
        insert_string(&mut metadata, "verb", verb);
        if let Some(target_path) = value.filter(|value| is_static_path(value)) {
            insert_string(&mut metadata, "target_path", target_path);
        }
    }

    metadata
}

pub(super) fn collect_jsx_htmx_attributes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for attribute in scan_markup_attributes(content, 0, content.len()) {
        if let Some(fact) =
            component_htmx_attribute_fact(language, tree, file_path, content, &attribute)
        {
            facts.push(fact);
        }
    }
    facts
}

/// htmx emission for Vue single-file-component `<template>` markup. Scanning is
/// restricted to template-section ranges so htmx attributes embedded in
/// `<script>` string literals stay silent.
pub(super) fn collect_vue_template_htmx_attributes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for (section_start, section_end) in
        crate::base::web_structural_facts::vue_template_section_ranges(content)
    {
        for attribute in scan_markup_attributes(content, section_start, section_end) {
            if let Some(fact) =
                component_htmx_attribute_fact(language, tree, file_path, content, &attribute)
            {
                facts.push(fact);
            }
        }
    }
    facts
}

/// Emit static or parser-proven templated htmx paths from component markup.
fn component_htmx_attribute_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    attribute: &MarkupAttribute,
) -> Option<StructuralFact> {
    let name = if language == "vue" {
        attribute
            .name
            .strip_prefix("v-bind:")
            .or_else(|| attribute.name.strip_prefix(':'))
            .unwrap_or(&attribute.name)
    } else {
        &attribute.name
    };
    let (attribute_name, data_prefix) = canonical_htmx_attribute_name(name)?;
    let value = attribute.value.as_deref()?;
    let dynamic = value.starts_with('{') || name != attribute.name;
    let fact = htmx_attribute_fact(
        language,
        tree,
        file_path,
        content,
        attribute,
        &attribute_name,
        data_prefix,
    )?;
    if dynamic
        && !fact
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.contains_key("normalized_route_template"))
    {
        return None;
    }
    Some(fact)
}

fn alpine_directive_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    attribute: &MarkupAttribute,
) -> Option<StructuralFact> {
    let directive = parse_alpine_directive(&attribute.name)?;
    let node =
        smallest_node_covering_range(tree.root_node(), attribute.start_byte, attribute.end_byte)?;
    if is_ignored_markup_node(node) {
        return None;
    }
    let span =
        NormalizedSpan::from_content_range(content, attribute.start_byte, attribute.end_byte)?;
    let mut metadata = base_metadata("frontend_interaction", "alpine");

    insert_string(&mut metadata, "directive", directive.name);
    if let Some(argument) = directive.argument {
        insert_string(&mut metadata, "argument", &argument);
    }
    if !directive.modifiers.is_empty() {
        metadata.insert(
            "modifiers".to_string(),
            Value::Array(
                directive
                    .modifiers
                    .iter()
                    .map(|modifier| Value::String(modifier.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(expression) = attribute.value.as_deref() {
        insert_string(&mut metadata, "expression", expression);
    }
    metadata.insert("shorthand".to_string(), Value::Bool(directive.shorthand));

    Some(fact_for_span(
        file_path,
        language,
        ALPINE_DIRECTIVE_PATTERN_ID,
        "directive",
        node.kind(),
        span,
        metadata,
    ))
}
fn htmx_request_verb(attribute_name: &str) -> Option<&'static str> {
    match attribute_name {
        "hx-get" => Some("GET"),
        "hx-post" => Some("POST"),
        "hx-put" => Some("PUT"),
        "hx-patch" => Some("PATCH"),
        "hx-delete" => Some("DELETE"),
        _ => None,
    }
}

fn is_static_path(value: &str) -> bool {
    value.starts_with('/') || value.starts_with("./") || value.starts_with("../")
}

fn is_alpine_attribute_name(language: &str, attribute_name: &str) -> bool {
    if attribute_name.starts_with("x-") {
        return true;
    }
    if language == "razor" {
        return false;
    }
    attribute_name.starts_with('@') || attribute_name.starts_with(':')
}

#[derive(Debug)]
struct AlpineDirective {
    name: &'static str,
    argument: Option<String>,
    modifiers: Vec<String>,
    shorthand: bool,
}

fn parse_alpine_directive(attribute_name: &str) -> Option<AlpineDirective> {
    if let Some(rest) = attribute_name.strip_prefix('@') {
        let (argument, modifiers) = split_argument_and_modifiers(rest);
        return Some(AlpineDirective {
            name: "x-on",
            argument,
            modifiers,
            shorthand: true,
        });
    }

    if let Some(rest) = attribute_name.strip_prefix(':') {
        let (argument, modifiers) = split_argument_and_modifiers(rest);
        return Some(AlpineDirective {
            name: "x-bind",
            argument,
            modifiers,
            shorthand: true,
        });
    }

    let rest = attribute_name.strip_prefix("x-")?;
    let base = rest
        .find(&[':', '.'][..])
        .map(|index| &rest[..index])
        .unwrap_or(rest);
    let directive_name = match base {
        "bind" => "x-bind",
        "on" => "x-on",
        "data" => "x-data",
        "show" => "x-show",
        "if" => "x-if",
        "for" => "x-for",
        "text" => "x-text",
        "html" => "x-html",
        "model" => "x-model",
        "effect" => "x-effect",
        "init" => "x-init",
        "ref" => "x-ref",
        "cloak" => "x-cloak",
        "transition" => "x-transition",
        "ignore" => "x-ignore",
        "id" => "x-id",
        "teleport" => "x-teleport",
        _ => return None,
    };

    let mut argument = None;
    let mut modifiers = Vec::new();
    let tail_start = "x-".len() + base.len();
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

    Some(AlpineDirective {
        name: directive_name,
        argument,
        modifiers,
        shorthand: false,
    })
}
