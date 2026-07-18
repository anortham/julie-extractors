//! Rust HTTP client-request facts (`http.client_request.v1`) for reqwest, hyper,
//! and ureq.
//!
//! - reqwest: scoped `reqwest::get("…")` and proven-receiver builder verbs
//! - hyper: import-gated `Request::builder()` / `hyper::Request::builder()` chains
//!   with a static `.uri(...)` and optional static `.method(...)`
//! - ureq: scoped `ureq::<verb>(static_url)` free functions
//!
//! Silence (design §4.4, M2): only lone static string literal URLs produce a fact;
//! dynamic URLs/methods, unproven receivers, and unrelated builders stay silent.

use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Tree};

use super::super::helpers::node_text;
use super::super::static_arg::{StaticArgLang, static_route_arg};
use super::client_fact;
use crate::base::http_boundary::classify_url;
use crate::base::types::StructuralFact;

struct RustClientRequest<'a> {
    client: &'static str,
    target_path: &'a str,
    verb: &'static str,
    verb_source: &'static str,
}

/// The reqwest/ureq request verb methods this collector recognises.
fn verb_for_method(method: &str) -> Option<&'static str> {
    match method {
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

fn http_verb_name(name: &str) -> Option<&'static str> {
    match name {
        "GET" | "Get" | "get" => Some("GET"),
        "POST" | "Post" | "post" => Some("POST"),
        "PUT" | "Put" | "put" => Some("PUT"),
        "PATCH" | "Patch" | "patch" => Some("PATCH"),
        "DELETE" | "Delete" | "delete" => Some("DELETE"),
        "HEAD" | "Head" | "head" => Some("HEAD"),
        "OPTIONS" | "Options" | "options" => Some("OPTIONS"),
        _ => None,
    }
}

pub(super) fn collect_rust_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let has_reqwest = content.contains("reqwest");
    // Coarse presence gate (may match comments/strings); bare `Request::builder`
    // additionally requires a parser-backed `use hyper...` import below.
    let has_hyper = content.contains("hyper");
    let has_ureq = content.contains("ureq");
    if !has_reqwest && !has_hyper && !has_ureq {
        return Vec::new();
    }

    let clients = if has_reqwest {
        collect_reqwest_clients(tree.root_node(), content)
    } else {
        HashSet::new()
    };
    let has_hyper_import = has_hyper && has_rust_use_crate(tree.root_node(), content, "hyper");

    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        &clients,
        has_reqwest,
        has_hyper,
        has_hyper_import,
        has_ureq,
        language,
        tree,
        file_path,
        content,
        &mut facts,
    );
    facts
}

#[allow(clippy::too_many_arguments)]
fn walk(
    node: Node,
    clients: &HashSet<String>,
    has_reqwest: bool,
    has_hyper: bool,
    has_hyper_import: bool,
    has_ureq: bool,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if node.kind() == "call_expression"
        && let Some(req) = classify_rust_client_request(
            node,
            clients,
            has_reqwest,
            has_hyper,
            has_hyper_import,
            has_ureq,
            content,
        )
        && let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            node.start_byte(),
            node.end_byte(),
            req.client,
            req.target_path,
            req.verb,
            req.verb_source,
            None,
        )
    {
        facts.push(fact);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            child,
            clients,
            has_reqwest,
            has_hyper,
            has_hyper_import,
            has_ureq,
            language,
            tree,
            file_path,
            content,
            facts,
        );
    }
}

fn classify_rust_client_request<'a>(
    call: Node<'_>,
    reqwest_clients: &HashSet<String>,
    has_reqwest: bool,
    has_hyper: bool,
    has_hyper_import: bool,
    has_ureq: bool,
    content: &'a str,
) -> Option<RustClientRequest<'a>> {
    if has_reqwest && let Some(req) = reqwest_request(call, reqwest_clients, content) {
        return Some(req);
    }
    if has_hyper && let Some(req) = hyper_builder_request(call, content, has_hyper_import) {
        return Some(req);
    }
    if has_ureq && let Some(req) = ureq_request(call, content) {
        return Some(req);
    }
    None
}

fn reqwest_request<'a>(
    call: Node<'_>,
    clients: &HashSet<String>,
    content: &'a str,
) -> Option<RustClientRequest<'a>> {
    let function = call.child_by_field_name("function")?;
    let (verb, require_url_like) = match function.kind() {
        "scoped_identifier" => {
            if !scoped_path_is_reqwest(function, content) {
                return None;
            }
            let method = node_text(content, function.child_by_field_name("name")?)?;
            (verb_for_method(method)?, false)
        }
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

    Some(RustClientRequest {
        client: "reqwest",
        target_path,
        verb,
        verb_source: "attested",
    })
}

/// Hyper `Request::builder()` / `hyper::Request::builder()` chain with static URI.
/// Emits once per chain by anchoring on the outermost call (not nested as a
/// receiver of another builder call).
fn hyper_builder_request<'a>(
    call: Node<'_>,
    content: &'a str,
    has_hyper_import: bool,
) -> Option<RustClientRequest<'a>> {
    if !is_builder_chain_terminal(call) {
        return None;
    }

    let mut target_path: Option<&'a str> = None;
    let mut verb: &'static str = "GET";
    let mut verb_source: &'static str = "default";
    let mut method_dynamic = false;

    let mut node = call;
    loop {
        let function = node.child_by_field_name("function")?;
        match function.kind() {
            "field_expression" => {
                let field = node_text(content, function.child_by_field_name("field")?)?;
                match field {
                    "uri" => {
                        let arg = first_positional_arg(node)?;
                        let path = static_route_arg(arg, content, StaticArgLang::Rust)?;
                        target_path = Some(path);
                    }
                    "method" => match parse_hyper_method_arg(node, content) {
                        Some(v) => {
                            verb = v;
                            verb_source = "attested";
                        }
                        None => method_dynamic = true,
                    },
                    _ => {}
                }
                let receiver = function.child_by_field_name("value")?;
                if receiver.kind() != "call_expression" {
                    return None;
                }
                node = receiver;
            }
            "scoped_identifier"
                if scoped_is_hyper_request_builder(function, content, has_hyper_import) =>
            {
                if method_dynamic {
                    return None;
                }
                let target_path = target_path?;
                return Some(RustClientRequest {
                    client: "hyper",
                    target_path,
                    verb,
                    verb_source,
                });
            }
            _ => return None,
        }
    }
}

fn ureq_request<'a>(call: Node<'_>, content: &'a str) -> Option<RustClientRequest<'a>> {
    let function = call.child_by_field_name("function")?;
    if function.kind() != "scoped_identifier" || !scoped_path_is_name(function, content, "ureq") {
        return None;
    }
    let method = node_text(content, function.child_by_field_name("name")?)?;
    let verb = verb_for_method(method)?;
    let target_path = static_route_arg(first_positional_arg(call)?, content, StaticArgLang::Rust)?;
    Some(RustClientRequest {
        client: "ureq",
        target_path,
        verb,
        verb_source: "attested",
    })
}

/// True when `call` is not the receiver of another call in a fluent chain.
fn is_builder_chain_terminal(call: Node<'_>) -> bool {
    let Some(parent) = call.parent() else {
        return true;
    };
    if parent.kind() != "field_expression" {
        return true;
    }
    let Some(grand) = parent.parent() else {
        return true;
    };
    if grand.kind() != "call_expression" {
        return true;
    }
    // Nested: outer.call where function.value == this call
    grand.child_by_field_name("function") != Some(parent)
        || parent.child_by_field_name("value") != Some(call)
}

fn scoped_is_hyper_request_builder(
    scoped: Node<'_>,
    content: &str,
    has_hyper_import: bool,
) -> bool {
    let ctor = scoped
        .child_by_field_name("name")
        .and_then(|name| node_text(content, name));
    if ctor != Some("builder") {
        return false;
    }
    let Some(path) = scoped.child_by_field_name("path") else {
        return false;
    };
    match path.kind() {
        // Bare `Request::builder` — only with a parser-backed `use hyper...`.
        "identifier" => node_text(content, path) == Some("Request") && has_hyper_import,
        // `hyper::Request::builder` → path name is `Request`, path's path is `hyper`.
        "scoped_identifier" => {
            path.child_by_field_name("name")
                .and_then(|name| node_text(content, name))
                == Some("Request")
                && path
                    .child_by_field_name("path")
                    .and_then(|inner| match inner.kind() {
                        "identifier" => node_text(content, inner),
                        "scoped_identifier" => inner
                            .child_by_field_name("name")
                            .and_then(|name| node_text(content, name)),
                        _ => None,
                    })
                    == Some("hyper")
        }
        _ => false,
    }
}

/// True when a real `use_declaration` AST node imports `crate_name` (or a path
/// under it). Comments and string literals never produce `use_declaration` nodes.
fn has_rust_use_crate(node: Node, content: &str, crate_name: &str) -> bool {
    if node.kind() == "use_declaration"
        && let Some(text) = node_text(content, node)
    {
        let rest = text.trim_start().trim_start_matches("use").trim_start();
        if rest == crate_name
            || rest.starts_with(&format!("{crate_name}::"))
            || rest.starts_with(&format!("{crate_name};"))
            || rest.starts_with(&format!("{crate_name} "))
        {
            return true;
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if has_rust_use_crate(child, content, crate_name) {
            return true;
        }
    }
    false
}

fn parse_hyper_method_arg(method_call: Node<'_>, content: &str) -> Option<&'static str> {
    let arg = first_positional_arg(method_call)?;
    if let Some(literal) = static_route_arg(arg, content, StaticArgLang::Rust) {
        return http_verb_name(literal);
    }
    match arg.kind() {
        "scoped_identifier" => {
            let name = node_text(content, arg.child_by_field_name("name")?)?;
            http_verb_name(name)
        }
        "identifier" => {
            let name = node_text(content, arg)?;
            http_verb_name(name)
        }
        _ => None,
    }
}

/// Whether a `scoped_identifier`'s path is (or ends in) the `reqwest` crate
/// alias: `reqwest::get` (path is `identifier "reqwest"`).
fn scoped_path_is_reqwest(scoped: Node, content: &str) -> bool {
    scoped_path_is_name(scoped, content, "reqwest")
}

fn scoped_path_is_name(scoped: Node, content: &str, expected: &str) -> bool {
    let Some(path) = scoped.child_by_field_name("path") else {
        return false;
    };
    match path.kind() {
        "identifier" => node_text(content, path) == Some(expected),
        "scoped_identifier" => {
            path.child_by_field_name("name")
                .and_then(|name| node_text(content, name))
                == Some(expected)
        }
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
