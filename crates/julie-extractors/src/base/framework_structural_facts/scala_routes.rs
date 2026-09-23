//! Scala HTTP route facts.
//!
//! - `akka_http.route.v1`: Akka HTTP and Apache Pekko HTTP routing directives.
//!   A route joins the static `path(…)` / `pathPrefix(…)` directives with the
//!   method directive (`get { … }`) that governs them, in either nesting
//!   order. A path matcher such as `LongNumber` becomes a `{LongNumber}`
//!   segment; any other matcher silences the routes it governs. A method
//!   directive with no enclosed or enclosing path directive (or
//!   `pathSingleSlash`) names no path and stays silent.
//! - `http4s.route.v1`: http4s `case GET -> Root / "users" / id =>` patterns.
//!   A string segment is literal, a bound name or extractor argument
//!   (`IntVar(id)`) is `{id}`, and a query matcher (`:?`) ends the path.
//!
//! Both collectors are gated on their library's import.

use tree_sitter::{Node, Tree};

use super::helpers::node_text;
use super::scan::{RouteFactSpec, route_fact};
use super::{AKKA_HTTP_ROUTE_PATTERN_ID, HTTP4S_ROUTE_PATTERN_ID};
use crate::base::http_boundary::{ParamFlavor, join_route_templates};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const METHOD_DIRECTIVES: &[(&str, &str)] = &[
    ("get", "GET"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
    ("head", "HEAD"),
    ("options", "OPTIONS"),
];

const HTTP4S_VERBS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// Akka / Pekko path matchers that extract one segment value.
const SEGMENT_MATCHERS: &[&str] = &[
    "IntNumber",
    "LongNumber",
    "HexIntNumber",
    "HexLongNumber",
    "DoubleNumber",
    "JavaUUID",
    "Segment",
    "Segments",
    "Remaining",
    "RemainingPath",
];

struct Ctx<'a> {
    language: &'a str,
    tree: &'a Tree,
    file_path: &'a str,
    content: &'a str,
}

pub(super) fn collect_scala_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let ctx = Ctx {
        language,
        tree,
        file_path,
        content,
    };
    let mut facts = Vec::new();
    let directive_framework = if content.contains("akka.http.scaladsl") {
        Some("akka_http")
    } else if content.contains("org.apache.pekko.http.scaladsl") {
        Some("pekko_http")
    } else {
        None
    };
    if let Some(framework) = directive_framework {
        let scope = Scope {
            prefix: None,
            verb: None,
        };
        walk_directives(&ctx, framework, tree.root_node(), scope, 0, &mut facts);
    }
    if content.contains("org.http4s") {
        walk_http4s(&ctx, tree.root_node(), 0, &mut facts);
    }
    facts
}

/// Where a directive sits: the static path joined so far (`None` outside
/// every path directive) and the method of an enclosing method directive.
#[derive(Clone, Copy)]
struct Scope<'a> {
    prefix: Option<&'a str>,
    verb: Option<&'static str>,
}

/// Walk Akka directives. A route is emitted where the method and the path are
/// both known and no path directive below refines the path: at a method
/// directive inside a path, or at the innermost path inside a method.
fn walk_directives(
    ctx: &Ctx,
    framework: &str,
    node: Node,
    scope: Scope,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    if node.kind() == "call_expression" {
        match directive(ctx.content, node) {
            Some(Directive::Path(Some(segment))) => {
                let joined = match scope.prefix {
                    Some(prefix) => join_route_templates(prefix, &segment),
                    None => format!("/{segment}"),
                };
                let inner = Scope {
                    prefix: Some(&joined),
                    ..scope
                };
                emit_leaf(ctx, framework, node, inner, facts);
                walk_children(ctx, framework, node, inner, child_depth, facts);
                return;
            }
            Some(Directive::Path(None)) => return,
            Some(Directive::Root) => {
                let inner = Scope {
                    prefix: Some(scope.prefix.unwrap_or("/")),
                    ..scope
                };
                emit_leaf(ctx, framework, node, inner, facts);
                walk_children(ctx, framework, node, inner, child_depth, facts);
                return;
            }
            Some(Directive::Method(verb)) => {
                let inner = Scope {
                    verb: Some(verb),
                    ..scope
                };
                emit_leaf(ctx, framework, node, inner, facts);
                walk_children(ctx, framework, node, inner, child_depth, facts);
                return;
            }
            None => {}
        }
    }
    walk_children(ctx, framework, node, scope, child_depth, facts);
}

fn emit_leaf(
    ctx: &Ctx,
    framework: &str,
    node: Node,
    scope: Scope,
    facts: &mut Vec<StructuralFact>,
) {
    let (Some(template), Some(verb)) = (scope.prefix, scope.verb) else {
        return;
    };
    let arguments = node.child_by_field_name("arguments");
    if arguments.is_some_and(|arguments| has_path_directive(ctx.content, arguments, 0)) {
        return;
    }
    facts.extend(directive_route_fact(ctx, framework, node, verb, template));
}

fn has_path_directive(content: &str, node: Node, depth: u32) -> bool {
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    if node.kind() == "call_expression"
        && matches!(
            directive(content, node),
            Some(Directive::Path(_) | Directive::Root)
        )
    {
        return true;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| has_path_directive(content, child, child_depth))
}

fn walk_children(
    ctx: &Ctx,
    framework: &str,
    node: Node,
    scope: Scope,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_directives(ctx, framework, child, scope, depth, facts);
    }
}

enum Directive {
    /// `path(…) { }` / `pathPrefix(…) { }`: `None` when the matcher is dynamic.
    Path(Option<String>),
    /// `pathSingleSlash { }` / `pathEndOrSingleSlash { }`: the root path.
    Root,
    Method(&'static str),
}

fn directive(content: &str, call: Node) -> Option<Directive> {
    let function = call.child_by_field_name("function")?;
    let arguments = call.child_by_field_name("arguments")?;
    if arguments.kind() != "block" {
        return None;
    }
    if function.kind() == "identifier" {
        let name = node_text(content, function)?;
        if matches!(name, "pathSingleSlash" | "pathEndOrSingleSlash") {
            return Some(Directive::Root);
        }
        return METHOD_DIRECTIVES
            .iter()
            .find(|(directive, _)| *directive == name)
            .map(|(_, verb)| Directive::Method(verb));
    }
    if function.kind() != "call_expression" {
        return None;
    }
    let name = node_text(content, function.child_by_field_name("function")?)?;
    if !matches!(name, "path" | "pathPrefix") {
        return None;
    }
    let matcher_arguments = function.child_by_field_name("arguments")?;
    let mut cursor = matcher_arguments.walk();
    let mut matchers = matcher_arguments.named_children(&mut cursor);
    let matcher = matchers.next()?;
    if matchers.next().is_some() {
        return Some(Directive::Path(None));
    }
    Some(Directive::Path(path_matcher(content, matcher, 0)))
}

/// `"users"`, `"users" / LongNumber`, `LongNumber`: the static template of a
/// path matcher, or `None` when any part is dynamic.
fn path_matcher(content: &str, node: Node, depth: u32) -> Option<String> {
    let child_depth = child_tree_depth(depth)?;
    match node.kind() {
        "string" => static_string(content, node).map(str::to_string),
        "identifier" => {
            let name = node_text(content, node)?;
            SEGMENT_MATCHERS
                .contains(&name)
                .then(|| format!("{{{name}}}"))
        }
        "infix_expression" => {
            let operator = node_text(content, node.child_by_field_name("operator")?)?;
            if operator != "/" {
                return None;
            }
            let left = path_matcher(content, node.child_by_field_name("left")?, child_depth)?;
            let right = path_matcher(content, node.child_by_field_name("right")?, child_depth)?;
            Some(format!("{left}/{right}"))
        }
        _ => None,
    }
}

fn directive_route_fact(
    ctx: &Ctx,
    framework: &str,
    node: Node,
    verb: &str,
    template: &str,
) -> Option<StructuralFact> {
    route_fact(
        ctx.language,
        ctx.tree,
        ctx.file_path,
        ctx.content,
        node.start_byte(),
        node.end_byte(),
        RouteFactSpec {
            framework,
            pattern_id: AKKA_HTTP_ROUTE_PATTERN_ID,
            capture_name: "route",
            api_style: "directive_routing",
            route_template: template,
            verb: Some(verb),
            verb_source: Some("attested"),
            flavor: ParamFlavor::Braces,
            prefix: None,
            prefix_key: None,
        },
        |_| {},
    )
}

fn walk_http4s(ctx: &Ctx, node: Node, depth: u32, facts: &mut Vec<StructuralFact>) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    if node.kind() == "case_clause"
        && let Some(pattern) = node.child_by_field_name("pattern")
        && let Some((verb, template)) = http4s_route(ctx.content, pattern)
    {
        facts.extend(route_fact(
            ctx.language,
            ctx.tree,
            ctx.file_path,
            ctx.content,
            pattern.start_byte(),
            pattern.end_byte(),
            RouteFactSpec {
                framework: "http4s",
                pattern_id: HTTP4S_ROUTE_PATTERN_ID,
                capture_name: "route",
                api_style: "pattern_routing",
                route_template: &template,
                verb: Some(verb),
                verb_source: Some("attested"),
                flavor: ParamFlavor::Braces,
                prefix: None,
                prefix_key: None,
            },
            |_| {},
        ));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_http4s(ctx, child, child_depth, facts);
    }
}

/// `GET -> Root / "users" / id` gives `("GET", "/users/{id}")`.
fn http4s_route(content: &str, pattern: Node) -> Option<(&'static str, String)> {
    let mut operands = Vec::new();
    let mut operators = Vec::new();
    flatten_infix_pattern(content, pattern, &mut operands, &mut operators, 0)?;
    if operators.first() != Some(&"->") || operands.len() < 2 {
        return None;
    }
    // `case req @ POST -> Root / …` binds the request around the verb.
    let verb_node = if operands[0].kind() == "capture_pattern" {
        operands[0].child_by_field_name("pattern")?
    } else {
        operands[0]
    };
    let verb_text = node_text(content, verb_node)?;
    let verb = HTTP4S_VERBS.iter().find(|verb| **verb == verb_text)?;
    if node_text(content, operands[1])? != "Root" {
        return None;
    }
    let mut segments = Vec::new();
    for (operator, operand) in operators[1..].iter().zip(&operands[2..]) {
        match *operator {
            "/" => segments.push(http4s_segment(content, *operand)?),
            ":?" => break,
            _ => return None,
        }
    }
    Some((verb, format!("/{}", segments.join("/"))))
}

fn flatten_infix_pattern<'tree>(
    content: &'tree str,
    node: Node<'tree>,
    operands: &mut Vec<Node<'tree>>,
    operators: &mut Vec<&'tree str>,
    depth: u32,
) -> Option<()> {
    let child_depth = child_tree_depth(depth)?;
    if node.kind() != "infix_pattern" {
        operands.push(node);
        return Some(());
    }
    flatten_infix_pattern(
        content,
        node.child_by_field_name("left")?,
        operands,
        operators,
        child_depth,
    )?;
    operators.push(node_text(content, node.child_by_field_name("operator")?)?);
    flatten_infix_pattern(
        content,
        node.child_by_field_name("right")?,
        operands,
        operators,
        child_depth,
    )
}

fn http4s_segment(content: &str, operand: Node) -> Option<String> {
    match operand.kind() {
        "string" => static_string(content, operand).map(str::to_string),
        "identifier" => {
            let name = node_text(content, operand)?;
            name.starts_with(|c: char| c.is_lowercase())
                .then(|| format!("{{{name}}}"))
        }
        "case_class_pattern" => {
            let mut cursor = operand.walk();
            let binding = operand
                .children_by_field_name("pattern", &mut cursor)
                .find(|child| child.kind() == "identifier")?;
            Some(format!("{{{}}}", node_text(content, binding)?))
        }
        _ => None,
    }
}

/// The text of a plain `"..."` string literal; `None` for interpolated or
/// triple-quoted strings.
fn static_string<'a>(content: &'a str, node: Node) -> Option<&'a str> {
    let text = node_text(content, node)?;
    if text.starts_with("\"\"\"") {
        return None;
    }
    text.strip_prefix('"')?.strip_suffix('"')
}
