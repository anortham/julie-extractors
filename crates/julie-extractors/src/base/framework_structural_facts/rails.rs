use tree_sitter::Tree;

use super::helpers::{
    base_metadata, fact_for_span, insert_string, insert_string_array, is_comment_or_string_node,
    skip_ascii_whitespace_until, smallest_node_covering_range,
};
use super::scan::parse_ruby_string_literal;
use super::{RAILS_MOUNT_PATTERN_ID, RAILS_RESOURCE_ROUTE_PATTERN_ID, RAILS_ROUTE_PATTERN_ID};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

/// The kinds of `do ... end` (and keyword) blocks tracked while walking a
/// routes file. Only `namespace`/`scope` blocks contribute to the scope path;
/// every other block still needs a stack entry so its `end` does not pop an
/// enclosing scope early.
enum BlockKind {
    Draw,
    Scope,
    Resource,
    Other,
}

struct ResourceContext {
    collection_path: String,
    member_path: String,
}

pub(super) fn collect_rails_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    // Rails 6.1+ split route files (`config/routes/*.rb`, loaded via
    // `draw :name`) hold top-level DSL; everything else requires the DSL to
    // sit inside a `routes.draw do ... end` block.
    let split_route_file = file_path.contains("config/routes/");
    if !split_route_file && !content.contains(".routes.draw") {
        return Vec::new();
    }
    let mut facts = Vec::new();
    let mut offset = 0;
    let mut scope_stack: Vec<Option<String>> = Vec::new();
    let mut block_stack: Vec<BlockKind> = Vec::new();
    let mut resource_stack: Vec<ResourceContext> = Vec::new();
    let mut draw_depth = 0usize;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim();
        let opens_block = rails_opens_block(trimmed);
        if opens_block {
            if trimmed.contains(".routes.draw") {
                block_stack.push(BlockKind::Draw);
                draw_depth += 1;
            } else if let Some(scope) = rails_scope_path(trimmed) {
                scope_stack.push(scope);
                block_stack.push(BlockKind::Scope);
            } else if let Some(scope) =
                rails_member_collection_scope(trimmed, resource_stack.last())
            {
                scope_stack.push(Some(scope));
                block_stack.push(BlockKind::Scope);
            } else if let Some(resource) = rails_resource_context(trimmed) {
                resource_stack.push(resource);
                block_stack.push(BlockKind::Resource);
            } else {
                block_stack.push(BlockKind::Other);
            }
        }
        let active = split_route_file || draw_depth > 0;
        if !active {
            offset += line.len();
            continue;
        }
        let Some(scope_path) = joined_scope(&scope_stack) else {
            if trimmed == "end" || trimmed.starts_with("end ") {
                pop_rails_block(
                    &mut block_stack,
                    &mut scope_stack,
                    &mut resource_stack,
                    &mut draw_depth,
                );
            }
            offset += line.len();
            continue;
        };
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
        } else {
            let resources = rails_resources(trimmed);
            if !resources.is_empty() {
                for (kind, resource_name) in resources {
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
                }
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
        }
        if trimmed == "end" || trimmed.starts_with("end ") {
            pop_rails_block(
                &mut block_stack,
                &mut scope_stack,
                &mut resource_stack,
                &mut draw_depth,
            );
        }
        offset += line.len();
    }
    facts
}

/// A routes-file line opens a block when it ends in a `do` (with or without
/// block parameters) or starts with a block keyword. Trailing `if`/`unless`
/// modifiers do not open blocks and are not matched here.
fn rails_opens_block(line: &str) -> bool {
    if line == "do" || line.ends_with(" do") || line.contains(" do |") {
        return true;
    }
    [
        "if ", "unless ", "case ", "while ", "until ", "def ", "class ", "module ",
    ]
    .iter()
    .any(|keyword| line.starts_with(keyword))
        || line == "begin"
}

fn pop_rails_block(
    block_stack: &mut Vec<BlockKind>,
    scope_stack: &mut Vec<Option<String>>,
    resource_stack: &mut Vec<ResourceContext>,
    draw_depth: &mut usize,
) {
    match block_stack.pop() {
        Some(BlockKind::Scope) => {
            scope_stack.pop();
        }
        Some(BlockKind::Resource) => {
            resource_stack.pop();
        }
        Some(BlockKind::Draw) => {
            *draw_depth = draw_depth.saturating_sub(1);
        }
        _ => {}
    }
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

fn rails_scope_path(line: &str) -> Option<Option<String>> {
    if let Some(rest) = line.strip_prefix("namespace ") {
        let name = rest.split_whitespace().next()?.trim_start_matches(':');
        return Some(Some(format!("/{name}")));
    }
    if let Some(rest) = line.strip_prefix("scope ") {
        if let Some(path) = string_keyword_value(rest, "path") {
            return Some(path);
        }
        let path_start = skip_ascii_whitespace_until(rest, 0, rest.len());
        if matches!(rest.as_bytes().get(path_start), Some(b'"' | b'\'')) {
            let (path, _) = parse_ruby_string_literal(rest, path_start)?;
            return Some(static_ruby_value(path));
        }
    }
    None
}

fn rails_member_collection_scope(
    line: &str,
    current_resource: Option<&ResourceContext>,
) -> Option<String> {
    let resource = current_resource?;
    if line == "member do" || line.starts_with("member do ") {
        return Some(resource.member_path.clone());
    }
    if line == "collection do" || line.starts_with("collection do ") {
        return Some(resource.collection_path.clone());
    }
    None
}

fn rails_resource_context(line: &str) -> Option<ResourceContext> {
    let (kind, resource_name) = rails_resources(line).into_iter().next()?;
    let collection_path = format!("/{resource_name}");
    let member_path = if kind == "singular" {
        collection_path.clone()
    } else {
        format!(
            "/{resource_name}/:{}_id",
            singular_resource_name(&resource_name)
        )
    };
    Some(ResourceContext {
        collection_path,
        member_path,
    })
}

fn joined_scope(scopes: &[Option<String>]) -> Option<String> {
    let mut joined = String::new();
    for scope in scopes {
        let scope = scope.as_deref()?;
        joined = if joined.is_empty() {
            scope.to_string()
        } else {
            join_route_templates(&joined, scope)
        };
    }
    Some(joined)
}

fn static_ruby_value(value: String) -> Option<String> {
    (!value.contains("#{")).then_some(value)
}

fn parse_static_ruby_string_literal(source: &str, start: usize) -> Option<(String, usize)> {
    let (value, end) = parse_ruby_string_literal(source, start)?;
    static_ruby_value(value).map(|value| (value, end))
}

fn string_keyword_value(source: &str, key: &str) -> Option<Option<String>> {
    let needle = format!("{key}:");
    let start = source.find(&needle)? + needle.len();
    let value_start = skip_ascii_whitespace_until(source, start, source.len());
    Some(
        parse_ruby_string_literal(source, value_start)
            .and_then(|(value, _)| static_ruby_value(value)),
    )
}

fn rails_verb_route(line: &str) -> Option<(String, String)> {
    for (method, verb) in [
        ("get", "GET"),
        ("post", "POST"),
        ("put", "PUT"),
        ("patch", "PATCH"),
        ("delete", "DELETE"),
    ] {
        if let Some(rest) = rails_method_args(line, method) {
            let (route, _) = parse_static_ruby_string_literal(
                rest,
                skip_ascii_whitespace_until(rest, 0, rest.len()),
            )?;
            return Some((verb.to_string(), route));
        }
    }
    None
}

fn rails_method_args<'a>(line: &'a str, method: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(method)?;
    let rest = rest.trim_start();
    if let Some(stripped) = rest.strip_prefix('(') {
        return Some(stripped);
    }
    (!rest.is_empty()).then_some(rest)
}

fn rails_root_route(line: &str) -> Option<String> {
    line.starts_with("root ").then(|| "/".to_string())
}

fn rails_match_route(line: &str) -> Option<(String, Vec<Option<String>>)> {
    let rest = line.strip_prefix("match ")?;
    let (route, _) =
        parse_static_ruby_string_literal(rest, skip_ascii_whitespace_until(rest, 0, rest.len()))?;
    if line.contains("via: :all") {
        return Some((route, vec![None]));
    }
    if let Some(verbs) = symbol_array_keyword(line, "via") {
        return Some((
            route,
            verbs
                .into_iter()
                .map(|verb| Some(verb.to_uppercase()))
                .collect(),
        ));
    }
    let verb = symbol_keyword(line, "via")?;
    Some((route, vec![Some(verb.to_uppercase())]))
}

fn rails_resources(line: &str) -> Vec<(&'static str, String)> {
    let (kind, rest) = if let Some(rest) = line.strip_prefix("resources ") {
        ("collection", rest)
    } else if let Some(rest) = line.strip_prefix("resource ") {
        ("singular", rest)
    } else {
        return Vec::new();
    };
    rails_resource_names(rest)
        .into_iter()
        .map(|name| (kind, name))
        .collect()
}

fn rails_resource_names(rest: &str) -> Vec<String> {
    let rest = rest
        .trim_start()
        .strip_prefix('(')
        .unwrap_or_else(|| rest.trim_start());
    let mut names = Vec::new();
    for part in rest.split(',') {
        let token = part.split_whitespace().next().unwrap_or("");
        if !token.starts_with(':') {
            break;
        }
        let name = token
            .trim_start_matches(':')
            .trim_end_matches(')')
            .trim_end_matches("do");
        if name.is_empty() || name.ends_with(':') {
            break;
        }
        names.push(name.to_string());
    }
    names
}

fn singular_resource_name(name: &str) -> String {
    if let Some(stem) = name.strip_suffix("ies") {
        return format!("{stem}y");
    }
    name.strip_suffix('s').unwrap_or(name).to_string()
}

fn rails_mount(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("mount ")?;
    if let Some((target, path_part)) = rest.split_once("=>") {
        let path_start = skip_ascii_whitespace_until(path_part, 0, path_part.len());
        let (path, _) = parse_static_ruby_string_literal(path_part, path_start)?;
        return Some((target.trim().trim_end_matches(',').to_string(), path));
    }
    if let Some(path) = string_keyword(rest, "at") {
        let target = rest.split(',').next()?.trim().to_string();
        return Some((target, path));
    }
    None
}

fn string_keyword(source: &str, key: &str) -> Option<String> {
    string_keyword_value(source, key)?
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
