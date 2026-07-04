//! axum router structural facts (`axum.route.v1`, `axum.nest.v1`).
//!
//! An axum app is a `Router` built with a fluent call chain:
//! `Router::new().route("/users/{id}", get(show).post(create)).nest("/api", api())`.
//! Every `.route(path, method_router)` call registers routes; `.nest(path, sub)`
//! mounts a sub-router at a prefix.
//!
//! This collector is AST-driven (design §4.2): it walks the tree for the
//! `receiver.route(...)` / `receiver.nest(...)` method-call nodes and reads every
//! path argument through the shared Rust static guard
//! (`static_route_arg(_, _, StaticArgLang::Rust)`, ADR-0005), so `format!(...)`,
//! concatenated, and `const`/identifier paths emit nothing (M2 silence — a false
//! static promotes a computed path to a guessed route).
//!
//! ## axum vs actix disambiguation (Task 6 coexistence)
//!
//! Both crates expose a `.route(path, _)` method, so the two collectors share the
//! `rust` dispatch arm and must not double-emit. axum is disambiguated by **both**:
//!
//! 1. **import gate** — the file must reference `axum` (actix files reference
//!    `actix_web` instead); and
//! 2. **method-router arg shape** — axum's second `.route` argument is a
//!    *bare-identifier verb call* (`get(handler)`, chained `get(a).post(b)`),
//!    whereas actix's is `web::get().to(handler)` — a `scoped_identifier`
//!    (`web::get`) base with a `.to(...)` call. [`axum_method_router_verbs`]
//!    requires the chain to bottom out at a bare `identifier` verb, so an actix
//!    `web::get().to(...)` argument returns `None` and emits nothing here.
//!
//! Task 6's actix collector gates on `actix_web` + the `web::get().to()` /
//! attribute-macro shapes, so the two never fire on the same call.
//!
//! ## Receiver tracing (design §4.3, Go `go_http.rs` poison model)
//!
//! `.route`/`.nest` are called on a `Router`. The receiver is single-assignment
//! traced same-file: a `Router::new()` chain (inline or via a variable assigned to
//! one) is a confirmed router; a variable also assigned a *conflicting non-router*
//! value is **poisoned**; a variable whose every assignment provably roots at a
//! non-router value (`let registry = build_registry();`) is **suppressed**. Both
//! poisoned and suppressed receivers stay silent (M2 — we can no longer confirm,
//! or have positively disproven, the receiver is an axum `Router`). A receiver we
//! never see assigned (a function parameter / return value — the common
//! `fn routes(app: Router) -> Router` idiom) is *unknown*, not suppressed, so its
//! routes still emit.
//!
//! ## Route param flavor & the axum 0.7/0.8 under-report
//!
//! axum 0.8 uses brace captures (`/users/{id}`), so paths normalize with
//! `ParamFlavor::Braces`. axum 0.7 used `:id`; the extractor cannot know the crate
//! version and does **not** version-sniff (design §11). A 0.7 `:id` template passes
//! through to a correct `normalized_route_template` join key but its `:id` segment
//! is not recorded in `dynamic_segments` — a documented honest under-report
//! recorded as a rust `open_gaps` entry, never a guessed route.

use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use super::helpers::{
    base_metadata, fact_for_span, insert_string, is_comment_or_string_node, node_text,
    smallest_node_covering_range,
};
use super::scan::{RouteFactSpec, route_fact};
use super::static_arg::{StaticArgLang, static_route_arg};
use super::{AXUM_NEST_PATTERN_ID, AXUM_ROUTE_PATTERN_ID};
use crate::base::http_boundary::{ParamFlavor, normalize_route_template};
use crate::base::span::NormalizedSpan;
use crate::base::types::StructuralFact;

/// Import gate (design §4.2): a file that never references `axum` builds no axum
/// routers, so the collector stays silent. Precision comes from the method-router
/// arg-shape gate in [`axum_method_router_verbs`]; this is the fast bail.
fn imports_axum(content: &str) -> bool {
    content.contains("axum")
}

pub(super) fn collect_axum_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !imports_axum(content) {
        return Vec::new();
    }
    let receivers = collect_router_receivers(tree.root_node(), content);
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        &receivers,
        language,
        tree,
        file_path,
        content,
        &mut facts,
    );
    facts
}

/// Confirmed / poisoned / suppressed state of a same-file `let`-bound router
/// variable. A name absent from the map is *unknown* (e.g. a function parameter)
/// and is treated as acceptable, so `fn add(app: Router) { app.route(...) }`
/// still emits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReceiverState {
    /// Every assignment roots at `Router::new()` (or another confirmed router).
    Router,
    /// A `Router::new()` assignment plus a conflicting non-router assignment —
    /// the variable can no longer be confirmed as an axum `Router` (M2 silence).
    Poisoned,
    /// The variable is assigned in-file but every assignment provably roots at a
    /// non-router value (`let registry = build_registry();`), so we have positive
    /// proof it is *not* an axum `Router`. Suppresses like `Poisoned` (M2) — this
    /// is what separates a locally-known non-router from an unknown parameter.
    Suppressed,
}

#[allow(clippy::too_many_arguments)]
fn walk(
    node: Node,
    receivers: &HashMap<String, ReceiverState>,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if node.kind() == "call_expression" {
        try_route(node, receivers, language, tree, file_path, content, facts);
        try_nest(node, receivers, language, tree, file_path, content, facts);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, receivers, language, tree, file_path, content, facts);
    }
}

// ---------------------------------------------------------------------------
// route registration
// ---------------------------------------------------------------------------

/// Emit one `axum.route.v1` per method-router verb for a
/// `receiver.route("/lit", get(h).post(c))` call. `any`/`any_service` omit the
/// verb (not verb-restricted). Stays silent when the receiver is poisoned, the
/// path is non-static, or the second argument is not an axum method router (which
/// rejects actix's `web::get().to(h)`).
#[allow(clippy::too_many_arguments)]
fn try_route(
    call: Node,
    receivers: &HashMap<String, ReceiverState>,
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
    if receiver_is_suppressed(receiver, content, receivers) {
        return;
    }
    let args = call_arguments(call);
    let (Some(path_arg), Some(router_arg)) = (args.first().copied(), args.get(1).copied()) else {
        return;
    };
    let Some(verbs) = axum_method_router_verbs(router_arg, content) else {
        return;
    };
    let Some(path) = static_route_arg(path_arg, content, StaticArgLang::Rust) else {
        return;
    };

    for verb in dedup_verbs(verbs) {
        let spec = RouteFactSpec {
            framework: "axum",
            pattern_id: AXUM_ROUTE_PATTERN_ID,
            capture_name: "route",
            api_style: "call_routing",
            route_template: path,
            verb: verb.map(VerbClass::name),
            verb_source: verb.map(|_| "attested"),
            flavor: ParamFlavor::Braces,
            prefix: None,
            prefix_key: None,
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
}

// ---------------------------------------------------------------------------
// nest (prefix-registration / mount family)
// ---------------------------------------------------------------------------

/// Emit `axum.nest.v1` for `receiver.nest("/lit", sub_router)` at its own site,
/// following the shipped mount-family metadata shape (`mount_path` /
/// `normalized_mount_path` / `mount_target`). The nested target is a cross-file
/// function/expression, so no route join is guessed — that is Miller's job
/// (decision 0004). Stays silent when the receiver is poisoned or the mount path
/// is non-static.
#[allow(clippy::too_many_arguments)]
fn try_nest(
    call: Node,
    receivers: &HashMap<String, ReceiverState>,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let Some((receiver, method)) = method_call_parts(call, content) else {
        return;
    };
    if method != "nest" {
        return;
    }
    if receiver_is_suppressed(receiver, content, receivers) {
        return;
    }
    let args = call_arguments(call);
    let (Some(path_arg), Some(target_arg)) = (args.first().copied(), args.get(1).copied()) else {
        return;
    };
    let Some(mount_path) = static_route_arg(path_arg, content, StaticArgLang::Rust) else {
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
    let mut metadata = base_metadata("framework", "axum");
    insert_string(&mut metadata, "mount_path", mount_path);
    insert_string(&mut metadata, "normalized_mount_path", &normalized.template);
    insert_string(&mut metadata, "mount_target", mount_target);
    facts.push(fact_for_span(
        file_path,
        language,
        AXUM_NEST_PATTERN_ID,
        "nest",
        anchor.kind(),
        span,
        metadata,
    ));
}

// ---------------------------------------------------------------------------
// method-router verb extraction (the axum-specific arg-shape gate)
// ---------------------------------------------------------------------------

/// A method-router entry: a named HTTP verb, or the wildcard `any`/`any_service`
/// (verb omitted — not verb-restricted).
#[derive(Clone, Copy, PartialEq, Eq)]
enum VerbClass {
    Named(&'static str),
    Any,
}

impl VerbClass {
    fn name(self) -> &'static str {
        match self {
            VerbClass::Named(name) => name,
            VerbClass::Any => unreachable!("Any carries no verb name"),
        }
    }
}

/// Classify a method-router function/method identifier. `Some(Named(..))` for a
/// verb, `Some(Any)` for the all-method `any`/`any_service`, and `None` for any
/// other identifier (which is not part of an axum method router).
fn classify_verb(name: &str) -> Option<VerbClass> {
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
        "any" | "any_service" => Some(VerbClass::Any),
        _ => None,
    }
}

/// Extract the verbs from an axum method-router argument, or `None` when the
/// argument is not a bare-verb method-router chain (which is how actix's
/// `web::get().to(h)` — a `scoped_identifier` base — is rejected).
///
/// The chain is `call_expression`s linked by `field_expression` callees,
/// bottoming out at a `call_expression` whose function is a bare `identifier`
/// verb (`get(h)`). Chained verbs (`get(a).post(b)`) each contribute a verb;
/// non-verb methods in the chain (`.layer(m)`, `.with_state(s)`) are transparent
/// middleware and are skipped. The chain must bottom out at a bare-identifier
/// verb call — a `scoped_identifier` base (`web::get()`), a non-call base, or a
/// non-verb bare base all return `None`.
fn axum_method_router_verbs(arg: Node, content: &str) -> Option<Vec<VerbClass>> {
    let mut verbs = Vec::new();
    let mut node = arg;
    loop {
        if node.kind() != "call_expression" {
            return None;
        }
        let function = node.child_by_field_name("function")?;
        match function.kind() {
            // Base of the chain: `get(handler)` — must be a known verb.
            "identifier" => {
                let verb = classify_verb(node_text(content, function)?)?;
                verbs.push(verb);
                return Some(verbs);
            }
            // Chained call: `<inner>.post(create)` / `<inner>.layer(mw)`.
            "field_expression" => {
                let method = function.child_by_field_name("field")?;
                if let Some(verb) = classify_verb(node_text(content, method)?) {
                    verbs.push(verb);
                }
                node = function.child_by_field_name("value")?;
            }
            // A `scoped_identifier` base (`web::get()`) is actix, not axum.
            _ => return None,
        }
    }
}

/// Drop duplicate verbs (a chain rarely repeats one) while collapsing every
/// `any`/`any_service` to a single verb-omitted entry. Returns each unique
/// `Some(named)` verb plus a single `None` when any wildcard was present.
fn dedup_verbs(verbs: Vec<VerbClass>) -> Vec<Option<VerbClass>> {
    let mut out: Vec<Option<VerbClass>> = Vec::new();
    let mut has_any = false;
    for verb in verbs {
        match verb {
            VerbClass::Any => has_any = true,
            named => {
                if !out.contains(&Some(named)) {
                    out.push(Some(named));
                }
            }
        }
    }
    if has_any {
        out.push(None);
    }
    out
}

// ---------------------------------------------------------------------------
// receiver single-assignment tracing (design §4.3)
// ---------------------------------------------------------------------------

/// Scan same-file `let name = <expr>;` bindings and build the
/// confirmed/poisoned/suppressed state of every router variable, resolving
/// `let app = app.route(...)` self-chains and cross-variable aliases to a
/// fixpoint (mirrors the Go single-assignment model). A name we can prove roots
/// only at non-router values becomes [`ReceiverState::Suppressed`]; a name never
/// assigned in-file (a parameter/field) is left out of the map (unknown → emit).
fn collect_router_receivers(root: Node, content: &str) -> HashMap<String, ReceiverState> {
    // Per-name assignment roots discovered in a single tree walk.
    let mut assignments: HashMap<String, Vec<AssignRoot>> = HashMap::new();
    collect_assignments(root, content, &mut assignments);

    // Fixpoint: a name *could* be a router if any assignment roots at
    // `Router::new()`, aliases another maybe-router name, or aliases an ABSENT
    // name (a parameter/field we cannot prove is a non-router). A name whose
    // every assignment provably roots at a non-router value never enters this set
    // and is suppressed below. This is the inverse of the confirmed-router
    // fixpoint: unknown aliases stay permissive here so `let b = param;` still
    // emits, while `let registry = build_registry();` does not.
    let mut maybe_router: HashMap<String, bool> = HashMap::new();
    loop {
        let mut changed = false;
        for (name, roots) in &assignments {
            if maybe_router.get(name).copied().unwrap_or(false) {
                continue;
            }
            let could_be_router = roots.iter().any(|root| match root {
                AssignRoot::RouterNew => true,
                // An absent alias target is an unknown (parameter) — it could be a
                // router; a present target follows its own maybe-router state.
                AssignRoot::Ident(other) => {
                    !assignments.contains_key(other)
                        || maybe_router.get(other).copied().unwrap_or(false)
                }
                AssignRoot::Other => false,
            });
            if could_be_router {
                maybe_router.insert(name.clone(), true);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut states = HashMap::new();
    for (name, roots) in assignments {
        if !maybe_router.get(&name).copied().unwrap_or(false) {
            // Every assignment provably roots at a non-router value → suppress.
            states.insert(name, ReceiverState::Suppressed);
            continue;
        }
        // Could be a router; a conflicting non-router root poisons it (M2).
        let poisoned = roots.iter().any(|root| matches!(root, AssignRoot::Other));
        states.insert(
            name,
            if poisoned {
                ReceiverState::Poisoned
            } else {
                ReceiverState::Router
            },
        );
    }
    states
}

/// The classified root of an assignment's value expression.
enum AssignRoot {
    /// Roots at `Router::new()` (or `axum::Router::new()`).
    RouterNew,
    /// Roots at a bare identifier (`app.route(...)` → `app`) — resolved against
    /// the other names' state at fixpoint.
    Ident(String),
    /// A non-router value (`build_app()`, `web::scope(...)`, a literal, ...).
    Other,
}

fn collect_assignments(
    node: Node,
    content: &str,
    assignments: &mut HashMap<String, Vec<AssignRoot>>,
) {
    if node.kind() == "let_declaration"
        && let (Some(pattern), Some(value)) = (
            node.child_by_field_name("pattern"),
            node.child_by_field_name("value"),
        )
        && pattern.kind() == "identifier"
        && let Some(name) = node_text(content, pattern)
    {
        assignments
            .entry(name.to_string())
            .or_default()
            .push(value_root(value, content));
    }
    if node.kind() == "assignment_expression"
        && let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        )
        && left.kind() == "identifier"
        && let Some(name) = node_text(content, left)
    {
        assignments
            .entry(name.to_string())
            .or_default()
            .push(value_root(right, content));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_assignments(child, content, assignments);
    }
}

/// Classify the root of a value expression by unwinding method-call chains down
/// to the base: `Router::new()` → `RouterNew`, a bare identifier → `Ident`, and
/// anything else (a non-router constructor call, `web::scope(...)`, a literal) →
/// `Other`.
fn value_root(node: Node, content: &str) -> AssignRoot {
    let mut node = node;
    loop {
        match node.kind() {
            "call_expression" => {
                let Some(function) = node.child_by_field_name("function") else {
                    return AssignRoot::Other;
                };
                match function.kind() {
                    // `<inner>.method(...)` — descend the receiver.
                    "field_expression" => {
                        let Some(value) = function.child_by_field_name("value") else {
                            return AssignRoot::Other;
                        };
                        node = value;
                    }
                    // `Router::new()` / `axum::Router::new()`.
                    "scoped_identifier" if scoped_is_router_new(function, content) => {
                        return AssignRoot::RouterNew;
                    }
                    // Any other base call (`build_app()`, `web::scope(...)`).
                    _ => return AssignRoot::Other,
                }
            }
            "identifier" => {
                return match node_text(content, node) {
                    Some(name) => AssignRoot::Ident(name.to_string()),
                    None => AssignRoot::Other,
                };
            }
            // `(expr)` / `expr.await` wrappers — descend where meaningful.
            "parenthesized_expression" => {
                let mut cursor = node.walk();
                match node.named_children(&mut cursor).next() {
                    Some(inner) => node = inner,
                    None => return AssignRoot::Other,
                }
            }
            _ => return AssignRoot::Other,
        }
    }
}

/// Whether a `scoped_identifier` is `Router::new` (or `…::Router::new`).
fn scoped_is_router_new(scoped: Node, content: &str) -> bool {
    let Some(name) = scoped.child_by_field_name("name") else {
        return false;
    };
    if node_text(content, name) != Some("new") {
        return false;
    }
    let Some(path) = scoped.child_by_field_name("path") else {
        return false;
    };
    match path.kind() {
        "identifier" => node_text(content, path) == Some("Router"),
        // `axum::Router::new` → path is `scoped_identifier` whose name is `Router`.
        "scoped_identifier" => {
            path.child_by_field_name("name")
                .and_then(|inner| node_text(content, inner))
                == Some("Router")
        }
        _ => false,
    }
}

/// Whether a `.route`/`.nest` receiver expression resolves to a *suppressed*
/// router variable — a `Poisoned` one (a `Router::new()` value later reassigned a
/// non-router) or a `Suppressed` one (a variable proven non-router in-file). A
/// `Router::new()` chain, a confirmed router variable, or an unknown receiver
/// (function parameter / field) all return `false` (emit).
fn receiver_is_suppressed(
    receiver: Node,
    content: &str,
    receivers: &HashMap<String, ReceiverState>,
) -> bool {
    match value_root(receiver, content) {
        AssignRoot::Ident(name) => matches!(
            receivers.get(&name),
            Some(ReceiverState::Poisoned | ReceiverState::Suppressed)
        ),
        // `Router::new()` chains and non-identifier receivers are never suppressed.
        AssignRoot::RouterNew | AssignRoot::Other => false,
    }
}

// ---------------------------------------------------------------------------
// call-node helpers
// ---------------------------------------------------------------------------

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
