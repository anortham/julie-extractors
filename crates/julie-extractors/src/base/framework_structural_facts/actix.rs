//! actix-web structural facts (`actix.attribute_route.v1`, `actix.scope_route.v1`,
//! `actix.mount.v1`).
//!
//! actix-web registers routes through **two** provenance models, so — mirroring
//! the shipped `aspnet.attribute_route.v1` vs `aspnet.minimal_api.route.v1` split
//! (design §2a/§4.5) — this collector emits two route pattern ids plus a mount:
//!
//! 1. **Attribute macros** (`#[get("/x")]`, `#[route("/x", method = "GET")]`) on a
//!    handler `fn` → [`ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID`]. The verb is ALWAYS
//!    known (from the macro name or a `method =` argument) and registration is
//!    **cross-file** (the app mounts the handler elsewhere), so there is no
//!    same-file prefix: no `route_group_prefix`/`effective_route_template` keys.
//! 2. **Scope-chained routes** (`web::scope("/api").route("/x", web::post().to(h))`)
//!    → [`ACTIX_SCOPE_ROUTE_PATTERN_ID`]. The scope prefix is same-file in the
//!    same call chain, so it flows into `route_group_prefix` +
//!    `effective_route_template`; the verb is OPT (present for `web::<verb>()`,
//!    omitted for the method-agnostic `web::route()`).
//! 3. **Scope mounts** (`web::scope("/api").configure(init)` / `.service(sub)`) →
//!    [`ACTIX_MOUNT_PATTERN_ID`], the prefix-registration fact recorded at the
//!    scope site (following the `express.router_mount.v1` shape). The delegated
//!    routes live in a cross-file `configure`/service target, so no route join is
//!    guessed (Miller's job, decision 0004).
//!
//! ## axum vs actix disambiguation (Task 6 coexistence)
//!
//! axum's collector shares the `rust` dispatch arm with this one. They never
//! double-emit because actix is gated on **both**:
//!
//! 1. **import gate** — the file must reference `actix_web` (axum files reference
//!    `axum` instead); and
//! 2. **arg shape** — actix routes are attribute macros (axum has none), and
//!    actix's scope routes bottom out at `web::scope(...)` receivers with
//!    `web::<verb>().to(h)` method routers (a `scoped_identifier` base), whereas
//!    axum's `.route` method router is a *bare-identifier* verb (`get(h)`). axum's
//!    [`super::axum`] rejects the `web::get().to()` shape, and this collector
//!    rejects axum's bare-identifier method routers and `Router::new()` receivers.
//!
//! ## Static-literal silence (design §4.4, ADR-0005)
//!
//! Every route/scope/mount path is read through the shared Rust static guard
//! (`static_route_arg(_, _, StaticArgLang::Rust)`), so `format!(...)`,
//! concatenated, and `const`/identifier paths emit nothing (M2 silence). A route
//! whose enclosing scope prefix is non-static also stays silent, because its
//! absolute path — and therefore its join key — is unknowable.
//!
//! ## Route param flavor
//!
//! actix uses brace captures (`/users/{id}`, `/{tail:.*}`), so paths normalize
//! with `ParamFlavor::Braces` (`{id}` → `:id`).

use tree_sitter::{Node, Tree};

use super::helpers::{
    base_metadata, fact_for_span, insert_string, is_comment_or_string_node, node_text,
    smallest_node_covering_range,
};
use super::scan::{RouteFactSpec, route_fact};
use super::static_arg::{StaticArgLang, static_route_arg};
use super::{
    ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID, ACTIX_MOUNT_PATTERN_ID, ACTIX_SCOPE_ROUTE_PATTERN_ID,
};
use crate::base::http_boundary::{ParamFlavor, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

/// Import gate (design §4.2): a file that never references `actix_web` builds no
/// actix routes, so the collector stays silent. Precision comes from the arg
/// shapes below; this is the fast bail that also keeps axum and actix apart.
fn imports_actix(content: &str) -> bool {
    content.contains("actix_web")
}

pub(super) fn collect_actix_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !imports_actix(content) {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(tree.root_node(), language, tree, file_path, content, &mut facts);
    facts
}

fn walk(
    node: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    match node.kind() {
        // `#[get("/x")]` / `#[route("/x", method = "GET")]` macros on a handler fn.
        "attribute_item" => try_attribute_route(node, language, tree, file_path, content, facts),
        // `web::scope("/api").route(...)` scope routes and
        // `web::scope("/api").configure/service(...)` mounts.
        "call_expression" => {
            try_scope_route(node, language, tree, file_path, content, facts);
            try_mount(node, language, tree, file_path, content, facts);
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, language, tree, file_path, content, facts);
    }
}

// ---------------------------------------------------------------------------
// attribute-macro routes (`actix.attribute_route.v1`)
// ---------------------------------------------------------------------------

/// Emit one `actix.attribute_route.v1` per verb for a `#[get("/x")]`-style macro
/// on a handler `fn`. The verb is ALWAYS known (from the macro name, or from each
/// `method = "VERB"` argument of `#[route(...)]`), so a `#[route]` with two
/// methods emits two facts. Registration is cross-file, so the fact carries no
/// prefix keys. The fact anchors on the **handler `function_item`** (a *following
/// sibling* of the attribute, verified by probe) so its `containing_symbol_id`
/// binds to the handler, not the enclosing module.
fn try_attribute_route(
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
    let Some(macro_name) = attribute_macro_name(attribute, content) else {
        return;
    };
    let Some(verbs) = actix_attribute_verbs(macro_name, attribute, content) else {
        return;
    };
    // The path is the first (positional) argument of the macro's token tree.
    let Some(token_tree) = attribute.child_by_field_name("arguments") else {
        return;
    };
    let mut arg_cursor = token_tree.walk();
    let Some(path_arg) = token_tree.named_children(&mut arg_cursor).next() else {
        return;
    };
    let Some(path) = static_route_arg(path_arg, content, StaticArgLang::Rust) else {
        return;
    };
    // Bind to the handler fn: anchor the fact on the `function_item` the macro
    // decorates (its following sibling), not the attribute (whose byte span sits
    // *outside* the handler symbol and would bind to the enclosing module).
    let Some(handler) = following_function_item(attr_item) else {
        return;
    };

    for verb in verbs {
        let spec = RouteFactSpec {
            framework: "actix",
            pattern_id: ACTIX_ATTRIBUTE_ROUTE_PATTERN_ID,
            capture_name: "attribute_route",
            api_style: "attribute",
            route_template: path,
            verb: Some(&verb),
            verb_source: Some("attested"),
            flavor: ParamFlavor::Braces,
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
}

/// Classify an attribute macro name into its verb(s), or `None` when it is not an
/// actix route macro. `#[get]`/`#[post]`/… map to their verb; `#[route(...)]`
/// takes its verbs from every `method = "VERB"` argument (uppercased), staying
/// silent (`None`) when no static `method =` is present.
fn actix_attribute_verbs(
    macro_name: &str,
    attribute: Node,
    content: &str,
) -> Option<Vec<String>> {
    if let Some(verb) = verb_macro(macro_name) {
        return Some(vec![verb.to_string()]);
    }
    if macro_name == "route" {
        let verbs = route_macro_methods(attribute, content);
        return (!verbs.is_empty()).then_some(verbs);
    }
    None
}

/// The uppercase HTTP verb for a verb-specific actix attribute macro
/// (`#[get]`/`#[post]`/…), or `None` for any other macro name.
fn verb_macro(name: &str) -> Option<&'static str> {
    match name {
        "get" => Some("GET"),
        "post" => Some("POST"),
        "put" => Some("PUT"),
        "patch" => Some("PATCH"),
        "delete" => Some("DELETE"),
        "head" => Some("HEAD"),
        "options" => Some("OPTIONS"),
        "trace" => Some("TRACE"),
        "connect" => Some("CONNECT"),
        _ => None,
    }
}

/// Extract the uppercased verbs from a `#[route("/x", method = "GET", method =
/// "POST")]` macro. Walks the macro token tree for `method` identifiers each
/// followed by a static string-literal value; a non-literal `method =` value is
/// skipped (M2 silence), so `route` with no static method emits nothing.
fn route_macro_methods(attribute: Node, content: &str) -> Vec<String> {
    let Some(token_tree) = attribute.child_by_field_name("arguments") else {
        return Vec::new();
    };
    let mut methods = Vec::new();
    let mut cursor = token_tree.walk();
    let mut expect_value = false;
    for child in token_tree.named_children(&mut cursor) {
        if child.kind() == "identifier" && node_text(content, child) == Some("method") {
            expect_value = true;
        } else if expect_value {
            if let Some(value) = static_route_arg(child, content, StaticArgLang::Rust) {
                methods.push(value.to_ascii_uppercase());
            }
            expect_value = false;
        }
    }
    methods
}

// ---------------------------------------------------------------------------
// scope-chained routes (`actix.scope_route.v1`)
// ---------------------------------------------------------------------------

/// Emit `actix.scope_route.v1` for a `web::scope("/api").route("/x",
/// web::post().to(h))` call. The scope prefix is read same-file by walking the
/// `.route` receiver chain down to its base `web::scope(literal)`, so it flows
/// into `route_group_prefix` + `effective_route_template`; the verb comes from
/// the `web::<verb>()` method router (OPT — omitted for `web::route()`). Stays
/// silent unless the receiver bottoms at a static-prefix `web::scope`, the method
/// router is a `web::<verb>().to(...)` builder, and the route path is static.
fn try_scope_route(
    call: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let Some((receiver, method)) = method_call_parts(call, content) else {
        return;
    };
    if method != "route" {
        return;
    }
    let Some(prefix) = scope_prefix(receiver, content) else {
        return;
    };
    let args = call_arguments(call);
    let (Some(path_arg), Some(router_arg)) = (args.first().copied(), args.get(1).copied()) else {
        return;
    };
    let Some(verb) = actix_method_router_verb(router_arg, content) else {
        return;
    };
    let Some(path) = static_route_arg(path_arg, content, StaticArgLang::Rust) else {
        return;
    };

    let spec = RouteFactSpec {
        framework: "actix",
        pattern_id: ACTIX_SCOPE_ROUTE_PATTERN_ID,
        capture_name: "scope_route",
        api_style: "call_routing",
        route_template: path,
        verb: verb.name(),
        verb_source: verb.name().map(|_| "attested"),
        flavor: ParamFlavor::Braces,
        prefix: Some(prefix),
        prefix_key: Some("route_group_prefix"),
    };
    if let Some(fact) = route_fact(
        language,
        tree,
        file_path,
        content,
        call.start_byte(),
        call.end_byte(),
        spec,
        |_| {},
    ) {
        facts.push(fact);
    }
}

/// A method-router verb: a named HTTP verb, or the method-agnostic `web::route()`
/// (verb omitted — not verb-restricted).
#[derive(Clone, Copy)]
enum VerbClass {
    Named(&'static str),
    Any,
}

impl VerbClass {
    fn name(self) -> Option<&'static str> {
        match self {
            VerbClass::Named(name) => Some(name),
            VerbClass::Any => None,
        }
    }
}

/// Extract the verb from an actix method-router argument
/// (`web::post().to(handler)`), or `None` when the argument is not a
/// `web::<verb>()` builder — which is how axum's bare-identifier `get(h)` method
/// router (and every non-actix shape) is rejected. Middleware chained after the
/// verb (`.wrap(m)` / `.guard(g)` / `.to(h)`) is transparent; the chain must
/// bottom out at a `web::<verb>()` / `web::route()` scoped call.
fn actix_method_router_verb(arg: Node, content: &str) -> Option<VerbClass> {
    let mut node = arg;
    loop {
        if node.kind() != "call_expression" {
            return None;
        }
        let function = node.child_by_field_name("function")?;
        match function.kind() {
            // Base of the chain: `web::post()` — must be a `web::<verb>` call.
            "scoped_identifier" => return actix_web_verb(function, content),
            // Chained call: `web::post().to(h)` / `.wrap(m)` — descend the receiver.
            "field_expression" => {
                node = function.child_by_field_name("value")?;
            }
            // A bare-identifier base (`get(h)`) is axum, not actix.
            _ => return None,
        }
    }
}

/// Classify a `web::<name>` scoped identifier into its verb: `Some(Named(..))`
/// for `web::get`/…, `Some(Any)` for the method-agnostic `web::route`, and `None`
/// for any other base (a non-`web` path, or `web::<unknown>`).
fn actix_web_verb(scoped: Node, content: &str) -> Option<VerbClass> {
    if !scoped_path_is_web(scoped, content) {
        return None;
    }
    let name = scoped
        .child_by_field_name("name")
        .and_then(|name| node_text(content, name))?;
    match name {
        "get" => Some(VerbClass::Named("GET")),
        "post" => Some(VerbClass::Named("POST")),
        "put" => Some(VerbClass::Named("PUT")),
        "patch" => Some(VerbClass::Named("PATCH")),
        "delete" => Some(VerbClass::Named("DELETE")),
        "head" => Some(VerbClass::Named("HEAD")),
        "options" => Some(VerbClass::Named("OPTIONS")),
        "trace" => Some(VerbClass::Named("TRACE")),
        "connect" => Some(VerbClass::Named("CONNECT")),
        // `web::route()` builds a method-agnostic route → verb omitted.
        "route" => Some(VerbClass::Any),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// scope mounts (`actix.mount.v1`)
// ---------------------------------------------------------------------------

/// Emit `actix.mount.v1` for a `web::scope("/api").configure(init)` /
/// `.service(sub)` call: the scope prefix registered at its own site, following
/// the shipped mount-family shape (`mount_path`/`normalized_mount_path`/
/// `mount_target`). The delegated routes live in the cross-file `configure`/
/// service target, so no route join is guessed (Miller's job, decision 0004).
/// Stays silent unless the receiver chain bottoms at a static-prefix `web::scope`.
fn try_mount(
    call: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let Some((receiver, method)) = method_call_parts(call, content) else {
        return;
    };
    if method != "configure" && method != "service" {
        return;
    }
    let Some(mount_path) = scope_prefix(receiver, content) else {
        return;
    };
    let args = call_arguments(call);
    let Some(target_arg) = args.first().copied() else {
        return;
    };
    let Some(mount_target) = node_text(content, target_arg) else {
        return;
    };

    let start = call.start_byte();
    let end = call.end_byte();
    let Some(anchor) = smallest_node_covering_range(tree.root_node(), start, end) else {
        return;
    };
    if is_comment_or_string_node(anchor.kind()) {
        return;
    }
    let Some(span) = NormalizedSpan::from_content_range(content, start, end) else {
        return;
    };

    let normalized = normalize_route_template(mount_path, ParamFlavor::Braces);
    let mut metadata = base_metadata("framework", "actix");
    insert_string(&mut metadata, "mount_path", mount_path);
    insert_string(&mut metadata, "normalized_mount_path", &normalized.template);
    insert_string(&mut metadata, "mount_target", mount_target);
    facts.push(fact_for_span(
        file_path,
        language,
        ACTIX_MOUNT_PATTERN_ID,
        "mount",
        anchor.kind(),
        span,
        metadata,
    ));
}

// ---------------------------------------------------------------------------
// shared shape helpers
// ---------------------------------------------------------------------------

/// The static literal prefix of the `web::scope("/lit")` a `.route`/`.configure`/
/// `.service` receiver chain bottoms out at, or `None` when the chain does not
/// root at a static-prefix `web::scope` (e.g. an `App::new()` base, a
/// variable-bound scope, or a non-literal scope argument — all M2-silent).
fn scope_prefix<'a>(receiver: Node, content: &'a str) -> Option<&'a str> {
    let mut node = receiver;
    loop {
        if node.kind() != "call_expression" {
            return None;
        }
        let function = node.child_by_field_name("function")?;
        match function.kind() {
            // Base of the chain: `web::scope("/lit")`.
            "scoped_identifier" => {
                if !scoped_is_web_scope(function, content) {
                    return None;
                }
                let args = call_arguments(node);
                let path_arg = args.first().copied()?;
                return static_route_arg(path_arg, content, StaticArgLang::Rust);
            }
            // Chained call: `<inner>.route(...)` / `.service(...)` — descend.
            "field_expression" => {
                node = function.child_by_field_name("value")?;
            }
            _ => return None,
        }
    }
}

/// Whether a `scoped_identifier` is `web::scope` (or `…::web::scope`).
fn scoped_is_web_scope(scoped: Node, content: &str) -> bool {
    scoped
        .child_by_field_name("name")
        .and_then(|name| node_text(content, name))
        == Some("scope")
        && scoped_path_is_web(scoped, content)
}

/// Whether a `scoped_identifier`'s `path` resolves to `web` — either the bare
/// `web` identifier (`web::scope`) or a nested path ending in `web`
/// (`actix_web::web::scope`).
fn scoped_path_is_web(scoped: Node, content: &str) -> bool {
    let Some(path) = scoped.child_by_field_name("path") else {
        return false;
    };
    match path.kind() {
        "identifier" => node_text(content, path) == Some("web"),
        // `actix_web::web::scope` → path is `scoped_identifier` whose name is `web`.
        "scoped_identifier" => {
            path.child_by_field_name("name")
                .and_then(|inner| node_text(content, inner))
                == Some("web")
        }
        _ => false,
    }
}

/// The macro name of an `attribute` node (`#[get(...)]` → `get`,
/// `#[actix_web::get(...)]` → `get`): the last path segment of the attribute's
/// leading identifier / scoped-identifier.
fn attribute_macro_name<'a>(attribute: Node, content: &'a str) -> Option<&'a str> {
    let mut cursor = attribute.walk();
    for child in attribute.children(&mut cursor) {
        match child.kind() {
            "identifier" => return node_text(content, child),
            "scoped_identifier" => {
                return child
                    .child_by_field_name("name")
                    .and_then(|name| node_text(content, name));
            }
            _ => {}
        }
    }
    None
}

/// The handler `function_item` an attribute decorates: the first following
/// sibling that is a `function_item`, skipping intervening `attribute_item`s
/// (a handler may carry several attributes). Returns `None` if a non-attribute,
/// non-function sibling intervenes (so a macro on a non-fn item emits nothing).
fn following_function_item(attr_item: Node) -> Option<Node> {
    let mut sibling = attr_item.next_sibling();
    while let Some(node) = sibling {
        match node.kind() {
            "attribute_item" => sibling = node.next_sibling(),
            "function_item" => return Some(node),
            _ => return None,
        }
    }
    None
}

/// The `(receiver, method_name)` of a `receiver.method(...)` call, or `None` when
/// the call's function is not a `field_expression` method callee.
fn method_call_parts<'a, 't>(call: Node<'t>, content: &'a str) -> Option<(Node<'t>, &'a str)> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "field_expression" {
        return None;
    }
    let receiver = function.child_by_field_name("value")?;
    let method = function.child_by_field_name("field")?;
    Some((receiver, node_text(content, method)?))
}

/// The positional argument value nodes of a `call_expression`, in order.
fn call_arguments(call: Node) -> Vec<Node> {
    let Some(arguments) = ({
        let mut cursor = call.walk();
        call.children(&mut cursor)
            .find(|child| child.kind() == "arguments")
    }) else {
        return Vec::new();
    };
    let mut cursor = arguments.walk();
    arguments.named_children(&mut cursor).collect()
}

/// The first child of `node` whose kind is `kind`.
fn child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|child| child.kind() == kind)
}
