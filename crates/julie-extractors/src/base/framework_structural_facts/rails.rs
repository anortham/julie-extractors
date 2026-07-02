use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Tree;

use super::helpers::{
    base_metadata, fact_for_span, insert_string, is_comment_or_string_node,
    skip_ascii_whitespace_until, smallest_node_covering_range,
};
use super::{RAILS_MOUNT_PATTERN_ID, RAILS_RESOURCE_ROUTE_PATTERN_ID, RAILS_ROUTE_PATTERN_ID};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

pub(super) fn collect_rails_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !file_path.contains("config/routes") && !content.contains("Rails.application.routes.draw") {
        return Vec::new();
    }
    let mut facts = Vec::new();
    let mut offset = 0;
    let mut scope_stack: Vec<String> = Vec::new();
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(scope) = rails_scope_path(trimmed) {
            scope_stack.push(scope);
        }
        let scope_path = joined_scope(&scope_stack);
        if let Some((verb, route_template)) = rails_verb_route(trimmed) {
            push_rails_route(
                language,
                tree,
                file_path,
                content,
                offset,
                line,
                &scope_path,
                Some(&verb),
                &route_template,
                trimmed,
                &mut facts,
            );
        } else if let Some(route_template) = rails_root_route(trimmed) {
            push_rails_route(
                language,
                tree,
                file_path,
                content,
                offset,
                line,
                &scope_path,
                Some("GET"),
                &route_template,
                trimmed,
                &mut facts,
            );
        } else if let Some((route_template, verbs)) = rails_match_route(trimmed) {
            for verb in verbs {
                push_rails_route(
                    language,
                    tree,
                    file_path,
                    content,
                    offset,
                    line,
                    &scope_path,
                    verb.as_deref(),
                    &route_template,
                    trimmed,
                    &mut facts,
                );
            }
        } else if let Some((kind, resource_name)) = rails_resource(trimmed) {
            push_rails_resource(
                language,
                tree,
                file_path,
                content,
                offset,
                line,
                &scope_path,
                kind,
                &resource_name,
                trimmed,
                &mut facts,
            );
        } else if let Some((mount_target, mount_path)) = rails_mount(trimmed) {
            push_rails_mount(
                language,
                tree,
                file_path,
                content,
                offset,
                line,
                &scope_path,
                &mount_target,
                &mount_path,
                &mut facts,
            );
        }
        if trimmed == "end" {
            scope_stack.pop();
        }
        offset += line.len();
    }
    facts
}

#[allow(clippy::too_many_arguments)]
fn push_rails_route(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    line_start: usize,
    line: &str,
    scope_path: &str,
    verb: Option<&str>,
    route_template: &str,
    source: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let start = line_start + line.len() - line.trim_start().len();
    let end = line_start + line.trim_end().len();
    let Some(node) = smallest_node_covering_range(tree.root_node(), start, end) else {
        return;
    };
    if is_comment_or_string_node(node.kind()) {
        return;
    }
    let Some(span) = NormalizedSpan::from_content_range(content, start, end) else {
        return;
    };
    let mut metadata = base_metadata("framework", "rails");
    insert_string(&mut metadata, "api_style", "dsl_routing");
    insert_string(&mut metadata, "route_template", route_template);
    let normalized_source = if scope_path.is_empty() {
        route_template.to_string()
    } else {
        insert_string(&mut metadata, "scope_path", scope_path);
        let effective = join_route_templates(scope_path, route_template);
        insert_string(&mut metadata, "effective_route_template", &effective);
        effective
    };
    let normalized = normalize_route_template(&normalized_source, ParamFlavor::Colon);
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
    if let Some(verb) = verb {
        insert_string(&mut metadata, "verb", verb);
        insert_string(&mut metadata, "verb_source", "attested");
    }
    if let Some(controller_action) = string_keyword(source, "to") {
        insert_string(&mut metadata, "controller_action", &controller_action);
    }
    if let Some(route_name) = symbol_keyword(source, "as") {
        insert_string(&mut metadata, "route_name", &route_name);
    }
    facts.push(fact_for_span(
        file_path,
        language,
        RAILS_ROUTE_PATTERN_ID,
        "route",
        node.kind(),
        span,
        metadata,
    ));
}

#[allow(clippy::too_many_arguments)]
fn push_rails_resource(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    line_start: usize,
    line: &str,
    scope_path: &str,
    kind: &str,
    resource_name: &str,
    source: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let start = line_start + line.len() - line.trim_start().len();
    let end = line_start + line.trim_end().len();
    let Some(node) = smallest_node_covering_range(tree.root_node(), start, end) else {
        return;
    };
    let Some(span) = NormalizedSpan::from_content_range(content, start, end) else {
        return;
    };
    let mut metadata = base_metadata("framework", "rails");
    insert_string(&mut metadata, "api_style", "dsl_routing");
    insert_string(&mut metadata, "resource_kind", kind);
    insert_string(&mut metadata, "resource_name", resource_name);
    if !scope_path.is_empty() {
        insert_string(&mut metadata, "scope_path", scope_path);
    }
    if let Some(only) = symbol_array_keyword(source, "only") {
        insert_string_array(&mut metadata, "only", only);
    }
    if let Some(except) = symbol_array_keyword(source, "except") {
        insert_string_array(&mut metadata, "except", except);
    }
    facts.push(fact_for_span(
        file_path,
        language,
        RAILS_RESOURCE_ROUTE_PATTERN_ID,
        "resource_route",
        node.kind(),
        span,
        metadata,
    ));
}

#[allow(clippy::too_many_arguments)]
fn push_rails_mount(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    line_start: usize,
    line: &str,
    scope_path: &str,
    mount_target: &str,
    mount_path: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let start = line_start + line.len() - line.trim_start().len();
    let end = line_start + line.trim_end().len();
    let Some(node) = smallest_node_covering_range(tree.root_node(), start, end) else {
        return;
    };
    let Some(span) = NormalizedSpan::from_content_range(content, start, end) else {
        return;
    };
    let mut metadata = base_metadata("framework", "rails");
    insert_string(&mut metadata, "mount_target", mount_target);
    let full_mount_path = if scope_path.is_empty() {
        mount_path.to_string()
    } else {
        insert_string(&mut metadata, "scope_path", scope_path);
        join_route_templates(scope_path, mount_path)
    };
    insert_string(&mut metadata, "mount_path", mount_path);
    let normalized = normalize_route_template(&full_mount_path, ParamFlavor::Colon);
    insert_string(&mut metadata, "normalized_mount_path", &normalized.template);
    facts.push(fact_for_span(
        file_path,
        language,
        RAILS_MOUNT_PATTERN_ID,
        "mount",
        node.kind(),
        span,
        metadata,
    ));
}

fn rails_scope_path(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("namespace ") {
        let name = rest.split_whitespace().next()?.trim_start_matches(':');
        return Some(format!("/{name}"));
    }
    if let Some(rest) = line.strip_prefix("scope ") {
        let (path, _) =
            parse_ruby_string_literal(rest, skip_ascii_whitespace_until(rest, 0, rest.len()))?;
        return Some(path);
    }
    None
}

fn joined_scope(scopes: &[String]) -> String {
    scopes.iter().fold(String::new(), |acc, scope| {
        if acc.is_empty() {
            scope.clone()
        } else {
            join_route_templates(&acc, scope)
        }
    })
}

fn rails_verb_route(line: &str) -> Option<(String, String)> {
    for (method, verb) in [
        ("get", "GET"),
        ("post", "POST"),
        ("put", "PUT"),
        ("patch", "PATCH"),
        ("delete", "DELETE"),
    ] {
        if let Some(rest) = line.strip_prefix(&format!("{method} ")) {
            let (route, _) =
                parse_ruby_string_literal(rest, skip_ascii_whitespace_until(rest, 0, rest.len()))?;
            return Some((verb.to_string(), route));
        }
    }
    None
}

fn rails_root_route(line: &str) -> Option<String> {
    line.starts_with("root ").then(|| "/".to_string())
}

fn rails_match_route(line: &str) -> Option<(String, Vec<Option<String>>)> {
    let rest = line.strip_prefix("match ")?;
    let (route, _) =
        parse_ruby_string_literal(rest, skip_ascii_whitespace_until(rest, 0, rest.len()))?;
    if line.contains("via: :all") {
        return Some((route, vec![None]));
    }
    let verbs = symbol_array_keyword(line, "via")?
        .into_iter()
        .map(|verb| Some(verb.to_uppercase()))
        .collect();
    Some((route, verbs))
}

fn rails_resource(line: &str) -> Option<(&'static str, String)> {
    if let Some(rest) = line.strip_prefix("resources ") {
        return Some((
            "collection",
            rest.split([',', ' '])
                .next()?
                .trim_start_matches(':')
                .to_string(),
        ));
    }
    if let Some(rest) = line.strip_prefix("resource ") {
        return Some((
            "singular",
            rest.split([',', ' '])
                .next()?
                .trim_start_matches(':')
                .to_string(),
        ));
    }
    None
}

fn rails_mount(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("mount ")?;
    if let Some((target, path_part)) = rest.split_once("=>") {
        let path_start = skip_ascii_whitespace_until(path_part, 0, path_part.len());
        let (path, _) = parse_ruby_string_literal(path_part, path_start)?;
        return Some((target.trim().trim_end_matches(',').to_string(), path));
    }
    if let Some(path) = string_keyword(rest, "at") {
        let target = rest.split(',').next()?.trim().to_string();
        return Some((target, path));
    }
    None
}

fn string_keyword(source: &str, key: &str) -> Option<String> {
    let needle = format!("{key}:");
    let start = source.find(&needle)? + needle.len();
    let value_start = skip_ascii_whitespace_until(source, start, source.len());
    parse_ruby_string_literal(source, value_start).map(|(value, _)| value)
}

fn symbol_keyword(source: &str, key: &str) -> Option<String> {
    let needle = format!("{key}:");
    let start = source.find(&needle)? + needle.len();
    let value_start = skip_ascii_whitespace_until(source, start, source.len());
    source[value_start..]
        .strip_prefix(':')?
        .split([',', ' ', '\n'])
        .next()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn symbol_array_keyword(source: &str, key: &str) -> Option<Vec<String>> {
    let needle = format!("{key}:");
    let start = source.find(&needle)? + needle.len();
    let open = source[start..].find('[')? + start;
    let close = source[open..].find(']')? + open;
    Some(
        source[open + 1..close]
            .split(',')
            .filter_map(|item| {
                let item = item
                    .trim()
                    .trim_start_matches(':')
                    .trim_matches(['\'', '"']);
                (!item.is_empty()).then(|| item.to_string())
            })
            .collect(),
    )
}

fn parse_ruby_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    let quote = content.as_bytes().get(start).copied()?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    let mut cursor = start + 1;
    let mut value = String::new();
    while cursor < content.len() {
        let byte = content.as_bytes()[cursor];
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

fn insert_string_array(metadata: &mut HashMap<String, Value>, key: &str, values: Vec<String>) {
    metadata.insert(
        key.to_string(),
        Value::Array(values.into_iter().map(Value::String).collect()),
    );
}
