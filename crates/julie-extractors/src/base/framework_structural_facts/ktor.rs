//! Ktor server routing facts (`ktor.route.v1`).
//!
//! Restricted lexical gate (design §4.6): emit only when **all** hold —
//! 1. callee is a bare `identifier` verb in
//!    `{get, post, put, patch, delete, head, options}` (rejects
//!    `navigation_expression` callees like `client.get` / `map.get`);
//! 2. the call has a trailing `annotated_lambda` / `lambda_literal` child;
//! 3. arg0 is a static `string_literal` (Braces flavor, via [`static_route_arg`]);
//! 4. the call is lexically contained in a `routing{}` / `route{}` lambda.
//!
use tree_sitter::{Node, Tree};

use super::KTOR_ROUTE_PATTERN_ID;
use super::helpers::child_of_kind;
use super::scan::{RouteFactSpec, route_fact};
use super::static_arg::{StaticArgLang, static_route_arg};
use crate::base::http_boundary::{ParamFlavor, join_route_templates};
use crate::base::types::StructuralFact;

/// Server-side Ktor package roots. `io.ktor.server` covers Ktor 2/3;
/// `io.ktor.routing`/`io.ktor.application` cover the Ktor 1.x server packages.
/// Client/http/util packages (`io.ktor.client`, `io.ktor.http`, `io.ktor.util`)
/// are intentionally excluded so a client-only file with a local `routing { }`
/// DSL does not emit a false route.
const SERVER_IMPORT_ROOTS: &[&str] = &["io.ktor.server", "io.ktor.routing", "io.ktor.application"];

fn is_server_import(import_target: &str) -> bool {
    SERVER_IMPORT_ROOTS.iter().any(|root| {
        import_target == *root
            || import_target
                .strip_prefix(root)
                .is_some_and(|rest| rest.starts_with('.'))
    })
}

pub(super) fn collect_ktor_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !has_ktor_server_import(tree.root_node(), content) {
        return Vec::new();
    }

    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        language,
        tree,
        file_path,
        content,
        &mut facts,
    );
    facts
}

fn has_ktor_server_import(node: Node, content: &str) -> bool {
    if matches!(node.kind(), "import" | "import_header") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() != "qualified_identifier" {
                continue;
            }
            if let Some(import_target) = content.get(child.start_byte()..child.end_byte())
                && is_server_import(import_target)
            {
                return true;
            }
        }
        return false;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if has_ktor_server_import(child, content) {
            return true;
        }
    }
    false
}

fn walk(
    node: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if node.kind() == "call_expression"
        && let Some((verb, path)) = try_verb_route_call(node, content)
        && let Some(enclosing_prefix) = enclosing_route_prefix(node, content)
    {
        let spec = RouteFactSpec {
            framework: "ktor",
            pattern_id: KTOR_ROUTE_PATTERN_ID,
            capture_name: "route",
            api_style: "call_routing",
            route_template: path,
            verb: Some(verb),
            verb_source: Some("attested"),
            flavor: ParamFlavor::Braces,
            prefix: enclosing_prefix.as_deref(),
            prefix_key: None,
        };
        if let Some(fact) = route_fact(
            language,
            tree,
            file_path,
            content,
            node.start_byte(),
            node.end_byte(),
            spec,
            |_| {},
        ) {
            facts.push(fact);
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, language, tree, file_path, content, facts);
    }
}

/// `(VERB, path)` when `node` is a trailing-lambda verb call with a static arg0.
fn try_verb_route_call<'a>(node: Node<'_>, content: &'a str) -> Option<(&'static str, &'a str)> {
    trailing_lambda(node)?;

    let head = call_head(node)?;
    let name = bare_identifier_callee(head, content)?;
    let verb = match name {
        "get" => "GET",
        "post" => "POST",
        "put" => "PUT",
        "patch" => "PATCH",
        "delete" => "DELETE",
        "head" => "HEAD",
        "options" => "OPTIONS",
        _ => return None,
    };
    let arg0 = first_value_argument_expr(head)?;
    let path = static_route_arg(arg0, content, StaticArgLang::Kotlin)?;
    Some((verb, path))
}

/// For curried `get("/x") { }` the outer `call_expression` wraps an inner
/// `call_expression` head; for a flat shape the head is `node` itself.
fn call_head(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let mut saw_identifier = false;
    let mut saw_args = false;
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "call_expression" => return Some(child),
            "identifier" | "simple_identifier" => saw_identifier = true,
            "value_arguments" => saw_args = true,
            "annotated_lambda" | "lambda_literal" => {}
            "navigation_expression" => return None,
            _ => {}
        }
    }
    if saw_identifier && saw_args {
        Some(node)
    } else {
        None
    }
}

fn bare_identifier_callee<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    let mut cursor = node.walk();
    let first = node.named_children(&mut cursor).next()?;
    match first.kind() {
        "identifier" | "simple_identifier" => content.get(first.start_byte()..first.end_byte()),
        _ => None,
    }
}

fn first_value_argument_expr(call: Node) -> Option<Node> {
    let args = child_of_kind(call, "value_arguments")?;
    let mut cursor = args.walk();
    let value_argument = args
        .named_children(&mut cursor)
        .find(|child| child.kind() == "value_argument")?;
    let mut arg_cursor = value_argument.walk();
    value_argument.named_children(&mut arg_cursor).next()
}

fn trailing_lambda(call: Node) -> Option<Node> {
    let mut cursor = call.walk();
    call.named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "annotated_lambda" | "lambda_literal"))
}

/// `None` when `node` is not lexically inside a `routing{}`/`route{}` lambda
/// (the fact must be rejected). `Some(None)` when enclosed with no static
/// `route("/prefix")` scope. `Some(Some(prefix))` gives the enclosing scopes
/// joined into a single prefix, which [`route_fact`] joins with the raw path
/// to produce `effective_route_template`.
fn enclosing_route_prefix(node: Node, content: &str) -> Option<Option<String>> {
    let mut prefixes = Vec::new();
    let mut enclosed_by_routing = false;
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "call_expression"
            && is_routing_or_route_call(parent, content)
            && let Some(lambda) = trailing_lambda(parent)
            && node_contains(lambda, node)
        {
            enclosed_by_routing = true;
            if routing_or_route_name(parent, content) == Some("route") {
                prefixes.push(static_route_scope_path(parent, content)?);
            }
        }
        current = parent.parent();
    }
    if !enclosed_by_routing {
        return None;
    }

    let mut combined: Option<String> = None;
    for prefix in prefixes {
        combined = Some(match combined {
            None => prefix.to_string(),
            Some(inner) => join_route_templates(prefix, &inner),
        });
    }
    Some(combined)
}

fn is_routing_or_route_call(call: Node, content: &str) -> bool {
    matches!(
        routing_or_route_name(call, content),
        Some("routing" | "route")
    )
}

fn routing_or_route_name<'a>(call: Node<'_>, content: &'a str) -> Option<&'a str> {
    if let Some(name) = bare_identifier_callee(call, content) {
        return Some(name);
    }
    let mut cursor = call.walk();
    for child in call.named_children(&mut cursor) {
        if child.kind() == "call_expression"
            && let Some(name) = bare_identifier_callee(child, content)
        {
            return Some(name);
        }
    }
    None
}

fn static_route_scope_path<'a>(call: Node<'_>, content: &'a str) -> Option<&'a str> {
    let head = call_head(call)?;
    if bare_identifier_callee(head, content) != Some("route") {
        return None;
    }
    let arg0 = first_value_argument_expr(head)?;
    static_route_arg(arg0, content, StaticArgLang::Kotlin)
}

fn node_contains(ancestor: Node, descendant: Node) -> bool {
    descendant.start_byte() >= ancestor.start_byte()
        && descendant.end_byte() <= ancestor.end_byte()
        && descendant.id() != ancestor.id()
}
