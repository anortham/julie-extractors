//! go-chi/chi, gorilla/mux, and gofiber/fiber route facts (`chi.route.v1`,
//! `chi.mount.v1`, `gorilla_mux.route.v1`, `fiber.route.v1`).
//!
//! A route registers on a receiver traced to the framework: a variable bound
//! to the router constructor, a parameter typed with a router type, or a
//! derived router that carries a same-file literal prefix (a chi
//! `r.Route("/admin", func(r chi.Router) {..})` closure parameter, a fiber
//! `app.Group("/api")`, a gorilla `r.PathPrefix("/api").Subrouter()`). A
//! non-literal path or prefix stays silent (M2).

use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use super::helpers::{base_metadata, fact_for_span, insert_string};
use super::scan::{RouteFactSpec, route_fact};
use super::{
    CHI_MOUNT_PATTERN_ID, CHI_ROUTE_PATTERN_ID, FIBER_ROUTE_PATTERN_ID,
    GORILLA_MUX_ROUTE_PATTERN_ID,
};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Framework {
    Chi,
    Gorilla,
    Fiber,
}

/// A receiver traced to a framework router, with its composed literal prefix.
#[derive(Clone)]
struct Router {
    framework: Framework,
    prefix: Option<String>,
}

struct Context<'a, 't> {
    content: &'a str,
    aliases: HashMap<Framework, String>,
    assignments: Vec<(String, Node<'t>)>,
}

pub(super) fn collect_go_router_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let aliases = router_imports(content);
    if aliases.is_empty() {
        return Vec::new();
    }
    let mut context = Context {
        content,
        aliases,
        assignments: Vec::new(),
    };
    collect_assignments(tree.root_node(), &mut context, 0);
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        &context,
        language,
        tree,
        file_path,
        0,
        &mut facts,
    );
    facts
}

fn router_imports(content: &str) -> HashMap<Framework, String> {
    let mut aliases = HashMap::new();
    for line in content.lines() {
        let line = line.trim().trim_start_matches("import ").trim();
        let Some(open) = line.find('"') else {
            continue;
        };
        let Some(close) = line[open + 1..].find('"').map(|index| index + open + 1) else {
            continue;
        };
        let path = &line[open + 1..close];
        let alias = line[..open].trim();
        let versionless = strip_major_version(path);
        let (framework, default) = match versionless {
            "github.com/go-chi/chi" => (Framework::Chi, "chi"),
            "github.com/gorilla/mux" => (Framework::Gorilla, "mux"),
            "github.com/gofiber/fiber" => (Framework::Fiber, "fiber"),
            _ => continue,
        };
        if alias == "_" || alias == "." {
            continue;
        }
        let alias = if alias.is_empty() { default } else { alias };
        aliases.insert(framework, alias.to_string());
    }
    aliases
}

fn strip_major_version(path: &str) -> &str {
    match path.rsplit_once("/v") {
        Some((head, version))
            if !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit()) =>
        {
            head
        }
        _ => path,
    }
}

fn collect_assignments<'t>(node: Node<'t>, context: &mut Context<'_, 't>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let (names, right): (Vec<Node>, Option<Node>) = match node.kind() {
        "short_var_declaration" | "assignment_statement" => (
            node.child_by_field_name("left")
                .map(|left| left.named_children(&mut left.walk()).collect())
                .unwrap_or_default(),
            node.child_by_field_name("right"),
        ),
        "var_spec" => (
            node.children_by_field_name("name", &mut node.walk())
                .collect(),
            node.child_by_field_name("value"),
        ),
        _ => (Vec::new(), None),
    };
    if let Some(right) = right {
        let values: Vec<Node> = right.named_children(&mut right.walk()).collect();
        if names.len() == values.len() {
            for (name, value) in names.into_iter().zip(values) {
                if let Some(name) = text(context.content, name) {
                    context.assignments.push((name.to_string(), value));
                }
            }
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.children(&mut node.walk()) {
        collect_assignments(child, context, child_depth);
    }
}

fn walk(
    node: Node,
    context: &Context,
    language: &str,
    tree: &Tree,
    file_path: &str,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "call_expression" {
        registration(node, context, language, tree, file_path, facts);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.children(&mut node.walk()) {
        walk(
            child,
            context,
            language,
            tree,
            file_path,
            child_depth,
            facts,
        );
    }
}

fn registration(
    call: Node,
    context: &Context,
    language: &str,
    tree: &Tree,
    file_path: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let Some((receiver, method)) = selector_call(call, context.content) else {
        return;
    };
    let Some(router) = resolve_receiver(receiver, context, 0) else {
        return;
    };
    let args = arguments(call);
    let path_at = |index: usize| {
        args.get(index)
            .and_then(|arg| static_string(*arg, context.content))
    };
    let mut emit = |pattern_id: &str, framework: &str, flavor, path: &str, verb: Option<&str>| {
        if let Some(fact) = route_fact(
            language,
            tree,
            file_path,
            context.content,
            call.start_byte(),
            call.end_byte(),
            RouteFactSpec {
                framework,
                pattern_id,
                capture_name: "route_call",
                api_style: "call_routing",
                route_template: path,
                verb,
                verb_source: verb.map(|_| "attested"),
                flavor,
                prefix: router.prefix.as_deref(),
                prefix_key: Some("route_group_prefix"),
            },
            |_| {},
        ) {
            facts.push(fact);
        }
    };
    match router.framework {
        Framework::Chi => {
            let chi_route = match method {
                "Handle" | "HandleFunc" => path_at(0).map(|path| (path, None)),
                "Method" | "MethodFunc" => path_at(0)
                    .zip(path_at(1))
                    .map(|(verb, path)| (path, Some(verb.to_ascii_uppercase()))),
                _ => title_case_verb(method)
                    .and_then(|verb| path_at(0).map(|path| (path, Some(verb.to_string())))),
            };
            if let Some((path, verb)) = chi_route {
                emit(
                    CHI_ROUTE_PATTERN_ID,
                    "chi",
                    ParamFlavor::Braces,
                    &path,
                    verb.as_deref(),
                );
            } else if method == "Mount"
                && let Some(path) = path_at(0)
            {
                facts.push(mount_fact(
                    call,
                    &router,
                    &path,
                    args.get(1).copied(),
                    context.content,
                    language,
                    file_path,
                ));
            }
        }
        Framework::Gorilla => {
            if !matches!(method, "Handle" | "HandleFunc") {
                return;
            }
            let Some(path) = path_at(0) else {
                return;
            };
            let verbs = gorilla_methods(call, context);
            if verbs.is_empty() {
                emit(
                    GORILLA_MUX_ROUTE_PATTERN_ID,
                    "gorilla/mux",
                    ParamFlavor::Braces,
                    &path,
                    None,
                );
            }
            for verb in verbs {
                emit(
                    GORILLA_MUX_ROUTE_PATTERN_ID,
                    "gorilla/mux",
                    ParamFlavor::Braces,
                    &path,
                    Some(&verb),
                );
            }
        }
        Framework::Fiber => {
            let verb = match method {
                "All" => None,
                _ => match title_case_verb(method) {
                    Some(verb) => Some(verb),
                    None => return,
                },
            };
            if let Some(path) = path_at(0) {
                emit(
                    FIBER_ROUTE_PATTERN_ID,
                    "fiber",
                    ParamFlavor::Colon,
                    &path,
                    verb,
                );
            }
        }
    }
}

fn mount_fact(
    call: Node,
    router: &Router,
    path: &str,
    target: Option<Node>,
    content: &str,
    language: &str,
    file_path: &str,
) -> StructuralFact {
    let mount_path = match router.prefix.as_deref() {
        Some(prefix) => join_route_templates(prefix, path),
        None => path.to_string(),
    };
    let normalized = normalize_route_template(&mount_path, ParamFlavor::Braces);
    let mut metadata = base_metadata("framework", "chi");
    insert_string(&mut metadata, "mount_path", &mount_path);
    insert_string(&mut metadata, "normalized_mount_path", &normalized.template);
    if let Some(target) = target.and_then(|target| text(content, target)) {
        insert_string(&mut metadata, "mount_target", target);
    }
    let span = NormalizedSpan::from_node(&call);
    fact_for_span(
        file_path,
        language,
        CHI_MOUNT_PATTERN_ID,
        "mount",
        call.kind(),
        span,
        metadata,
    )
}

/// The verbs of a gorilla route's chained `.Methods("GET", http.MethodPost)`.
fn gorilla_methods(call: Node, context: &Context) -> Vec<String> {
    let mut verbs = Vec::new();
    let mut current = call;
    while let Some(selector) = current
        .parent()
        .filter(|parent| parent.kind() == "selector_expression")
    {
        let Some(outer) = selector
            .parent()
            .filter(|parent| parent.kind() == "call_expression")
        else {
            break;
        };
        let method = selector
            .child_by_field_name("field")
            .and_then(|field| text(context.content, field));
        if method == Some("Methods") {
            for argument in arguments(outer) {
                if let Some(verb) = static_string(argument, context.content) {
                    verbs.push(verb.to_ascii_uppercase());
                } else if let Some(verb) = text(context.content, argument)
                    .and_then(|constant| constant.split_once(".Method"))
                    .map(|(_, verb)| verb.to_ascii_uppercase())
                {
                    verbs.push(verb);
                }
            }
        }
        current = outer;
    }
    verbs
}

/// Trace a receiver expression to a framework router.
fn resolve_receiver(receiver: Node, context: &Context, depth: u32) -> Option<Router> {
    let child_depth = child_tree_depth(depth)?;
    match receiver.kind() {
        "identifier" => {
            let name = text(context.content, receiver)?;
            chi_route_parameter(receiver, name, context, child_depth)
                .or_else(|| assigned_router(receiver, name, context, child_depth))
                .or_else(|| typed_parameter(receiver, name, context))
        }
        "call_expression" => derived_router(receiver, context, child_depth),
        _ => None,
    }
}

/// `r.Route("/admin", func(sub chi.Router) {..})`: `sub` carries the outer
/// router's prefix joined with `/admin`; `r.Group(func(sub chi.Router) {..})`
/// keeps the outer prefix.
fn chi_route_parameter(
    use_site: Node,
    name: &str,
    context: &Context,
    depth: u32,
) -> Option<Router> {
    let mut current = use_site.parent();
    while let Some(node) = current {
        if node.kind() == "func_literal" && declares_parameter(node, name, context.content) {
            let call = node.parent()?.parent()?;
            let (outer, method) = selector_call(call, context.content)?;
            let router = resolve_receiver(outer, context, depth)?;
            if router.framework != Framework::Chi {
                return None;
            }
            return match method {
                "Route" => {
                    let prefix = static_string(*arguments(call).first()?, context.content)?;
                    Some(Router {
                        framework: Framework::Chi,
                        prefix: Some(join_prefix(router.prefix.as_deref(), &prefix)),
                    })
                }
                "Group" => Some(router),
                _ => None,
            };
        }
        current = node.parent();
    }
    None
}

fn declares_parameter(function: Node, name: &str, content: &str) -> bool {
    function
        .child_by_field_name("parameters")
        .is_some_and(|parameters| {
            parameters
                .named_children(&mut parameters.walk())
                .any(|declaration| {
                    declaration
                        .children_by_field_name("name", &mut declaration.walk())
                        .any(|param| text(content, param) == Some(name))
                })
        })
}

/// The router a variable was assigned: prefer an assignment inside the same
/// function as the use.
fn assigned_router(use_site: Node, name: &str, context: &Context, depth: u32) -> Option<Router> {
    let function = enclosing_function(use_site);
    let mut candidates: Vec<&Node> = context
        .assignments
        .iter()
        .filter(|(assigned, _)| assigned == name)
        .map(|(_, value)| value)
        .collect();
    candidates.sort_by_key(|value| enclosing_function(**value) != function);
    candidates
        .into_iter()
        .find_map(|value| router_value(*value, context, depth))
}

fn router_value(value: Node, context: &Context, depth: u32) -> Option<Router> {
    if value.kind() != "call_expression" {
        return None;
    }
    let function = value.child_by_field_name("function")?;
    if function.kind() == "selector_expression" {
        let operand = function.child_by_field_name("operand")?;
        let field = text(context.content, function.child_by_field_name("field")?)?;
        if operand.kind() == "identifier" {
            let package = text(context.content, operand)?;
            for (framework, alias) in &context.aliases {
                if alias == package && is_constructor(*framework, field) {
                    return Some(Router {
                        framework: *framework,
                        prefix: None,
                    });
                }
            }
        }
    }
    derived_router(value, context, depth)
}

fn is_constructor(framework: Framework, name: &str) -> bool {
    match framework {
        Framework::Chi => matches!(name, "NewRouter" | "NewMux"),
        Framework::Gorilla => name == "NewRouter",
        Framework::Fiber => name == "New",
    }
}

/// A router derived from another by a call: chi `With`/`Group`, fiber
/// `Group("/p")`/`Route("/p")`, gorilla `PathPrefix("/p").Subrouter()`.
fn derived_router(call: Node, context: &Context, depth: u32) -> Option<Router> {
    let (receiver, method) = selector_call(call, context.content)?;
    let child_depth = child_tree_depth(depth)?;
    match method {
        "With" => resolve_receiver(receiver, context, child_depth)
            .filter(|router| router.framework == Framework::Chi),
        "Subrouter" => {
            let (inner, inner_method) = selector_call(receiver, context.content)?;
            if inner_method != "PathPrefix" {
                return None;
            }
            let router = resolve_receiver(inner, context, child_depth)
                .filter(|router| router.framework == Framework::Gorilla)?;
            let prefix = static_string(*arguments(receiver).first()?, context.content)?;
            Some(Router {
                framework: Framework::Gorilla,
                prefix: Some(join_prefix(router.prefix.as_deref(), &prefix)),
            })
        }
        "Group" => {
            let router = resolve_receiver(receiver, context, child_depth)
                .filter(|router| router.framework == Framework::Fiber)?;
            let prefix = static_string(*arguments(call).first()?, context.content)?;
            Some(Router {
                framework: Framework::Fiber,
                prefix: Some(join_prefix(router.prefix.as_deref(), &prefix)),
            })
        }
        _ => None,
    }
}

/// A parameter of the enclosing function typed with a framework router type.
fn typed_parameter(use_site: Node, name: &str, context: &Context) -> Option<Router> {
    let mut current = use_site.parent();
    while let Some(node) = current {
        if matches!(
            node.kind(),
            "function_declaration" | "method_declaration" | "func_literal"
        ) && let Some(parameters) = node.child_by_field_name("parameters")
        {
            for declaration in parameters.named_children(&mut parameters.walk()) {
                let declares = declaration
                    .children_by_field_name("name", &mut declaration.walk())
                    .any(|param| text(context.content, param) == Some(name));
                if !declares {
                    continue;
                }
                let declared = text(context.content, declaration.child_by_field_name("type")?)?
                    .trim_start_matches('*');
                for (framework, alias) in &context.aliases {
                    let type_name = declared
                        .strip_prefix(alias.as_str())
                        .and_then(|rest| rest.strip_prefix('.'));
                    let is_router_type = type_name.is_some_and(|type_name| match framework {
                        Framework::Chi => matches!(type_name, "Router" | "Mux"),
                        Framework::Gorilla => type_name == "Router",
                        Framework::Fiber => matches!(type_name, "App" | "Router"),
                    });
                    if is_router_type {
                        return Some(Router {
                            framework: *framework,
                            prefix: None,
                        });
                    }
                }
                return None;
            }
        }
        current = node.parent();
    }
    None
}

fn enclosing_function(node: Node) -> Option<usize> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(
            parent.kind(),
            "function_declaration" | "method_declaration" | "func_literal"
        ) {
            return Some(parent.id());
        }
        current = parent.parent();
    }
    None
}

fn join_prefix(outer: Option<&str>, prefix: &str) -> String {
    match outer {
        Some(outer) => join_route_templates(outer, prefix),
        None => prefix.to_string(),
    }
}

fn selector_call<'t, 'a>(call: Node<'t>, content: &'a str) -> Option<(Node<'t>, &'a str)> {
    if call.kind() != "call_expression" {
        return None;
    }
    let function = call.child_by_field_name("function")?;
    if function.kind() != "selector_expression" {
        return None;
    }
    Some((
        function.child_by_field_name("operand")?,
        text(content, function.child_by_field_name("field")?)?,
    ))
}

fn arguments(call: Node) -> Vec<Node> {
    call.child_by_field_name("arguments")
        .map(|arguments| arguments.named_children(&mut arguments.walk()).collect())
        .unwrap_or_default()
}

fn title_case_verb(method: &str) -> Option<&'static str> {
    match method {
        "Get" => Some("GET"),
        "Post" => Some("POST"),
        "Put" => Some("PUT"),
        "Patch" => Some("PATCH"),
        "Delete" => Some("DELETE"),
        "Head" => Some("HEAD"),
        "Options" => Some("OPTIONS"),
        "Connect" => Some("CONNECT"),
        "Trace" => Some("TRACE"),
        _ => None,
    }
}

/// The text of a Go string literal with no escape sequences.
fn static_string(node: Node, content: &str) -> Option<String> {
    let raw = text(content, node)?;
    match node.kind() {
        "interpreted_string_literal"
            if !node
                .named_children(&mut node.walk())
                .any(|child| child.kind() == "escape_sequence") =>
        {
            raw.strip_prefix('"')?.strip_suffix('"').map(str::to_string)
        }
        "raw_string_literal" => raw.strip_prefix('`')?.strip_suffix('`').map(str::to_string),
        _ => None,
    }
}

fn text<'a>(content: &'a str, node: Node) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}
