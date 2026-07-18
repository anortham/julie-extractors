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
    match method {
        "get" | "GET" => Some("GET"),
        "post" | "POST" => Some("POST"),
        "put" | "PUT" => Some("PUT"),
        "patch" | "PATCH" => Some("PATCH"),
        "delete" | "DELETE" => Some("DELETE"),
        "head" | "HEAD" => Some("HEAD"),
        "options" | "OPTIONS" => Some("OPTIONS"),
        _ => None,
    }
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
    // Parser-backed use/import gates — comments never create namespace_use_declaration.
    let guzzle = has_php_use_containing(root, content, GUZZLE_NEEDLE);
    let http_facade = has_php_use_containing(root, content, HTTP_FACADE_NEEDLE)
        || has_php_use_containing(root, content, "Illuminate\\Support\\Facades\\Http");
    let symfony = has_php_use_containing(root, content, SYMFONY_INTERFACE_NEEDLE)
        || has_php_use_containing(root, content, SYMFONY_CLIENT_NEEDLE);
    // cURL is call-name based (not an import); AST recognition still requires real calls.
    let curl = content.contains("curl_init") || content.contains("curl_setopt");
    if !guzzle && !http_facade && !symfony && !curl {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(
        root,
        guzzle,
        http_facade,
        symfony,
        language,
        tree,
        file_path,
        content,
        &mut facts,
    );
    if curl {
        collect_curl_facts(root, language, tree, file_path, content, &mut facts);
    }
    facts
}

/// True when a real `namespace_use_declaration` AST node imports a path that
/// contains `needle` (FQN fragment). Comments/strings never produce use nodes.
fn has_php_use_containing(node: Node, content: &str, needle: &str) -> bool {
    if node.kind() == "namespace_use_declaration"
        && let Some(text) = node_text(content, node)
    {
        let normalized = text.replace("\\\\", "\\");
        if normalized.contains(needle) {
            return true;
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if has_php_use_containing(child, content, needle) {
            return true;
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
fn walk(
    node: Node,
    guzzle: bool,
    http_facade: bool,
    symfony: bool,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if matches!(
        node.kind(),
        "scoped_call_expression" | "member_call_expression"
    ) && let Some(fact) = client_request_fact(
        node,
        guzzle,
        http_facade,
        symfony,
        language,
        tree,
        file_path,
        content,
    ) {
        facts.push(fact);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            child,
            guzzle,
            http_facade,
            symfony,
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
    symfony: bool,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let method = node_text(content, call.child_by_field_name("name")?)?;
    let arguments = call.child_by_field_name("arguments")?;

    // Symfony request(method, url) — check before Guzzle request so typed
    // HttpClientInterface params are not mis-attributed to Guzzle.
    if symfony && method == "request" && symfony_receiver_ok(call, content) {
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
    if guzzle && matches!(method, "request" | "requestAsync") {
        if !matches!(client_root(call, content)?, ClientRoot::Variable(_)) {
            return None;
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
        ClientRoot::Facade(name) if http_facade && name == "Http" => "laravel_http",
        ClientRoot::Variable(_) if guzzle => "guzzle",
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
    walk_assignments(function, content, &mut |var, value| {
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
        && node_text(content, scope)
            .map(|s| s.ends_with("HttpClient") || s == "HttpClient")
            .unwrap_or(false)
}

fn ident_is_symfony_typed_param(name: &str, from: Node, content: &str) -> bool {
    let Some(function) = enclosing_function(from) else {
        return false;
    };
    let bare = name.trim_start_matches('$');
    // Walk the whole function signature region for a typed parameter whose
    // variable matches and whose type text mentions HttpClient*.
    let mut stack = vec![function];
    while let Some(node) = stack.pop() {
        if node.kind() == "variable_name"
            && let Some(var) = node_text(content, node)
        {
            let var_bare = var.trim_start_matches('$');
            if var_bare == bare {
                // Walk siblings / ancestors for a nearby type node in the same parameter.
                if let Some(param) = node.parent() {
                    let text = node_text(content, param).unwrap_or("");
                    if text.contains("HttpClientInterface") || text.contains("HttpClient") {
                        return true;
                    }
                }
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    false
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

fn walk_assignments(node: Node, content: &str, f: &mut dyn FnMut(&str, Node)) {
    if node.kind() == "assignment_expression"
        && let Some(left) = node.child_by_field_name("left")
        && left.kind() == "variable_name"
        && let Some(name) = node_text(content, left)
        && let Some(right) = node.child_by_field_name("right")
    {
        f(name, right);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_assignments(child, content, f);
    }
}

fn client_root(call: Node, content: &str) -> Option<ClientRoot> {
    let mut receiver = match call.kind() {
        "scoped_call_expression" => {
            let scope = node_text(content, call.child_by_field_name("scope")?)?;
            if scope.ends_with("HttpClient")
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
                if scope.ends_with("HttpClient") && name == "create" {
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

fn collect_curl_facts(
    root: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    // Per-function single-assignment tracking only.
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if matches!(
            node.kind(),
            "function_definition" | "method_declaration" | "anonymous_function"
        ) {
            collect_curl_in_function(node, language, tree, file_path, content, facts);
            continue;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
}

fn collect_curl_in_function(
    function: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let mut handles: HashMap<String, CurlHandle> = HashMap::new();
    let mut anonymous: Vec<CurlHandle> = Vec::new();
    gather_curl(function, content, &mut handles, &mut anonymous);

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
                handles.insert(name.to_string(), handle);
            }
        }
        "function_call_expression" => {
            if is_curl_init_call(node, content) {
                // Only anonymous if not the right-hand side of an assignment
                // (assignment path handles named). Parent check:
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
        handle.url_start = init.start_byte();
        handle.url_end = init.end_byte();
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
            let Some(verb_lit) = static_route_arg(verb_arg, content, StaticArgLang::Php) else {
                return;
            };
            if let Some(verb) = verb_for_method(verb_lit) {
                handle.verb = verb;
                handle.verb_source = "attested";
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
