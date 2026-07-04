//! PHP HTTP client-request facts (`http.client_request.v1`) for Guzzle and the
//! Laravel `Http` facade — the two dominant single-call-idiom PHP clients.
//!
//! - **Guzzle**: `$client->get('https://…')` — a `member_call_expression` whose
//!   receiver is a `variable_name` and whose method is an HTTP verb. This is the
//!   same receiver-`.verb("url")` shape the shipped Go/Java/Ktor collectors use.
//!   Because a bare `$x->get(...)` is highly ambiguous, this arm fires only when
//!   the file imports `GuzzleHttp` (the import gate).
//! - **Laravel `Http` facade**: `Http::get('url')` (and chained
//!   `Http::withToken($t)->get('url')`) — the receiver chain roots at the bare
//!   `Http` facade. Gated on the `Facades\Http` import.
//!
//! Silence (design §4.4, M2): only a lone static string-literal URL (via the
//! shared PHP static guard, ADR-0005) produces a fact; interpolated /
//! concatenated / variable / `self::CONST` URLs emit nothing.
//!
//! Deferred (documented `open_gaps`): Symfony HttpClient, the `curl_*` family,
//! and Guzzle's `->request('GET', 'url')` two-argument form — each has a
//! detection shape unlike any shipped collector and needs its own fixture.

use tree_sitter::{Node, Tree};

use super::super::helpers::node_text;
use super::super::static_arg::{StaticArgLang, static_route_arg};
use super::client_fact;
use crate::base::types::StructuralFact;

/// Guzzle import gate: the client type lives under the `GuzzleHttp` namespace.
const GUZZLE_NEEDLE: &str = "GuzzleHttp";
/// Laravel `Http` facade import gate: `use Illuminate\Support\Facades\Http;`.
const HTTP_FACADE_NEEDLE: &str = "Facades\\Http";

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

/// What a client call's receiver chain roots at: a named facade/class (`Http`)
/// or a variable (`$client`).
enum ClientRoot {
    Facade(String),
    Variable,
}

pub(super) fn collect_php_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let guzzle = content.contains(GUZZLE_NEEDLE);
    let http_facade = content.contains(HTTP_FACADE_NEEDLE);
    if !guzzle && !http_facade {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        guzzle,
        http_facade,
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
    guzzle: bool,
    http_facade: bool,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if matches!(
        node.kind(),
        "scoped_call_expression" | "member_call_expression"
    ) && let Some(fact) =
        client_request_fact(node, guzzle, http_facade, language, tree, file_path, content)
    {
        facts.push(fact);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            child,
            guzzle,
            http_facade,
            language,
            tree,
            file_path,
            content,
            facts,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn client_request_fact(
    call: Node,
    guzzle: bool,
    http_facade: bool,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let method = node_text(content, call.child_by_field_name("name")?)?;
    let verb = verb_for_method(method)?;

    let client = match client_root(call, content)? {
        ClientRoot::Facade(name) if http_facade && name == "Http" => "laravel_http",
        ClientRoot::Variable if guzzle => "guzzle",
        _ => return None,
    };

    let arguments = call.child_by_field_name("arguments")?;
    let url_argument = first_positional_arg_value(arguments)?;
    let target_path = static_route_arg(url_argument, content, StaticArgLang::Php)?;

    client_fact(
        language,
        tree,
        file_path,
        content,
        call.start_byte(),
        call.end_byte(),
        client,
        target_path,
        verb,
        "attested",
        None,
    )
}

/// The root of a call's receiver chain: the innermost `scoped_call_expression`
/// scope (a facade/class name) after walking any `member_call_expression`
/// chain, or `Variable` when the chain roots at a `variable_name` (`$client`).
fn client_root(call: Node, content: &str) -> Option<ClientRoot> {
    let mut receiver = match call.kind() {
        "scoped_call_expression" => {
            return node_text(content, call.child_by_field_name("scope")?)
                .map(|name| ClientRoot::Facade(name.to_string()));
        }
        "member_call_expression" => call.child_by_field_name("object")?,
        _ => return None,
    };
    loop {
        match receiver.kind() {
            "member_call_expression" => receiver = receiver.child_by_field_name("object")?,
            "scoped_call_expression" => {
                return node_text(content, receiver.child_by_field_name("scope")?)
                    .map(|name| ClientRoot::Facade(name.to_string()));
            }
            "name" => {
                return node_text(content, receiver).map(|name| ClientRoot::Facade(name.to_string()));
            }
            "variable_name" => return Some(ClientRoot::Variable),
            _ => return None,
        }
    }
}

/// The value node of the first positional `argument` in an `arguments` node.
fn first_positional_arg_value(arguments: Node) -> Option<Node> {
    let mut cursor = arguments.walk();
    for argument in arguments.children(&mut cursor) {
        if argument.kind() != "argument" {
            continue;
        }
        let mut arg_cursor = argument.walk();
        return argument.named_children(&mut arg_cursor).next();
    }
    None
}
