use std::collections::HashMap;

use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

use super::markup_scan::{MarkupAttribute, scan_markup_attributes, split_argument_and_modifiers};
use super::span::NormalizedSpan;
use super::structural_facts::sort_structural_facts;
use super::types::{StructuralFact, Symbol, stable_location_id};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const ASPNET_MINIMAL_API_ROUTE_PATTERN_ID: &str = "aspnet.minimal_api.route.v1";
const ASPNET_MINIMAL_API_ROUTE_GROUP_PATTERN_ID: &str = "aspnet.minimal_api.route_group.v1";
const ASPNET_ATTRIBUTE_ROUTE_PATTERN_ID: &str = "aspnet.attribute_route.v1";
const HTMX_ATTRIBUTE_PATTERN_ID: &str = "htmx.attribute.v1";
const ALPINE_DIRECTIVE_PATTERN_ID: &str = "alpine.directive.v1";
const RAZOR_PAGE_DIRECTIVE_PATTERN_ID: &str = "razor.page_directive.v1";
const RAZOR_CODE_BLOCK_PATTERN_ID: &str = "razor.code_block.v1";
const RAZOR_TEMPLATE_EXPRESSION_PATTERN_ID: &str = "razor.template_expression.v1";

#[cfg(all(test, feature = "test-capability-matrix"))]
const CSHARP_FRAMEWORK_PATTERN_IDS: &[&str] = &[
    ASPNET_ATTRIBUTE_ROUTE_PATTERN_ID,
    ASPNET_MINIMAL_API_ROUTE_GROUP_PATTERN_ID,
    ASPNET_MINIMAL_API_ROUTE_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const MARKUP_FRAMEWORK_PATTERN_IDS: &[&str] =
    &[HTMX_ATTRIBUTE_PATTERN_ID, ALPINE_DIRECTIVE_PATTERN_ID];
// Component markup (JSX/TSX and Vue `<template>`) carries htmx-driven requests
// too, but not the Alpine directive surface the html/razor scan claims.
#[cfg(all(test, feature = "test-capability-matrix"))]
const COMPONENT_MARKUP_FRAMEWORK_PATTERN_IDS: &[&str] = &[HTMX_ATTRIBUTE_PATTERN_ID];
#[cfg(all(test, feature = "test-capability-matrix"))]
const RAZOR_FRAMEWORK_PATTERN_IDS: &[&str] = &[
    ALPINE_DIRECTIVE_PATTERN_ID,
    HTMX_ATTRIBUTE_PATTERN_ID,
    RAZOR_CODE_BLOCK_PATTERN_ID,
    RAZOR_PAGE_DIRECTIVE_PATTERN_ID,
    RAZOR_TEMPLATE_EXPRESSION_PATTERN_ID,
];

const ASPNET_ROUTE_METHODS: &[(&str, &str)] = &[
    ("MapGet", "GET"),
    ("MapPost", "POST"),
    ("MapPut", "PUT"),
    ("MapPatch", "PATCH"),
    ("MapDelete", "DELETE"),
];

pub fn collect_framework_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let mut facts = match language {
        "csharp" => {
            let mut csharp_facts =
                collect_aspnet_minimal_api_routes(language, tree, file_path, content);
            csharp_facts.extend(collect_aspnet_attribute_routes(
                language, tree, file_path, content,
            ));
            csharp_facts
        }
        "html" => collect_markup_framework_attributes(language, tree, file_path, content),
        "razor" => {
            let mut razor_facts = collect_razor_structural_facts(tree, file_path, content);
            razor_facts.extend(collect_markup_framework_attributes(
                language, tree, file_path, content,
            ));
            razor_facts
        }
        "javascript" | "jsx" | "tsx" => {
            collect_jsx_htmx_attributes(language, tree, file_path, content)
        }
        "vue" => collect_vue_template_htmx_attributes(language, tree, file_path, content),
        _ => Vec::new(),
    };

    attach_containing_symbols(&mut facts, symbols);
    sort_structural_facts(&mut facts);
    facts
}

#[cfg(all(test, feature = "test-capability-matrix"))]
pub(crate) fn framework_structural_fact_pattern_ids_for_language(
    language: &str,
) -> &'static [&'static str] {
    match language {
        "csharp" => CSHARP_FRAMEWORK_PATTERN_IDS,
        "html" => MARKUP_FRAMEWORK_PATTERN_IDS,
        "razor" => RAZOR_FRAMEWORK_PATTERN_IDS,
        "javascript" | "jsx" | "tsx" | "vue" => COMPONENT_MARKUP_FRAMEWORK_PATTERN_IDS,
        _ => &[],
    }
}

fn collect_razor_structural_facts(
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    collect_razor_node(tree.root_node(), file_path, content, &mut facts, 0);
    facts
}

fn collect_razor_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "razor_page_directive" => {
            if let Some(fact) = razor_page_directive_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "razor_block" => {
            if let Some(fact) = razor_code_block_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "razor_expression" | "razor_implicit_expression" => {
            if let Some(fact) = razor_template_expression_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_razor_node(child, file_path, content, facts, child_depth);
    }
}

fn razor_page_directive_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let route = razor_child_text(node, content, "string_literal")?;
    let route = route.trim_matches('"').trim_matches('\'').to_string();
    if route.is_empty() {
        return None;
    }

    let route_parameters = parse_razor_route_parameters(&route);
    let has_route_constraints = route_parameters
        .iter()
        .any(|parameter| parameter.constraint.is_some());

    let mut metadata = base_metadata("component_routing", "razor");
    insert_string(&mut metadata, "directive", "page");
    insert_string(&mut metadata, "route", &route);
    insert_string(&mut metadata, "route_template", &route);
    metadata.insert(
        "route_parameter_count".to_string(),
        Value::Number(Number::from(route_parameters.len())),
    );
    metadata.insert(
        "has_route_constraints".to_string(),
        Value::Bool(has_route_constraints),
    );
    metadata.insert(
        "route_parameters".to_string(),
        Value::Array(
            route_parameters
                .into_iter()
                .map(razor_route_parameter_value)
                .collect(),
        ),
    );

    Some(fact_for_node(
        file_path,
        "razor",
        RAZOR_PAGE_DIRECTIVE_PATTERN_ID,
        "page_directive",
        node,
        metadata,
    ))
}

#[derive(Debug, Clone)]
struct RazorRouteParameter {
    name: String,
    constraint: Option<String>,
    optional: bool,
    catch_all: bool,
}

fn parse_razor_route_parameters(route: &str) -> Vec<RazorRouteParameter> {
    let mut parameters = Vec::new();
    let mut search_start = 0;
    while let Some(open_relative) = route[search_start..].find('{') {
        let open = search_start + open_relative;
        if route.as_bytes().get(open + 1) == Some(&b'{') {
            search_start = open + 2;
            continue;
        }
        let Some(close_relative) = route[open + 1..].find('}') else {
            break;
        };
        let close = open + 1 + close_relative;
        if let Some(parameter) = parse_razor_route_parameter_inner(&route[open + 1..close]) {
            parameters.push(parameter);
        }
        search_start = close + 1;
    }
    parameters
}

fn parse_razor_route_parameter_inner(inner: &str) -> Option<RazorRouteParameter> {
    let mut remainder = inner.trim();
    let catch_all = remainder.starts_with('*');
    if catch_all {
        remainder = remainder.trim_start_matches('*');
    }
    let optional = remainder.ends_with('?');
    if optional {
        remainder = &remainder[..remainder.len() - 1];
    }
    let (name, constraint) = if let Some(colon) = remainder.find(':') {
        (
            remainder[..colon].trim(),
            Some(remainder[colon + 1..].trim().to_string()),
        )
    } else {
        (remainder.trim(), None)
    };
    if name.is_empty() {
        return None;
    }

    Some(RazorRouteParameter {
        name: name.to_string(),
        constraint: constraint.filter(|value| !value.is_empty()),
        optional,
        catch_all,
    })
}

fn razor_route_parameter_value(parameter: RazorRouteParameter) -> Value {
    let mut fields = serde_json::Map::new();
    fields.insert("name".to_string(), Value::String(parameter.name));
    fields.insert("optional".to_string(), Value::Bool(parameter.optional));
    fields.insert("catch_all".to_string(), Value::Bool(parameter.catch_all));
    if let Some(constraint) = parameter.constraint {
        fields.insert("constraint".to_string(), Value::String(constraint));
    }
    Value::Object(fields)
}

fn razor_code_block_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let block_type = if text.contains("@code") {
        "code"
    } else if text.contains("@functions") {
        "functions"
    } else {
        return None;
    };

    let mut metadata = base_metadata("component_code", "razor");
    insert_string(&mut metadata, "block_type", block_type);

    Some(fact_for_node(
        file_path,
        "razor",
        RAZOR_CODE_BLOCK_PATTERN_ID,
        "code_block",
        node,
        metadata,
    ))
}

fn razor_template_expression_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let expression = node_text(content, node)?
        .trim()
        .trim_start_matches('@')
        .trim()
        .to_string();
    if expression.is_empty() {
        return None;
    }

    let mut metadata = base_metadata("component_template", "razor");
    insert_string(&mut metadata, "expression", &expression);
    metadata.insert(
        "implicit".to_string(),
        Value::Bool(node.kind() == "razor_implicit_expression"),
    );

    Some(fact_for_node(
        file_path,
        "razor",
        RAZOR_TEMPLATE_EXPRESSION_PATTERN_ID,
        "template_expression",
        node,
        metadata,
    ))
}

fn fact_for_node(
    file_path: &str,
    language: &str,
    pattern_id: &str,
    capture_name: &str,
    node: Node<'_>,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    let span = NormalizedSpan::from_node(&node);
    fact_for_span(
        file_path,
        language,
        pattern_id,
        capture_name,
        node.kind(),
        span,
        metadata,
    )
}

fn node_text<'a>(content: &'a str, node: Node<'_>) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}

fn razor_child_text<'a>(node: Node<'_>, content: &'a str, child_kind: &str) -> Option<&'a str> {
    razor_child_text_at_depth(node, content, child_kind, 0)
}

fn razor_child_text_at_depth<'a>(
    node: Node<'_>,
    content: &'a str,
    child_kind: &str,
    depth: u32,
) -> Option<&'a str> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == child_kind {
            return node_text(content, child);
        }
        if let Some(text) = razor_child_text_at_depth(child, content, child_kind, child_depth) {
            return Some(text);
        }
    }
    None
}

fn collect_aspnet_minimal_api_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let route_groups = collect_aspnet_minimal_api_route_groups(language, tree, file_path, content);
    let group_prefixes = route_groups
        .iter()
        .filter_map(|fact| {
            let metadata = fact.metadata.as_ref()?;
            let group_variable = metadata.get("group_variable")?.as_str()?;
            let route_prefix = metadata.get("route_prefix")?.as_str()?;
            Some((group_variable.to_string(), route_prefix.to_string()))
        })
        .collect::<HashMap<_, _>>();
    facts.extend(route_groups);

    for (method_name, verb) in ASPNET_ROUTE_METHODS {
        let mut search_start = 0;
        while let Some(relative_start) = content[search_start..].find(method_name) {
            let method_start = search_start + relative_start;
            search_start = method_start + method_name.len();

            if !is_identifier_boundary(content, method_start, method_name.len()) {
                continue;
            }

            let open_paren = skip_ascii_whitespace(content, search_start);
            if content.as_bytes().get(open_paren) != Some(&b'(') {
                continue;
            }

            let Some(close_paren) = find_matching_paren(content, open_paren) else {
                continue;
            };
            let Some((route_template, route_arg_end, route_source)) =
                parse_first_route_argument(content, open_paren + 1, close_paren)
            else {
                continue;
            };
            let Some(node) =
                smallest_node_covering_range(tree.root_node(), method_start, close_paren + 1)
            else {
                continue;
            };
            if is_comment_or_string_node(node.kind()) {
                continue;
            }
            let Some(span) =
                NormalizedSpan::from_content_range(content, method_start, close_paren + 1)
            else {
                continue;
            };

            let mut metadata = base_metadata("framework", "aspnet");
            insert_string(&mut metadata, "api_style", "minimal_api");
            insert_string(&mut metadata, "verb", verb);
            insert_string(&mut metadata, "route_template", &route_template);
            insert_string(&mut metadata, "route_source", route_source);
            let route_group_prefix = parse_csharp_member_receiver(content, method_start)
                .and_then(|receiver| group_prefixes.get(receiver).cloned())
                .or_else(|| parse_chained_map_group_prefix(content, method_start));
            if let Some(route_group_prefix) = route_group_prefix {
                insert_string(&mut metadata, "route_group_prefix", &route_group_prefix);
                insert_string(
                    &mut metadata,
                    "effective_route_template",
                    &join_route_templates(&route_group_prefix, &route_template),
                );
                insert_string(&mut metadata, "route_group_source", "map_group");
            }

            if let Some(handler) = parse_handler_argument(content, route_arg_end, close_paren) {
                insert_string(&mut metadata, "handler_kind", handler.kind);
                if let Some(name) = handler.name {
                    insert_string(&mut metadata, "handler_name", &name);
                }
            }

            facts.push(fact_for_span(
                file_path,
                language,
                ASPNET_MINIMAL_API_ROUTE_PATTERN_ID,
                "route_call",
                node.kind(),
                span,
                metadata,
            ));
        }
    }

    facts
}

fn collect_aspnet_minimal_api_route_groups(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let method_name = "MapGroup";
    let mut search_start = 0;

    while let Some(relative_start) = content[search_start..].find(method_name) {
        let method_start = search_start + relative_start;
        search_start = method_start + method_name.len();

        if !is_identifier_boundary(content, method_start, method_name.len()) {
            continue;
        }

        let open_paren = skip_ascii_whitespace(content, search_start);
        if content.as_bytes().get(open_paren) != Some(&b'(') {
            continue;
        }

        let Some(close_paren) = find_matching_paren(content, open_paren) else {
            continue;
        };
        let Some((route_prefix, _, route_source)) =
            parse_first_route_argument(content, open_paren + 1, close_paren)
        else {
            continue;
        };
        let Some(node) =
            smallest_node_covering_range(tree.root_node(), method_start, close_paren + 1)
        else {
            continue;
        };
        if is_comment_or_string_node(node.kind()) {
            continue;
        }
        let Some(span) = NormalizedSpan::from_content_range(content, method_start, close_paren + 1)
        else {
            continue;
        };

        let mut metadata = base_metadata("framework", "aspnet");
        insert_string(&mut metadata, "api_style", "minimal_api");
        insert_string(&mut metadata, "route_prefix", &route_prefix);
        insert_string(&mut metadata, "route_source", route_source);
        insert_string(&mut metadata, "source_kind", "map_group");
        if let Some(group_variable) = parse_map_group_assignment_variable(content, method_start) {
            insert_string(&mut metadata, "group_variable", &group_variable);
        }

        facts.push(fact_for_span(
            file_path,
            language,
            ASPNET_MINIMAL_API_ROUTE_GROUP_PATTERN_ID,
            "route_group",
            node.kind(),
            span,
            metadata,
        ));
    }

    facts
}

fn parse_map_group_assignment_variable(content: &str, method_start: usize) -> Option<String> {
    let statement_start = content[..method_start]
        .rfind(['\n', ';', '{'])
        .map(|index| index + 1)
        .unwrap_or(0);
    let before_method = content.get(statement_start..method_start)?;
    let equals = before_method.rfind('=')?;
    let candidate = before_method[..equals].split_whitespace().last()?;
    is_csharp_identifier(candidate).then(|| candidate.to_string())
}

fn parse_csharp_member_receiver(content: &str, method_start: usize) -> Option<&str> {
    let bytes = content.as_bytes();
    let mut dot = method_start;
    while dot > 0 && bytes.get(dot - 1).is_some_and(u8::is_ascii_whitespace) {
        dot -= 1;
    }
    if dot == 0 || bytes.get(dot - 1) != Some(&b'.') {
        return None;
    }

    let mut end = dot - 1;
    while end > 0 && bytes.get(end - 1).is_some_and(u8::is_ascii_whitespace) {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && bytes.get(start - 1).is_some_and(is_csharp_identifier_byte) {
        start -= 1;
    }
    let receiver = content.get(start..end)?;
    is_csharp_identifier(receiver).then_some(receiver)
}

fn parse_chained_map_group_prefix(content: &str, method_start: usize) -> Option<String> {
    let bytes = content.as_bytes();
    let mut dot = method_start;
    while dot > 0 && bytes.get(dot - 1).is_some_and(u8::is_ascii_whitespace) {
        dot -= 1;
    }
    if dot == 0 || bytes.get(dot - 1) != Some(&b'.') {
        return None;
    }

    let mut cursor = dot - 1;
    while cursor > 0 && bytes.get(cursor - 1).is_some_and(u8::is_ascii_whitespace) {
        cursor -= 1;
    }
    if cursor == 0 || bytes.get(cursor - 1) != Some(&b')') {
        return None;
    }
    let close_paren = cursor - 1;
    let open_paren = find_matching_paren_backwards(content, close_paren)?;

    let mut method_end = open_paren;
    while method_end > 0
        && bytes
            .get(method_end - 1)
            .is_some_and(u8::is_ascii_whitespace)
    {
        method_end -= 1;
    }
    let mut method_name_start = method_end;
    while method_name_start > 0
        && bytes
            .get(method_name_start - 1)
            .is_some_and(is_csharp_identifier_byte)
    {
        method_name_start -= 1;
    }
    if content.get(method_name_start..method_end) != Some("MapGroup") {
        return None;
    }

    parse_first_route_argument(content, open_paren + 1, close_paren)
        .map(|(route_prefix, _, _)| route_prefix)
}

fn is_csharp_identifier_byte(byte: &u8) -> bool {
    matches!(byte, b'_' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9')
}

fn join_route_templates(prefix: &str, route_template: &str) -> String {
    match (prefix.ends_with('/'), route_template.starts_with('/')) {
        (true, true) => format!("{}{}", prefix.trim_end_matches('/'), route_template),
        (false, false) => format!("{prefix}/{route_template}"),
        _ => format!("{prefix}{route_template}"),
    }
}

/// Collect `aspnet.attribute_route.v1` facts for attribute-routed controllers.
///
/// Uses tree-sitter attribution (attribute node -> owning class/method
/// declaration) rather than raw text association. Conventional (non-attribute)
/// routing is intentionally out of scope. Attributes whose route argument is not
/// a plain string literal (interpolation, concatenation, `nameof`, constants)
/// stay silent.
fn collect_aspnet_attribute_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    collect_attribute_route_classes(
        tree.root_node(),
        language,
        file_path,
        content,
        &mut facts,
        0,
    );
    facts
}

fn collect_attribute_route_classes(
    node: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "class_declaration" {
            collect_attribute_routes_for_class(child, language, file_path, content, facts);
        }
        // Recurse so nested type declarations (and top-level namespaces) are
        // visited; each class computes its own controller context.
        collect_attribute_route_classes(child, language, file_path, content, facts, child_depth);
    }
}

fn collect_attribute_routes_for_class(
    class_node: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let class_name = class_node
        .child_by_field_name("name")
        .and_then(|name| node_text(content, name));
    let controller_token = class_name.map(controller_token_from_class_name);

    // Class-level [Route("...")] attributes -> controller_route facts. The first
    // literal template becomes the controller template shared with methods.
    let mut controller_template: Option<String> = None;
    for attribute in class_attribute_nodes(class_node) {
        let Some(name) = attribute_route_name(content, attribute) else {
            continue;
        };
        if !is_route_attribute(&name) {
            continue;
        }
        match attribute_route_argument(content, attribute) {
            AttributeRouteArgument::NonLiteral => continue,
            AttributeRouteArgument::Absent => continue,
            AttributeRouteArgument::Literal(template) => {
                if controller_template.is_none() {
                    controller_template = Some(template.clone());
                }
                let (effective, tokens) =
                    substitute_route_tokens(&template, controller_token.as_deref(), None);
                let mut metadata = base_metadata("framework", "aspnet");
                insert_string(&mut metadata, "api_style", "attribute_routing");
                insert_string(&mut metadata, "attribute_kind", "controller_route");
                insert_string(&mut metadata, "route_template", &template);
                insert_string(&mut metadata, "effective_route_template", &effective);
                insert_route_tokens(&mut metadata, tokens);
                facts.push(fact_for_node(
                    file_path,
                    language,
                    ASPNET_ATTRIBUTE_ROUTE_PATTERN_ID,
                    "attribute_route",
                    attribute,
                    metadata,
                ));
            }
        }
    }

    // Method-level attributes on this class's direct action methods.
    let Some(body) = class_node.child_by_field_name("body") else {
        return;
    };
    let mut body_cursor = body.walk();
    for member in body.children(&mut body_cursor) {
        if member.kind() != "method_declaration" {
            continue;
        }
        collect_attribute_routes_for_method(
            member,
            language,
            file_path,
            content,
            controller_token.as_deref(),
            controller_template.as_deref(),
            facts,
        );
    }
}

fn collect_attribute_routes_for_method(
    method_node: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    controller_token: Option<&str>,
    controller_template: Option<&str>,
    facts: &mut Vec<StructuralFact>,
) {
    let action_name = method_node
        .child_by_field_name("name")
        .and_then(|name| node_text(content, name));

    let attributes = method_attribute_nodes(method_node);
    let has_http_verb = attributes.iter().any(|attribute| {
        attribute_route_name(content, *attribute)
            .and_then(|name| attribute_route_verb(&name).map(|_| ()))
            .is_some()
    });

    for attribute in attributes {
        let Some(name) = attribute_route_name(content, attribute) else {
            continue;
        };
        let verb = attribute_route_verb(&name);
        let is_route = is_route_attribute(&name);
        if verb.is_none() && !is_route {
            continue;
        }
        // A method-level [Route] only emits its own `route` fact when the method
        // has no Http* verb attribute; otherwise the verb attribute carries the
        // template.
        if verb.is_none() && has_http_verb {
            continue;
        }

        let method_template = match attribute_route_argument(content, attribute) {
            AttributeRouteArgument::NonLiteral => continue,
            AttributeRouteArgument::Absent => None,
            AttributeRouteArgument::Literal(template) => Some(template),
        };

        let mut metadata = base_metadata("framework", "aspnet");
        insert_string(&mut metadata, "api_style", "attribute_routing");
        if let Some(verb) = verb {
            insert_string(&mut metadata, "attribute_kind", "http_method");
            insert_string(&mut metadata, "verb", verb);
        } else {
            insert_string(&mut metadata, "attribute_kind", "route");
        }
        if let Some(method_template) = method_template.as_deref() {
            insert_string(&mut metadata, "route_template", method_template);
        }
        if let Some(controller_template) = controller_template {
            insert_string(
                &mut metadata,
                "controller_route_template",
                controller_template,
            );
        }

        if let Some(raw) = join_effective_route(controller_template, method_template.as_deref()) {
            let (effective, tokens) = substitute_route_tokens(&raw, controller_token, action_name);
            insert_string(&mut metadata, "effective_route_template", &effective);
            insert_route_tokens(&mut metadata, tokens);
        }

        facts.push(fact_for_node(
            file_path,
            language,
            ASPNET_ATTRIBUTE_ROUTE_PATTERN_ID,
            "attribute_route",
            attribute,
            metadata,
        ));
    }
}

fn class_attribute_nodes(class_node: Node<'_>) -> Vec<Node<'_>> {
    // Class-level attribute lists are direct children preceding the `class`
    // keyword; a nested class's own attributes belong to that inner declaration.
    attribute_nodes_from_lists(class_node)
}

fn method_attribute_nodes(method_node: Node<'_>) -> Vec<Node<'_>> {
    attribute_nodes_from_lists(method_node)
}

fn attribute_nodes_from_lists(declaration: Node<'_>) -> Vec<Node<'_>> {
    let mut attributes = Vec::new();
    let mut cursor = declaration.walk();
    for child in declaration.children(&mut cursor) {
        if child.kind() != "attribute_list" {
            continue;
        }
        let mut list_cursor = child.walk();
        for attribute in child.children(&mut list_cursor) {
            if attribute.kind() == "attribute" {
                attributes.push(attribute);
            }
        }
    }
    attributes
}

/// Normalize an attribute's name: take the last `.`-separated segment and strip
/// a trailing `Attribute` suffix (`Microsoft.AspNetCore.Mvc.HttpGetAttribute`
/// -> `HttpGet`).
fn attribute_route_name(content: &str, attribute: Node<'_>) -> Option<String> {
    let name_node = attribute.child_by_field_name("name")?;
    let raw = node_text(content, name_node)?;
    let last = raw.rsplit('.').next().unwrap_or(raw).trim();
    let normalized = last.strip_suffix("Attribute").unwrap_or(last);
    if normalized.is_empty() {
        return None;
    }
    Some(normalized.to_string())
}

fn is_route_attribute(name: &str) -> bool {
    name == "Route"
}

fn attribute_route_verb(name: &str) -> Option<&'static str> {
    match name {
        "HttpGet" => Some("GET"),
        "HttpPost" => Some("POST"),
        "HttpPut" => Some("PUT"),
        "HttpPatch" => Some("PATCH"),
        "HttpDelete" => Some("DELETE"),
        "HttpHead" => Some("HEAD"),
        "HttpOptions" => Some("OPTIONS"),
        _ => None,
    }
}

enum AttributeRouteArgument {
    /// No argument list, or an empty one (`[HttpGet]`, `[HttpGet()]`).
    Absent,
    /// A first argument that is not a plain string literal -> stay silent.
    NonLiteral,
    /// A first-positional string-literal template.
    Literal(String),
}

fn attribute_route_argument(content: &str, attribute: Node<'_>) -> AttributeRouteArgument {
    let mut cursor = attribute.walk();
    let argument_list = attribute
        .children(&mut cursor)
        .find(|child| child.kind() == "attribute_argument_list");
    let Some(argument_list) = argument_list else {
        return AttributeRouteArgument::Absent;
    };

    let mut list_cursor = argument_list.walk();
    let first_argument = argument_list
        .children(&mut list_cursor)
        .find(|child| child.kind() == "attribute_argument");
    let Some(first_argument) = first_argument else {
        return AttributeRouteArgument::Absent;
    };

    match parse_csharp_string_literal(content, first_argument.start_byte()) {
        Some((value, _, _)) => AttributeRouteArgument::Literal(value),
        None => AttributeRouteArgument::NonLiteral,
    }
}

/// The controller substitution value: class name minus a trailing `Controller`.
fn controller_token_from_class_name(class_name: &str) -> String {
    match class_name.strip_suffix("Controller") {
        Some(stripped) if !stripped.is_empty() => stripped.to_string(),
        _ => class_name.to_string(),
    }
}

/// Join a controller template with a method template into the raw effective
/// template (before token substitution). Returns `None` when neither template
/// contributes a path segment.
fn join_effective_route(
    controller_template: Option<&str>,
    method_template: Option<&str>,
) -> Option<String> {
    match (controller_template, method_template) {
        (Some(controller), Some(method)) => Some(join_route_templates(controller, method)),
        (Some(controller), None) => Some(controller.to_string()),
        (None, Some(method)) => Some(method.to_string()),
        (None, None) => None,
    }
}

/// Substitute `[controller]`/`[action]` tokens using the lowercased identifiers
/// and normalize a single leading `/`, producing a stable join key. Returns the
/// substituted template and the list of tokens actually replaced.
fn substitute_route_tokens(
    raw: &str,
    controller_token: Option<&str>,
    action_token: Option<&str>,
) -> (String, Vec<&'static str>) {
    let mut output = raw.to_string();
    let mut tokens = Vec::new();
    if let Some(controller) = controller_token
        && output.contains("[controller]")
    {
        output = output.replace("[controller]", &controller.to_ascii_lowercase());
        tokens.push("controller");
    }
    if let Some(action) = action_token
        && output.contains("[action]")
    {
        output = output.replace("[action]", &action.to_ascii_lowercase());
        tokens.push("action");
    }
    (format!("/{}", output.trim_start_matches('/')), tokens)
}

fn insert_route_tokens(metadata: &mut HashMap<String, Value>, tokens: Vec<&'static str>) {
    metadata.insert(
        "route_tokens".to_string(),
        Value::Array(
            tokens
                .into_iter()
                .map(|token| Value::String(token.to_string()))
                .collect(),
        ),
    );
}

fn collect_markup_framework_attributes(
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

        if is_alpine_attribute_name(&attribute.name)
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
    let span =
        NormalizedSpan::from_content_range(content, attribute.start_byte, attribute.end_byte)?;
    let mut metadata = base_metadata("frontend_interaction", "htmx");

    insert_string(&mut metadata, "attribute_name", attribute_name);
    if data_prefix {
        metadata.insert("data_prefix".to_string(), Value::Bool(true));
    }
    if let Some(value) = attribute.value.as_deref() {
        insert_string(&mut metadata, "attribute_value", value);
    }
    if let Some(verb) = htmx_request_verb(attribute_name) {
        insert_string(&mut metadata, "verb", verb);
        if let Some(target_path) = attribute
            .value
            .as_deref()
            .filter(|value| is_static_path(value))
        {
            insert_string(&mut metadata, "target_path", target_path);
        }
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

fn canonical_htmx_attribute_name(attribute_name: &str) -> Option<(String, bool)> {
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
fn collect_jsx_htmx_attributes(
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
fn collect_vue_template_htmx_attributes(
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

/// Emit an htmx fact for component markup (JSX/TSX and Vue templates), mirroring
/// the html/razor fact shape. Unlike the html path, only STATIC STRING values
/// emit: JSX brace expressions (`hx-post={url}`) parse to a `{...}` value and
/// stay silent, and Vue dynamic bindings (`:hx-post`, `v-bind:hx-post`) never
/// match the `hx-*`/`data-hx-*` name shape. Alpine directives are intentionally
/// not scanned on this surface.
fn component_htmx_attribute_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    attribute: &MarkupAttribute,
) -> Option<StructuralFact> {
    let (attribute_name, data_prefix) = canonical_htmx_attribute_name(&attribute.name)?;
    // Only static string values emit; a brace-expression value (parsed as a
    // leading `{`) or a valueless attribute stays silent so dynamic component
    // bindings are never misread as static request paths.
    let value = attribute.value.as_deref()?;
    if value.starts_with('{') {
        return None;
    }
    htmx_attribute_fact(
        language,
        tree,
        file_path,
        content,
        attribute,
        &attribute_name,
        data_prefix,
    )
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

fn fact_for_span(
    file_path: &str,
    language: &str,
    pattern_id: &str,
    capture_name: &str,
    node_kind: &str,
    span: NormalizedSpan,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    StructuralFact {
        id: stable_location_id(file_path, &format!("{pattern_id}:{capture_name}"), span),
        file_path: file_path.to_string(),
        language: language.to_string(),
        pattern_id: pattern_id.to_string(),
        capture_name: capture_name.to_string(),
        node_kind: node_kind.to_string(),
        containing_symbol_id: None,
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        confidence: 1.0,
        metadata: Some(metadata),
    }
}

fn base_metadata(query_family: &str, framework: &str) -> HashMap<String, Value> {
    HashMap::from([
        (
            "pattern_version".to_string(),
            Value::Number(Number::from(1)),
        ),
        (
            "query_family".to_string(),
            Value::String(query_family.to_string()),
        ),
        (
            "framework".to_string(),
            Value::String(framework.to_string()),
        ),
    ])
}

fn insert_string(metadata: &mut HashMap<String, Value>, key: &str, value: &str) {
    metadata.insert(key.to_string(), Value::String(value.to_string()));
}

fn parse_first_route_argument(
    content: &str,
    args_start: usize,
    args_end: usize,
) -> Option<(String, usize, &'static str)> {
    let route_start = skip_ascii_whitespace_until(content, args_start, args_end);
    if route_start >= args_end {
        return None;
    }
    parse_csharp_string_literal(content, route_start)
        .filter(|(_, route_end, _)| *route_end <= args_end)
}

fn parse_csharp_string_literal(
    content: &str,
    start: usize,
) -> Option<(String, usize, &'static str)> {
    let bytes = content.as_bytes();
    if bytes.get(start) == Some(&b'$')
        || (bytes.get(start) == Some(&b'@') && bytes.get(start + 1) == Some(&b'$'))
    {
        return None;
    }

    if bytes.get(start) == Some(&b'@') && bytes.get(start + 1) == Some(&b'"') {
        return parse_verbatim_csharp_string(content, start + 2)
            .map(|(value, end)| (value, end, "string_literal"));
    }

    if bytes.get(start) == Some(&b'"') {
        return parse_normal_csharp_string(content, start + 1)
            .map(|(value, end)| (value, end, "string_literal"));
    }

    None
}

fn parse_normal_csharp_string(content: &str, mut cursor: usize) -> Option<(String, usize)> {
    let mut value = String::new();
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        match byte {
            b'\\' => {
                let escaped_start = cursor + 1;
                let escaped = content.get(escaped_start..)?.chars().next()?;
                value.push(match escaped {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                });
                cursor = escaped_start + escaped.len_utf8();
            }
            b'"' => return Some((value, cursor + 1)),
            _ => {
                let ch = content.get(cursor..)?.chars().next()?;
                value.push(ch);
                cursor += ch.len_utf8();
            }
        }
    }
    None
}

fn parse_verbatim_csharp_string(content: &str, mut cursor: usize) -> Option<(String, usize)> {
    let mut value = String::new();
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
        if byte == b'"' {
            if content.as_bytes().get(cursor + 1) == Some(&b'"') {
                value.push('"');
                cursor += 2;
            } else {
                return Some((value, cursor + 1));
            }
        } else {
            let ch = content.get(cursor..)?.chars().next()?;
            value.push(ch);
            cursor += ch.len_utf8();
        }
    }
    None
}

#[derive(Debug)]
struct HandlerMetadata {
    kind: &'static str,
    name: Option<String>,
}

fn parse_handler_argument(
    content: &str,
    route_arg_end: usize,
    args_end: usize,
) -> Option<HandlerMetadata> {
    let comma = skip_ascii_whitespace_until(content, route_arg_end, args_end);
    if content.as_bytes().get(comma) != Some(&b',') {
        return None;
    }
    let handler_start = skip_ascii_whitespace_until(content, comma + 1, args_end);
    if handler_start >= args_end {
        return None;
    }
    let handler_end = find_top_level_comma_or_end(content, handler_start, args_end);
    let expression = content.get(handler_start..handler_end)?.trim();

    if expression.contains("=>") {
        return Some(HandlerMetadata {
            kind: "lambda",
            name: None,
        });
    }

    parse_identifier_path(expression).map(|name| HandlerMetadata {
        kind: "method_group",
        name: Some(name),
    })
}

fn parse_identifier_path(expression: &str) -> Option<String> {
    let mut segments = expression.split('.');
    let first = segments.next()?;
    if !is_csharp_identifier(first) {
        return None;
    }
    for segment in segments {
        if !is_csharp_identifier(segment) {
            return None;
        }
    }
    Some(expression.to_string())
}

fn is_csharp_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
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

fn is_alpine_attribute_name(attribute_name: &str) -> bool {
    attribute_name.starts_with("x-")
        || attribute_name.starts_with('@')
        || attribute_name.starts_with(':')
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

fn find_matching_paren(content: &str, open_paren: usize) -> Option<usize> {
    find_matching_delimiter(content, open_paren, b'(', b')')
}

fn find_matching_paren_backwards(content: &str, close_paren: usize) -> Option<usize> {
    if content.as_bytes().get(close_paren) != Some(&b')') {
        return None;
    }
    let mut candidates = Vec::new();
    let mut cursor = 0;
    while cursor <= close_paren {
        if content.as_bytes().get(cursor) == Some(&b'(')
            && let Some(candidate_close) = find_matching_paren(content, cursor)
            && candidate_close == close_paren
        {
            candidates.push(cursor);
        }
        cursor += 1;
    }
    candidates.pop()
}

fn find_matching_delimiter(content: &str, open_byte: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = content.as_bytes();
    let mut cursor = open_byte;
    let mut depth = 0usize;
    let mut normal_string = false;
    let mut verbatim_string = false;
    let mut line_comment = false;
    let mut block_comment = false;

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        let next = bytes.get(cursor + 1).copied();

        if line_comment {
            if byte == b'\n' {
                line_comment = false;
            }
            cursor += 1;
            continue;
        }
        if block_comment {
            if byte == b'*' && next == Some(b'/') {
                block_comment = false;
                cursor += 2;
            } else {
                cursor += 1;
            }
            continue;
        }
        if normal_string {
            if byte == b'\\' {
                cursor += 2;
            } else {
                normal_string = byte != b'"';
                cursor += 1;
            }
            continue;
        }
        if verbatim_string {
            if byte == b'"' && next == Some(b'"') {
                cursor += 2;
            } else {
                verbatim_string = byte != b'"';
                cursor += 1;
            }
            continue;
        }

        if byte == b'/' && next == Some(b'/') {
            line_comment = true;
            cursor += 2;
            continue;
        }
        if byte == b'/' && next == Some(b'*') {
            block_comment = true;
            cursor += 2;
            continue;
        }
        if byte == b'@' && next == Some(b'"') {
            verbatim_string = true;
            cursor += 2;
            continue;
        }
        if byte == b'$' && next == Some(b'@') && bytes.get(cursor + 2) == Some(&b'"') {
            verbatim_string = true;
            cursor += 3;
            continue;
        }
        if byte == b'"' {
            normal_string = true;
            cursor += 1;
            continue;
        }

        if byte == open {
            depth += 1;
        } else if byte == close {
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
    let mut normal_string = false;

    while cursor < end {
        let byte = content.as_bytes()[cursor];
        if normal_string {
            if byte == b'\\' {
                cursor += 2;
            } else {
                normal_string = byte != b'"';
                cursor += 1;
            }
            continue;
        }
        match byte {
            b'"' => normal_string = true,
            b'(' => paren_depth += 1,
            b')' => paren_depth = paren_depth.saturating_sub(1),
            b'{' => brace_depth += 1,
            b'}' => brace_depth = brace_depth.saturating_sub(1),
            b'[' => bracket_depth += 1,
            b']' => bracket_depth = bracket_depth.saturating_sub(1),
            b',' if paren_depth == 0 && brace_depth == 0 && bracket_depth == 0 => return cursor,
            _ => {}
        }
        cursor += 1;
    }

    end
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
        if let Some(descendant) =
            smallest_node_covering_range_at_depth(child, start_byte, end_byte, child_depth)
        {
            return Some(descendant);
        }
    }

    Some(node)
}

fn is_comment_or_string_node(node_kind: &str) -> bool {
    node_kind.contains("comment") || node_kind.contains("string")
}

fn is_ignored_markup_node(mut node: Node<'_>) -> bool {
    loop {
        let kind = node.kind();
        if is_comment_or_string_node(kind)
            || matches!(
                kind,
                "raw_text" | "text" | "script_element" | "style_element"
            )
        {
            return true;
        }
        let Some(parent) = node.parent() else {
            return false;
        };
        node = parent;
    }
}

fn is_identifier_boundary(content: &str, start: usize, len: usize) -> bool {
    let bytes = content.as_bytes();
    let before = start.checked_sub(1).and_then(|index| bytes.get(index));
    let after = bytes.get(start + len);
    !before.is_some_and(|byte| is_identifier_byte(*byte))
        && !after.is_some_and(|byte| is_identifier_byte(*byte))
}

fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

fn skip_ascii_whitespace(content: &str, mut cursor: usize) -> usize {
    while content
        .as_bytes()
        .get(cursor)
        .is_some_and(u8::is_ascii_whitespace)
    {
        cursor += 1;
    }
    cursor
}

fn skip_ascii_whitespace_until(content: &str, mut cursor: usize, end: usize) -> usize {
    while cursor < end
        && content
            .as_bytes()
            .get(cursor)
            .is_some_and(u8::is_ascii_whitespace)
    {
        cursor += 1;
    }
    cursor
}

fn attach_containing_symbols(facts: &mut [StructuralFact], symbols: &[Symbol]) {
    for fact in facts {
        fact.containing_symbol_id = symbols
            .iter()
            .filter(|symbol| {
                symbol.start_byte <= fact.start_byte && symbol.end_byte >= fact.end_byte
            })
            .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
            .map(|symbol| symbol.id.clone());
    }
}
