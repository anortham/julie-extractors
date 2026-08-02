//! NestJS controller route facts (`nestjs.route.v1`).
//!
//! NestJS rides the existing TypeScript/JavaScript path: it is dispatched from
//! the same ts/js/jsx/tsx arms as the Express/Node collector and reuses the
//! shared JS string parser (`parse_js_string_literal`) plus the shared route
//! builders (`normalize_route_template`/`join_route_templates`). Unlike Express
//! (call routing), NestJS is decorator routing, so this collector walks the AST
//! for the `@Controller` class prefix and the HTTP-method method decorators and
//! joins them with the Spring class+method model (`spring.rs`).
//!
//! Static-literal silence (design §4.4, whole-argument rule): a route fact is
//! emitted only when the decorator argument node is *itself* a plain `string`
//! node (or an array/object of plain strings). Template literals
//! (`` `/a/${x}` ``), string concatenation (`'/a/' + x`), identifier/const
//! references (`PATHS.USER`), and other computed arguments are rejected by node
//! kind before any value is read — this is the AST analog of how `node.rs`
//! rejects dynamic Express route args.
//!
//! The fact span is anchored to the `method_definition` node so
//! `attach_containing_symbols` binds `containing_symbol_id` to the handler
//! method. (In this repo's tree-sitter-typescript grammar the method decorator
//! is a *preceding sibling* of `method_definition`, i.e. outside the method
//! symbol range, so a decorator-anchored span would bind to the class instead;
//! anchoring on the method node binds correctly under both the TS grammar
//! (decorator = sibling) and the JS grammar (decorator = child).)

use tree_sitter::{Node, Tree};

use super::NESTJS_ROUTE_PATTERN_ID;
use super::helpers::{
    base_metadata, fact_for_span, insert_string, insert_string_array, is_comment_or_string_node,
    smallest_node_covering_range,
};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::base::web_structural_facts::js_object_scan::parse_js_string_literal;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) fn collect_nestjs_route_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    // Import gate (design §4.2): the routing decorators come from
    // `@nestjs/common`; without that import there is no NestJS controller.
    if !content.contains("@nestjs/common") {
        return Vec::new();
    }

    let mut class_nodes = Vec::new();
    collect_class_declarations(tree.root_node(), 0, &mut class_nodes);

    let mut facts = Vec::new();
    for class_node in class_nodes {
        let class_prefixes = controller_prefixes(class_node, content);
        let Some(body) = class_body(class_node) else {
            continue;
        };
        collect_class_body_routes(
            language,
            tree,
            file_path,
            content,
            body,
            &class_prefixes,
            &mut facts,
        );
    }
    facts
}

/// Depth-first collect every `class_declaration` node in the tree.
fn collect_class_declarations<'tree>(node: Node<'tree>, depth: u32, out: &mut Vec<Node<'tree>>) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "class_declaration" {
            out.push(child);
        }
        collect_class_declarations(child, child_depth, out);
    }
}

fn class_body(class_node: Node) -> Option<Node> {
    class_node.child_by_field_name("body").or_else(|| {
        let mut cursor = class_node.walk();
        class_node
            .children(&mut cursor)
            .find(|child| child.kind() == "class_body")
    })
}

/// Resolve the static `@Controller(...)` prefix templates for a class.
///
/// The `@Controller` decorator is a child of the `class_declaration` (JS, and
/// TS without `export`) or of the wrapping `export_statement` (TS `export
/// class`). A missing/empty/non-static argument yields no prefix, degrading the
/// route to `route_template` only (the same-file poison rule of design §3).
fn controller_prefixes(class_node: Node, content: &str) -> Vec<String> {
    let mut decorators = Vec::new();
    collect_child_decorators(class_node, &mut decorators);
    if let Some(parent) = class_node.parent()
        && parent.kind() == "export_statement"
    {
        collect_child_decorators(parent, &mut decorators);
    }

    for decorator in decorators {
        if let Some((name, args)) = decorator_callee(decorator, content)
            && name == "Controller"
        {
            return controller_arg_templates(args, content);
        }
    }
    Vec::new()
}

fn collect_child_decorators<'tree>(node: Node<'tree>, out: &mut Vec<Node<'tree>>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "decorator" {
            out.push(child);
        }
    }
}

/// Emit one route fact per (class prefix × method sub-path) combination for each
/// HTTP-method decorator on a `method_definition` in `body`.
#[allow(clippy::too_many_arguments)]
fn collect_class_body_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    body: Node,
    class_prefixes: &[String],
    facts: &mut Vec<StructuralFact>,
) {
    // In tree-sitter-typescript method decorators are preceding siblings within
    // `class_body`; in tree-sitter-javascript they are children of the
    // `method_definition`. Accumulate the preceding-sibling decorators and fold
    // in the method's own child decorators so both grammars are covered.
    let mut pending: Vec<Node> = Vec::new();
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        match child.kind() {
            "decorator" => pending.push(child),
            "comment" => {}
            "method_definition" => {
                let mut decorators = std::mem::take(&mut pending);
                collect_child_decorators(child, &mut decorators);
                emit_method_routes(
                    language,
                    tree,
                    file_path,
                    content,
                    child,
                    &decorators,
                    class_prefixes,
                    facts,
                );
            }
            // Any other member (field, index signature, `{`/`}` tokens) owns its
            // own decorators; drop the pending set so they cannot leak onto a
            // later method.
            _ => pending.clear(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_method_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    method: Node,
    decorators: &[Node],
    class_prefixes: &[String],
    facts: &mut Vec<StructuralFact>,
) {
    let start = method.start_byte();
    let end = method.end_byte();
    for decorator in decorators {
        let Some((name, args)) = decorator_callee(*decorator, content) else {
            continue;
        };
        let Some(verb) = http_method_verb(&name) else {
            continue;
        };
        // Static-literal silence: a dynamic argument yields nothing at all.
        let Some(templates) = method_arg_templates(args, content) else {
            continue;
        };

        if class_prefixes.is_empty() {
            for template in &templates {
                push_route(
                    language, tree, file_path, content, start, end, template, None, verb, facts,
                );
            }
        } else {
            for prefix in class_prefixes {
                for template in &templates {
                    push_route(
                        language,
                        tree,
                        file_path,
                        content,
                        start,
                        end,
                        template,
                        Some(prefix.as_str()),
                        verb,
                        facts,
                    );
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push_route(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    start: usize,
    end: usize,
    route_template: &str,
    class_route_template: Option<&str>,
    verb: Option<&str>,
    facts: &mut Vec<StructuralFact>,
) {
    if let Some(fact) = nestjs_route_fact(
        language,
        tree,
        file_path,
        content,
        start,
        end,
        route_template,
        class_route_template,
        verb,
    ) {
        facts.push(fact);
    }
}

/// Map a decorator identifier to its HTTP verb.
///
/// Returns `Some(Some(verb))` for a verb-restricted decorator, `Some(None)` for
/// `@All` (accepts any method → verb omitted), and `None` when the decorator is
/// not an HTTP route decorator at all.
fn http_method_verb(name: &str) -> Option<Option<&'static str>> {
    match name {
        "Get" => Some(Some("GET")),
        "Post" => Some(Some("POST")),
        "Put" => Some(Some("PUT")),
        "Patch" => Some(Some("PATCH")),
        "Delete" => Some(Some("DELETE")),
        "Options" => Some(Some("OPTIONS")),
        "Head" => Some(Some("HEAD")),
        "All" => Some(None),
        _ => None,
    }
}

/// Return the decorator's callee identifier and its `arguments` node.
///
/// Only a bare `identifier` callee is accepted (`@Get(...)`); a namespaced
/// `member_expression` callee (`@common.Get(...)`) is rejected so it cannot be
/// mistaken for a verb.
fn decorator_callee<'tree>(
    decorator: Node<'tree>,
    content: &str,
) -> Option<(String, Option<Node<'tree>>)> {
    let mut cursor = decorator.walk();
    for child in decorator.children(&mut cursor) {
        match child.kind() {
            "call_expression" => {
                let callee = child.child_by_field_name("function")?;
                if callee.kind() != "identifier" {
                    return None;
                }
                let name = content.get(callee.byte_range())?.to_string();
                return Some((name, child.child_by_field_name("arguments")));
            }
            // Bare `@Get` reference (no call): treated as an empty path.
            "identifier" => {
                let name = content.get(child.byte_range())?.to_string();
                return Some((name, None));
            }
            _ => {}
        }
    }
    None
}

/// Static route sub-paths for a method HTTP decorator argument.
///
/// - no argument / `@Get()` → one empty path (`""`).
/// - `@Get('/x')` → `["/x"]`.
/// - `@Get(['a', 'b'])` → each element (all must be plain strings).
/// - anything else (template literal, concat, identifier, object, …) → `None`
///   (emit nothing).
fn method_arg_templates(args: Option<Node>, content: &str) -> Option<Vec<String>> {
    let Some(args) = args else {
        return Some(vec![String::new()]);
    };
    let Some(arg0) = first_argument(args) else {
        return Some(vec![String::new()]);
    };
    match arg0.kind() {
        "string" => static_string_value(arg0, content).map(|value| vec![value]),
        "array" => array_string_values(arg0, content),
        _ => None,
    }
}

/// Static class prefixes for a `@Controller` argument.
///
/// Accepts `@Controller('x')`, `@Controller({ path: 'x' })`, and
/// `@Controller(['a', 'b'])`. Empty strings and non-static forms yield no
/// prefix.
fn controller_arg_templates(args: Option<Node>, content: &str) -> Vec<String> {
    let Some(args) = args else {
        return Vec::new();
    };
    let Some(arg0) = first_argument(args) else {
        return Vec::new();
    };
    let raw = match arg0.kind() {
        "string" => static_string_value(arg0, content).into_iter().collect(),
        "array" => array_string_values(arg0, content).unwrap_or_default(),
        "object" => object_path_value(arg0, content).into_iter().collect(),
        _ => Vec::new(),
    };
    raw.into_iter().filter(|value| !value.is_empty()).collect()
}

fn first_argument(args: Node) -> Option<Node> {
    let mut cursor = args.walk();
    args.children(&mut cursor)
        .find(|child| child.is_named() && child.kind() != "comment")
}

/// Extract a plain string literal's value via the shared JS string parser.
fn static_string_value(node: Node, content: &str) -> Option<String> {
    parse_js_string_literal(content, node.start_byte()).map(|(value, _end)| value)
}

/// Collect string values from an `array` literal, or `None` if any element is
/// not a plain string literal (the array is poisoned as a whole).
fn array_string_values(node: Node, content: &str) -> Option<Vec<String>> {
    let mut cursor = node.walk();
    let mut values = Vec::new();
    for child in node.children(&mut cursor) {
        if !child.is_named() || child.kind() == "comment" {
            continue;
        }
        if child.kind() != "string" {
            return None;
        }
        values.push(static_string_value(child, content)?);
    }
    Some(values)
}

/// Extract the `path` property value from a `@Controller({ path: '...' })`
/// object, requiring it to be a plain string literal.
fn object_path_value(node: Node, content: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "pair" {
            continue;
        }
        let key = child.child_by_field_name("key")?;
        let key_name = match key.kind() {
            "property_identifier" => content.get(key.byte_range())?.to_string(),
            "string" => static_string_value(key, content)?,
            _ => continue,
        };
        if key_name == "path" {
            let value = child.child_by_field_name("value")?;
            return (value.kind() == "string")
                .then(|| static_string_value(value, content))
                .flatten();
        }
    }
    None
}

/// Build a NestJS route fact, joining the class `@Controller` prefix with the
/// method sub-path. Modeled on Spring's `mapping_fact` (design §4.2 sanctions
/// the hand-rolled shape when class-prefix semantics don't fit `RouteFactSpec`)
/// so the empty-method-path case resolves to the class prefix alone instead of
/// the trailing-slash join `route_fact` would produce.
#[allow(clippy::too_many_arguments)]
fn nestjs_route_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    start: usize,
    end: usize,
    route_template: &str,
    class_route_template: Option<&str>,
    verb: Option<&str>,
) -> Option<StructuralFact> {
    let node = smallest_node_covering_range(tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, start, end)?;

    let effective: Option<String> = class_route_template.map(|prefix| {
        if route_template.is_empty() {
            prefix.to_string()
        } else {
            join_route_templates(prefix, route_template)
        }
    });
    let normalized_source = effective.as_deref().unwrap_or(route_template);
    let normalized = normalize_route_template(normalized_source, ParamFlavor::Colon);

    let mut metadata = base_metadata("framework", "nestjs");
    insert_string(&mut metadata, "api_style", "decorator_routing");
    insert_string(&mut metadata, "route_template", route_template);
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
    if let Some(prefix) = class_route_template {
        insert_string(&mut metadata, "class_route_template", prefix);
    }
    if let Some(effective) = effective.as_deref() {
        insert_string(&mut metadata, "effective_route_template", effective);
    }
    if let Some(verb) = verb {
        insert_string(&mut metadata, "verb", verb);
        insert_string(&mut metadata, "verb_source", "attested");
    }

    Some(fact_for_span(
        file_path,
        language,
        NESTJS_ROUTE_PATTERN_ID,
        "route_decorator",
        node.kind(),
        span,
        metadata,
    ))
}
