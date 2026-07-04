//! Rust HTTP client-request facts (`http.client_request.v1`) for the reqwest
//! client.
//!
//! reqwest is the dominant Rust HTTP client and has two clean grammar shapes the
//! shipped Go/Java collectors already prove:
//!
//! - the scoped convenience free function `reqwest::get("https://…")` — a
//!   `call_expression` whose function is a `scoped_identifier` (`reqwest` `::`
//!   `get`); and
//! - the builder verb `client.get("https://…")` /
//!   `reqwest::Client::new().get("…")` — a `receiver.verb("url")` call, the same
//!   `field_expression`-callee shape the Ktor and Go collectors detect.
//!
//! Silence (design §4.4, M2): only a lone static string literal URL (via the
//! shared Rust static guard) produces a fact; `format!(...)`, concatenated, and
//! variable URLs emit nothing. The `reqwest` import gate keeps the match from
//! firing outside a reqwest file.
//!
//! Collision guard: Rust's `HashMap::get(&str)` shares the bare `x.get("k")`
//! shape. For the receiver form only, the URL must be *url-like* (absolute
//! `scheme://…` or a `/`-rooted path) so a map lookup `map.get("key")` (a
//! `relative` literal) stays silent. reqwest requests in practice use absolute or
//! `/`-rooted URLs, so this trims false positives at negligible recall cost (M2 —
//! a false positive is worse than a miss). The scoped `reqwest::get(...)` form is
//! unambiguous and needs no such guard.
//!
//! Deferred as documented `open_gaps` (each an unlike-any-shipped detection shape
//! plus its own fixture): hyper (low-level, request-builder + body-future) and
//! ureq (blocking `ureq::get("url").call()`).

use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Tree};

use super::super::helpers::node_text;
use super::super::static_arg::{StaticArgLang, static_route_arg};
use super::client_fact;
use crate::base::http_boundary::classify_url;
use crate::base::types::StructuralFact;

/// Import gate: a reqwest request goes through the `reqwest` crate, so a file
/// that never names it issues no reqwest requests. Precision comes from the exact
/// `reqwest`-scoped / verb match below; this is the fast bail.
const IMPORT_NEEDLE: &str = "reqwest";

/// The reqwest request verb methods this collector recognises.
fn verb_for_method(method: &str) -> Option<&'static str> {
    match method {
        "get" => Some("GET"),
        "post" => Some("POST"),
        "put" => Some("PUT"),
        "patch" => Some("PATCH"),
        "delete" => Some("DELETE"),
        "head" => Some("HEAD"),
        _ => None,
    }
}

pub(super) fn collect_rust_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains(IMPORT_NEEDLE) {
        return Vec::new();
    }
    // Same-file proof of which local variables hold a reqwest `Client`, so the
    // ambiguous builder-verb shape (`x.get("/path")`) only emits on a proven
    // client receiver (design §4.4, M2).
    let clients = collect_reqwest_clients(tree.root_node(), content);
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        &clients,
        language,
        tree,
        file_path,
        content,
        &mut facts,
    );
    facts
}

fn walk(
    node: Node,
    clients: &HashSet<String>,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if node.kind() == "call_expression"
        && let Some(fact) = client_request_fact(node, clients, language, tree, file_path, content)
    {
        facts.push(fact);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, clients, language, tree, file_path, content, facts);
    }
}

/// Build a `http.client_request.v1` fact for a reqwest verb call, or `None` when
/// the call is not a recognised reqwest verb call with a static URL.
fn client_request_fact(
    call: Node,
    clients: &HashSet<String>,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let function = call.child_by_field_name("function")?;
    let (verb, require_url_like) = match function.kind() {
        // `reqwest::get("url")` — scoped convenience free function. The path must
        // end in the `reqwest` alias so `Foo::get(...)` on another type is not a
        // request.
        "scoped_identifier" => {
            if !scoped_path_is_reqwest(function, content) {
                return None;
            }
            let method = node_text(content, function.child_by_field_name("name")?)?;
            (verb_for_method(method)?, false)
        }
        // `client.get("url")` / `reqwest::Client::new().get("url")` — builder
        // verb. The bare `x.get("k")` shape collides with `HashMap::get`,
        // `store.get`, etc., so the PRIMARY gate is proving the receiver is a
        // reqwest client (inline ctor chain, a same-file variable assigned from
        // one, or a `reqwest::Client` typed parameter). The url-like guard below
        // stays on as a secondary filter (design §4.4, M2).
        "field_expression" => {
            let method = node_text(content, function.child_by_field_name("field")?)?;
            let verb = verb_for_method(method)?;
            let receiver = function.child_by_field_name("value")?;
            if !receiver_is_proven_reqwest(receiver, content, clients, call) {
                return None;
            }
            (verb, true)
        }
        _ => return None,
    };

    let url_argument = first_positional_arg(call)?;
    let target_path = static_route_arg(url_argument, content, StaticArgLang::Rust)?;
    if require_url_like && classify_url(target_path) == "relative" {
        return None;
    }

    client_fact(
        language,
        tree,
        file_path,
        content,
        call.start_byte(),
        call.end_byte(),
        "reqwest",
        target_path,
        verb,
        "attested",
        None,
    )
}

/// Whether a `scoped_identifier`'s path is (or ends in) the `reqwest` crate
/// alias: `reqwest::get` (path is `identifier "reqwest"`).
fn scoped_path_is_reqwest(scoped: Node, content: &str) -> bool {
    let Some(path) = scoped.child_by_field_name("path") else {
        return false;
    };
    match path.kind() {
        "identifier" => node_text(content, path) == Some("reqwest"),
        // `some::reqwest::get` — path ends in `reqwest`.
        "scoped_identifier" => path
            .child_by_field_name("name")
            .and_then(|name| node_text(content, name))
            == Some("reqwest"),
        _ => false,
    }
}

/// The first positional argument value of a `call_expression` — the first named
/// child of its `arguments` node.
fn first_positional_arg(call: Node) -> Option<Node> {
    let mut cursor = call.walk();
    let arguments = call
        .children(&mut cursor)
        .find(|child| child.kind() == "arguments")?;
    let mut arg_cursor = arguments.walk();
    arguments.named_children(&mut arg_cursor).next()
}

// ---------------------------------------------------------------------------
// reqwest client receiver proof (design §4.4, mirrors the axum receiver trace)
// ---------------------------------------------------------------------------

/// The classified root of a value expression, for reqwest-client proof.
enum ReqwestRoot {
    /// Roots at a reqwest `Client` constructor: `reqwest::Client::new()` /
    /// `::builder()`, or the import-gated bare `Client::new()` / `::builder()`.
    ClientCtor,
    /// Roots at a bare identifier — resolved against the proven-client set and the
    /// enclosing function's typed parameters.
    Ident(String),
    /// Any other root (a non-reqwest constructor, a literal, a field access, ...).
    Other,
}

/// Whether a builder-verb receiver is a PROVEN reqwest client: an inline client
/// constructor chain, a same-file variable single-assigned from one, or a
/// parameter typed `reqwest::Client` / `&reqwest::Client`. Any other receiver
/// (a map, a store, an unproven binding) stays silent (M2 — a false client
/// request corrupts the `normalized_route_template` join Miller trusts).
fn receiver_is_proven_reqwest(
    receiver: Node,
    content: &str,
    clients: &HashSet<String>,
    call: Node,
) -> bool {
    match reqwest_value_root(receiver, content) {
        ReqwestRoot::ClientCtor => true,
        ReqwestRoot::Ident(name) => {
            clients.contains(&name) || ident_is_reqwest_typed_param(&name, call, content)
        }
        ReqwestRoot::Other => false,
    }
}

/// Scan same-file `let name = <expr>;` / `name = <expr>;` bindings and return the
/// set of variables PROVEN to hold a reqwest `Client`: every assignment roots at
/// a reqwest client constructor (directly, or via an alias to another proven
/// client). A name with any conflicting non-reqwest assignment is left out
/// (unproven → silence). Mirrors the axum single-assignment receiver trace, but
/// proof-to-emit rather than proof-to-suppress.
fn collect_reqwest_clients(root: Node, content: &str) -> HashSet<String> {
    let mut assignments: HashMap<String, Vec<ReqwestRoot>> = HashMap::new();
    collect_reqwest_assignments(root, content, &mut assignments);

    // Fixpoint: a name is client-ish if any assignment roots at a client ctor or
    // aliases another (already client-ish) name.
    let mut client_ish: HashMap<String, bool> = HashMap::new();
    loop {
        let mut changed = false;
        for (name, roots) in &assignments {
            if client_ish.get(name).copied().unwrap_or(false) {
                continue;
            }
            let is_client = roots.iter().any(|root| match root {
                ReqwestRoot::ClientCtor => true,
                ReqwestRoot::Ident(other) => client_ish.get(other).copied().unwrap_or(false),
                ReqwestRoot::Other => false,
            });
            if is_client {
                client_ish.insert(name.clone(), true);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Proven = client-ish with no conflicting non-reqwest assignment: a variable
    // that is sometimes a non-client can no longer be proven at the call site.
    assignments
        .into_iter()
        .filter(|(name, roots)| {
            client_ish.get(name).copied().unwrap_or(false)
                && !roots.iter().any(|root| matches!(root, ReqwestRoot::Other))
        })
        .map(|(name, _)| name)
        .collect()
}

fn collect_reqwest_assignments(
    node: Node,
    content: &str,
    assignments: &mut HashMap<String, Vec<ReqwestRoot>>,
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
            .push(reqwest_value_root(value, content));
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
            .push(reqwest_value_root(right, content));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_reqwest_assignments(child, content, assignments);
    }
}

/// Classify the root of a value expression by unwinding method-call chains down
/// to the base: a reqwest `Client` constructor → `ClientCtor`, a bare identifier
/// → `Ident`, anything else → `Other`. Mirrors the axum `value_root` shape.
fn reqwest_value_root(node: Node, content: &str) -> ReqwestRoot {
    let mut node = node;
    loop {
        match node.kind() {
            "call_expression" => {
                let Some(function) = node.child_by_field_name("function") else {
                    return ReqwestRoot::Other;
                };
                match function.kind() {
                    // `<inner>.method(...)` (`.build()`, `.timeout(..)`) — descend.
                    "field_expression" => {
                        let Some(value) = function.child_by_field_name("value") else {
                            return ReqwestRoot::Other;
                        };
                        node = value;
                    }
                    // `reqwest::Client::new()` / `Client::builder()` base.
                    "scoped_identifier" if scoped_is_reqwest_client_ctor(function, content) => {
                        return ReqwestRoot::ClientCtor;
                    }
                    _ => return ReqwestRoot::Other,
                }
            }
            "identifier" => {
                return match node_text(content, node) {
                    Some(name) => ReqwestRoot::Ident(name.to_string()),
                    None => ReqwestRoot::Other,
                };
            }
            "parenthesized_expression" => {
                let mut cursor = node.walk();
                match node.named_children(&mut cursor).next() {
                    Some(inner) => node = inner,
                    None => return ReqwestRoot::Other,
                }
            }
            _ => return ReqwestRoot::Other,
        }
    }
}

/// Whether a `scoped_identifier` is a reqwest `Client` constructor —
/// `reqwest::Client::new` / `reqwest::Client::builder`, or the import-gated bare
/// `Client::new` / `Client::builder` (`use reqwest::Client;`).
fn scoped_is_reqwest_client_ctor(scoped: Node, content: &str) -> bool {
    let ctor = scoped
        .child_by_field_name("name")
        .and_then(|name| node_text(content, name));
    if ctor != Some("new") && ctor != Some("builder") {
        return false;
    }
    let Some(path) = scoped.child_by_field_name("path") else {
        return false;
    };
    match path.kind() {
        // Bare `Client::new` — the `use reqwest::Client;` idiom (import-gated).
        "identifier" => node_text(content, path) == Some("Client"),
        // `reqwest::Client::new` → path is `reqwest::Client` (name `Client`).
        "scoped_identifier" => {
            path.child_by_field_name("name")
                .and_then(|name| node_text(content, name))
                == Some("Client")
        }
        _ => false,
    }
}

/// Whether `name` is a parameter of the enclosing `fn` typed as a reqwest
/// `Client` (`reqwest::Client`, `&reqwest::Client`, or the import-gated bare
/// `Client`) — the common injected-shared-client idiom.
fn ident_is_reqwest_typed_param(name: &str, from: Node, content: &str) -> bool {
    let mut cursor = Some(from);
    while let Some(node) = cursor {
        if node.kind() == "function_item" {
            return function_param_is_reqwest_client(node, name, content);
        }
        cursor = node.parent();
    }
    false
}

/// Whether the `function_item`'s parameter list binds `name` to a reqwest
/// `Client` type.
fn function_param_is_reqwest_client(function_item: Node, name: &str, content: &str) -> bool {
    let Some(params) = function_item.child_by_field_name("parameters") else {
        return false;
    };
    let mut cursor = params.walk();
    params.named_children(&mut cursor).any(|param| {
        param.kind() == "parameter"
            && param
                .child_by_field_name("pattern")
                .and_then(|pattern| node_text(content, pattern))
                == Some(name)
            && param
                .child_by_field_name("type")
                .is_some_and(|ty| type_is_reqwest_client(ty, content))
    })
}

/// Whether a parameter type node names the reqwest `Client`, unwrapping a leading
/// `&`/`&mut` reference. Bare `Client` counts (import-gated).
fn type_is_reqwest_client(ty: Node, content: &str) -> bool {
    let ty = if ty.kind() == "reference_type" {
        match ty.child_by_field_name("type") {
            Some(inner) => inner,
            None => return false,
        }
    } else {
        ty
    };
    match ty.kind() {
        "type_identifier" => node_text(content, ty) == Some("Client"),
        "scoped_type_identifier" => {
            ty.child_by_field_name("name")
                .and_then(|name| node_text(content, name))
                == Some("Client")
        }
        _ => false,
    }
}
