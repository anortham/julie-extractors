//! Laravel facade-route structural facts.
//!
//! Laravel registers routes through the `Route` facade in `routes/*.php`:
//! `Route::get('/x', [Ctrl::class, 'm'])`, resource routes
//! `Route::resource('/photos', Ctrl::class)`, and same-file prefix groups
//! `Route::prefix('admin')->group(fn)` / `Route::group(['prefix' => 'admin'], fn)`.
//!
//! This collector is AST-driven (design §4.2): it walks the tree for `Route`
//! facade calls and reads every path/prefix argument through the shared PHP
//! static guard (`static_route_arg(_, _, StaticArgLang::Php)`, ADR-0005), so
//! interpolated / concatenated / `self::CONST` / variable paths emit nothing
//! (M2 silence — a false static promotes a computed path to a guessed route).
//!
//! Prefix model: `Route::prefix()->group(closure)` and
//! `Route::group(['prefix' => …], closure)` are *lexical containment* (design §3,
//! §4.3), not single-assignment data-flow — the prefix governs every route
//! lexically inside the closure. This is the Rails `scope_stack` shape
//! implemented over AST: a prefix stack pushed on entry to a group closure and
//! popped on exit (the recursion boundary), a non-literal prefix *poisons* the
//! stack (contained routes then emit `route_template` only). Each static prefix
//! also emits a `laravel.route_prefix.v1` mount-family fact at its own call site.
//!
//! Out of scope here: PHP `#[Route]` attributes (a Symfony idiom handled by
//! `symfony.route.v1`) and cross-file `RouteServiceProvider` group prefixes —
//! the routes' `route_template` is therefore NOT guaranteed to be the absolute
//! public path when an out-of-file prefix applies.

use tree_sitter::{Node, Tree};

use super::helpers::{
    base_metadata, fact_for_span, insert_string, is_comment_or_string_node, node_text,
    smallest_node_covering_range,
};
use super::scan::{RouteFactSpec, route_fact};
use super::static_arg::{StaticArgLang, static_route_arg};
use super::{
    LARAVEL_RESOURCE_ROUTE_PATTERN_ID, LARAVEL_ROUTE_PATTERN_ID, LARAVEL_ROUTE_PREFIX_PATTERN_ID,
};
use crate::base::http_boundary::{ParamFlavor, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Import gate (design §4.2): every Laravel route registration goes through the
/// `Route` facade, so a file with no `Route::` reference registers no routes and
/// the collector stays silent.
const IMPORT_NEEDLE: &str = "Route::";

pub(super) fn collect_laravel_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains(IMPORT_NEEDLE) {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        &[],
        language,
        tree,
        file_path,
        content,
        0,
        &mut facts,
    );
    facts
}

/// Depth-first walk carrying the enclosing prefix stack. Each element is a group
/// prefix segment; `None` marks a poisoned (non-literal) prefix so contained
/// routes emit only their own template. A group node consumes its own subtree
/// (it recurses into the closure body with the updated stack and returns), so a
/// closure is never re-walked with the wrong stack.
#[allow(clippy::too_many_arguments)]
fn walk(
    node: Node,
    prefix_stack: &[Option<String>],
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

    if try_group(
        node,
        prefix_stack,
        language,
        tree,
        file_path,
        content,
        depth,
        facts,
    ) || try_route(
        node,
        prefix_stack,
        language,
        tree,
        file_path,
        content,
        facts,
    ) || try_resource(
        node,
        prefix_stack,
        language,
        tree,
        file_path,
        content,
        facts,
    ) {
        return;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            child,
            prefix_stack,
            language,
            tree,
            file_path,
            content,
            child_depth,
            facts,
        );
    }
}

// ---------------------------------------------------------------------------
// Group prefixes (lexical containment)
// ---------------------------------------------------------------------------

/// A static or poisoned group prefix, with the AST node whose span the
/// `laravel.route_prefix.v1` fact anchors on (the prefix's own call/entry site).
enum PrefixResult<'t> {
    Static { value: String, site: Node<'t> },
    Poisoned,
}

/// Handle a `Route::…->group(closure)` / `Route::group([…], closure)` call:
/// emit a `laravel.route_prefix.v1` fact for a static prefix, then recurse into
/// the closure body with the prefix pushed (or a poison marker). Returns `true`
/// when the node was a route group (its subtree is fully handled here).
#[allow(clippy::too_many_arguments)]
fn try_group(
    node: Node,
    prefix_stack: &[Option<String>],
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) -> bool {
    if !is_route_facade_call(node, "group", content) {
        return false;
    }
    let Some(arguments) = call_arguments(node) else {
        return false;
    };
    // A route group we trace must carry a closure body; a group whose target is
    // a cross-file include (`Route::group([...], base_path('routes/x.php'))`) has
    // no same-file body and falls through to a normal descend.
    let Some(closure_body) = group_closure_body(arguments) else {
        return false;
    };

    let prefix = array_config_prefix(arguments, content).or_else(|| chain_prefix(node, content));
    let mut new_stack = prefix_stack.to_vec();
    match prefix {
        Some(PrefixResult::Static { value, site }) => {
            push_prefix_fact(
                language,
                tree,
                file_path,
                content,
                site,
                prefix_stack,
                &value,
                facts,
            );
            new_stack.push(Some(value));
        }
        Some(PrefixResult::Poisoned) => new_stack.push(None),
        // A middleware-only group (no prefix) still bounds the routes lexically
        // but adds no path segment; leave the stack unchanged.
        None => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return true;
    };

    walk(
        closure_body,
        &new_stack,
        language,
        tree,
        file_path,
        content,
        child_depth,
        facts,
    );
    true
}

/// The prefix declared in a `['prefix' => 'lit', …]` group config array (form 2),
/// or `None` when the group takes no array config / has no `prefix` key.
fn array_config_prefix<'t>(arguments: Node<'t>, content: &str) -> Option<PrefixResult<'t>> {
    for value in positional_arg_values(arguments) {
        if value.kind() != "array_creation_expression" {
            continue;
        }
        for init in named_children_of_kind(value, "array_element_initializer") {
            let mut cursor = init.walk();
            let pair: Vec<Node> = init.named_children(&mut cursor).collect();
            let [key, val] = pair.as_slice() else {
                continue;
            };
            if static_route_arg(*key, content, StaticArgLang::Php) != Some("prefix") {
                continue;
            }
            return Some(match static_route_arg(*val, content, StaticArgLang::Php) {
                Some(literal) => PrefixResult::Static {
                    value: literal.to_string(),
                    site: init,
                },
                None => PrefixResult::Poisoned,
            });
        }
        // Array config present but with no `prefix` key: no prefix from this form.
        return None;
    }
    None
}

/// The prefix declared by a `->prefix('lit')` call anywhere in a `->group()`
/// member-call chain (form 1: `Route::prefix('admin')->group(...)`, also
/// `Route::middleware('m')->prefix('admin')->group(...)`).
fn chain_prefix<'t>(group_call: Node<'t>, content: &str) -> Option<PrefixResult<'t>> {
    let mut receiver = group_call.child_by_field_name("object")?;
    loop {
        if !matches!(
            receiver.kind(),
            "member_call_expression" | "scoped_call_expression"
        ) {
            return None;
        }
        if call_method_name(receiver, content) == Some("prefix") {
            let arguments = call_arguments(receiver)?;
            let first = positional_arg_values(arguments).into_iter().next()?;
            return Some(match static_route_arg(first, content, StaticArgLang::Php) {
                Some(literal) => PrefixResult::Static {
                    value: literal.to_string(),
                    site: receiver,
                },
                None => PrefixResult::Poisoned,
            });
        }
        match receiver.kind() {
            "member_call_expression" => receiver = receiver.child_by_field_name("object")?,
            // A `scoped_call_expression` that is not `prefix` is the facade root
            // (`Route::…`); the chain carries no prefix.
            _ => return None,
        }
    }
}

/// The `body` (`compound_statement`) of the group's closure argument, or `None`
/// when no `function () { … }` closure argument is present.
fn group_closure_body<'t>(arguments: Node<'t>) -> Option<Node<'t>> {
    positional_arg_values(arguments)
        .into_iter()
        .find(|value| value.kind() == "anonymous_function")
        .and_then(|closure| closure.child_by_field_name("body"))
}

/// Emit the `laravel.route_prefix.v1` mount-family fact for a static group
/// prefix at its own call/entry site. `mount_path` is the raw literal at this
/// site; `normalized_mount_path` includes the enclosing same-file scope (the
/// parent prefix stack), mirroring `rails.mount.v1`.
#[allow(clippy::too_many_arguments)]
fn push_prefix_fact(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    site: Node,
    parent_stack: &[Option<String>],
    prefix: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let start = site.start_byte();
    let end = site.end_byte();
    let Some(node) = smallest_node_covering_range(tree.root_node(), start, end) else {
        return;
    };
    if is_comment_or_string_node(node.kind()) {
        return;
    }
    let Some(span) = NormalizedSpan::from_content_range(content, start, end) else {
        return;
    };

    // Absolute mount path including same-file scope. When a parent prefix is
    // poisoned (non-literal) the absolute path is unknowable in-file, so fall
    // back to this segment alone rather than inventing one.
    let mut full = parent_stack.to_vec();
    full.push(Some(prefix.to_string()));
    let absolute = joined_prefix(&full).unwrap_or_else(|| prefix.to_string());
    let normalized = normalize_route_template(&absolute, ParamFlavor::Braces);

    let mut metadata = base_metadata("framework", "laravel");
    insert_string(&mut metadata, "mount_path", prefix);
    insert_string(&mut metadata, "normalized_mount_path", &normalized.template);
    facts.push(fact_for_span(
        file_path,
        language,
        LARAVEL_ROUTE_PREFIX_PATTERN_ID,
        "route_prefix",
        node.kind(),
        span,
        metadata,
    ));
}

/// Join static prefix segments into an absolute path (`/a/b`). Returns `None`
/// when any segment is poisoned (non-literal) or the stack is empty.
fn joined_prefix(stack: &[Option<String>]) -> Option<String> {
    if stack.is_empty() {
        return None;
    }
    let mut out = String::new();
    for segment in stack {
        let trimmed = segment.as_deref()?.trim_matches('/');
        if trimmed.is_empty() {
            continue;
        }
        out.push('/');
        out.push_str(trimmed);
    }
    (!out.is_empty()).then_some(out)
}

// ---------------------------------------------------------------------------
// Verb / any / match routes
// ---------------------------------------------------------------------------

/// The route-registration shape a facade method names.
enum RouteKind {
    /// A verb-restricted route (`get`/`post`/…): the uppercase verb.
    Verb(&'static str),
    /// `Route::any(...)` — accepts any method, so the verb is omitted.
    Any,
    /// `Route::match([verbs], path, …)` — verbs in the first argument array.
    Match,
}

fn route_kind(method: &str) -> Option<RouteKind> {
    match method {
        "get" => Some(RouteKind::Verb("GET")),
        "post" => Some(RouteKind::Verb("POST")),
        "put" => Some(RouteKind::Verb("PUT")),
        "patch" => Some(RouteKind::Verb("PATCH")),
        "delete" => Some(RouteKind::Verb("DELETE")),
        "options" => Some(RouteKind::Verb("OPTIONS")),
        "any" => Some(RouteKind::Any),
        "match" => Some(RouteKind::Match),
        _ => None,
    }
}

/// Emit `laravel.route.v1` for a `Route::VERB(...)` / `Route::any(...)` /
/// `Route::match(...)` call. Returns `true` when the node was such a call (its
/// subtree needs no further descent) — including when a dynamic argument makes
/// it stay silent.
#[allow(clippy::too_many_arguments)]
fn try_route(
    node: Node,
    prefix_stack: &[Option<String>],
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) -> bool {
    let Some(method) = call_method_name_if_facade(node, content) else {
        return false;
    };
    let Some(kind) = route_kind(method) else {
        return false;
    };
    let Some(arguments) = call_arguments(node) else {
        return true;
    };
    let args = positional_arg_values(arguments);

    match kind {
        RouteKind::Match => {
            let (Some(verb_array), Some(path_node)) = (args.first(), args.get(1)) else {
                return true;
            };
            let (Some(verbs), Some(path)) = (
                static_verb_array(*verb_array, content),
                static_route_arg(*path_node, content, StaticArgLang::Php),
            ) else {
                return true;
            };
            let controller_action = args
                .get(2)
                .and_then(|h| controller_action_text(*h, content));
            for verb in verbs {
                emit_route(
                    node,
                    prefix_stack,
                    Some(&verb),
                    path,
                    controller_action.as_deref(),
                    language,
                    tree,
                    file_path,
                    content,
                    facts,
                );
            }
        }
        RouteKind::Verb(_) | RouteKind::Any => {
            let Some(path_node) = args.first() else {
                return true;
            };
            let Some(path) = static_route_arg(*path_node, content, StaticArgLang::Php) else {
                return true;
            };
            let verb = match kind {
                RouteKind::Verb(verb) => Some(verb),
                _ => None,
            };
            let controller_action = args
                .get(1)
                .and_then(|h| controller_action_text(*h, content));
            emit_route(
                node,
                prefix_stack,
                verb,
                path,
                controller_action.as_deref(),
                language,
                tree,
                file_path,
                content,
                facts,
            );
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn emit_route(
    node: Node,
    prefix_stack: &[Option<String>],
    verb: Option<&str>,
    route_template: &str,
    controller_action: Option<&str>,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let prefix = joined_prefix(prefix_stack);
    let spec = RouteFactSpec {
        framework: "laravel",
        pattern_id: LARAVEL_ROUTE_PATTERN_ID,
        capture_name: "route",
        api_style: "call_routing",
        route_template,
        verb,
        verb_source: verb.map(|_| "attested"),
        flavor: ParamFlavor::Braces,
        prefix: prefix.as_deref(),
        prefix_key: Some("route_group_prefix"),
    };
    if let Some(fact) = route_fact(
        language,
        tree,
        file_path,
        content,
        node.start_byte(),
        node.end_byte(),
        spec,
        |metadata| {
            if let Some(controller_action) = controller_action {
                insert_string(metadata, "controller_action", controller_action);
            }
        },
    ) {
        facts.push(fact);
    }
}

/// Static uppercase verbs from a `Route::match(['get','post'], …)` verb array.
/// `None` when the first argument is not an array or any element is non-literal
/// (M2 silence — a dynamic verb set poisons the whole `match`).
fn static_verb_array(node: Node, content: &str) -> Option<Vec<String>> {
    if node.kind() != "array_creation_expression" {
        return None;
    }
    let mut verbs = Vec::new();
    for init in named_children_of_kind(node, "array_element_initializer") {
        let value = first_named_child(init)?;
        let verb = static_route_arg(value, content, StaticArgLang::Php)?;
        verbs.push(verb.to_ascii_uppercase());
    }
    (!verbs.is_empty()).then_some(verbs)
}

// ---------------------------------------------------------------------------
// Resource routes
// ---------------------------------------------------------------------------

/// Emit `laravel.resource_route.v1` for `Route::resource(...)` /
/// `Route::apiResource(...)`. Returns `true` when the node was such a call.
#[allow(clippy::too_many_arguments)]
fn try_resource(
    node: Node,
    prefix_stack: &[Option<String>],
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) -> bool {
    let Some(method) = call_method_name_if_facade(node, content) else {
        return false;
    };
    let resource_kind = match method {
        "resource" => "resource",
        "apiResource" => "api_resource",
        _ => return false,
    };
    let Some(arguments) = call_arguments(node) else {
        return true;
    };
    let args = positional_arg_values(arguments);
    let Some(resource_name) = args
        .first()
        .and_then(|name| static_route_arg(*name, content, StaticArgLang::Php))
    else {
        return true;
    };
    let controller = args.get(1).and_then(|c| controller_class_text(*c, content));

    let start = node.start_byte();
    let end = node.end_byte();
    let Some(anchor) = smallest_node_covering_range(tree.root_node(), start, end) else {
        return true;
    };
    if is_comment_or_string_node(anchor.kind()) {
        return true;
    }
    let Some(span) = NormalizedSpan::from_content_range(content, start, end) else {
        return true;
    };
    let mut metadata = base_metadata("framework", "laravel");
    insert_string(&mut metadata, "api_style", "resource_routing");
    insert_string(&mut metadata, "resource_name", resource_name);
    insert_string(&mut metadata, "resource_kind", resource_kind);
    if let Some(controller) = controller {
        insert_string(&mut metadata, "controller", &controller);
    }
    if let Some(prefix) = joined_prefix(prefix_stack) {
        insert_string(&mut metadata, "route_group_prefix", &prefix);
    }
    facts.push(fact_for_span(
        file_path,
        language,
        LARAVEL_RESOURCE_ROUTE_PATTERN_ID,
        "resource_route",
        anchor.kind(),
        span,
        metadata,
    ));
    true
}

// ---------------------------------------------------------------------------
// Handler / controller metadata
// ---------------------------------------------------------------------------

/// A readable controller action for a route's handler argument: `Ctrl@method`
/// for `[Ctrl::class, 'method']`, the literal for a `'Ctrl@method'` string, or
/// `None` for a closure / variable handler (no static action to record).
fn controller_action_text(node: Node, content: &str) -> Option<String> {
    match node.kind() {
        "array_creation_expression" => {
            let inits = named_children_of_kind(node, "array_element_initializer");
            let class = controller_class_text(first_named_child(*inits.first()?)?, content)?;
            let method_node = first_named_child(*inits.get(1)?)?;
            let method = static_route_arg(method_node, content, StaticArgLang::Php)?;
            Some(format!("{class}@{method}"))
        }
        "string" | "encapsed_string" => {
            static_route_arg(node, content, StaticArgLang::Php).map(str::to_string)
        }
        _ => None,
    }
}

/// The controller class name for a `Ctrl::class` reference (or a
/// `'Ctrl'` string). `None` for any other expression.
fn controller_class_text(node: Node, content: &str) -> Option<String> {
    match node.kind() {
        "class_constant_access_expression" => {
            node_text(content, first_named_child(node)?).map(str::to_string)
        }
        "string" | "encapsed_string" => {
            static_route_arg(node, content, StaticArgLang::Php).map(str::to_string)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// PHP call-node helpers
// ---------------------------------------------------------------------------

/// The method name of a `Route` facade call (`scoped_call`/`member_call` whose
/// receiver chain roots at the bare `Route` facade), or `None` otherwise. This
/// is the single gate that separates `Route::get('/x')` (a route) from
/// `$client->get('url')` (a client request handled elsewhere).
fn call_method_name_if_facade<'a>(node: Node, content: &'a str) -> Option<&'a str> {
    if !matches!(
        node.kind(),
        "scoped_call_expression" | "member_call_expression"
    ) {
        return None;
    }
    (call_facade_name(node, content) == Some("Route"))
        .then(|| call_method_name(node, content))
        .flatten()
}

fn is_route_facade_call(node: Node, method: &str, content: &str) -> bool {
    call_method_name_if_facade(node, content) == Some(method)
}

fn call_method_name<'a>(call: Node, content: &'a str) -> Option<&'a str> {
    node_text(content, call.child_by_field_name("name")?)
}

fn call_arguments(call: Node) -> Option<Node> {
    call.child_by_field_name("arguments")
}

/// The bare facade/class name a call's receiver chain roots at: the `scope` of
/// the innermost `scoped_call_expression` (`Route::…`) after walking any
/// `member_call_expression` chain. A `variable_name` receiver (`$client->…`)
/// yields `None` — not a facade call.
fn call_facade_name<'a>(call: Node, content: &'a str) -> Option<&'a str> {
    let mut receiver = match call.kind() {
        "scoped_call_expression" => return node_text(content, call.child_by_field_name("scope")?),
        "member_call_expression" => call.child_by_field_name("object")?,
        _ => return None,
    };
    loop {
        match receiver.kind() {
            "member_call_expression" => receiver = receiver.child_by_field_name("object")?,
            "scoped_call_expression" => {
                return node_text(content, receiver.child_by_field_name("scope")?);
            }
            "name" => return node_text(content, receiver),
            _ => return None,
        }
    }
}

/// The value node of each positional `argument` in an `arguments` node, in order.
fn positional_arg_values(arguments: Node) -> Vec<Node> {
    let mut cursor = arguments.walk();
    arguments
        .children(&mut cursor)
        .filter(|child| child.kind() == "argument")
        .filter_map(|argument| first_named_child(argument))
        .collect()
}

fn named_children_of_kind<'t>(node: Node<'t>, kind: &'static str) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == kind)
        .collect()
}

fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}
