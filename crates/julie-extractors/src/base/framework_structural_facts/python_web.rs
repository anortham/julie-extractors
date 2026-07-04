use std::collections::HashMap;

use tree_sitter::Tree;

use super::helpers::{
    base_metadata, fact_for_span, insert_string, insert_string_array, is_ascii_identifier,
    is_comment_or_string_node, is_identifier_boundary, skip_ascii_whitespace_until,
    smallest_node_covering_range,
};
use super::scan::{
    MaskLanguage, RouteFactSpec, SourceMask, find_matching_bracket_within, find_matching_paren,
    find_top_level_comma_or_end, parse_python_string_literal, route_fact,
};
use super::{
    DJANGO_URL_INCLUDE_PATTERN_ID, DJANGO_URL_PATTERN_ID, FASTAPI_INCLUDE_ROUTER_PATTERN_ID,
    FASTAPI_ROUTE_PATTERN_ID, FLASK_BLUEPRINT_REGISTRATION_PATTERN_ID, FLASK_ROUTE_PATTERN_ID,
};
use crate::base::http_boundary::{ParamFlavor, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

struct PythonFactContext<'a> {
    language: &'a str,
    tree: &'a Tree,
    file_path: &'a str,
    content: &'a str,
    mask: &'a SourceMask,
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
    if imports.is_empty() {
        return Vec::new();
    }
    let mask = SourceMask::new(content, MaskLanguage::Python);
    let context = PythonFactContext {
        language,
        tree,
        file_path,
        content,
        mask: &mask,
    };
    let fastapi = collect_fastapi_receivers(&context, &imports);
    let flask = collect_flask_receivers(&context, &imports);

    let mut facts = Vec::new();
    facts.extend(collect_fastapi_routes(&context, &fastapi));
    facts.extend(collect_fastapi_includes(&context, &fastapi));
    facts.extend(collect_flask_routes(&context, &flask));
    facts.extend(collect_flask_blueprint_registrations(&context, &flask));
    if imports.django_path.is_some() || imports.django_re_path.is_some() {
        facts.extend(collect_django_urls(&context, &imports));
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

impl PythonImports {
    fn is_empty(&self) -> bool {
        self.fastapi_class.is_none()
            && self.api_router_class.is_none()
            && self.flask_class.is_none()
            && self.blueprint_class.is_none()
            && self.django_path.is_none()
            && self.django_re_path.is_none()
            && self.django_include.is_none()
    }
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
    for trimmed in python_logical_lines(content) {
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
        } else if let Some(rest) = trimmed.strip_prefix("import ") {
            for (module, local) in parse_module_import_items(rest) {
                if module == "fastapi" {
                    imports.fastapi_class = Some(format!("{local}.FastAPI"));
                    imports.api_router_class = Some(format!("{local}.APIRouter"));
                }
            }
        }
    }
    imports
}

fn python_logical_lines(content: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut pending = String::new();
    let mut paren_depth = 0isize;
    for line in content.lines() {
        let trimmed = line.trim();
        if pending.is_empty() {
            pending.push_str(trimmed);
        } else {
            pending.push(' ');
            pending.push_str(trimmed);
        }
        paren_depth += trimmed.matches('(').count() as isize;
        paren_depth -= trimmed.matches(')').count() as isize;
        if paren_depth <= 0 {
            lines.push(std::mem::take(&mut pending));
            paren_depth = 0;
        }
    }
    if !pending.is_empty() {
        lines.push(pending);
    }
    lines
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

fn parse_module_import_items(rest: &str) -> Vec<(String, String)> {
    rest.split('#')
        .next()
        .unwrap_or(rest)
        .split(',')
        .filter_map(|item| {
            let item = item.trim();
            if item.is_empty() {
                return None;
            }
            let mut parts = item.split_whitespace();
            let module = parts.next()?.to_string();
            let local = if parts.next() == Some("as") {
                parts.next()?.to_string()
            } else {
                module.clone()
            };
            Some((module, local))
        })
        .collect()
}

fn collect_fastapi_receivers(
    context: &PythonFactContext<'_>,
    imports: &PythonImports,
) -> HashMap<String, FastApiReceiver> {
    let mut receivers = HashMap::new();
    if let Some(class_name) = imports.fastapi_class.as_deref() {
        for assignment in collect_constructor_assignments(context, class_name) {
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
        for assignment in collect_constructor_assignments(context, class_name) {
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
    context: &PythonFactContext<'_>,
    imports: &PythonImports,
) -> HashMap<String, FlaskReceiver> {
    let mut receivers = HashMap::new();
    if let Some(class_name) = imports.flask_class.as_deref() {
        for assignment in collect_constructor_assignments(context, class_name) {
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
        for assignment in collect_constructor_assignments(context, class_name) {
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

fn collect_constructor_assignments(
    context: &PythonFactContext<'_>,
    class_name: &str,
) -> Vec<ConstructorAssignment> {
    let content = context.content;
    let mut assignments = Vec::new();
    let needle = format!("{class_name}(");
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(&needle) {
        let class_start = cursor + relative;
        cursor = class_start + needle.len();
        if context.mask.is_string_or_comment(class_start) {
            continue;
        }
        let open = class_start + class_name.len();
        let Some(close) = find_matching_paren(content, context.mask, open) else {
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
            .filter(|value| is_ascii_identifier(value))
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
    context: &PythonFactContext<'_>,
    receivers: &HashMap<String, FastApiReceiver>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for decorator in collect_decorator_calls(context) {
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
                context.language,
                context.tree,
                context.file_path,
                context.content,
                decorator.start,
                decorator.end,
                RouteFactSpec {
                    framework: "fastapi",
                    pattern_id: FASTAPI_ROUTE_PATTERN_ID,
                    capture_name: "route",
                    api_style: "decorator_routing",
                    route_template,
                    verb: Some(&verb),
                    verb_source: Some("attested"),
                    flavor: ParamFlavor::Braces,
                    prefix: receiver.prefix.as_deref(),
                    prefix_key: None,
                },
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
    context: &PythonFactContext<'_>,
    receivers: &HashMap<String, FastApiReceiver>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for receiver in receivers.iter().filter_map(|(name, receiver)| {
        (receiver.framework_kind == FastApiReceiverKind::App).then_some(name)
    }) {
        let needle = format!("{receiver}.include_router");
        collect_mount_calls(
            context,
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
    context: &PythonFactContext<'_>,
    receivers: &HashMap<String, FlaskReceiver>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for decorator in collect_decorator_calls(context) {
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
        let has_methods_keyword = keyword_value_start(&decorator.args, "methods").is_some();
        for verb in verbs {
            let verb_source = if decorator.method == "route" && !has_methods_keyword {
                "default"
            } else {
                "attested"
            };
            if let Some(fact) = route_fact(
                context.language,
                context.tree,
                context.file_path,
                context.content,
                decorator.start,
                decorator.end,
                RouteFactSpec {
                    framework: "flask",
                    pattern_id: FLASK_ROUTE_PATTERN_ID,
                    capture_name: "route",
                    api_style: "decorator_routing",
                    route_template,
                    verb: Some(&verb),
                    verb_source: Some(verb_source),
                    flavor: ParamFlavor::AngleBrackets,
                    prefix: receiver.prefix.as_deref(),
                    prefix_key: None,
                },
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
    context: &PythonFactContext<'_>,
    receivers: &HashMap<String, FlaskReceiver>,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for receiver in receivers
        .iter()
        .filter_map(|(name, receiver)| (receiver.kind == FlaskReceiverKind::App).then_some(name))
    {
        let needle = format!("{receiver}.register_blueprint");
        collect_mount_calls(
            context,
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
    context: &PythonFactContext<'_>,
    imports: &PythonImports,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    if let Some(path_name) = imports.django_path.as_deref() {
        collect_django_calls(
            context,
            path_name,
            "path",
            imports.django_include.as_deref(),
            &mut facts,
        );
    }
    if let Some(re_path_name) = imports.django_re_path.as_deref() {
        collect_django_calls(
            context,
            re_path_name,
            "regex",
            imports.django_include.as_deref(),
            &mut facts,
        );
    }
    facts
}

fn collect_django_calls(
    context: &PythonFactContext<'_>,
    function_name: &str,
    route_syntax: &str,
    include_name: Option<&str>,
    facts: &mut Vec<StructuralFact>,
) {
    let content = context.content;
    let needle = format!("{function_name}(");
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(&needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if !is_identifier_boundary(content, call_start, function_name.len())
            || context.mask.is_string_or_comment(call_start)
        {
            continue;
        }
        let open = call_start + function_name.len();
        let Some(close) = find_matching_paren(content, context.mask, open) else {
            continue;
        };
        let first_start = skip_ascii_whitespace_until(content, open + 1, close);
        let first_end = find_top_level_comma_or_end(content, context.mask, first_start, close);
        let Some((route_template, route_end)) = parse_python_string_literal(content, first_start)
        else {
            continue;
        };
        if skip_ascii_whitespace_until(content, route_end, first_end) != first_end {
            continue;
        }
        // A path()/re_path() call needs a view (or include) second argument;
        // single-argument calls have nothing to bind and stay silent.
        let second_start = skip_ascii_whitespace_until(content, first_end + 1, close);
        if second_start >= close {
            continue;
        }
        let second_end = find_top_level_comma_or_end(content, context.mask, second_start, close);
        let second = content[second_start..second_end].trim();
        if second.is_empty() {
            continue;
        }
        if include_name.is_some_and(|name| second.starts_with(&format!("{name}("))) {
            if let Some(fact) = django_include_fact(
                context,
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
            context,
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

fn collect_decorator_calls(context: &PythonFactContext<'_>) -> Vec<DecoratorCall> {
    let content = context.content;
    let mut decorators = Vec::new();
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let leading = line.len() - trimmed.len();
        if let Some(after_at) = trimmed.strip_prefix('@') {
            let start = offset + leading;
            if context.mask.is_string_or_comment(start) {
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
            if !is_ascii_identifier(receiver) || !is_ascii_identifier(method) {
                offset += line.len();
                continue;
            }
            let open = start + 1 + dot + 1 + open_relative;
            let Some(close) = find_matching_paren(content, context.mask, open) else {
                offset += line.len();
                continue;
            };
            let (fact_start, function_end) =
                next_def_line_range(content, close + 1).unwrap_or((start, close + 1));
            let args = content[open + 1..close].to_string();
            let first_start = skip_ascii_whitespace_until(content, open + 1, close);
            let first_end = find_top_level_comma_or_end(content, context.mask, first_start, close);
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
        if context.mask.is_string_or_comment(call_start) {
            continue;
        }
        let open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, context.mask, open) else {
            continue;
        };
        let first_start = skip_ascii_whitespace_until(content, open + 1, close);
        let first_end = find_top_level_comma_or_end(content, context.mask, first_start, close);
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
    } else if route_syntax == "regex"
        && let Some((template, dynamic_segments)) = normalize_django_regex_route(route_template)
    {
        insert_string(&mut metadata, "normalized_route_template", &template);
        if !dynamic_segments.is_empty() {
            insert_string_array(&mut metadata, "dynamic_segments", dynamic_segments);
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

fn normalize_django_regex_route(pattern: &str) -> Option<(String, Vec<String>)> {
    let mut source = pattern;
    if let Some(stripped) = source.strip_prefix('^') {
        source = stripped;
    }
    if let Some(stripped) = source.strip_suffix('$') {
        source = stripped;
    }

    let bytes = source.as_bytes();
    let mut cursor = 0usize;
    let mut template = String::new();
    let mut dynamic_segments = Vec::new();
    while cursor < bytes.len() {
        if source[cursor..].starts_with("(?P<") {
            let name_start = cursor + "(?P<".len();
            let name_end = source[name_start..].find('>')? + name_start;
            let name = &source[name_start..name_end];
            if !is_ascii_identifier(name) {
                return None;
            }
            let group_end = regex_group_end(source, cursor)?;
            template.push(':');
            template.push_str(name);
            dynamic_segments.push(name.to_string());
            cursor = group_end + 1;
            continue;
        }

        let byte = bytes[cursor];
        if matches!(
            byte,
            b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'+' | b'*' | b'?' | b'|' | b'\\'
        ) {
            return None;
        }
        template.push(byte as char);
        cursor += 1;
    }

    if !template.starts_with('/') {
        template.insert(0, '/');
    }
    Some((template, dynamic_segments))
}

fn regex_group_end(pattern: &str, open: usize) -> Option<usize> {
    let bytes = pattern.as_bytes();
    let mut cursor = open + 1;
    let mut in_class = false;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 2,
            b'[' if !in_class => {
                in_class = true;
                cursor += 1;
            }
            b']' if in_class => {
                in_class = false;
                cursor += 1;
            }
            b')' if !in_class => return Some(cursor),
            _ => cursor += 1,
        }
    }
    None
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
    let args_mask = SourceMask::new(args, MaskLanguage::Python);
    let Some(end) = find_matching_bracket_within(args, &args_mask, value_start, args.len()) else {
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
    let args_mask = SourceMask::new(args, MaskLanguage::Python);
    let mut cursor = 0;
    for current in 0..=index {
        cursor = skip_ascii_whitespace_until(args, cursor, args.len());
        let end = find_top_level_comma_or_end(args, &args_mask, cursor, args.len());
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
    let needle = key;
    let mut cursor = 0;
    while let Some(relative) = args[cursor..].find(needle) {
        let key_start = cursor + relative;
        cursor = key_start + key.len();
        if !is_identifier_boundary(args, key_start, key.len()) {
            continue;
        }
        let equals = skip_ascii_whitespace_until(args, cursor, args.len());
        if args.as_bytes().get(equals) != Some(&b'=') {
            continue;
        }
        return Some(skip_ascii_whitespace_until(args, equals + 1, args.len()));
    }
    None
}
