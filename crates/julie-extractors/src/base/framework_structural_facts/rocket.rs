//! Rocket structural facts (`rocket.route.v1`, `rocket.mount.v1`).
//!
//! Rocket declares a route with an attribute macro on the handler
//! (`#[get("/hello/<name>")]`, `#[route(GET, uri = "/x")]`) and registers
//! handlers with `.mount("/api", routes![a, b])`. The mount can live in another
//! file, so a route fact carries no prefix; the mount fact records the prefix and
//! the handler names at its own site (code-kb joins them, decision 0004). Rocket
//! captures are angle brackets (`<id>`, `<path..>`), normalized to `:id`.

use tree_sitter::{Node, Tree};

use super::actix::{
    attribute_macro_name, call_arguments, following_function_item, method_call_parts,
};
use super::helpers::{
    base_metadata, child_of_kind, fact_for_span, insert_string, insert_string_array,
    is_comment_or_string_node, node_text, smallest_node_covering_range,
};
use super::scan::{RouteFactSpec, route_fact};
use super::static_arg::{StaticArgLang, static_route_arg};
use super::{ROCKET_MOUNT_PATTERN_ID, ROCKET_ROUTE_PATTERN_ID};
use crate::base::http_boundary::{ParamFlavor, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) fn collect_rocket_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let imports_rocket = content.contains("rocket::") || content.contains("crate rocket");
    if !imports_rocket || content.contains("actix_web") {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        language,
        tree,
        file_path,
        content,
        0,
        &mut facts,
    );
    facts
}

fn walk(
    node: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "attribute_item" => try_route(node, language, tree, file_path, content, facts),
        "call_expression" => try_mount(node, language, tree, file_path, content, facts),
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            child,
            language,
            tree,
            file_path,
            content,
            child_depth,
            facts,
        );
    }
}

/// One `rocket.route.v1` per route attribute, anchored on the handler fn.
fn try_route(
    attr_item: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let Some(attribute) = child_of_kind(attr_item, "attribute") else {
        return;
    };
    let Some(arguments) = attribute.child_by_field_name("arguments") else {
        return;
    };
    let tokens: Vec<Node> = arguments.named_children(&mut arguments.walk()).collect();
    let Some((verb, path_arg)) = route_verb_and_path(attribute, &tokens, content) else {
        return;
    };
    let Some(path) = static_route_arg(path_arg, content, StaticArgLang::Rust) else {
        return;
    };
    let Some(handler) = following_function_item(attr_item) else {
        return;
    };
    let spec = RouteFactSpec {
        framework: "rocket",
        pattern_id: ROCKET_ROUTE_PATTERN_ID,
        capture_name: "route",
        api_style: "attribute",
        route_template: path,
        verb: Some(verb),
        verb_source: Some("attested"),
        flavor: ParamFlavor::AngleBrackets,
        prefix: None,
        prefix_key: None,
    };
    if let Some(fact) = route_fact(
        language,
        tree,
        file_path,
        content,
        handler.start_byte(),
        handler.end_byte(),
        spec,
        |_| {},
    ) {
        facts.push(fact);
    }
}

/// `#[get("/x")]` takes its verb from the macro name and its path from the
/// first argument; `#[route(GET, uri = "/x")]` names both in its arguments.
fn route_verb_and_path<'t>(
    attribute: Node,
    tokens: &[Node<'t>],
    content: &str,
) -> Option<(&'static str, Node<'t>)> {
    let macro_name = attribute_macro_name(attribute, content)?;
    if macro_name == "route" {
        let verb = rocket_verb(&node_text(content, *tokens.first()?)?.to_ascii_lowercase())?;
        let path = tokens
            .iter()
            .skip(1)
            .find(|token| static_route_arg(**token, content, StaticArgLang::Rust).is_some())?;
        return Some((verb, *path));
    }
    Some((rocket_verb(macro_name)?, *tokens.first()?))
}

fn rocket_verb(name: &str) -> Option<&'static str> {
    match name {
        "get" => Some("GET"),
        "post" => Some("POST"),
        "put" => Some("PUT"),
        "patch" => Some("PATCH"),
        "delete" => Some("DELETE"),
        "head" => Some("HEAD"),
        "options" => Some("OPTIONS"),
        _ => None,
    }
}

/// `rocket::build().mount("/api", routes![a, b])`: the prefix and the handler
/// names the `routes!` list registers.
fn try_mount(
    call: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let Some((_, method)) = method_call_parts(call, content) else {
        return;
    };
    if method != "mount" {
        return;
    }
    let args = call_arguments(call);
    let (Some(path_arg), Some(routes_arg)) = (args.first().copied(), args.get(1).copied()) else {
        return;
    };
    let Some(handlers) = routes_macro_handlers(routes_arg, content) else {
        return;
    };
    let Some(mount_path) = static_route_arg(path_arg, content, StaticArgLang::Rust) else {
        return;
    };
    let (start, end) = (call.start_byte(), call.end_byte());
    let Some(anchor) = smallest_node_covering_range(tree.root_node(), start, end) else {
        return;
    };
    if is_comment_or_string_node(anchor.kind()) {
        return;
    }
    let Some(span) = NormalizedSpan::from_content_range(content, start, end) else {
        return;
    };
    let normalized = normalize_route_template(mount_path, ParamFlavor::AngleBrackets);
    let mut metadata = base_metadata("framework", "rocket");
    insert_string(&mut metadata, "mount_path", mount_path);
    insert_string(&mut metadata, "normalized_mount_path", &normalized.template);
    insert_string(
        &mut metadata,
        "mount_target",
        node_text(content, routes_arg).unwrap_or_default(),
    );
    insert_string_array(&mut metadata, "handler_names", handlers);
    facts.push(fact_for_span(
        file_path,
        language,
        ROCKET_MOUNT_PATTERN_ID,
        "mount",
        anchor.kind(),
        span,
        metadata,
    ));
}

/// The handler names in a `routes![a, module::b]` argument: the last path
/// segment of each comma-separated entry.
fn routes_macro_handlers(arg: Node, content: &str) -> Option<Vec<String>> {
    if arg.kind() != "macro_invocation" {
        return None;
    }
    let name = arg.child_by_field_name("macro")?;
    if node_text(content, name)? != "routes" {
        return None;
    }
    let body = child_of_kind(arg, "token_tree")?;
    let mut handlers = Vec::new();
    let mut last_identifier = None;
    for token in body.children(&mut body.walk()) {
        match token.kind() {
            "identifier" => last_identifier = node_text(content, token),
            "," | ")" | "]" | "}" => {
                if let Some(handler) = last_identifier.take() {
                    handlers.push(handler.to_string());
                }
            }
            _ => {}
        }
    }
    Some(handlers)
}
