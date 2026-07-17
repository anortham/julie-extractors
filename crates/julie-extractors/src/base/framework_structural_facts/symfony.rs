//! Symfony `#[Route]` attribute route facts (`symfony.route.v1`).
//!
//! PHP attribute routing is a Symfony idiom, not Laravel. This collector is
//! import-gated on `Symfony\Component\Routing\` (Attribute or Annotation) and
//! emits only for static-literal path arguments (M2 silence via
//! [`static_route_arg`]). Class-level `#[Route]` prefixes join into method
//! `effective_route_template` using the Spring/NestJS class+method model.

use tree_sitter::{Node, Tree};

use super::SYMFONY_ROUTE_PATTERN_ID;
use super::helpers::{
    base_metadata, child_of_kind, fact_for_span, insert_string, insert_string_array,
    is_comment_or_string_node, smallest_node_covering_range,
};
use super::static_arg::{StaticArgLang, static_route_arg};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

const IMPORT_NAMESPACE: &str = "Symfony\\Component\\Routing";

pub(super) fn collect_symfony_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !has_symfony_routing_import(tree.root_node(), content) {
        return Vec::new();
    }

    let mut class_nodes = Vec::new();
    collect_nodes_of_kind(tree.root_node(), "class_declaration", &mut class_nodes);

    let mut facts = Vec::new();
    for class_node in class_nodes {
        let class_prefixes = class_route_prefixes(class_node, content);
        for prefix in &class_prefixes {
            if let Some(fact) = route_fact(
                language,
                tree,
                file_path,
                content,
                class_node,
                "class_route",
                prefix,
                prefix,
                None,
                None,
                None,
            ) {
                facts.push(fact);
            }
        }

        let Some(body) = child_of_kind(class_node, "declaration_list") else {
            continue;
        };
        let mut cursor = body.walk();
        for child in body.named_children(&mut cursor) {
            if child.kind() != "method_declaration" {
                continue;
            }
            emit_method_routes(
                language,
                tree,
                file_path,
                content,
                child,
                &class_prefixes,
                &mut facts,
            );
        }
    }
    facts
}

fn has_symfony_routing_import(node: Node, content: &str) -> bool {
    if node.kind() == "namespace_use_declaration" {
        return contains_symfony_routing_import_target(node, content);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if has_symfony_routing_import(child, content) {
            return true;
        }
    }
    false
}

fn contains_symfony_routing_import_target(node: Node, content: &str) -> bool {
    if matches!(node.kind(), "qualified_name" | "namespace_name")
        && let Some(import_target) = content.get(node.start_byte()..node.end_byte())
    {
        let import_target = import_target.trim_start_matches('\\');
        if import_target == IMPORT_NAMESPACE
            || import_target.starts_with(&format!("{IMPORT_NAMESPACE}\\"))
        {
            return true;
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if contains_symfony_routing_import_target(child, content) {
            return true;
        }
    }
    false
}

fn collect_nodes_of_kind<'tree>(node: Node<'tree>, kind: &str, out: &mut Vec<Node<'tree>>) {
    if node.kind() == kind {
        out.push(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_nodes_of_kind(child, kind, out);
    }
}

fn class_route_prefixes(class_node: Node, content: &str) -> Vec<String> {
    let mut prefixes = Vec::new();
    for attribute in route_attributes_on(class_node, content) {
        let parsed = parse_route_attribute(attribute, content);
        // Class-level Route with a non-static path is poisoned → no prefix.
        if parsed.had_path_argument && parsed.path.is_none() {
            continue;
        }
        if let Some(path) = parsed.path {
            prefixes.push(path);
        }
    }
    prefixes
}

fn emit_method_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    method: Node,
    class_prefixes: &[String],
    facts: &mut Vec<StructuralFact>,
) {
    for attribute in route_attributes_on(method, content) {
        let parsed = parse_route_attribute(attribute, content);
        // Static-literal silence: a present but non-static path emits nothing.
        let template = match &parsed.path {
            Some(path) => path.as_str(),
            None if parsed.had_path_argument => continue,
            // Bare `#[Route]` / `#[Route(methods: ...)]` → empty sub-path.
            None => "",
        };
        emit_for_template(
            language,
            tree,
            file_path,
            content,
            method,
            template,
            class_prefixes,
            &parsed.verbs,
            facts,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_for_template(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    handler: Node,
    template: &str,
    class_prefixes: &[String],
    verbs: &[String],
    facts: &mut Vec<StructuralFact>,
) {
    let attribute_kind = if verbs.is_empty() {
        "request_mapping"
    } else {
        "http_method"
    };
    let verb_opts: Vec<Option<&str>> = if verbs.is_empty() {
        vec![None]
    } else {
        verbs.iter().map(|v| Some(v.as_str())).collect()
    };
    let class_templates: Vec<Option<&str>> = if class_prefixes.is_empty() {
        vec![None]
    } else {
        class_prefixes.iter().map(|p| Some(p.as_str())).collect()
    };

    for class_template in class_templates {
        let effective = class_template.map(|prefix| {
            if template.is_empty() {
                prefix.to_string()
            } else {
                join_route_templates(prefix, template)
            }
        });
        let normalized_source = effective.as_deref().unwrap_or(template);
        for verb in &verb_opts {
            if let Some(fact) = route_fact(
                language,
                tree,
                file_path,
                content,
                handler,
                attribute_kind,
                template,
                normalized_source,
                class_template,
                effective.as_deref(),
                *verb,
            ) {
                facts.push(fact);
            }
        }
    }
}

struct ParsedRoute {
    path: Option<String>,
    had_path_argument: bool,
    verbs: Vec<String>,
}

fn parse_route_attribute(attribute: Node, content: &str) -> ParsedRoute {
    let mut parsed = ParsedRoute {
        path: None,
        had_path_argument: false,
        verbs: Vec::new(),
    };
    let Some(arguments) = child_of_kind(attribute, "arguments") else {
        return parsed;
    };
    let mut cursor = arguments.walk();
    let mut positional_index = 0usize;
    for argument in arguments.named_children(&mut cursor) {
        if argument.kind() != "argument" {
            continue;
        }
        let (name, value) = split_argument(argument, content);
        match name.as_deref() {
            Some("path") => {
                parsed.had_path_argument = true;
                if let Some(value) = value
                    && let Some(text) = static_route_arg(value, content, StaticArgLang::Php)
                {
                    parsed.path = Some(text.to_string());
                }
            }
            Some("methods") => {
                if let Some(value) = value {
                    collect_methods(value, content, &mut parsed.verbs);
                }
            }
            Some(_) => {
                // name, requirements, defaults, … — ignored
            }
            None => {
                // Positional: first is path.
                if positional_index == 0 {
                    parsed.had_path_argument = true;
                    if let Some(value) = value
                        && let Some(text) = static_route_arg(value, content, StaticArgLang::Php)
                    {
                        parsed.path = Some(text.to_string());
                    }
                }
                positional_index += 1;
            }
        }
    }
    parsed
}

fn split_argument<'t>(argument: Node<'t>, content: &str) -> (Option<String>, Option<Node<'t>>) {
    // Named: `name` `:` value. Positional: bare value child.
    let mut cursor = argument.walk();
    let children: Vec<Node> = argument.named_children(&mut cursor).collect();
    if children.len() >= 2 {
        let name_node = children[0];
        if name_node.kind() == "name" {
            let name = content
                .get(name_node.start_byte()..name_node.end_byte())
                .map(|s| s.to_string());
            return (name, Some(children[1]));
        }
    }
    (None, children.into_iter().next())
}

fn collect_methods(value: Node, content: &str, verbs: &mut Vec<String>) {
    match value.kind() {
        "array_creation_expression" => {
            let mut cursor = value.walk();
            for element in value.named_children(&mut cursor) {
                if element.kind() != "array_element_initializer" {
                    continue;
                }
                let mut el_cursor = element.walk();
                for child in element.named_children(&mut el_cursor) {
                    if let Some(text) = static_route_arg(child, content, StaticArgLang::Php) {
                        verbs.push(text.to_ascii_uppercase());
                    }
                }
            }
        }
        _ => {
            if let Some(text) = static_route_arg(value, content, StaticArgLang::Php) {
                verbs.push(text.to_ascii_uppercase());
            }
        }
    }
}

fn route_attributes_on<'tree>(node: Node<'tree>, content: &str) -> Vec<Node<'tree>> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "attribute_list" {
            continue;
        }
        let mut list_cursor = child.walk();
        for group in child.children(&mut list_cursor) {
            if group.kind() != "attribute_group" {
                continue;
            }
            let mut group_cursor = group.walk();
            for attribute in group.named_children(&mut group_cursor) {
                if attribute.kind() == "attribute" && attribute_is_route(attribute, content) {
                    out.push(attribute);
                }
            }
        }
    }
    out
}

fn attribute_is_route(attribute: Node, content: &str) -> bool {
    let Some(name_node) = attribute_name_node(attribute) else {
        return false;
    };
    let Some(text) = content.get(name_node.start_byte()..name_node.end_byte()) else {
        return false;
    };
    // Bare `Route` or a qualified name ending in `\Route`.
    text == "Route" || text.ends_with("\\Route")
}

fn attribute_name_node(attribute: Node) -> Option<Node> {
    // `attribute` → `name` or `qualified_name` (last `name` child).
    let mut cursor = attribute.walk();
    for child in attribute.named_children(&mut cursor) {
        match child.kind() {
            "name" => return Some(child),
            "qualified_name" => {
                let mut q = child.walk();
                return child
                    .named_children(&mut q)
                    .filter(|n| n.kind() == "name")
                    .last();
            }
            _ => {}
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn route_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    handler: Node,
    attribute_kind: &str,
    route_template: &str,
    normalized_source: &str,
    class_route_template: Option<&str>,
    effective_route_template: Option<&str>,
    verb: Option<&str>,
) -> Option<StructuralFact> {
    let start = handler.start_byte();
    let end = handler.end_byte();
    let node = smallest_node_covering_range(tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    let normalized = normalize_route_template(normalized_source, ParamFlavor::Braces);

    let mut metadata = base_metadata("framework", "symfony");
    insert_string(&mut metadata, "api_style", "annotation_routing");
    insert_string(&mut metadata, "attribute_kind", attribute_kind);
    insert_string(&mut metadata, "route_template", route_template);
    insert_string(
        &mut metadata,
        "normalized_route_template",
        &normalized.template,
    );
    if !normalized.dynamic_segments.is_empty() {
        insert_string_array(
            &mut metadata,
            "dynamic_segments",
            normalized.dynamic_segments,
        );
    }
    if let Some(class_route_template) = class_route_template {
        insert_string(&mut metadata, "class_route_template", class_route_template);
    }
    if let Some(effective_route_template) = effective_route_template {
        insert_string(
            &mut metadata,
            "effective_route_template",
            effective_route_template,
        );
    }
    if let Some(verb) = verb {
        insert_string(&mut metadata, "verb", verb);
        insert_string(&mut metadata, "verb_source", "attested");
    }

    Some(fact_for_span(
        file_path,
        language,
        SYMFONY_ROUTE_PATTERN_ID,
        "request_mapping",
        node.kind(),
        span,
        metadata,
    ))
}
