use std::collections::HashMap;

use tree_sitter::Tree;

use super::helpers::{
    base_metadata, fact_for_span, insert_string, insert_string_array, is_ascii_identifier,
    is_comment_or_string_node, is_identifier_boundary, skip_ascii_whitespace_until,
    smallest_node_covering_range,
};
use super::scan::{
    MaskLanguage, RouteFactSpec, SourceMask, find_matching_brace_within,
    find_matching_bracket_within, find_matching_paren, find_matching_paren_within,
    find_top_level_comma_or_end, parse_python_string_literal, route_fact,
};
use super::{
    DJANGO_URL_INCLUDE_PATTERN_ID, DJANGO_URL_PATTERN_ID, DRF_API_VIEW_PATTERN_ID,
    DRF_ROUTER_REGISTRATION_PATTERN_ID, DRF_VIEWSET_ACTION_PATTERN_ID,
    FASTAPI_INCLUDE_ROUTER_PATTERN_ID, FASTAPI_ROUTE_PATTERN_ID,
    FLASK_BLUEPRINT_REGISTRATION_PATTERN_ID, FLASK_ROUTE_PATTERN_ID,
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
    facts.extend(collect_flask_url_rules(&context, &flask));
    facts.extend(collect_flask_blueprint_registrations(&context, &flask));
    if imports.django_path.is_some() || imports.django_re_path.is_some() {
        facts.extend(collect_django_urls(&context, &imports));
    }
    facts.extend(collect_drf_facts(&context, &imports));
    facts
}

#[derive(Default)]
struct PythonImports {
    fastapi_class: Option<String>,
    api_router_class: Option<String>,
    flask_class: Option<String>,
    blueprint_class: Option<String>,
    /// The file imports from `flask`, so an imported receiver's route
    /// decorators are Flask routes.
    flask_imported: bool,
    /// Local names bound by `from <module> import ...` from other modules.
    imported_names: Vec<String>,
    django_path: Option<String>,
    django_re_path: Option<String>,
    django_include: Option<String>,
    /// Local names of the Django REST Framework router classes.
    drf_router_classes: Vec<String>,
    drf_action: Option<String>,
    drf_api_view: Option<String>,
}

impl PythonImports {
    fn is_empty(&self) -> bool {
        self.fastapi_class.is_none()
            && self.api_router_class.is_none()
            && self.flask_class.is_none()
            && self.blueprint_class.is_none()
            && !self.flask_imported
            && self.django_path.is_none()
            && self.django_re_path.is_none()
            && self.django_include.is_none()
            && self.drf_router_classes.is_empty()
            && self.drf_action.is_none()
            && self.drf_api_view.is_none()
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
    /// A Flask app or blueprint constructed in another module and imported here.
    Imported,
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
            imports.flask_imported = true;
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
        } else if let Some(rest) = trimmed.strip_prefix("from rest_framework.routers import ") {
            for (imported, local) in parse_from_import_items(rest) {
                if matches!(imported.as_str(), "DefaultRouter" | "SimpleRouter") {
                    imports.drf_router_classes.push(local);
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("from rest_framework.decorators import ") {
            for (imported, local) in parse_from_import_items(rest) {
                match imported.as_str() {
                    "action" => imports.drf_action = Some(local),
                    "api_view" => imports.drf_api_view = Some(local),
                    _ => {}
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("from rest_framework import ") {
            for (imported, local) in parse_from_import_items(rest) {
                if imported == "routers" {
                    imports.drf_router_classes.extend(
                        ["DefaultRouter", "SimpleRouter"].map(|class| format!("{local}.{class}")),
                    );
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("import ") {
            for (module, local) in parse_module_import_items(rest) {
                if module == "fastapi" {
                    imports.fastapi_class = Some(format!("{local}.FastAPI"));
                    imports.api_router_class = Some(format!("{local}.APIRouter"));
                } else if module == "flask" {
                    imports.flask_imported = true;
                    imports.flask_class = Some(format!("{local}.Flask"));
                    imports.blueprint_class = Some(format!("{local}.Blueprint"));
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("from ")
            && let Some((_, items)) = rest.split_once(" import ")
        {
            imports.imported_names.extend(
                parse_from_import_items(items)
                    .into_iter()
                    .map(|(_, local)| local)
                    .filter(|local| is_ascii_identifier(local)),
            );
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
    if imports.flask_imported {
        for name in &imports.imported_names {
            receivers
                .entry(name.clone())
                .or_insert_with(|| FlaskReceiver {
                    kind: FlaskReceiverKind::Imported,
                    blueprint_name: None,
                    prefix: None,
                });
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
            .strip_suffix('=')
            .and_then(|target| target.split(':').next())
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

/// Django REST Framework facts: `router.register(prefix, ViewSet)` on a
/// same-file DefaultRouter/SimpleRouter, `@action(...)` extra viewset routes,
/// and `@api_view([...])` function views.
fn collect_drf_facts(
    context: &PythonFactContext<'_>,
    imports: &PythonImports,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    for class_name in &imports.drf_router_classes {
        for router in collect_constructor_assignments(context, class_name) {
            collect_drf_registrations(context, &router.name, &mut facts);
        }
    }
    if let Some(action) = imports.drf_action.as_deref() {
        for decorator in collect_bare_decorator_calls(context, action) {
            let verbs = methods_keyword(&decorator.args);
            let verbs = if verbs.is_empty() {
                vec!["GET".to_string()]
            } else {
                verbs
            };
            let detail = keyword_value_start(&decorator.args, "detail")
                .is_some_and(|start| decorator.args[start..].starts_with("True"));
            let url_path = keyword_string_arg(&decorator.args, "url_path")
                .unwrap_or_else(|| decorator.function_name.clone());
            let Some(mut fact) = drf_fact(
                context,
                decorator.start,
                decorator.end,
                DRF_VIEWSET_ACTION_PATTERN_ID,
                "viewset_action",
                verbs,
            ) else {
                continue;
            };
            let metadata = fact.metadata.get_or_insert_with(HashMap::new);
            metadata.insert("detail".to_string(), serde_json::Value::Bool(detail));
            insert_string(metadata, "url_path", &url_path);
            if let Some(url_name) = keyword_string_arg(&decorator.args, "url_name") {
                insert_string(metadata, "url_name", &url_name);
            }
            facts.push(fact);
        }
    }
    if let Some(api_view) = imports.drf_api_view.as_deref() {
        for decorator in collect_bare_decorator_calls(context, api_view) {
            let verbs = methods_list_arg(&decorator.args)
                .filter(|verbs| !verbs.is_empty())
                .unwrap_or_else(|| vec!["GET".to_string()]);
            if let Some(fact) = drf_fact(
                context,
                decorator.start,
                decorator.end,
                DRF_API_VIEW_PATTERN_ID,
                "api_view",
                verbs,
            ) {
                facts.push(fact);
            }
        }
    }
    facts
}

fn collect_drf_registrations(
    context: &PythonFactContext<'_>,
    router: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let content = context.content;
    let needle = format!("{router}.register");
    let mut cursor = 0;
    while let Some(relative) = content[cursor..].find(&needle) {
        let call_start = cursor + relative;
        cursor = call_start + needle.len();
        if !is_identifier_boundary(content, call_start, router.len())
            || context.mask.is_string_or_comment(call_start)
        {
            continue;
        }
        let open = skip_ascii_whitespace_until(content, cursor, content.len());
        if content.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let Some(close) = find_matching_paren(content, context.mask, open) else {
            continue;
        };
        let args = &content[open + 1..close];
        let Some(prefix) = positional_string_arg(args, 0) else {
            continue;
        };
        let args_mask = SourceMask::new(args, MaskLanguage::Python);
        let first_end = find_top_level_comma_or_end(args, &args_mask, 0, args.len());
        let viewset_start = skip_ascii_whitespace_until(args, first_end + 1, args.len());
        let viewset_end = find_top_level_comma_or_end(args, &args_mask, viewset_start, args.len());
        let viewset = args.get(viewset_start..viewset_end).unwrap_or("").trim();
        if viewset.is_empty() || viewset.contains('=') {
            continue;
        }
        let Some(node) =
            smallest_node_covering_range(context.tree.root_node(), call_start, close + 1)
        else {
            continue;
        };
        if is_comment_or_string_node(node.kind()) {
            continue;
        }
        let Some(span) = NormalizedSpan::from_content_range(content, call_start, close + 1) else {
            continue;
        };
        let mut metadata = base_metadata("framework", "django_rest_framework");
        insert_string(&mut metadata, "api_style", "resource_routing");
        insert_string(&mut metadata, "resource_name", &prefix);
        insert_string(&mut metadata, "viewset", viewset);
        insert_string(&mut metadata, "router", router);
        if let Some(basename) = keyword_string_arg(args, "basename") {
            insert_string(&mut metadata, "basename", &basename);
        }
        facts.push(fact_for_span(
            context.file_path,
            context.language,
            DRF_ROUTER_REGISTRATION_PATTERN_ID,
            "router_registration",
            node.kind(),
            span,
            metadata,
        ));
    }
}

fn drf_fact(
    context: &PythonFactContext<'_>,
    start: usize,
    end: usize,
    pattern_id: &str,
    api_style: &str,
    verbs: Vec<String>,
) -> Option<StructuralFact> {
    let node = smallest_node_covering_range(context.tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(context.content, start, end)?;
    let mut metadata = base_metadata("framework", "django_rest_framework");
    insert_string(&mut metadata, "api_style", api_style);
    insert_string_array(&mut metadata, "verbs", verbs);
    Some(fact_for_span(
        context.file_path,
        context.language,
        pattern_id,
        api_style,
        node.kind(),
        span,
        metadata,
    ))
}

/// The uppercase verbs of a leading `["GET", "POST"]` list argument.
fn methods_list_arg(args: &str) -> Option<Vec<String>> {
    let start = skip_ascii_whitespace_until(args, 0, args.len());
    if args.as_bytes().get(start) != Some(&b'[') {
        return keyword_value_start(args, "http_method_names")
            .map(|_| methods_keyword_named(args, "http_method_names"));
    }
    Some(methods_keyword_named(
        &format!("methods={}", &args[start..]),
        "methods",
    ))
}

struct BareDecoratorCall {
    start: usize,
    end: usize,
    args: String,
    function_name: String,
}

/// `@name(...)` decorators on their own line, with the decorated function's
/// `def` line as the fact range.
fn collect_bare_decorator_calls(
    context: &PythonFactContext<'_>,
    name: &str,
) -> Vec<BareDecoratorCall> {
    let content = context.content;
    let mut calls = Vec::new();
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let start = offset + line.len() - trimmed.len();
        offset += line.len();
        let Some(after_at) = trimmed.strip_prefix('@') else {
            continue;
        };
        let Some(rest) = after_at.strip_prefix(name) else {
            continue;
        };
        if !rest.trim_start().starts_with('(') || context.mask.is_string_or_comment(start) {
            continue;
        }
        let open = start + 1 + name.len() + (rest.len() - rest.trim_start().len());
        let Some(close) = find_matching_paren(content, context.mask, open) else {
            continue;
        };
        let Some((def_start, def_end)) = next_def_line_range(content, close + 1) else {
            continue;
        };
        let function_name = content[def_start + "def ".len()..def_end]
            .split(['(', '[', ':'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !is_ascii_identifier(&function_name) {
            continue;
        }
        calls.push(BareDecoratorCall {
            start: def_start,
            end: def_end,
            args: content[open + 1..close].to_string(),
            function_name,
        });
    }
    calls
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

/// `app.add_url_rule("/ping", view_func=ping, methods=[...])` registers a
/// route without a decorator. The view is the `view_func` keyword or the third
/// positional argument.
fn collect_flask_url_rules(
    context: &PythonFactContext<'_>,
    receivers: &HashMap<String, FlaskReceiver>,
) -> Vec<StructuralFact> {
    let content = context.content;
    let mut facts = Vec::new();
    for (name, receiver) in receivers {
        let needle = format!("{name}.add_url_rule");
        let mut cursor = 0;
        while let Some(relative) = content[cursor..].find(&needle) {
            let call_start = cursor + relative;
            cursor = call_start + needle.len();
            if context.mask.is_string_or_comment(call_start)
                || !is_identifier_boundary(content, call_start, name.len())
            {
                continue;
            }
            let open = skip_ascii_whitespace_until(content, cursor, content.len());
            if content.as_bytes().get(open) != Some(&b'(') {
                continue;
            }
            let Some(close) = find_matching_paren(content, context.mask, open) else {
                continue;
            };
            let args = &content[open + 1..close];
            let Some(route_template) =
                positional_string_arg(args, 0).or_else(|| keyword_string_arg(args, "rule"))
            else {
                continue;
            };
            let view_target = keyword_value_start(args, "view_func")
                .map(|start| {
                    let args_mask = SourceMask::new(args, MaskLanguage::Python);
                    let end = find_top_level_comma_or_end(args, &args_mask, start, args.len());
                    args[start..end].trim().to_string()
                })
                .or_else(|| positional_raw_arg(args, 2));
            let methods = methods_keyword(args);
            let verb_source = if methods.is_empty() {
                "default"
            } else {
                "attested"
            };
            let verbs = if methods.is_empty() {
                vec!["GET".to_string()]
            } else {
                methods
            };
            for verb in verbs {
                if let Some(fact) = route_fact(
                    context.language,
                    context.tree,
                    context.file_path,
                    content,
                    call_start,
                    close + 1,
                    RouteFactSpec {
                        framework: "flask",
                        pattern_id: FLASK_ROUTE_PATTERN_ID,
                        capture_name: "route",
                        api_style: "call_routing",
                        route_template: &route_template,
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
                        if let Some(view_target) = view_target.as_deref() {
                            insert_string(metadata, "view_target", view_target);
                        }
                    },
                ) {
                    facts.push(fact);
                }
            }
        }
    }
    facts.sort_by_key(|fact| (fact.start_byte, fact.end_byte));
    facts
}

fn positional_raw_arg(args: &str, index: usize) -> Option<String> {
    let args_mask = SourceMask::new(args, MaskLanguage::Python);
    let mut cursor = 0;
    for current in 0..=index {
        cursor = skip_ascii_whitespace_until(args, cursor, args.len());
        if cursor >= args.len() {
            return None;
        }
        let end = find_top_level_comma_or_end(args, &args_mask, cursor, args.len());
        if current == index {
            let value = args[cursor..end].trim();
            return (!value.is_empty() && !value.contains('=')).then(|| value.to_string());
        }
        cursor = end.saturating_add(1);
    }
    None
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

/// A join key for a Django `re_path` regex, under a conservative policy:
/// `^`/`$` anchors drop, a trailing `/?` is an optional trailing slash, a
/// named group `(?P<id>...)` becomes `:id`, an unnamed group becomes the
/// positional `:arg1`, `:arg2`, ... (Django passes those as positional view
/// arguments), and an escaped punctuation character is literal. Anything else
/// (alternation, optional or repeated fragments, lookaround, character
/// classes outside a group, mixed named and unnamed groups) yields no key.
fn normalize_django_regex_route(pattern: &str) -> Option<(String, Vec<String>)> {
    let mut source = pattern;
    if let Some(stripped) = source.strip_prefix('^') {
        source = stripped;
    }
    if let Some(stripped) = source.strip_suffix('$') {
        source = stripped;
    }
    let optional_trailing_slash = source.ends_with("/?");
    if optional_trailing_slash {
        source = &source[..source.len() - 1];
    }

    let bytes = source.as_bytes();
    let mut cursor = 0usize;
    let mut template = String::new();
    let mut dynamic_segments = Vec::new();
    let mut named_groups = false;
    let mut positional_groups = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor] == b'(' {
            let group_end = regex_group_end(source, cursor)?;
            let name = if let Some(rest) = source[cursor..].strip_prefix("(?P<") {
                let name_end = rest.find('>')?;
                let name = &rest[..name_end];
                if !is_ascii_identifier(name) {
                    return None;
                }
                named_groups = true;
                name.to_string()
            } else {
                let group = &source[cursor + 1..group_end];
                if group.starts_with('?') || group.contains(['|', '(']) {
                    return None;
                }
                positional_groups += 1;
                format!("arg{positional_groups}")
            };
            if matches!(bytes.get(group_end + 1), Some(b'?' | b'*' | b'+' | b'{')) {
                return None;
            }
            template.push(':');
            template.push_str(&name);
            dynamic_segments.push(name);
            cursor = group_end + 1;
            continue;
        }

        let byte = bytes[cursor];
        if byte == b'\\' {
            let escaped = *bytes.get(cursor + 1)?;
            if escaped.is_ascii_alphanumeric() {
                return None;
            }
            template.push(escaped as char);
            cursor += 2;
            continue;
        }
        if matches!(
            byte,
            b')' | b'[' | b']' | b'{' | b'}' | b'+' | b'*' | b'?' | b'|'
        ) {
            return None;
        }
        template.push(byte as char);
        cursor += 1;
    }
    if named_groups && positional_groups > 0 {
        return None;
    }
    if optional_trailing_slash && !template.ends_with('/') {
        template.push('/');
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
    insert_string(
        &mut metadata,
        "included_module",
        &included_module(include_expr),
    );
    if let Some(namespace) = keyword_string_arg(trailing_args, "namespace")
        .or_else(|| keyword_string_arg(include_expr, "namespace"))
    {
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

/// The module an `include(...)` mounts: the string literal argument, the
/// first element of a `(module, app_name)` tuple, or else the source text of
/// the argument (`router.urls`).
fn included_module(include_expr: &str) -> String {
    let Some(open) = include_expr.find('(') else {
        return include_expr.to_string();
    };
    let close = include_expr.rfind(')').unwrap_or(include_expr.len());
    let mask = SourceMask::new(include_expr, MaskLanguage::Python);
    let mut arg_start = skip_ascii_whitespace_until(include_expr, open + 1, close);
    let arg_end = find_top_level_comma_or_end(include_expr, &mask, arg_start, close);
    if include_expr.as_bytes().get(arg_start) == Some(&b'(') {
        arg_start = skip_ascii_whitespace_until(include_expr, arg_start + 1, arg_end);
    }
    if let Some((module, _)) = parse_python_string_literal(include_expr, arg_start) {
        return module;
    }
    include_expr[arg_start..arg_end].trim().to_string()
}

fn methods_keyword(args: &str) -> Vec<String> {
    methods_keyword_named(args, "methods")
}

fn methods_keyword_named(args: &str, key: &str) -> Vec<String> {
    let Some(value_start) = keyword_value_start(args, key) else {
        return Vec::new();
    };
    let args_mask = SourceMask::new(args, MaskLanguage::Python);
    let end = match args.as_bytes().get(value_start) {
        Some(b'[') => find_matching_bracket_within(args, &args_mask, value_start, args.len()),
        Some(b'(') => find_matching_paren_within(args, &args_mask, value_start, args.len()),
        Some(b'{') => find_matching_brace_within(args, &args_mask, value_start, args.len()),
        _ => None,
    };
    let Some(end) = end else {
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
