//! Zig structural facts: the `build.zig` build graph and http.zig routes.
//!
//! `build.zig` is every Zig project's manifest. Its artifacts
//! (`b.addExecutable(.{ .name = "app", .root_source_file = b.path(..) })`),
//! package dependencies (`b.dependency("httpz", ..)`), modules
//! (`b.addModule("name", ..)`, `x.addImport("name", ..)`), and named steps
//! (`b.step("run", "..")`) are calls with static string or anonymous-struct
//! arguments, read only in a file named `build.zig`.
//!
//! http.zig (`@import("httpz")`) registers routes with
//! `router.get("/users/:id", handler, .{})` on a router from
//! `server.router(..)` or a `router.group("/prefix", ..)` of one.

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::helpers::{base_metadata, fact_for_span, insert_string, node_text};
use super::scan::{RouteFactSpec, route_fact};
use super::{
    HTTPZ_ROUTE_PATTERN_ID, ZIG_BUILD_ARTIFACT_PATTERN_ID, ZIG_BUILD_DEPENDENCY_PATTERN_ID,
    ZIG_BUILD_MODULE_IMPORT_PATTERN_ID, ZIG_BUILD_MODULE_PATTERN_ID, ZIG_BUILD_STEP_PATTERN_ID,
};
use crate::base::http_boundary::ParamFlavor;
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) fn collect_zig_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let is_build_script = file_path.rsplit(['/', '\\']).next() == Some("build.zig");
    let routers = if content.contains("@import(\"httpz\")") {
        let mut routers = HashMap::new();
        collect_routers(tree.root_node(), content, 0, &mut routers);
        routers
    } else {
        HashMap::new()
    };
    if !is_build_script && routers.is_empty() {
        return Vec::new();
    }
    let context = Context {
        language,
        tree,
        file_path,
        content,
        is_build_script,
        routers,
    };
    let mut facts = Vec::new();
    walk(tree.root_node(), &context, 0, &mut facts);
    facts
}

struct Context<'a> {
    language: &'a str,
    tree: &'a Tree,
    file_path: &'a str,
    content: &'a str,
    is_build_script: bool,
    routers: HashMap<String, Option<String>>,
}

fn walk(node: Node, context: &Context, depth: u32, facts: &mut Vec<StructuralFact>) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "call_expression"
        && let Some((receiver, method)) = method_call(node, context.content)
    {
        if context.is_build_script {
            build_fact(node, method, context, facts);
        }
        if !context.routers.is_empty() {
            route(node, receiver, method, context, facts);
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.children(&mut node.walk()) {
        walk(child, context, child_depth, facts);
    }
}

fn build_fact(call: Node, method: &str, context: &Context, facts: &mut Vec<StructuralFact>) {
    let content = context.content;
    let args = arguments(call);
    let (pattern_id, capture, metadata) = match method {
        "addExecutable" | "addLibrary" | "addStaticLibrary" | "addSharedLibrary" | "addObject"
        | "addTest" => {
            let Some(options) = args.first().filter(|arg| is_struct_literal(**arg)) else {
                return;
            };
            let mut metadata = base_metadata("build", "zig.build");
            insert_string(&mut metadata, "artifact_kind", artifact_kind(method));
            if let Some(name) =
                struct_field(*options, "name", content).and_then(|name| string_value(name, content))
            {
                insert_string(&mut metadata, "artifact_name", name);
            }
            if let Some(root) = root_source_file(*options, content) {
                insert_string(&mut metadata, "root_source_file", root);
            }
            (ZIG_BUILD_ARTIFACT_PATTERN_ID, "artifact", metadata)
        }
        "dependency" => {
            let Some(name) = args.first().and_then(|arg| string_value(*arg, content)) else {
                return;
            };
            let mut metadata = base_metadata("build", "zig.build");
            insert_string(&mut metadata, "dependency_name", name);
            (ZIG_BUILD_DEPENDENCY_PATTERN_ID, "dependency", metadata)
        }
        "addModule" | "createModule" => {
            let (name, options) = match method {
                "addModule" => (
                    args.first().and_then(|arg| string_value(*arg, content)),
                    args.get(1),
                ),
                _ => (None, args.first()),
            };
            let Some(options) = options.filter(|arg| is_struct_literal(**arg)) else {
                return;
            };
            let Some(root) = root_source_file(*options, content) else {
                return;
            };
            let mut metadata = base_metadata("build", "zig.build");
            if let Some(name) = name {
                insert_string(&mut metadata, "module_name", name);
            }
            insert_string(&mut metadata, "root_source_file", root);
            (ZIG_BUILD_MODULE_PATTERN_ID, "module", metadata)
        }
        "addImport" | "addAnonymousImport" => {
            let Some(name) = args.first().and_then(|arg| string_value(*arg, content)) else {
                return;
            };
            let mut metadata = base_metadata("build", "zig.build");
            insert_string(&mut metadata, "import_name", name);
            if let Some(source) = args.get(1).and_then(|arg| module_source(*arg, content)) {
                insert_string(&mut metadata, "module_source", &source);
            }
            (
                ZIG_BUILD_MODULE_IMPORT_PATTERN_ID,
                "module_import",
                metadata,
            )
        }
        "step" => {
            let (Some(name), Some(description)) = (
                args.first().and_then(|arg| string_value(*arg, content)),
                args.get(1).and_then(|arg| string_value(*arg, content)),
            ) else {
                return;
            };
            let mut metadata = base_metadata("build", "zig.build");
            insert_string(&mut metadata, "step_name", name);
            insert_string(&mut metadata, "step_description", description);
            (ZIG_BUILD_STEP_PATTERN_ID, "step", metadata)
        }
        _ => return,
    };
    let Some(span) =
        NormalizedSpan::from_content_range(content, call.start_byte(), call.end_byte())
    else {
        return;
    };
    facts.push(fact_for_span(
        context.file_path,
        context.language,
        pattern_id,
        capture,
        call.kind(),
        span,
        metadata,
    ));
}

fn artifact_kind(method: &str) -> &'static str {
    match method {
        "addExecutable" => "executable",
        "addLibrary" => "library",
        "addStaticLibrary" => "static_library",
        "addSharedLibrary" => "shared_library",
        "addObject" => "object",
        _ => "test",
    }
}

/// `.root_source_file = b.path("src/main.zig")` (or a plain string), directly
/// or inside `.root_module = b.createModule(.{ .. })`.
fn root_source_file<'a>(options: Node, content: &'a str) -> Option<&'a str> {
    if let Some(root) = struct_field(options, "root_source_file", content) {
        return path_value(root, content);
    }
    let module = struct_field(options, "root_module", content)?;
    let (_, method) = method_call(module, content)?;
    if method != "createModule" {
        return None;
    }
    let module_options = arguments(module).into_iter().next()?;
    path_value(
        struct_field(module_options, "root_source_file", content)?,
        content,
    )
}

fn path_value<'a>(node: Node, content: &'a str) -> Option<&'a str> {
    if let Some(value) = string_value(node, content) {
        return Some(value);
    }
    let (_, method) = method_call(node, content)?;
    if method != "path" {
        return None;
    }
    string_value(*arguments(node).first()?, content)
}

/// `dep.module("httpz")` names its dependency and module; any other value is
/// kept as its source text.
fn module_source(node: Node, content: &str) -> Option<String> {
    let text = node_text(content, node)?;
    Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Router receivers: names bound to `server.router(..)` (no prefix) or to
/// `router.group("/p", ..)` of a known router (its joined prefix).
fn collect_routers(
    node: Node,
    content: &str,
    depth: u32,
    routers: &mut HashMap<String, Option<String>>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "variable_declaration"
        && let Some(name) = node
            .children(&mut node.walk())
            .find(|child| child.kind() == "identifier")
            .and_then(|name| node_text(content, name))
        && let Some(value) = initializer(node).map(unwrap_try)
        && let Some(prefix) = router_value(value, content, routers)
    {
        routers.insert(name.to_string(), prefix);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.children(&mut node.walk()) {
        collect_routers(child, content, child_depth, routers);
    }
}

/// `Some(prefix)` when `value` evaluates to a router.
fn router_value(
    value: Node,
    content: &str,
    routers: &HashMap<String, Option<String>>,
) -> Option<Option<String>> {
    let (receiver, method) = method_call(value, content)?;
    match method {
        "router" => Some(None),
        "group" => {
            let outer = receiver_router(receiver, content, routers)?;
            let prefix = string_value(*arguments(value).first()?, content)?;
            Some(Some(join_prefix(outer.as_deref(), prefix)))
        }
        _ => None,
    }
}

fn receiver_router(
    receiver: Node,
    content: &str,
    routers: &HashMap<String, Option<String>>,
) -> Option<Option<String>> {
    match receiver.kind() {
        "identifier" => routers.get(node_text(content, receiver)?).cloned(),
        _ => router_value(unwrap_try(receiver), content, routers),
    }
}

fn join_prefix(outer: Option<&str>, prefix: &str) -> String {
    match outer {
        Some(outer) => format!(
            "{}/{}",
            outer.trim_end_matches('/'),
            prefix.trim_start_matches('/')
        ),
        None => prefix.to_string(),
    }
}

fn route(
    call: Node,
    receiver: Node,
    method: &str,
    context: &Context,
    facts: &mut Vec<StructuralFact>,
) {
    let content = context.content;
    let verb = match method {
        "get" => Some("GET"),
        "post" => Some("POST"),
        "put" => Some("PUT"),
        "delete" => Some("DELETE"),
        "patch" => Some("PATCH"),
        "head" => Some("HEAD"),
        "options" => Some("OPTIONS"),
        "all" => None,
        _ => return,
    };
    let Some(prefix) = receiver_router(receiver, content, &context.routers) else {
        return;
    };
    let args = arguments(call);
    let (Some(path), Some(handler)) = (
        args.first().and_then(|arg| string_value(*arg, content)),
        args.get(1)
            .filter(|handler| matches!(handler.kind(), "identifier" | "field_expression"))
            .and_then(|handler| node_text(content, *handler)),
    ) else {
        return;
    };
    let spec = RouteFactSpec {
        framework: "httpz",
        pattern_id: HTTPZ_ROUTE_PATTERN_ID,
        capture_name: "route_call",
        api_style: "call_routing",
        route_template: path,
        verb,
        verb_source: verb.map(|_| "attested"),
        flavor: ParamFlavor::Colon,
        prefix: prefix.as_deref(),
        prefix_key: Some("route_group_prefix"),
    };
    if let Some(fact) = route_fact(
        context.language,
        context.tree,
        context.file_path,
        content,
        call.start_byte(),
        call.end_byte(),
        spec,
        |metadata| {
            metadata.insert(
                "handler_name".to_string(),
                Value::String(handler.to_string()),
            );
        },
    ) {
        facts.push(fact);
    }
}

pub(super) fn method_call<'t, 'a>(call: Node<'t>, content: &'a str) -> Option<(Node<'t>, &'a str)> {
    if call.kind() != "call_expression" {
        return None;
    }
    let function = call.child_by_field_name("function")?;
    if function.kind() != "field_expression" {
        return None;
    }
    let receiver = function.child_by_field_name("object")?;
    let member = node_text(content, function.child_by_field_name("member")?)?;
    Some((receiver, member))
}

pub(super) fn arguments(call: Node) -> Vec<Node> {
    let function = call.child_by_field_name("function").map(|f| f.id());
    call.named_children(&mut call.walk())
        .filter(|child| Some(child.id()) != function)
        .collect()
}

pub(super) fn string_value<'a>(node: Node, content: &'a str) -> Option<&'a str> {
    if node.kind() != "string" {
        return None;
    }
    let parts: Vec<Node> = node.named_children(&mut node.walk()).collect();
    match parts.as_slice() {
        [body] if body.kind() == "string_content" => node_text(content, *body),
        _ => None,
    }
}

fn is_struct_literal(node: Node) -> bool {
    node.kind() == "anonymous_struct_initializer"
}

/// The value assigned to `.name` in an anonymous struct literal `.{ .name = v }`.
pub(super) fn struct_field<'t>(literal: Node<'t>, name: &str, content: &str) -> Option<Node<'t>> {
    let list = literal
        .named_children(&mut literal.walk())
        .find(|child| child.kind() == "initializer_list")?;
    list.named_children(&mut list.walk())
        .filter(|entry| entry.kind() == "assignment_expression")
        .find(|entry| {
            entry
                .child_by_field_name("left")
                .and_then(|left| left.child_by_field_name("member"))
                .and_then(|member| node_text(content, member))
                == Some(name)
        })
        .and_then(|entry| entry.child_by_field_name("right"))
}

pub(super) fn initializer(declaration: Node) -> Option<Node> {
    let children: Vec<Node> = declaration.children(&mut declaration.walk()).collect();
    let equals = children.iter().position(|child| child.kind() == "=")?;
    children[equals + 1..]
        .iter()
        .copied()
        .find(|child| child.is_named())
}

pub(super) fn unwrap_try(node: Node) -> Node {
    if node.kind() == "try_expression" {
        node.named_child(0).unwrap_or(node)
    } else {
        node
    }
}
