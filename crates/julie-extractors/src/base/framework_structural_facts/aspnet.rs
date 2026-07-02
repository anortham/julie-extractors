use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::helpers::{
    base_metadata, fact_for_node, fact_for_span, find_matching_paren, find_matching_paren_backwards,
    insert_string, is_comment_or_string_node, is_csharp_identifier, is_identifier_boundary,
    node_text, parse_csharp_string_literal, parse_first_route_argument, parse_handler_argument,
    skip_ascii_whitespace, skip_ascii_whitespace_until, smallest_node_covering_range,
};
use super::{
    ASPNET_ATTRIBUTE_ROUTE_PATTERN_ID, ASPNET_MINIMAL_API_ROUTE_GROUP_PATTERN_ID,
    ASPNET_MINIMAL_API_ROUTE_PATTERN_ID,
};
use crate::base::http_boundary::{normalize_route_template, ParamFlavor};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const ASPNET_ROUTE_METHODS: &[(&str, &str)] = &[
    ("MapGet", "GET"),
    ("MapPost", "POST"),
    ("MapPut", "PUT"),
    ("MapPatch", "PATCH"),
    ("MapDelete", "DELETE"),
];

pub(super) fn collect_aspnet_minimal_api_routes(
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
            let mut normalized_source = route_template.clone();
            let route_group_prefix = parse_csharp_member_receiver(content, method_start)
                .and_then(|receiver| group_prefixes.get(receiver).cloned())
                .or_else(|| parse_chained_map_group_prefix(content, method_start));
            if let Some(route_group_prefix) = route_group_prefix {
                let effective = join_route_templates(&route_group_prefix, &route_template);
                insert_string(&mut metadata, "route_group_prefix", &route_group_prefix);
                insert_string(&mut metadata, "effective_route_template", &effective);
                insert_string(&mut metadata, "route_group_source", "map_group");
                normalized_source = effective;
            }
            insert_normalized_route_template(&mut metadata, &normalized_source);

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
        insert_normalized_route_template(&mut metadata, &route_prefix);
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
pub(super) fn collect_aspnet_attribute_routes(
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
                insert_normalized_route_template(&mut metadata, &effective);
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
            insert_normalized_route_template(&mut metadata, &effective);
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
    for argument in argument_list.children(&mut list_cursor) {
        if argument.kind() != "attribute_argument" {
            continue;
        }
        if is_named_attribute_argument(content, argument) {
            continue;
        }
        return match parse_csharp_string_literal(content, argument.start_byte()) {
            Some((value, _, _)) => AttributeRouteArgument::Literal(value),
            None => AttributeRouteArgument::NonLiteral,
        };
    }

    AttributeRouteArgument::Absent
}

fn is_named_attribute_argument(content: &str, argument: Node<'_>) -> bool {
    let Some(raw) = node_text(content, argument) else {
        return false;
    };
    let trimmed = raw.trim_start();
    let bytes = trimmed.as_bytes();
    if bytes
        .first()
        .is_none_or(|byte| !matches!(byte, b'_' | b'a'..=b'z' | b'A'..=b'Z'))
    {
        return false;
    }
    let mut cursor = 1;
    while cursor < bytes.len() && is_csharp_identifier_byte(&bytes[cursor]) {
        cursor += 1;
    }
    cursor = skip_ascii_whitespace_until(trimmed, cursor, trimmed.len());
    bytes.get(cursor) == Some(&b'=')
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
        (Some(_), Some(method)) if is_absolute_route_template(method) => Some(method.to_string()),
        (Some(controller), Some(method)) => Some(join_route_templates(controller, method)),
        (Some(controller), None) => Some(controller.to_string()),
        (None, Some(method)) => Some(method.to_string()),
        (None, None) => None,
    }
}

fn is_absolute_route_template(template: &str) -> bool {
    template.starts_with('/') || template.starts_with("~/")
}

fn insert_normalized_route_template(metadata: &mut HashMap<String, Value>, template: &str) {
    let normalized = normalize_route_template(template, ParamFlavor::Braces);
    insert_string(metadata, "normalized_route_template", &normalized.template);
}

fn slash_normalized_route_template(template: &str) -> String {
    let normalized = template
        .strip_prefix("~/")
        .unwrap_or_else(|| template.trim_start_matches('/'));
    format!("/{normalized}")
}

/// Substitute `[controller]`/`[action]` tokens using the lowercased identifiers
/// and normalize a single leading `/`. Returns the substituted template and the
/// list of tokens actually replaced.
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
    (slash_normalized_route_template(&output), tokens)
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
