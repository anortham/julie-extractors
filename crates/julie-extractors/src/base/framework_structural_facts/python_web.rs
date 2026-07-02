use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Tree;

use super::helpers::{
    base_metadata, fact_for_span, insert_string, is_comment_or_string_node, is_identifier_boundary,
    skip_ascii_whitespace_until, smallest_node_covering_range,
};
use super::{
    DJANGO_URL_INCLUDE_PATTERN_ID, DJANGO_URL_PATTERN_ID, FASTAPI_INCLUDE_ROUTER_PATTERN_ID,
    FASTAPI_ROUTE_PATTERN_ID, FLASK_BLUEPRINT_REGISTRATION_PATTERN_ID, FLASK_ROUTE_PATTERN_ID,
};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

struct PythonFactContext<'a> {
    language: &'a str,
    tree: &'a Tree,
    file_path: &'a str,
    content: &'a str,
}

struct MountCallSpec<'a> {
    needle: &'a str,
    framework: &'static str,
    pattern_id: &'static str,
    capture_name: &'static str,
    prefix_keyword: &'static str,
}

const DECORATOR_VERBS: &[(&str, &str)] = &[
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
    ("head", "HEAD"),
    ("options", "OPTIONS"),
    ("trace", "TRACE"),
];

pub(super) fn collect_python_web_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let imports = collect_imports(content);
    let fastapi = collect_fastapi_receivers(content, &imports);
    let flask = collect_flask_receivers(content, &imports);

    let mut facts = Vec::new();
    facts.extend(collect_fastapi_routes(
        language, tree, file_path, content, &fastapi,
    ));
    facts.extend(collect_fastapi_includes(
        language, tree, file_path, content, &fastapi,
    ));
    facts.extend(collect_flask_routes(
        language, tree, file_path, content, &flask,
    ));
    facts.extend(collect_flask_blueprint_registrations(
        language, tree, file_path, content, &flask,
    ));
    if imports.django_path.is_some() || imports.django_re_path.is_some() {
        facts.extend(collect_django_urls(
            language, tree, file_path, content, &imports,
        ));
    }
    facts
}

#[derive(Default)]
struct PythonImports {
    fastapi_class: Option<String>,
    api_router_class: Option<String>,
    flask_class: Option<String>,
    blueprint_class: Option<String>,
    django_path: Option<String>,
    django_re_path: Option<String>,
    django_include: Option<String>,
}

#[derive(Clone)]
struct FastApiReceiver {
    framework_kind: FastApiReceiverKind,
    prefix: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FastApiReceiverKind {
    App,
    Router,
}

#[derive(Clone)]
struct FlaskReceiver {
    kind: FlaskReceiverKind,
    blueprint_name: Option<String>,
    prefix: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlaskReceiverKind {
    App,
    Blueprint,
}

fn collect_imports(content: &str) -> PythonImports {
    let mut imports = PythonImports::default();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("from fastapi import ") {
            for (imported, local) in parse_from_import_items(rest) {
                match imported.as_str() {
                    "FastAPI" => imports.fastapi_class = Some(local),
                    "APIRouter" => imports.api_router_class = Some(local),
                    _ => {}
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("from flask import ") {
            for (imported, local) in parse_from_import_items(rest) {
                match imported.as_str() {
                    "Flask" => imports.flask_class = Some(local),
                    "Blueprint" => imports.blueprint_class = Some(local),
                    _ => {}
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("from django.urls import ") {
            for (imported, local) in parse_from_import_items(rest) {
                match imported.as_str() {
                    "path" => imports.django_path = Some(local),
                    "re_path" => imports.django_re_path = Some(local),
                    "include" => imports.django_include = Some(local),
                    _ => {}
                }
            }
        }
    }
    imports
}

fn parse_from_import_items(rest: &str) -> Vec<(String, String)> {
    rest.trim_matches(['(', ')'])
        .split(',')
        .filter_map(|item| {
            let item = item.trim();
            if item.is_empty() {
                return None;
            }
            let mut parts = item.split_whitespace();
            let imported = parts.next()?.to_string();
            let local = if parts.next() == Some("as") {
                parts.next()?.to_string()
            } else {
                imported.clone()
            };
            Some((imported, local))
        })
        .collect()
}

fn collect_fastapi_receivers(
    content: &str,
    imports: &PythonImports,
) -> HashMap<String, FastApiReceiver> {
    let mut receivers = HashMap::new();
    if let Some(class_name) = imports.fastapi_class.as_deref() {
        for assignment in collect_constructor_assignments(content, class_name) {
            receivers.insert(
                assignment.name,
                FastApiReceiver {
                    framework_kind: FastApiReceiverKind::App,
                    prefix: None,
                },
            );
        }
    }
    if let Some(class_name) = imports.api_router_class.as_deref() {
        for assignment in collect_constructor_assignments(content, class_name) {
            receivers.insert(
                assignment.name,
                FastApiReceiver {
                    framework_kind: FastApiReceiverKind::Router,
                    prefix: keyword_string_arg(&assignment.args, "prefix"),
                },
            );
        }
    }
    receivers
}

fn collect_flask_receivers(
    content: &str,
    imports: &PythonImports,
) -> HashMap<String, FlaskReceiver> {
    let mut receivers = HashMap::new();
    if let Some(class_name) = imports.flask_class.as_deref() {
        for assignment in collect_constructor_assignments(content, class_name) {
            receivers.insert(
                assignment.name,
                FlaskReceiver {
                    kind: FlaskReceiverKind::App,
                    blueprint_name: None,
                    prefix: None,
                },
            );
        }
    }
    if let Some(class_name) = imports.blueprint_class.as_deref() {
        for assignment in collect_constructor_assignments(content, class_name) {
            let blueprint_name = positional_string_arg(&assignment.args, 0);
            receivers.insert(
                assignment.name,
                FlaskReceiver {
                    kind: FlaskReceiverKind::Blueprint,
                    blueprint_name,
                    prefix: keyword_string_arg(&assignment.args, "url_prefix"),
                },
            );
        }
    }
    receivers
}

struct ConstructorAssignment {
    name: String,
    args: String,
}

fn collect_constructor_assignments(content: &str, class_name: &str) -> Vec<ConstructorAssignment> {
    let mut assignments = Vec::new();
    let needle = format!("{class_name}(");
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(&needle) {
        let class_start = cursor + relative;
        cursor = class_start + needle.len();
        if is_in_python_string_or_comment(content, class_start) {
            continue;
        }
        let open = class_start + class_name.len();
        let Some(close) = find_matching_paren(content, open) else {
            continue;
        };
        let statement_start = content[..class_start]
            .rfind(['\n', ';'])
            .map(|index| index + 1)
            .unwrap_or(0);
        let before = content[statement_start..class_start].trim();
        let Some(name) = before
            .split('=')
            .next()
            .map(str::trim)
            .filter(|value| is_python_identifier(value))
        else {
            continue;
        };
        assignments.push(ConstructorAssignment {
            name: name.to_string(),
            args: content[open + 1..close].to_string(),
        });
    }
    assignments
}

fn collect_fastapi_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    receivers: &HashMap<String, FastApiReceiver>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for decorator in collect_decorator_calls(content) {
        let Some(receiver) = receivers.get(&decorator.receiver) else {
            continue;
        };
        let route_template = match decorator.first_arg.as_deref() {
            Some(route) => route,
            None => continue,
        };
        let verbs = if decorator.method == "api_route" {
            methods_keyword(&decorator.args)
        } else {
            DECORATOR_VERBS
                .iter()
                .find(|(method, _)| *method == decorator.method)
                .map(|(_, verb)| vec![(*verb).to_string()])
                .unwrap_or_default()
        };
        if verbs.is_empty() {
            continue;
        }
        for verb in verbs {
            if let Some(fact) = route_fact(
                language,
                tree,
                file_path,
                content,
                decorator.start,
                decorator.end,
                "fastapi",
                FASTAPI_ROUTE_PATTERN_ID,
                route_template,
                Some(&verb),
                Some("attested"),
                "decorator_routing",
                ParamFlavor::Braces,
                receiver.prefix.as_deref(),
                |metadata| {
                    if let Some(prefix) = receiver.prefix.as_deref() {
                        insert_string(metadata, "router_prefix", prefix);
                    }
                },
            ) {
                facts.push(fact);
            }
        }
    }
    facts
}

fn collect_fastapi_includes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    receivers: &HashMap<String, FastApiReceiver>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let context = PythonFactContext {
        language,
        tree,
        file_path,
        content,
    };
    for receiver in receivers.iter().filter_map(|(name, receiver)| {
        (receiver.framework_kind == FastApiReceiverKind::App).then_some(name)
    }) {
        let needle = format!("{receiver}.include_router");
        collect_mount_calls(
            &context,
            MountCallSpec {
                needle: &needle,
                framework: "fastapi",
                pattern_id: FASTAPI_INCLUDE_ROUTER_PATTERN_ID,
                capture_name: "include_router",
                prefix_keyword: "prefix",
            },
            &mut facts,
        );
    }
    facts
}

fn collect_flask_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    receivers: &HashMap<String, FlaskReceiver>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for decorator in collect_decorator_calls(content) {
        let Some(receiver) = receivers.get(&decorator.receiver) else {
            continue;
        };
        let route_template = match decorator.first_arg.as_deref() {
            Some(route) => route,
            None => continue,
        };
        let verbs = if decorator.method == "route" {
            let methods = methods_keyword(&decorator.args);
            if methods.is_empty() {
                vec!["GET".to_string()]
            } else {
                methods
            }
        } else {
            DECORATOR_VERBS
                .iter()
                .find(|(method, _)| *method == decorator.method)
                .map(|(_, verb)| vec![(*verb).to_string()])
                .unwrap_or_default()
        };
        if verbs.is_empty() {
            continue;
        }
        for verb in verbs {
            let verb_source = if decorator.method == "route" && !decorator.args.contains("methods")
            {
                "default"
            } else {
                "attested"
            };
            if let Some(fact) = route_fact(
                language,
                tree,
                file_path,
                content,
                decorator.start,
                decorator.end,
                "flask",
                FLASK_ROUTE_PATTERN_ID,
                route_template,
                Some(&verb),
                Some(verb_source),
                "decorator_routing",
                ParamFlavor::AngleBrackets,
                receiver.prefix.as_deref(),
                |metadata| {
                    if let Some(prefix) = receiver.prefix.as_deref() {
                        insert_string(metadata, "url_prefix", prefix);
                    }
                    if let Some(name) = receiver.blueprint_name.as_deref() {
                        insert_string(metadata, "blueprint", name);
                    }
                },
            ) {
                facts.push(fact);
            }
        }
    }
    facts
}

fn collect_flask_blueprint_registrations(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    receivers: &HashMap<String, FlaskReceiver>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    let context = PythonFactContext {
        language,
        tree,
        file_path,
        content,
    };
    for receiver in receivers
        .iter()
        .filter_map(|(name, receiver)| (receiver.kind == FlaskReceiverKind::App).then_some(name))
    {
        let needle = format!("{receiver}.register_blueprint");
        collect_mount_calls(
            &context,
            MountCallSpec {
                needle: &needle,
                framework: "flask",
                pattern_id: FLASK_BLUEPRINT_REGISTRATION_PATTERN_ID,
                capture_name: "blueprint_registration",
                prefix_keyword: "url_prefix",
            },
            &mut facts,
        );
    }
    facts
}

fn collect_django_urls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    imports: &PythonImports,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    if let Some(path_name) = imports.django_path.as_deref() {
        collect_django_calls(
            language,
            tree,
            file_path,
            content,
            path_name,
            "path",
            imports.django_include.as_deref(),
            &mut facts,
        );
    }
    if let Some(re_path_name) = imports.django_re_path.as_deref() {
        collect_django_calls(
            language,
            tree,
            file_path,
            content,
            re_path_name,
            "regex",
            imports.django_include.as_deref(),
            &mut facts,
        );
    }
    facts
}

#[allow(clippy::too_many_arguments)]
fn collect_django_calls(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    function_name: &str,
    route_syntax: &str,
    include_name: Option<&str>,
    facts: &mut Vec<StructuralFact>,
) {
    let needle = format!("{function_name}(");
    let context = PythonFactContext {
        language,
        tree,
        file_path,
        content,
    };
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(&needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if !is_identifier_boundary(content, call_start, function_name.len())
            || is_in_python_string_or_comment(content, call_start)
        {
            continue;
        }
        let open = call_start + function_name.len();
        let Some(close) = find_matching_paren(content, open) else {
            continue;
        };
        let first_start = skip_ascii_whitespace_until(content, open + 1, close);
        let first_end = find_top_level_comma_or_end(content, first_start, close);
        let Some((route_template, route_end)) = parse_python_string_literal(content, first_start)
        else {
            continue;
        };
        if skip_ascii_whitespace_until(content, route_end, first_end) != first_end {
            continue;
        }
        let second_start = skip_ascii_whitespace_until(content, first_end + 1, close);
        let second_end = find_top_level_comma_or_end(content, second_start, close);
        let second = content[second_start..second_end].trim();
        if include_name.is_some_and(|name| second.starts_with(&format!("{name}("))) {
            if let Some(fact) = django_include_fact(
                &context,
                call_start,
                close + 1,
                &route_template,
                second,
                &content[second_end..close],
            ) {
                facts.push(fact);
            }
            continue;
        }
        if let Some(fact) = django_route_fact(
            &context,
            call_start,
            close + 1,
            &route_template,
            route_syntax,
            second,
            &content[second_end..close],
        ) {
            facts.push(fact);
        }
    }
}

struct DecoratorCall {
    start: usize,
    end: usize,
    receiver: String,
    method: String,
    args: String,
    first_arg: Option<String>,
}

fn collect_decorator_calls(content: &str) -> Vec<DecoratorCall> {
    let mut decorators = Vec::new();
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let leading = line.len() - trimmed.len();
        if let Some(after_at) = trimmed.strip_prefix('@') {
            let start = offset + leading;
            if is_in_python_string_or_comment(content, start) {
                offset += line.len();
                continue;
            }
            let Some(dot) = after_at.find('.') else {
                offset += line.len();
                continue;
            };
            let receiver = after_at[..dot].trim();
            let rest = &after_at[dot + 1..];
            let Some(open_relative) = rest.find('(') else {
                offset += line.len();
                continue;
            };
            let method = rest[..open_relative].trim();
            if !is_python_identifier(receiver) || !is_python_identifier(method) {
                offset += line.len();
                continue;
            }
            let open = start + 1 + dot + 1 + open_relative;
            let Some(close) = find_matching_paren(content, open) else {
                offset += line.len();
                continue;
            };
            let (fact_start, function_end) =
                next_def_line_range(content, close + 1).unwrap_or((start, close + 1));
            let args = content[open + 1..close].to_string();
            let first_start = skip_ascii_whitespace_until(content, open + 1, close);
            let first_end = find_top_level_comma_or_end(content, first_start, close);
            let first_arg = parse_python_string_literal(content, first_start)
                .filter(|(_, end)| {
                    skip_ascii_whitespace_until(content, *end, first_end) == first_end
                })
                .map(|(value, _)| value);
            decorators.push(DecoratorCall {
                start: fact_start,
                end: function_end,
                receiver: receiver.to_string(),
                method: method.to_string(),
                args,
                first_arg,
            });
        }
        offset += line.len();
    }
    decorators
}

fn next_def_line_range(content: &str, start: usize) -> Option<(usize, usize)> {
    let relative = content[start..].find("def ")?;
    let def_start = start + relative;
    let def_end = content[def_start..]
        .find('\n')
        .map(|line| def_start + line)
        .unwrap_or(content.len());
    Some((def_start, def_end))
}

#[allow(clippy::too_many_arguments)]
fn route_fact<F>(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    start: usize,
    end: usize,
    framework: &str,
    pattern_id: &str,
    route_template: &str,
    verb: Option<&str>,
    verb_source: Option<&str>,
    api_style: &str,
    flavor: ParamFlavor,
    prefix: Option<&str>,
    enrich: F,
) -> Option<StructuralFact>
where
    F: FnOnce(&mut HashMap<String, Value>),
{
    let node = smallest_node_covering_range(tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    let mut metadata = base_metadata("framework", framework);
    insert_string(&mut metadata, "api_style", api_style);
    insert_string(&mut metadata, "route_template", route_template);
    let mut normalized_source = route_template.to_string();
    if let Some(prefix) = prefix {
        let effective = join_route_templates(prefix, route_template);
        insert_string(&mut metadata, "effective_route_template", &effective);
        normalized_source = effective;
    }
    let normalized = normalize_route_template(&normalized_source, flavor);
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
    }
    if let Some(verb_source) = verb_source {
        insert_string(&mut metadata, "verb_source", verb_source);
    }
    enrich(&mut metadata);
    Some(fact_for_span(
        file_path,
        language,
        pattern_id,
        "route",
        node.kind(),
        span,
        metadata,
    ))
}

fn collect_mount_calls(
    context: &PythonFactContext<'_>,
    spec: MountCallSpec<'_>,
    facts: &mut Vec<StructuralFact>,
) {
    let content = context.content;
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(spec.needle) {
        let call_start = cursor + relative;
        cursor = call_start + spec.needle.len();
        if is_in_python_string_or_comment(content, call_start) {
            continue;
        }
        let open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, open) else {
            continue;
        };
        let first_start = skip_ascii_whitespace_until(content, open + 1, close);
        let first_end = find_top_level_comma_or_end(content, first_start, close);
        let mount_target = content[first_start..first_end].trim();
        if mount_target.is_empty() || mount_target.starts_with(['\'', '"']) {
            continue;
        }
        let args = &content[open + 1..close];
        let mount_path = keyword_string_arg(args, spec.prefix_keyword);
        let node =
            match smallest_node_covering_range(context.tree.root_node(), call_start, close + 1) {
                Some(node) if !is_comment_or_string_node(node.kind()) => node,
                _ => continue,
            };
        let Some(span) = NormalizedSpan::from_content_range(content, call_start, close + 1) else {
            continue;
        };
        let mut metadata = base_metadata("framework", spec.framework);
        insert_string(&mut metadata, "mount_target", mount_target);
        if let Some(mount_path) = mount_path {
            let normalized = normalize_route_template(&mount_path, ParamFlavor::Colon);
            insert_string(&mut metadata, "mount_path", &mount_path);
            insert_string(&mut metadata, "normalized_mount_path", &normalized.template);
        }
        facts.push(fact_for_span(
            context.file_path,
            context.language,
            spec.pattern_id,
            spec.capture_name,
            node.kind(),
            span,
            metadata,
        ));
    }
}

fn django_route_fact(
    context: &PythonFactContext<'_>,
    start: usize,
    end: usize,
    route_template: &str,
    route_syntax: &str,
    view_target: &str,
    trailing_args: &str,
) -> Option<StructuralFact> {
    let content = context.content;
    let node = smallest_node_covering_range(context.tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    let mut metadata = base_metadata("framework", "django");
    insert_string(&mut metadata, "api_style", "dsl_routing");
    insert_string(&mut metadata, "route_template", route_template);
    insert_string(&mut metadata, "route_syntax", route_syntax);
    insert_string(&mut metadata, "view_target", view_target);
    if route_syntax == "path" {
        let normalized = normalize_route_template(route_template, ParamFlavor::AngleBrackets);
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
    }
    if let Some(name) = keyword_string_arg(trailing_args, "name") {
        insert_string(&mut metadata, "route_name", &name);
    }
    Some(fact_for_span(
        context.file_path,
        context.language,
        DJANGO_URL_PATTERN_ID,
        "url_pattern",
        node.kind(),
        span,
        metadata,
    ))
}

fn django_include_fact(
    context: &PythonFactContext<'_>,
    start: usize,
    end: usize,
    mount_path: &str,
    include_expr: &str,
    trailing_args: &str,
) -> Option<StructuralFact> {
    let content = context.content;
    let node = smallest_node_covering_range(context.tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    let normalized = normalize_route_template(mount_path, ParamFlavor::AngleBrackets);
    let mut metadata = base_metadata("framework", "django");
    insert_string(&mut metadata, "mount_path", mount_path);
    insert_string(&mut metadata, "normalized_mount_path", &normalized.template);
    if let Some(open) = include_expr.find('(') {
        let arg_start = skip_ascii_whitespace_until(include_expr, open + 1, include_expr.len());
        if let Some((module, _)) = parse_python_string_literal(include_expr, arg_start) {
            insert_string(&mut metadata, "included_module", &module);
        } else {
            insert_string(&mut metadata, "included_module", include_expr);
        }
    } else {
        insert_string(&mut metadata, "included_module", include_expr);
    }
    if let Some(namespace) = keyword_string_arg(trailing_args, "namespace") {
        insert_string(&mut metadata, "namespace", &namespace);
    }
    Some(fact_for_span(
        context.file_path,
        context.language,
        DJANGO_URL_INCLUDE_PATTERN_ID,
        "url_include",
        node.kind(),
        span,
        metadata,
    ))
}

fn methods_keyword(args: &str) -> Vec<String> {
    let Some(value_start) = keyword_value_start(args, "methods") else {
        return Vec::new();
    };
    if args.as_bytes().get(value_start) != Some(&b'[') {
        return Vec::new();
    }
    let Some(end) = find_matching_delimiter(args, value_start, b'[', b']') else {
        return Vec::new();
    };
    let mut methods = Vec::new();
    let mut cursor = value_start + 1;
    while cursor < end {
        cursor = skip_ascii_whitespace_until(args, cursor, end);
        if cursor >= end {
            break;
        }
        let Some((method, method_end)) = parse_python_string_literal(args, cursor) else {
            return Vec::new();
        };
        methods.push(method.to_uppercase());
        cursor = skip_ascii_whitespace_until(args, method_end, end);
        if args.as_bytes().get(cursor) == Some(&b',') {
            cursor += 1;
        }
    }
    methods
}

fn keyword_string_arg(args: &str, key: &str) -> Option<String> {
    let value_start = keyword_value_start(args, key)?;
    parse_python_string_literal(args, value_start).map(|(value, _)| value)
}

fn positional_string_arg(args: &str, index: usize) -> Option<String> {
    let mut cursor = 0;
    for current in 0..=index {
        cursor = skip_ascii_whitespace_until(args, cursor, args.len());
        let end = find_top_level_comma_or_end(args, cursor, args.len());
        if current == index {
            return parse_python_string_literal(args, cursor)
                .filter(|(_, literal_end)| {
                    skip_ascii_whitespace_until(args, *literal_end, end) == end
                })
                .map(|(value, _)| value);
        }
        cursor = end.saturating_add(1);
    }
    None
}

fn keyword_value_start(args: &str, key: &str) -> Option<usize> {
    let needle = format!("{key}=");
    let mut cursor = 0;
    while let Some(relative) = args[cursor..].find(&needle) {
        let key_start = cursor + relative;
        cursor = key_start + needle.len();
        if !is_identifier_boundary(args, key_start, key.len()) {
            continue;
        }
        return Some(skip_ascii_whitespace_until(args, cursor, args.len()));
    }
    None
}

fn insert_string_array(metadata: &mut HashMap<String, Value>, key: &str, values: Vec<String>) {
    metadata.insert(
        key.to_string(),
        Value::Array(values.into_iter().map(Value::String).collect()),
    );
}

fn parse_python_string_literal(content: &str, start: usize) -> Option<(String, usize)> {
    let bytes = content.as_bytes();
    let mut cursor = start;
    while matches!(bytes.get(cursor).copied(), Some(b'r' | b'R' | b'u' | b'U')) {
        cursor += 1;
    }
    if matches!(bytes.get(cursor).copied(), Some(b'f' | b'F' | b'b' | b'B')) {
        return None;
    }
    let quote = bytes
        .get(cursor)
        .copied()
        .filter(|byte| matches!(*byte, b'\'' | b'"'))?;
    let mut index = cursor + 1;
    let mut value = String::new();
    while index < content.len() {
        let byte = bytes[index];
        if byte == b'\\' {
            let escaped_start = index + 1;
            let escaped = content.get(escaped_start..)?.chars().next()?;
            value.push(escaped);
            index = escaped_start + escaped.len_utf8();
        } else if byte == quote {
            return Some((value, index + 1));
        } else {
            let ch = content.get(index..)?.chars().next()?;
            value.push(ch);
            index += ch.len_utf8();
        }
    }
    None
}

fn find_matching_paren(content: &str, open: usize) -> Option<usize> {
    find_matching_delimiter(content, open, b'(', b')')
}

fn find_matching_delimiter(content: &str, open: usize, left: u8, right: u8) -> Option<usize> {
    if content.as_bytes().get(open) != Some(&left) {
        return None;
    }
    let bytes = content.as_bytes();
    let mut cursor = open;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    while cursor < content.len() {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        } else if byte == left {
            depth += 1;
        } else if byte == right {
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
    let bytes = content.as_bytes();
    let mut cursor = start;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
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
        } else if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        } else {
            match byte {
                b'(' => paren_depth += 1,
                b')' => paren_depth = paren_depth.saturating_sub(1),
                b'[' => bracket_depth += 1,
                b']' => bracket_depth = bracket_depth.saturating_sub(1),
                b'{' => brace_depth += 1,
                b'}' => brace_depth = brace_depth.saturating_sub(1),
                b',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                    return cursor;
                }
                _ => {}
            }
        }
        cursor += 1;
    }
    end
}

fn is_python_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_in_python_string_or_comment(content: &str, target: usize) -> bool {
    let bytes = content.as_bytes();
    let mut cursor = 0;
    let mut quote = None;
    let mut escaped = false;
    while cursor < target {
        let byte = bytes[cursor];
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
        } else if byte == b'#' {
            while cursor < target && bytes.get(cursor) != Some(&b'\n') {
                cursor += 1;
            }
        } else if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        }
        cursor += 1;
    }
    quote.is_some()
}
