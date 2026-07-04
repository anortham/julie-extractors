//! Phoenix router-macro structural facts (`phoenix.route.v1`,
//! `phoenix.resource_route.v1`, `phoenix.forward.v1`).
//!
//! A Phoenix router is a module that `use`s `Phoenix.Router` (directly, or via
//! the generated `use MyAppWeb, :router`) and declares routes with bare macro
//! calls in a `scope` block:
//! `get "/users/:id", UserController, :show`, `resources "/photos", PhotoController`,
//! and `forward "/health", HealthPlug`, all lexically nested under
//! `scope "/api", MyAppWeb do … end` prefixes.
//!
//! This collector is AST-driven (design §4.2): it walks the tree for the bare
//! router-macro `call` nodes and reads every path/prefix argument through the
//! shared Elixir static guard (`static_route_arg(_, _, StaticArgLang::Elixir)`,
//! ADR-0005), so interpolated (`"/u/#{id}"`), concatenated (`"/a/" <> id`),
//! `~r` regex-sigil, and `@attr`/identifier paths emit nothing (M2 silence — a
//! false static promotes a computed path to a guessed route).
//!
//! Prefix model: `scope "/api" do … end` is *lexical containment* (design §3,
//! §4.3), not single-assignment data-flow — the prefix governs every route
//! lexically inside the block. This reuses the Rails `scope_stack` shape over the
//! AST: a prefix stack pushed on entry to a `scope` block and popped on exit (the
//! recursion boundary), where an interpolated/non-literal prefix *poisons* the
//! stack (contained routes then emit `route_template` only). A `scope` with no
//! positional string arg0 (options-only, or an alias-only `scope MyAppWeb do`)
//! still bounds routes lexically but contributes no path segment.
//!
//! Only `forward` emits a mount-family fact (`phoenix.forward.v1`, following the
//! shipped `mount_path`/`normalized_mount_path`/`mount_target` shape); a bare
//! `scope` prefix flows only into `route_group_prefix`/`effective_route_template`
//! on the routes it governs, exactly like the Rails `scope`.
//!
//! Documented exclusions (emit nothing): `pipe_through`, `live`, `socket`, and
//! `channel` router macros, and cross-file `scope`/router prefixes — recorded as
//! `open_gaps` on the elixir capability row.

use tree_sitter::{Node, Tree};

use super::helpers::{
    base_metadata, fact_for_span, insert_string, is_comment_or_string_node, node_text,
    smallest_node_covering_range,
};
use super::scan::{RouteFactSpec, route_fact};
use super::static_arg::{StaticArgLang, static_route_arg};
use super::{
    PHOENIX_FORWARD_PATTERN_ID, PHOENIX_RESOURCE_ROUTE_PATTERN_ID, PHOENIX_ROUTE_PATTERN_ID,
};
use crate::base::http_boundary::{ParamFlavor, join_route_templates, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

/// Import gate (design §4.2): a Phoenix router either `use`s `Phoenix.Router`
/// directly or, in generated apps, `use MyAppWeb, :router` (which expands to it).
/// A file matching neither marker declares no Phoenix routes, so the collector
/// stays silent. Precision comes from the bare-identifier macro match in the
/// walk; this gate is the fast bail.
fn is_phoenix_router(content: &str) -> bool {
    content.contains("Phoenix.Router") || content.contains(":router")
}

pub(super) fn collect_phoenix_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !is_phoenix_router(content) {
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
        &mut facts,
    );
    facts
}

/// Depth-first walk carrying the enclosing `scope` prefix stack. Each element is
/// a prefix segment; `None` marks a poisoned (interpolated/non-literal) prefix so
/// contained routes emit only their own template. A `scope` node consumes its own
/// subtree (it recurses into the block body with the updated stack and returns),
/// so a block is never re-walked with the wrong stack.
#[allow(clippy::too_many_arguments)]
fn walk(
    node: Node,
    prefix_stack: &[Option<String>],
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if try_scope(
        node,
        prefix_stack,
        language,
        tree,
        file_path,
        content,
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
    ) || try_forward(
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
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            child,
            prefix_stack,
            language,
            tree,
            file_path,
            content,
            facts,
        );
    }
}

// ---------------------------------------------------------------------------
// scope prefixes (lexical containment)
// ---------------------------------------------------------------------------

/// Handle a `scope "/api", … do … end` block: push the static prefix segment
/// (or a poison marker, or nothing for an options-only scope) and recurse into
/// the block body with the updated stack. Returns `true` when the node was a
/// `scope` block (its subtree is fully handled here). A `scope` emits no fact of
/// its own — the prefix flows into the routes it governs (design §4.3).
#[allow(clippy::too_many_arguments)]
fn try_scope(
    node: Node,
    prefix_stack: &[Option<String>],
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) -> bool {
    if call_macro_name(node, content) != Some("scope") {
        return false;
    }
    let Some(block) = call_do_block(node) else {
        // A `scope` without a `do` block is not a lexical container (rare); let
        // the generic descent handle it rather than swallowing the subtree.
        return false;
    };

    let mut new_stack = prefix_stack.to_vec();
    // The prefix is the first positional argument only when it is a static
    // string/sigil/charlist. An alias-only (`scope MyAppWeb do`) or options-only
    // (`scope host: "x" do`) scope has no positional path arg0 and adds no
    // segment; an interpolated path poisons the stack.
    if let Some(arg0) = first_positional_arg(node)
        && is_string_arg_kind(arg0)
    {
        match static_route_arg(arg0, content, StaticArgLang::Elixir) {
            Some(prefix) => new_stack.push(Some(prefix.to_string())),
            None => new_stack.push(None),
        }
    }

    let mut cursor = block.walk();
    for child in block.children(&mut cursor) {
        walk(child, &new_stack, language, tree, file_path, content, facts);
    }
    true
}

// ---------------------------------------------------------------------------
// verb routes
// ---------------------------------------------------------------------------

fn verb_for_macro(macro_name: &str) -> Option<&'static str> {
    match macro_name {
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

/// Emit `phoenix.route.v1` for a `get "/x", Ctrl, :action` verb macro. Returns
/// `true` when the node was such a macro call (its subtree needs no descent) —
/// including when a dynamic path argument makes it stay silent.
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
    let Some(macro_name) = call_macro_name(node, content) else {
        return false;
    };
    let Some(verb) = verb_for_macro(macro_name) else {
        return false;
    };
    if !is_block_statement(node) {
        // A genuine route macro is a statement in the router/scope block body; a
        // verb-named call nested in a `def get("/x")` function head (parent
        // `arguments`) is not a route (M2 silence).
        return false;
    }
    let args = positional_args(node);
    let Some(path) = args
        .first()
        .and_then(|arg| static_route_arg(*arg, content, StaticArgLang::Elixir))
    else {
        return true;
    };
    let controller = args.get(1).and_then(|arg| alias_text(*arg, content));
    let action = args.get(2).and_then(|arg| atom_name(*arg, content));

    let prefix = joined_prefix(prefix_stack);
    let spec = RouteFactSpec {
        framework: "phoenix",
        pattern_id: PHOENIX_ROUTE_PATTERN_ID,
        capture_name: "route",
        api_style: "dsl_routing",
        route_template: path,
        verb: Some(verb),
        verb_source: Some("attested"),
        flavor: ParamFlavor::Colon,
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
            if let Some(controller) = controller {
                insert_string(metadata, "controller", &controller);
            }
            if let Some(action) = action {
                insert_string(metadata, "action", &action);
            }
        },
    ) {
        facts.push(fact);
    }
    true
}

// ---------------------------------------------------------------------------
// resource routes
// ---------------------------------------------------------------------------

/// Emit `phoenix.resource_route.v1` for `resources "/photos", PhotoController`.
/// Returns `true` when the node was a `resources` macro call.
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
    if call_macro_name(node, content) != Some("resources") || !is_block_statement(node) {
        return false;
    }
    let args = positional_args(node);
    let Some(path) = args
        .first()
        .and_then(|arg| static_route_arg(*arg, content, StaticArgLang::Elixir))
    else {
        return true;
    };
    let controller = args.get(1).and_then(|arg| alias_text(*arg, content));

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

    let prefix = joined_prefix(prefix_stack);
    let effective = match &prefix {
        Some(prefix) => join_route_templates(prefix, path),
        None => path.to_string(),
    };
    let normalized = normalize_route_template(&effective, ParamFlavor::Colon);

    let mut metadata = base_metadata("framework", "phoenix");
    insert_string(&mut metadata, "api_style", "resource_routing");
    insert_string(&mut metadata, "resource_path", path);
    insert_string(
        &mut metadata,
        "normalized_resource_path",
        &normalized.template,
    );
    if let Some(controller) = controller {
        insert_string(&mut metadata, "controller", &controller);
    }
    if let Some(prefix) = prefix {
        insert_string(&mut metadata, "route_group_prefix", &prefix);
    }
    facts.push(fact_for_span(
        file_path,
        language,
        PHOENIX_RESOURCE_ROUTE_PATTERN_ID,
        "resource_route",
        anchor.kind(),
        span,
        metadata,
    ));
    true
}

// ---------------------------------------------------------------------------
// forward (prefix-registration / mount family)
// ---------------------------------------------------------------------------

/// Emit `phoenix.forward.v1` for `forward "/health", HealthPlug` at its own site,
/// following the shipped mount-family metadata shape. Returns `true` when the
/// node was a `forward` macro call.
#[allow(clippy::too_many_arguments)]
fn try_forward(
    node: Node,
    prefix_stack: &[Option<String>],
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) -> bool {
    if call_macro_name(node, content) != Some("forward") || !is_block_statement(node) {
        return false;
    }
    let args = positional_args(node);
    // A forward needs both a static mount path and a resolvable plug target; a
    // dynamic path or non-alias target stays silent (mount_target is required).
    let (Some(mount_path), Some(mount_target)) = (
        args.first()
            .and_then(|arg| static_route_arg(*arg, content, StaticArgLang::Elixir)),
        args.get(1).and_then(|arg| alias_text(*arg, content)),
    ) else {
        return true;
    };

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

    let prefix = joined_prefix(prefix_stack);
    let absolute = match &prefix {
        Some(prefix) => join_route_templates(prefix, mount_path),
        None => mount_path.to_string(),
    };
    let normalized = normalize_route_template(&absolute, ParamFlavor::Colon);

    let mut metadata = base_metadata("framework", "phoenix");
    insert_string(&mut metadata, "mount_path", mount_path);
    insert_string(&mut metadata, "normalized_mount_path", &normalized.template);
    insert_string(&mut metadata, "mount_target", &mount_target);
    if let Some(prefix) = prefix {
        insert_string(&mut metadata, "route_group_prefix", &prefix);
    }
    facts.push(fact_for_span(
        file_path,
        language,
        PHOENIX_FORWARD_PATTERN_ID,
        "forward",
        anchor.kind(),
        span,
        metadata,
    ));
    true
}

// ---------------------------------------------------------------------------
// prefix joining
// ---------------------------------------------------------------------------

/// Join static prefix segments into an absolute path (`/a/b`). Returns `None`
/// when any segment is poisoned (interpolated/non-literal) or the stack is empty
/// — a poisoned enclosing scope makes the absolute path unknowable in-file, so a
/// contained route degrades to its own template (M2 silence).
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
// Elixir call-node helpers
// ---------------------------------------------------------------------------

/// The bare macro name a router `call` invokes (`get`/`scope`/`forward`/…), or
/// `None` when the node is not a `call` or its target is not a bare
/// `identifier`. A qualified `target:(dot)` callee (`Map.get`, `Req.get`) is not
/// a router macro and yields `None` — the single gate separating the routing DSL
/// from ordinary qualified calls.
fn call_macro_name<'a>(node: Node, content: &'a str) -> Option<&'a str> {
    if node.kind() != "call" {
        return None;
    }
    let target = node.child_by_field_name("target")?;
    if target.kind() != "identifier" {
        return None;
    }
    node_text(content, target)
}

/// Whether a `call` node is a statement directly in a `do ... end` block body
/// (the module body or a `scope`/`pipeline` block). A genuine router macro is
/// always such a statement; a verb-named call nested elsewhere — notably a
/// `def get("/x")` function head, whose `get` call's parent is `arguments` — is
/// not a route and must stay silent (M2).
fn is_block_statement(node: Node) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "do_block")
}

/// The `do_block` child of a `call`, or `None` when the call has none.
fn call_do_block(call: Node) -> Option<Node> {
    let mut cursor = call.walk();
    call.children(&mut cursor)
        .find(|child| child.kind() == "do_block")
}

/// The positional argument value nodes of a `call`, in order. Skips the trailing
/// `keywords` node (`as: :foo`, `host: "x"`), so only the leading positional
/// arguments (path, controller alias, action atom) are returned.
fn positional_args(call: Node) -> Vec<Node> {
    let Some(arguments) = ({
        let mut cursor = call.walk();
        call.children(&mut cursor)
            .find(|child| child.kind() == "arguments")
    }) else {
        return Vec::new();
    };
    let mut cursor = arguments.walk();
    arguments
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "keywords")
        .collect()
}

fn first_positional_arg(call: Node) -> Option<Node> {
    positional_args(call).into_iter().next()
}

/// Whether a node kind is one the Elixir static guard could accept as a path
/// literal (used to decide a `scope` prefix vs an options-only scope before
/// invoking the guard).
fn is_string_arg_kind(node: Node) -> bool {
    matches!(node.kind(), "string" | "charlist" | "sigil")
}

/// The text of an `alias` node (`UserController`, `HealthPlug`), or `None` for
/// any other node kind (closure, variable, `__MODULE__`, …).
fn alias_text(node: Node, content: &str) -> Option<String> {
    (node.kind() == "alias")
        .then(|| node_text(content, node))
        .flatten()
        .map(str::to_string)
}

/// The name of an `atom` action node (`:show` → `show`), or `None` for any other
/// node kind.
fn atom_name(node: Node, content: &str) -> Option<String> {
    if node.kind() != "atom" {
        return None;
    }
    node_text(content, node).map(|text| text.trim_start_matches(':').to_string())
}
