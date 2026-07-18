//! PHP HTTP client-request facts (`http.client_request.v1`) for Guzzle, Laravel
//! `Http`, Symfony HttpClient, and cURL.
//!
//! Silence (design §4.4, M2): only static string-literal URLs/methods produce a
//! fact; dynamic arguments, unproven receivers, and cross-function curl handles
//! emit nothing.

use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use super::super::helpers::node_text;
use super::super::static_arg::{StaticArgLang, static_route_arg};
use super::client_fact;
use crate::base::types::StructuralFact;

const GUZZLE_NEEDLE: &str = "GuzzleHttp";
const HTTP_FACADE_NEEDLE: &str = "Facades\\Http";
const SYMFONY_INTERFACE_NEEDLE: &str = "Symfony\\Contracts\\HttpClient\\HttpClientInterface";
const SYMFONY_CLIENT_NEEDLE: &str = "Symfony\\Component\\HttpClient\\HttpClient";

fn verb_for_method(method: &str) -> Option<&'static str> {
    super::verb_for_token(method)
}

#[derive(Default)]
struct ImportGates {
    guzzle: bool,
    http_facade: bool,
    symfony: bool,
}

enum ClientRoot {
    Facade(String),
    Variable(String),
    CreateChain,
}

struct CurlHandle {
    target_path: Option<String>,
    verb: &'static str,
    verb_source: &'static str,
    url_start: usize,
    url_end: usize,
}

pub(super) fn collect_php_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let root = tree.root_node();
    let mut gates = ImportGates::default();
    collect_import_gates(root, content, &mut gates);
    // cURL is call-name based (not an import); AST recognition still requires real calls.
    let curl = content.contains("curl_init") || content.contains("curl_setopt");
    if !gates.guzzle && !gates.http_facade && !gates.symfony && !curl {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(root, &gates, language, tree, file_path, content, &mut facts);
    if curl {
        collect_curl_facts(root, language, tree, file_path, content, &mut facts);
    }
    facts
}

/// Parser-backed client gates: `namespace_use_declaration` imports and
/// `qualified_name` FQN references (`new \GuzzleHttp\Client()`). Comments and
/// strings produce neither node kind.
fn collect_import_gates(node: Node, content: &str, gates: &mut ImportGates) {
    if matches!(node.kind(), "namespace_use_declaration" | "qualified_name")
        && let Some(text) = node_text(content, node)
    {
        let normalized = text.replace("\\\\", "\\");
        gates.guzzle |= normalized.contains(GUZZLE_NEEDLE);
        gates.http_facade |= normalized.contains(HTTP_FACADE_NEEDLE);
        gates.symfony |= normalized.contains(SYMFONY_INTERFACE_NEEDLE)
            || normalized.contains(SYMFONY_CLIENT_NEEDLE);
    }
    if gates.guzzle && gates.http_facade && gates.symfony {
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_import_gates(child, content, gates);
    }
}

fn walk(
    node: Node,
    gates: &ImportGates,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if matches!(
        node.kind(),
        "scoped_call_expression" | "member_call_expression"
    ) && let Some(fact) = client_request_fact(node, gates, language, tree, file_path, content)
    {
        facts.push(fact);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, gates, language, tree, file_path, content, facts);
    }
}

fn client_request_fact(
    call: Node,
    gates: &ImportGates,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let method = node_text(content, call.child_by_field_name("name")?)?;
    let arguments = call.child_by_field_name("arguments")?;

    // Symfony request(method, url) — check before Guzzle request so typed
    // HttpClientInterface params are not mis-attributed to Guzzle.
    if gates.symfony && method == "request" && symfony_receiver_ok(call, content) {
        let verb_arg = nth_positional_arg_value(arguments, 0)?;
        let verb_lit = static_route_arg(verb_arg, content, StaticArgLang::Php)?;
        let verb = verb_for_method(verb_lit)?;
        let url_arg = nth_positional_arg_value(arguments, 1)?;
        let target_path = static_route_arg(url_arg, content, StaticArgLang::Php)?;
        return client_fact(
            language,
            tree,
            file_path,
            content,
            call.start_byte(),
            call.end_byte(),
            "symfony_http_client",
            target_path,
            verb,
            "attested",
            None,
        );
    }

    // Guzzle request/requestAsync(method, url)
    if gates.guzzle && matches!(method, "request" | "requestAsync") {
        match client_root(call, content)? {
            ClientRoot::Variable(name) if ident_is_guzzle_client(&name, call, content) => {}
            _ => return None,
        }
        let verb_arg = nth_positional_arg_value(arguments, 0)?;
        let verb_lit = static_route_arg(verb_arg, content, StaticArgLang::Php)?;
        let verb = verb_for_method(verb_lit)?;
        let url_arg = nth_positional_arg_value(arguments, 1)?;
        let target_path = static_route_arg(url_arg, content, StaticArgLang::Php)?;
        return client_fact(
            language,
            tree,
            file_path,
            content,
            call.start_byte(),
            call.end_byte(),
            "guzzle",
            target_path,
            verb,
            "attested",
            None,
        );
    }

    // Verb methods: Guzzle / Laravel Http
    let verb = verb_for_method(method)?;
    let client = match client_root(call, content)? {
        ClientRoot::Facade(name) if gates.http_facade && name == "Http" => "laravel_http",
        ClientRoot::Variable(name)
            if gates.guzzle && ident_is_guzzle_client(&name, call, content) =>
        {
            "guzzle"
        }
        ClientRoot::CreateChain => return None,
        _ => return None,
    };
    let url_argument = nth_positional_arg_value(arguments, 0)?;
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

fn symfony_receiver_ok(call: Node, content: &str) -> bool {
    match client_root(call, content) {
        Some(ClientRoot::CreateChain) => true,
        Some(ClientRoot::Variable(name)) => {
            ident_is_symfony_typed_param(&name, call, content)
                || variable_from_http_client_create(&name, call, content)
        }
        _ => false,
    }
}

fn variable_from_http_client_create(name: &str, from: Node, content: &str) -> bool {
    let Some(function) = enclosing_function(from) else {
        return false;
    };
    let mut found = false;
    walk_assignments(function, content, from.start_byte(), &mut |var, value| {
        if var == name && is_http_client_create(value, content) {
            found = true;
        }
    });
    found
}

fn is_http_client_create(node: Node, content: &str) -> bool {
    if node.kind() != "scoped_call_expression" {
        return false;
    }
    let Some(scope) = node.child_by_field_name("scope") else {
        return false;
    };
    let Some(name) = node.child_by_field_name("name") else {
        return false;
    };
    node_text(content, name) == Some("create")
        && node_text(content, scope).is_some_and(scope_names_symfony_http_client)
}

/// The scope's LAST namespace segment must be exactly `HttpClient`:
/// `AcmeHttpClient::create()` never proves a Symfony receiver.
fn scope_names_symfony_http_client(scope: &str) -> bool {
    let base = scope.trim_start_matches('\\');
    base.rsplit('\\').next().unwrap_or(base) == "HttpClient"
}

/// The receiver is a parameter of the enclosing function whose DECLARED type is
/// `HttpClientInterface` / `HttpClient` (optionally nullable or namespace
/// qualified). Body text mentioning HttpClient never proves a receiver (M2).
fn ident_is_symfony_typed_param(name: &str, from: Node, content: &str) -> bool {
    let Some(function) = enclosing_function(from) else {
        return false;
    };
    let Some(params) = function.child_by_field_name("parameters") else {
        return false;
    };
    let bare = name.trim_start_matches('$');
    let mut cursor = params.walk();
    params.named_children(&mut cursor).any(|param| {
        param
            .child_by_field_name("name")
            .and_then(|n| node_text(content, n))
            .is_some_and(|var| var.trim_start_matches('$') == bare)
            && param
                .child_by_field_name("type")
                .is_some_and(|ty| type_names_symfony_client(ty, content))
    })
}

fn type_names_symfony_client(ty: Node, content: &str) -> bool {
    node_text(content, ty).is_some_and(|text| {
        let base = text.trim_start_matches('?');
        let last = base.rsplit('\\').next().unwrap_or(base);
        last == "HttpClientInterface" || last == "HttpClient"
    })
}

/// The receiver is a proven Guzzle client: a `Client`/`ClientInterface`-typed
/// parameter, or a variable assigned from `new Client()` / `new
/// \GuzzleHttp\Client()` in the enclosing scope. Any other variable receiver
/// stays silent (M2) even when the Guzzle gate is open.
fn ident_is_guzzle_client(name: &str, from: Node, content: &str) -> bool {
    ident_is_guzzle_typed_param(name, from, content)
        || variable_from_guzzle_new(name, from, content)
}

fn ident_is_guzzle_typed_param(name: &str, from: Node, content: &str) -> bool {
    let Some(function) = enclosing_function(from) else {
        return false;
    };
    let Some(params) = function.child_by_field_name("parameters") else {
        return false;
    };
    let bare = name.trim_start_matches('$');
    let mut cursor = params.walk();
    params.named_children(&mut cursor).any(|param| {
        param
            .child_by_field_name("name")
            .and_then(|n| node_text(content, n))
            .is_some_and(|var| var.trim_start_matches('$') == bare)
            && param
                .child_by_field_name("type")
                .is_some_and(|ty| type_names_guzzle_client(ty, content))
    })
}

fn type_names_guzzle_client(ty: Node, content: &str) -> bool {
    node_text(content, ty).is_some_and(|text| {
        let base = text.trim_start_matches('?').trim_start_matches('\\');
        let last = base.rsplit('\\').next().unwrap_or(base);
        last == "Client" || last == "ClientInterface"
    })
}

fn variable_from_guzzle_new(name: &str, from: Node, content: &str) -> bool {
    let scope = enclosing_function(from).unwrap_or_else(|| {
        let mut node = from;
        while let Some(parent) = node.parent() {
            node = parent;
        }
        node
    });
    let mut found = false;
    walk_assignments(scope, content, from.start_byte(), &mut |var, value| {
        if var == name && is_guzzle_client_new(value, content) {
            found = true;
        }
    });
    found
}

/// `new Client()` (import-gated bare name) or the exact `new
/// \GuzzleHttp\Client()` FQN — a foreign `new \Aws\Client()` never proves.
fn is_guzzle_client_new(node: Node, content: &str) -> bool {
    if node.kind() != "object_creation_expression" {
        return false;
    }
    let mut cursor = node.walk();
    let Some(class) = node
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "name" | "qualified_name"))
    else {
        return false;
    };
    let Some(text) = node_text(content, class) else {
        return false;
    };
    let normalized = text.replace("\\\\", "\\");
    let base = normalized.trim_start_matches('\\');
    match class.kind() {
        "name" => base == "Client",
        _ => base == "GuzzleHttp\\Client",
    }
}

fn enclosing_function(from: Node) -> Option<Node> {
    let mut cursor = Some(from);
    while let Some(node) = cursor {
        if matches!(
            node.kind(),
            "function_definition" | "method_declaration" | "anonymous_function"
        ) {
            return Some(node);
        }
        cursor = node.parent();
    }
    None
}

/// Visit assignments in the current scope that COMPLETE before byte `limit`
/// (the call being proven): later assignments and nested-closure assignments
/// never prove a receiver.
fn walk_assignments(node: Node, content: &str, limit: usize, f: &mut dyn FnMut(&str, Node)) {
    if node.kind() == "assignment_expression"
        && node.end_byte() <= limit
        && let Some(left) = node.child_by_field_name("left")
        && left.kind() == "variable_name"
        && let Some(name) = node_text(content, left)
        && let Some(right) = node.child_by_field_name("right")
    {
        f(name, right);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(
            child.kind(),
            "function_definition" | "method_declaration" | "anonymous_function"
        ) {
            continue;
        }
        walk_assignments(child, content, limit, f);
    }
}

fn client_root(call: Node, content: &str) -> Option<ClientRoot> {
    let mut receiver = match call.kind() {
        "scoped_call_expression" => {
            let scope = node_text(content, call.child_by_field_name("scope")?)?;
            if scope_names_symfony_http_client(scope)
                && node_text(content, call.child_by_field_name("name")?) == Some("create")
            {
                return Some(ClientRoot::CreateChain);
            }
            return Some(ClientRoot::Facade(scope.to_string()));
        }
        "member_call_expression" => call.child_by_field_name("object")?,
        _ => return None,
    };
    loop {
        match receiver.kind() {
            "member_call_expression" => receiver = receiver.child_by_field_name("object")?,
            "scoped_call_expression" => {
                let scope = node_text(content, receiver.child_by_field_name("scope")?)?;
                let name = node_text(content, receiver.child_by_field_name("name")?)?;
                if scope_names_symfony_http_client(scope) && name == "create" {
                    return Some(ClientRoot::CreateChain);
                }
                return Some(ClientRoot::Facade(scope.to_string()));
            }
            "name" => {
                return node_text(content, receiver)
                    .map(|name| ClientRoot::Facade(name.to_string()));
            }
            "variable_name" => {
                return node_text(content, receiver)
                    .map(|name| ClientRoot::Variable(name.to_string()));
            }
            _ => return None,
        }
    }
}

/// Emit curl facts per scope: the top-level script body and each
/// function/method/closure body, tracked independently (cross-scope handles
/// stay silent).
fn collect_curl_facts(
    root: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    collect_curl_in_scope(root, language, tree, file_path, content, facts);
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if matches!(
            node.kind(),
            "function_definition" | "method_declaration" | "anonymous_function"
        ) {
            collect_curl_in_scope(node, language, tree, file_path, content, facts);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
}

fn collect_curl_in_scope(
    scope: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let mut handles: HashMap<String, CurlHandle> = HashMap::new();
    let mut anonymous: Vec<CurlHandle> = Vec::new();
    gather_curl(scope, content, &mut handles, &mut anonymous);

    for handle in handles.into_values().chain(anonymous) {
        let Some(target_path) = handle.target_path.as_deref() else {
            continue;
        };
        if let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            handle.url_start,
            handle.url_end,
            "curl",
            target_path,
            handle.verb,
            handle.verb_source,
            None,
        ) {
            facts.push(fact);
        }
    }
}

fn gather_curl(
    node: Node,
    content: &str,
    handles: &mut HashMap<String, CurlHandle>,
    anonymous: &mut Vec<CurlHandle>,
) {
    match node.kind() {
        "assignment_expression" => {
            if let (Some(left), Some(right)) = (
                node.child_by_field_name("left"),
                node.child_by_field_name("right"),
            ) && left.kind() == "variable_name"
                && let Some(name) = node_text(content, left)
                && is_curl_init_call(right, content)
            {
                let handle = curl_handle_from_init(right, content);
                if let Some(previous) = handles.insert(name.to_string(), handle) {
                    anonymous.push(previous);
                }
            }
        }
        "function_call_expression" => {
            if is_curl_init_call(node, content) {
                let assigned = node
                    .parent()
                    .is_some_and(|p| p.kind() == "assignment_expression");
                if !assigned {
                    anonymous.push(curl_handle_from_init(node, content));
                }
            } else if is_named_call(node, content, "curl_setopt") {
                apply_curl_setopt(node, content, handles);
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(
            child.kind(),
            "function_definition" | "method_declaration" | "anonymous_function"
        ) {
            continue;
        }
        gather_curl(child, content, handles, anonymous);
    }
}

fn is_curl_init_call(node: Node, content: &str) -> bool {
    node.kind() == "function_call_expression" && is_named_call(node, content, "curl_init")
}

fn is_named_call(node: Node, content: &str, name: &str) -> bool {
    node.child_by_field_name("function")
        .and_then(|f| node_text(content, f))
        == Some(name)
}

fn curl_handle_from_init(init: Node, content: &str) -> CurlHandle {
    let mut handle = CurlHandle {
        target_path: None,
        verb: "GET",
        verb_source: "default",
        url_start: init.start_byte(),
        url_end: init.end_byte(),
    };
    if let Some(arguments) = init.child_by_field_name("arguments")
        && let Some(url_arg) = nth_positional_arg_value(arguments, 0)
        && let Some(path) = static_route_arg(url_arg, content, StaticArgLang::Php)
    {
        handle.target_path = Some(path.to_string());
    }
    handle
}

fn apply_curl_setopt(call: Node, content: &str, handles: &mut HashMap<String, CurlHandle>) {
    let Some(arguments) = call.child_by_field_name("arguments") else {
        return;
    };
    let Some(handle_arg) = nth_positional_arg_value(arguments, 0) else {
        return;
    };
    if handle_arg.kind() != "variable_name" {
        return;
    }
    let Some(handle_name) = node_text(content, handle_arg) else {
        return;
    };
    let Some(opt_arg) = nth_positional_arg_value(arguments, 1) else {
        return;
    };
    let Some(opt) = constant_name(opt_arg, content) else {
        return;
    };
    let Some(handle) = handles.get_mut(handle_name) else {
        return;
    };
    match opt {
        "CURLOPT_URL" => {
            let Some(url_arg) = nth_positional_arg_value(arguments, 2) else {
                return;
            };
            let Some(path) = static_route_arg(url_arg, content, StaticArgLang::Php) else {
                handle.target_path = None;
                return;
            };
            handle.target_path = Some(path.to_string());
            handle.url_start = call.start_byte();
            handle.url_end = call.end_byte();
        }
        "CURLOPT_CUSTOMREQUEST" => {
            let Some(verb_arg) = nth_positional_arg_value(arguments, 2) else {
                return;
            };
            // A dynamic or unrecognized method would emit a wrong default-GET
            // verb — silence the handle instead (M2).
            match static_route_arg(verb_arg, content, StaticArgLang::Php).and_then(verb_for_method)
            {
                Some(verb) => {
                    handle.verb = verb;
                    handle.verb_source = "attested";
                }
                None => handle.target_path = None,
            }
        }
        "CURLOPT_POST" => {
            if let Some(val_arg) = nth_positional_arg_value(arguments, 2)
                && is_php_truthy_literal(val_arg, content)
            {
                handle.verb = "POST";
                handle.verb_source = "attested";
            }
        }
        _ => {}
    }
}

fn constant_name<'a>(node: Node<'a>, content: &'a str) -> Option<&'a str> {
    match node.kind() {
        "name" | "identifier" => node_text(content, node),
        "qualified_name" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .last()
                .and_then(|n| node_text(content, n))
        }
        _ => node_text(content, node),
    }
}

fn is_php_truthy_literal(node: Node, content: &str) -> bool {
    matches!(node_text(content, node), Some("true") | Some("1"))
}

fn nth_positional_arg_value(arguments: Node, index: usize) -> Option<Node> {
    let mut cursor = arguments.walk();
    let mut i = 0;
    for argument in arguments.children(&mut cursor) {
        if argument.kind() != "argument" {
            continue;
        }
        if i == index {
            let mut arg_cursor = argument.walk();
            return argument.named_children(&mut arg_cursor).next();
        }
        i += 1;
    }
    None
}
