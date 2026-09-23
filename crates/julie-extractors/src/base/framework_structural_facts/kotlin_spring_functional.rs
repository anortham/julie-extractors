//! Kotlin Spring functional routing facts (`spring.functional_route.v1`).
//!
//! The `coRouter { }` (WebFlux) and `router { }` (WebMvc / WebFlux) DSLs
//! register routes with bare verb calls such as `GET("/x", handler::get)` or
//! `GET("/x") { … }`. A `"/prefix".nest { }` or `path("/prefix").nest { }`
//! block adds a lexical prefix; any other `.nest { }` (for example
//! `accept(JSON).nest { }`) keeps the prefix unchanged. A route or prefix that
//! is not a static string literal silences the routes it governs. Import-gated
//! on the Spring functional server packages.

use tree_sitter::{Node, Tree};

use super::SPRING_FUNCTIONAL_ROUTE_PATTERN_ID;
use super::helpers::{child_of_kind, insert_string, node_text};
use super::scan::{RouteFactSpec, route_fact};
use super::static_arg::{StaticArgLang, static_route_arg};
use crate::base::http_boundary::{ParamFlavor, join_route_templates};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const IMPORT_NEEDLES: &[&str] = &[
    "org.springframework.web.reactive.function.server",
    "org.springframework.web.servlet.function",
];

const ROUTER_BUILDERS: &[&str] = &["coRouter", "router"];

const VERBS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

struct Ctx<'a> {
    language: &'a str,
    tree: &'a Tree,
    file_path: &'a str,
    content: &'a str,
}

enum Scope {
    Outside,
    Router {
        builder: &'static str,
        prefix: Option<String>,
    },
}

pub(super) fn collect_kotlin_spring_functional_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !IMPORT_NEEDLES.iter().any(|needle| content.contains(needle)) {
        return Vec::new();
    }
    let ctx = Ctx {
        language,
        tree,
        file_path,
        content,
    };
    let mut facts = Vec::new();
    walk(&ctx, tree.root_node(), &Scope::Outside, 0, &mut facts);
    facts
}

fn walk(ctx: &Ctx, node: Node, scope: &Scope, depth: u32, facts: &mut Vec<StructuralFact>) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    if node.kind() == "call_expression" {
        if let Some(builder) = router_builder(node, ctx.content) {
            let inner = Scope::Router {
                builder,
                prefix: None,
            };
            walk_children(ctx, node, &inner, child_depth, facts);
            return;
        }
        if let &Scope::Router {
            builder,
            ref prefix,
        } = scope
        {
            if let Some(nest) = nest_prefix(node, ctx.content) {
                let Some(nested) = nest else {
                    return;
                };
                let joined = match prefix {
                    Some(prefix) => join_route_templates(prefix, nested),
                    None => nested.to_string(),
                };
                let inner = Scope::Router {
                    builder,
                    prefix: Some(joined),
                };
                walk_children(ctx, node, &inner, child_depth, facts);
                return;
            }
            if let Some((verb, template)) = verb_route(node, ctx.content) {
                facts.extend(verb_fact(
                    ctx,
                    node,
                    builder,
                    prefix.as_deref(),
                    verb,
                    template,
                ));
                return;
            }
        }
    }
    walk_children(ctx, node, scope, child_depth, facts);
}

fn walk_children(
    ctx: &Ctx,
    node: Node,
    scope: &Scope,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(ctx, child, scope, depth, facts);
    }
}

fn verb_fact(
    ctx: &Ctx,
    node: Node,
    builder: &'static str,
    prefix: Option<&str>,
    verb: &'static str,
    template: &str,
) -> Option<StructuralFact> {
    let spec = RouteFactSpec {
        framework: "spring",
        pattern_id: SPRING_FUNCTIONAL_ROUTE_PATTERN_ID,
        capture_name: "route",
        api_style: "functional_routing",
        route_template: template,
        verb: Some(verb),
        verb_source: Some("attested"),
        flavor: ParamFlavor::Braces,
        prefix,
        prefix_key: Some("nest_route_template"),
    };
    route_fact(
        ctx.language,
        ctx.tree,
        ctx.file_path,
        ctx.content,
        node.start_byte(),
        node.end_byte(),
        spec,
        |metadata| insert_string(metadata, "router", builder),
    )
}

/// `coRouter { }` / `router { }` with a bare builder callee and a trailing lambda.
fn router_builder(node: Node, content: &str) -> Option<&'static str> {
    child_of_kind(node, "annotated_lambda")?;
    if child_of_kind(node, "value_arguments").is_some() {
        return None;
    }
    let name = bare_callee(node, content)?;
    ROUTER_BUILDERS
        .iter()
        .find(|builder| **builder == name)
        .copied()
}

/// `Some(Some(prefix))` for a static `"/p".nest { }` or `path("/p").nest { }`,
/// `Some(None)` for a dynamic prefix (its routes stay silent), and `None` when
/// the call is not a prefix-bearing nest. `accept(…).nest { }` is not a prefix.
fn nest_prefix<'a>(node: Node, content: &'a str) -> Option<Option<&'a str>> {
    child_of_kind(node, "annotated_lambda")?;
    let navigation = child_of_kind(node, "navigation_expression")?;
    let mut cursor = navigation.walk();
    let mut parts = navigation.named_children(&mut cursor);
    let receiver = parts.next()?;
    let member = parts.next()?;
    if node_text(content, member)? != "nest" {
        return None;
    }
    match receiver.kind() {
        "string_literal" | "multiline_string_literal" => {
            Some(static_route_arg(receiver, content, StaticArgLang::Kotlin))
        }
        "call_expression" if bare_callee(receiver, content) == Some("path") => Some(
            first_argument(receiver)
                .and_then(|arg| static_route_arg(arg, content, StaticArgLang::Kotlin)),
        ),
        _ => None,
    }
}

/// `(VERB, template)` for `GET("/x", handler)` or the curried `GET("/x") { … }`.
fn verb_route<'a>(node: Node, content: &'a str) -> Option<(&'static str, &'a str)> {
    let head = child_of_kind(node, "call_expression")
        .filter(|_| child_of_kind(node, "annotated_lambda").is_some())
        .unwrap_or(node);
    let name = bare_callee(head, content)?;
    let verb = VERBS.iter().find(|verb| **verb == name).copied()?;
    let template = static_route_arg(first_argument(head)?, content, StaticArgLang::Kotlin)?;
    Some((verb, template))
}

fn bare_callee<'a>(call: Node, content: &'a str) -> Option<&'a str> {
    let mut cursor = call.walk();
    let first = call.named_children(&mut cursor).next()?;
    (first.kind() == "identifier")
        .then(|| node_text(content, first))
        .flatten()
}

fn first_argument(call: Node) -> Option<Node> {
    let args = child_of_kind(call, "value_arguments")?;
    let mut cursor = args.walk();
    let argument = args
        .named_children(&mut cursor)
        .find(|child| child.kind() == "value_argument")?;
    let mut arg_cursor = argument.walk();
    argument.named_children(&mut arg_cursor).next()
}
