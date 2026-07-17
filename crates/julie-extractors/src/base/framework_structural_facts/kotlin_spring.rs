//! Kotlin Spring MVC annotation-controller route facts.
//!
//! Kotlin reuses the Java `spring.request_mapping.v1` pattern id (registry
//! `languages = [java, kotlin]`) so Miller's two-sided join contract stays
//! uniform, but it needs a **separate collector**: Kotlin lexing, bracket-array
//! annotation values (`["/a", "/b"]`, not Java's `{…}`), and `$`-interpolation
//! differ from Java. Unlike the Java collector (a byte-level `SourceMask` scan),
//! this collector is AST-driven per design §4.2 — `MaskLanguage` is deliberately
//! not extended to Kotlin (ADR-0005).
//!
//! Static-literal silence (design §4.4, ADR-0005): every route/prefix argument is
//! read through `static_route_arg(_, _, StaticArgLang::Kotlin)` on the *whole*
//! argument node, so `@GetMapping("$base/x")`, `@GetMapping("/a" + x)`, and
//! const/identifier references emit nothing.
//!
//! Binding: in tree-sitter-kotlin-ng the annotation lives in a `modifiers` node
//! that is the *first child* of the `function_declaration`, so the declaration's
//! byte span starts at the annotation — but the handler *symbol* span starts at
//! the `fun` keyword. Anchoring the fact on the whole declaration would bind
//! `containing_symbol_id` to the enclosing class; so the span is anchored on
//! `[fun … end]` (the modifiers stripped), which equals the handler symbol span
//! and binds to the handler function.

use tree_sitter::{Node, Tree};

use super::SPRING_REQUEST_MAPPING_PATTERN_ID;
use super::helpers::{
    base_metadata, child_of_kind, fact_for_span, insert_string, insert_string_array,
    is_comment_or_string_node, node_text, smallest_node_covering_range,
};
use super::static_arg::{StaticArgLang, static_route_arg};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

/// Import gate (design §4.2): the Spring routing annotations live under
/// `org.springframework.web.bind.annotation`. Without that import there is no
/// Spring controller, and the collector stays silent.
const IMPORT_NEEDLE: &str = "org.springframework.web.bind.annotation";

/// HTTP methods recognised in `@RequestMapping(method = [RequestMethod.X])`.
const REQUEST_METHODS: &[&str] = &[
    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE",
];

pub(super) fn collect_kotlin_spring_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains(IMPORT_NEEDLE) {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk_types(
        tree.root_node(),
        language,
        tree,
        file_path,
        content,
        &mut facts,
    );
    facts
}

/// Depth-first walk. Every `class` / `object` / `companion object` is an
/// independent prefix scope (the class `@RequestMapping` prefix resets per type,
/// per the brief): for each such type node emit the routes of its *direct* member
/// functions joined to that type's own prefix, then recurse so nested types
/// (a `companion object`, a nested class) are processed under their own prefix.
fn walk_types(
    node: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(
            child.kind(),
            "class_declaration" | "object_declaration" | "companion_object"
        ) && let Some(body) = class_body(child)
        {
            let prefixes = class_prefixes(child, content);
            emit_direct_member_routes(body, &prefixes, language, tree, file_path, content, facts);
        }
        walk_types(child, language, tree, file_path, content, facts);
    }
}

fn class_body(type_node: Node) -> Option<Node> {
    let mut cursor = type_node.walk();
    type_node
        .children(&mut cursor)
        .find(|child| child.kind() == "class_body")
}

fn modifiers(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == "modifiers")
}

/// The static class-level `@RequestMapping` prefixes for a type, or empty when
/// the type has none or the prefix is dynamic (poisoned → routes emit their own
/// path only, design §3).
///
/// Two grammar forms are handled (tree-sitter-kotlin-ng resolves a top-level
/// `@Ann("x") class` ambiguously): the annotations may be a `modifiers` child of
/// the declaration, **or** a preceding-sibling `annotated_expression` whose
/// call args split into a `parenthesized_expression`.
fn class_prefixes(type_node: Node, content: &str) -> Vec<String> {
    // Form 1: annotations as a `modifiers` child of the declaration.
    if let Some(modifiers) = modifiers(type_node)
        && let Some(prefix) = request_mapping_prefix_in_modifiers(modifiers, content)
    {
        return prefix;
    }
    // Form 2: leading annotations parsed as a preceding-sibling
    // `annotated_expression` (a top-level `@RestController @RequestMapping("/x")
    // class …`), where `@RequestMapping("/x")` splits into a bare `annotation`
    // followed by a `parenthesized_expression` holding `("/x")`.
    if let Some(previous) = type_node.prev_sibling()
        && matches!(previous.kind(), "annotated_expression" | "annotation")
        && let Some(prefix) = request_mapping_prefix_in_sibling(previous, content)
    {
        return prefix;
    }
    Vec::new()
}

/// The `@RequestMapping` prefix templates from a `modifiers` node. `None` when no
/// `@RequestMapping` is present (so the caller can try the sibling form); an
/// empty `Some(vec)` marks a present-but-dynamic prefix (poisoned).
fn request_mapping_prefix_in_modifiers(modifiers: Node, content: &str) -> Option<Vec<String>> {
    let mut cursor = modifiers.walk();
    for annotation in modifiers.children(&mut cursor) {
        if annotation.kind() != "annotation" {
            continue;
        }
        let Some((name, invocation)) = annotation_name_and_invocation(annotation, content) else {
            continue;
        };
        if name != "RequestMapping" {
            continue;
        }
        return Some(match invocation {
            Some(invocation) => parse_mapping_arguments(invocation, content).route_templates,
            // A bare `@RequestMapping` (no args) is not a static path prefix.
            None => Vec::new(),
        });
    }
    None
}

/// The `@RequestMapping` prefix templates recovered from a preceding-sibling
/// `annotated_expression`, where `@RequestMapping("x")` is a bare `annotation`
/// followed by a `parenthesized_expression` holding its arguments.
fn request_mapping_prefix_in_sibling(node: Node, content: &str) -> Option<Vec<String>> {
    let mut pairs = Vec::new();
    collect_annotated_expression_annotations(node, content, &mut pairs);
    for (name, args) in pairs {
        if name != "RequestMapping" {
            continue;
        }
        return Some(match args {
            Some(parens) => templates_from_parenthesized(parens, content),
            None => Vec::new(),
        });
    }
    None
}

/// Collect `(annotation_name, parenthesized_args)` pairs from an
/// `annotated_expression` subtree: an `annotation` immediately followed by a
/// `parenthesized_expression` carries that annotation's arguments; nested
/// `annotated_expression` children are searched recursively.
fn collect_annotated_expression_annotations<'t>(
    node: Node<'t>,
    content: &str,
    out: &mut Vec<(String, Option<Node<'t>>)>,
) {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for (index, child) in children.iter().enumerate() {
        match child.kind() {
            "annotation" => {
                if let Some((name, _)) = annotation_name_and_invocation(*child, content) {
                    let args = children
                        .get(index + 1)
                        .filter(|next| next.kind() == "parenthesized_expression")
                        .copied();
                    out.push((name, args));
                }
            }
            "annotated_expression" => {
                collect_annotated_expression_annotations(*child, content, out)
            }
            _ => {}
        }
    }
}

/// Route templates from a `parenthesized_expression` annotation argument
/// (`("/api")`, `(["/a", "/b"])`): the first named child run through the static
/// guard. Non-literal contents yield an empty (poisoned) prefix.
fn templates_from_parenthesized(parens: Node, content: &str) -> Vec<String> {
    let mut cursor = parens.walk();
    let Some(inner) = parens.children(&mut cursor).find(|child| child.is_named()) else {
        return Vec::new();
    };
    static_templates(inner, content).unwrap_or_default()
}

/// Emit route facts for the `function_declaration`s that are *direct* members of
/// `body`, joined to `class_prefixes`. Nested types inside `body` are skipped
/// here (the outer `walk_types` recursion reaches them under their own prefix).
#[allow(clippy::too_many_arguments)]
fn emit_direct_member_routes(
    body: Node,
    class_prefixes: &[String],
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let mut cursor = body.walk();
    for member in body.children(&mut cursor) {
        if member.kind() != "function_declaration" {
            continue;
        }
        let Some(member_modifiers) = modifiers(member) else {
            continue;
        };
        let mut ann_cursor = member_modifiers.walk();
        for annotation in member_modifiers.children(&mut ann_cursor) {
            if annotation.kind() != "annotation" {
                continue;
            }
            emit_annotation_routes(
                annotation,
                member,
                class_prefixes,
                language,
                tree,
                file_path,
                content,
                facts,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_annotation_routes(
    annotation: Node,
    handler: Node,
    class_prefixes: &[String],
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let Some((name, invocation)) = annotation_name_and_invocation(annotation, content) else {
        return;
    };
    let Some((default_verb, attribute_kind)) = mapping_kind(&name) else {
        return;
    };

    let args = match invocation {
        Some(invocation) => parse_mapping_arguments(invocation, content),
        // Bare `@GetMapping` (no parentheses): index route, no path argument.
        None => MappingArguments::default(),
    };

    // Static-literal silence: a route argument that was present but non-literal
    // leaves `route_templates` empty → emit nothing (no guessed route).
    let templates: Vec<String> = if args.route_templates.is_empty() {
        if args.had_route_argument {
            return;
        }
        vec![String::new()]
    } else {
        args.route_templates
    };

    let verbs: Vec<Option<String>> = match default_verb {
        Some(verb) => vec![Some(verb.to_string())],
        None => {
            if args.method_verbs.is_empty() {
                vec![None]
            } else {
                args.method_verbs.into_iter().map(Some).collect()
            }
        }
    };

    let class_templates: Vec<Option<&str>> = if class_prefixes.is_empty() {
        vec![None]
    } else {
        class_prefixes.iter().map(|p| Some(p.as_str())).collect()
    };

    for class_template in &class_templates {
        for template in &templates {
            let effective = class_template.map(|prefix| join_prefix(prefix, template));
            let normalized_source = effective.as_deref().unwrap_or(template);
            for verb in &verbs {
                if let Some(fact) = mapping_fact(
                    language,
                    tree,
                    file_path,
                    content,
                    handler,
                    attribute_kind,
                    template,
                    normalized_source,
                    *class_template,
                    effective.as_deref(),
                    verb.as_deref(),
                ) {
                    facts.push(fact);
                }
            }
        }
    }
}

/// Join a class prefix with a method sub-path, resolving the empty method path to
/// the prefix alone (no trailing slash) — the Spring/NestJS class+method model.
fn join_prefix(prefix: &str, template: &str) -> String {
    if template.is_empty() {
        prefix.to_string()
    } else {
        join_route_templates(prefix, template)
    }
}

/// `(default_verb, attribute_kind)` for a Spring mapping annotation, or `None`
/// when the annotation is not a Spring route mapping.
fn mapping_kind(name: &str) -> Option<(Option<&'static str>, &'static str)> {
    match name {
        "GetMapping" => Some((Some("GET"), "http_method")),
        "PostMapping" => Some((Some("POST"), "http_method")),
        "PutMapping" => Some((Some("PUT"), "http_method")),
        "PatchMapping" => Some((Some("PATCH"), "http_method")),
        "DeleteMapping" => Some((Some("DELETE"), "http_method")),
        "RequestMapping" => Some((None, "request_mapping")),
        _ => None,
    }
}

#[derive(Default)]
struct MappingArguments {
    /// Static route templates collected from the positional / `value =` / `path =`
    /// argument. Empty when the route argument was dynamic (poisoned).
    route_templates: Vec<String>,
    /// Whether a route argument (positional / `value` / `path`) was present at all.
    had_route_argument: bool,
    /// Verbs collected from `method = [RequestMethod.X]`.
    method_verbs: Vec<String>,
}

/// Parse a `constructor_invocation`'s `value_arguments` into route templates and
/// verbs. `value`/`path`/positional args yield route templates (via the static
/// guard); `method` yields verbs. `produces`/`consumes`/`params`/`headers` are
/// ignored (their strings are not routes).
fn parse_mapping_arguments(invocation: Node, content: &str) -> MappingArguments {
    let mut args = MappingArguments::default();
    let Some(value_arguments) = child_of_kind(invocation, "value_arguments") else {
        return args;
    };
    let mut cursor = value_arguments.walk();
    for value_argument in value_arguments.children(&mut cursor) {
        if value_argument.kind() != "value_argument" {
            continue;
        }
        let (name, value) = split_value_argument(value_argument, content);
        let Some(value) = value else {
            continue;
        };
        match name.as_deref() {
            None | Some("value") | Some("path") => {
                args.had_route_argument = true;
                // A dynamic route arg leaves `route_templates` untouched (empty)
                // while `had_route_argument` records its presence.
                if let Some(mut templates) = static_templates(value, content) {
                    args.route_templates.append(&mut templates);
                }
            }
            Some("method") => collect_request_methods(value, content, &mut args.method_verbs),
            _ => {}
        }
    }
    args
}

/// Static route templates from a route-argument value node: a lone string
/// literal yields one template; a `["/a", "/b"]` array yields one per element.
/// Returns `None` (poison) when the value — or any array element — is not a
/// plain static string literal.
fn static_templates(value: Node, content: &str) -> Option<Vec<String>> {
    match value.kind() {
        "string_literal" | "multiline_string_literal" => {
            static_route_arg(value, content, StaticArgLang::Kotlin).map(|v| vec![v.to_string()])
        }
        "collection_literal" => {
            let mut cursor = value.walk();
            let mut templates = Vec::new();
            for element in value.children(&mut cursor) {
                if !element.is_named() {
                    continue;
                }
                let literal = static_route_arg(element, content, StaticArgLang::Kotlin)?;
                templates.push(literal.to_string());
            }
            Some(templates)
        }
        _ => None,
    }
}

/// Collect `RequestMethod.X` verbs from a `method =` value (a single
/// `navigation_expression` or a `[…]` `collection_literal` of them).
fn collect_request_methods(value: Node, content: &str, verbs: &mut Vec<String>) {
    let mut push = |node: Node| {
        if let Some(name) = last_identifier_text(node, content) {
            let upper = name.to_ascii_uppercase();
            if REQUEST_METHODS.contains(&upper.as_str()) {
                verbs.push(upper);
            }
        }
    };
    match value.kind() {
        "navigation_expression" => push(value),
        "collection_literal" => {
            let mut cursor = value.walk();
            for element in value.children(&mut cursor) {
                if element.kind() == "navigation_expression" {
                    push(element);
                }
            }
        }
        _ => {}
    }
}

/// The last identifier of a `navigation_expression` (`RequestMethod.GET` → `GET`).
fn last_identifier_text<'a>(node: Node, content: &'a str) -> Option<&'a str> {
    let mut cursor = node.walk();
    let mut last = None;
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" {
            last = Some(child);
        }
    }
    last.and_then(|child| node_text(content, child))
}

/// The annotation's name and its `constructor_invocation` (the argument-bearing
/// form), or `None` for a bare `@Name` reference (no arguments).
///
/// `annotation → @ constructor_invocation(user_type(identifier) value_arguments)`
/// for `@Get("/x")`, or `annotation → @ user_type(identifier)` for a bare `@Get`.
fn annotation_name_and_invocation<'t>(
    annotation: Node<'t>,
    content: &str,
) -> Option<(String, Option<Node<'t>>)> {
    let mut cursor = annotation.walk();
    for child in annotation.children(&mut cursor) {
        match child.kind() {
            "constructor_invocation" => {
                let user_type = child_of_kind(child, "user_type")?;
                let name = type_identifier(user_type, content)?;
                return Some((name, Some(child)));
            }
            "user_type" => {
                let name = type_identifier(child, content)?;
                return Some((name, None));
            }
            _ => {}
        }
    }
    None
}

fn type_identifier(user_type: Node, content: &str) -> Option<String> {
    child_of_kind(user_type, "identifier")
        .and_then(|id| node_text(content, id))
        .map(str::to_string)
}

/// Split a `value_argument` into `(name, value)`: named for `name = value`,
/// positional (`name = None`) otherwise.
fn split_value_argument<'t>(
    value_argument: Node<'t>,
    content: &str,
) -> (Option<String>, Option<Node<'t>>) {
    let mut cursor = value_argument.walk();
    let children: Vec<Node> = value_argument.children(&mut cursor).collect();
    if let Some(equals) = children.iter().position(|child| child.kind() == "=") {
        let name = children[..equals]
            .iter()
            .find(|child| child.kind() == "identifier" || child.kind() == "simple_identifier")
            .and_then(|child| node_text(content, *child))
            .map(str::to_string);
        let value = children[equals + 1..]
            .iter()
            .find(|child| child.is_named())
            .copied();
        (name, value)
    } else {
        let value = children.iter().find(|child| child.is_named()).copied();
        (None, value)
    }
}

/// The handler-binding span: `[fun … end]` with the leading `modifiers`
/// (annotations, `suspend`, visibility) stripped, so the span sits inside the
/// handler symbol range and binds `containing_symbol_id` to the handler function
/// rather than the enclosing class.
fn handler_span(handler: Node) -> (usize, usize) {
    let end = handler.end_byte();
    let mut cursor = handler.walk();
    for child in handler.children(&mut cursor) {
        if child.kind() != "modifiers" {
            return (child.start_byte(), end);
        }
    }
    (handler.start_byte(), end)
}

#[allow(clippy::too_many_arguments)]
fn mapping_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    handler: Node,
    attribute_kind: &str,
    route_template: &str,
    normalized_source: &str,
    class_route_template: Option<&str>,
    effective_route_template: Option<&str>,
    verb: Option<&str>,
) -> Option<StructuralFact> {
    let (start, end) = handler_span(handler);
    let node = smallest_node_covering_range(tree.root_node(), start, end)?;
    if is_comment_or_string_node(node.kind()) {
        return None;
    }
    let span = NormalizedSpan::from_content_range(content, start, end)?;
    let normalized = normalize_route_template(normalized_source, ParamFlavor::Braces);

    let mut metadata = base_metadata("framework", "spring");
    insert_string(&mut metadata, "api_style", "annotation_routing");
    insert_string(&mut metadata, "attribute_kind", attribute_kind);
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
    if let Some(class_route_template) = class_route_template {
        insert_string(&mut metadata, "class_route_template", class_route_template);
    }
    if let Some(effective_route_template) = effective_route_template {
        insert_string(
            &mut metadata,
            "effective_route_template",
            effective_route_template,
        );
    }
    if let Some(verb) = verb {
        insert_string(&mut metadata, "verb", verb);
        insert_string(&mut metadata, "verb_source", "attested");
    }

    Some(fact_for_span(
        file_path,
        language,
        SPRING_REQUEST_MAPPING_PATTERN_ID,
        "request_mapping",
        node.kind(),
        span,
        metadata,
    ))
}
