//! Kotlin HTTP client-request facts (`http.client_request.v1`) for Ktor, OkHttp,
//! Retrofit, Spring WebClient, and RestTemplate.
//!
//! Silence (design §4.4, M2): only static string-literal URLs produce a fact.

use tree_sitter::{Node, Tree};

use super::super::helpers::{child_of_kind, node_text};
use super::super::static_arg::{StaticArgLang, static_route_arg};
use super::{client_fact, verb_for_lower_method, verb_for_token};
use crate::base::types::StructuralFact;

const KTOR_NEEDLE: &str = "io.ktor.client";
const OKHTTP_NEEDLE: &str = "okhttp3.Request";
const RETROFIT_NEEDLE: &str = "retrofit2.http.";
const WEBCLIENT_NEEDLE: &str = "org.springframework.web.reactive.function.client.WebClient";
const RESTTEMPLATE_NEEDLE: &str = "org.springframework.web.client.RestTemplate";

fn resttemplate_verb(method: &str) -> Option<&'static str> {
    match method {
        "getForObject" | "getForEntity" => Some("GET"),
        "postForObject" | "postForEntity" => Some("POST"),
        "put" => Some("PUT"),
        "delete" => Some("DELETE"),
        "patchForObject" => Some("PATCH"),
        _ => None,
    }
}

fn retrofit_verb(name: &str) -> Option<&'static str> {
    match name {
        "GET" => Some("GET"),
        "POST" => Some("POST"),
        "PUT" => Some("PUT"),
        "PATCH" => Some("PATCH"),
        "DELETE" => Some("DELETE"),
        "HEAD" => Some("HEAD"),
        "OPTIONS" => Some("OPTIONS"),
        "HTTP" => None, // needs method= element; unsupported without static method
        _ => None,
    }
}

struct KotlinGates {
    ktor: bool,
    okhttp: bool,
    retrofit: bool,
    webclient: bool,
    resttemplate: bool,
}

pub(super) fn collect_kotlin_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let gates = KotlinGates {
        ktor: content.contains(KTOR_NEEDLE),
        okhttp: content.contains(OKHTTP_NEEDLE),
        retrofit: content.contains(RETROFIT_NEEDLE),
        webclient: content.contains(WEBCLIENT_NEEDLE),
        resttemplate: content.contains(RESTTEMPLATE_NEEDLE),
    };
    if !gates.ktor && !gates.okhttp && !gates.retrofit && !gates.webclient && !gates.resttemplate {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        &gates,
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
    gates: &KotlinGates,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if node.kind() == "call_expression" {
        if gates.ktor
            && let Some(fact) = ktor_request(node, language, tree, file_path, content)
        {
            facts.push(fact);
        }
        if gates.okhttp
            && let Some(fact) = okhttp_request(node, language, tree, file_path, content)
        {
            facts.push(fact);
        }
        if gates.webclient
            && let Some(fact) = spring_webclient_request(node, language, tree, file_path, content)
        {
            facts.push(fact);
        }
        if gates.resttemplate
            && let Some(fact) =
                spring_resttemplate_request(node, language, tree, file_path, content)
        {
            facts.push(fact);
        }
    }
    if gates.retrofit
        && node.kind() == "annotation"
        && let Some(fact) = retrofit_annotation(node, language, tree, file_path, content)
    {
        facts.push(fact);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, gates, language, tree, file_path, content, facts);
    }
}

fn ktor_request(
    call: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let callee = call
        .child_by_field_name("function")
        .or_else(|| first_child(call))?;
    if callee.kind() != "navigation_expression" {
        return None;
    }
    let method = last_identifier_text(callee, content)?;
    let verb = verb_for_lower_method(method)?;
    if !ktor_receiver_proven(callee, call, tree.root_node(), content) {
        return None;
    }
    let value_arguments = child_of_kind(call, "value_arguments")?;
    let url_argument = first_named_argument_value(value_arguments)?;
    let target_path = static_route_arg(url_argument, content, StaticArgLang::Kotlin)?;
    client_fact(
        language,
        tree,
        file_path,
        content,
        call.start_byte(),
        call.end_byte(),
        "ktor",
        target_path,
        verb,
        "attested",
        None,
    )
}

/// Receiver must be a proven Ktor `HttpClient`: an `HttpClient`-typed
/// function/class parameter, a same-file local consistently assigned from an
/// `HttpClient(...)` constructor, or an inline `HttpClient(...)` call. Any
/// other `.get("literal")` receiver (headers, maps, caches) stays silent (M2).
fn ktor_receiver_proven(callee: Node, call: Node, root: Node, content: &str) -> bool {
    let Some(receiver) = first_child(callee) else {
        return false;
    };
    match receiver.kind() {
        "simple_identifier" | "identifier" => {
            let Some(name) = node_text(content, receiver) else {
                return false;
            };
            ident_is_kotlin_typed_param(name, call, content, "HttpClient")
                || local_is_http_client(name, root, content)
        }
        "call_expression" => call_roots_at_http_client(receiver, content),
        _ => false,
    }
}

fn call_roots_at_http_client(call: Node, content: &str) -> bool {
    let Some(function) = call
        .child_by_field_name("function")
        .or_else(|| first_child(call))
    else {
        return false;
    };
    match function.kind() {
        "simple_identifier" | "identifier" => node_text(content, function) == Some("HttpClient"),
        "navigation_expression" => last_identifier_text(function, content) == Some("HttpClient"),
        _ => false,
    }
}

fn local_is_http_client(name: &str, root: Node, content: &str) -> bool {
    let mut proven = false;
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "property_declaration"
            && property_declared_name(node, content) == Some(name)
        {
            if property_initializer_is_http_client(node, content) {
                proven = true;
            } else {
                return false;
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    proven
}

fn property_declared_name<'a>(property: Node, content: &'a str) -> Option<&'a str> {
    let declaration = child_of_kind(property, "variable_declaration")?;
    let mut cursor = declaration.walk();
    declaration
        .children(&mut cursor)
        .find(|child| matches!(child.kind(), "simple_identifier" | "identifier"))
        .and_then(|child| node_text(content, child))
}

fn property_initializer_is_http_client(property: Node, content: &str) -> bool {
    child_of_kind(property, "call_expression")
        .is_some_and(|initializer| call_roots_at_http_client(initializer, content))
}

fn okhttp_request(
    call: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    // Emit once per builder chain, on the terminal call (usually .build()).
    if !is_chain_terminal(call) {
        return None;
    }
    let chain = collect_builder_chain(call);
    let mut target_path = None;
    let mut verb = "GET";
    let mut verb_source = "default";
    let mut saw_request_builder = false;
    for c in &chain {
        if is_request_builder_call(*c, content) {
            saw_request_builder = true;
        }
        let Some(function) = c
            .child_by_field_name("function")
            .or_else(|| first_child(*c))
        else {
            continue;
        };
        if function.kind() != "navigation_expression" {
            continue;
        }
        let Some(method) = last_identifier_text(function, content) else {
            continue;
        };
        match method {
            "url" => {
                if let Some(args) = child_of_kind(*c, "value_arguments")
                    && let Some(arg) = first_named_argument_value(args)
                {
                    target_path = Some(static_route_arg(arg, content, StaticArgLang::Kotlin)?);
                }
            }
            "get" | "post" | "put" | "patch" | "delete" | "head" => {
                if let Some(v) = verb_for_lower_method(method) {
                    verb = v;
                    verb_source = "attested";
                }
            }
            "method" => {
                if let Some(args) = child_of_kind(*c, "value_arguments")
                    && let Some(arg) = first_named_argument_value(args)
                {
                    let lit = static_route_arg(arg, content, StaticArgLang::Kotlin)?;
                    verb = verb_for_token(lit)?;
                    verb_source = "attested";
                }
            }
            "Builder" if navigation_roots_at_request(function, content) => {
                saw_request_builder = true;
            }
            _ => {}
        }
    }

    if !saw_request_builder {
        return None;
    }
    let target_path = target_path?;
    client_fact(
        language,
        tree,
        file_path,
        content,
        call.start_byte(),
        call.end_byte(),
        "okhttp",
        target_path,
        verb,
        verb_source,
        None,
    )
}

fn collect_builder_chain(call: Node) -> Vec<Node> {
    let mut out = Vec::new();
    let mut node = Some(call);
    while let Some(n) = node {
        if n.kind() == "call_expression" {
            out.push(n);
            let function = n.child_by_field_name("function").or_else(|| first_child(n));
            node = function.and_then(|f| {
                if f.kind() == "navigation_expression" {
                    first_child(f).filter(|c| c.kind() == "call_expression")
                } else {
                    None
                }
            });
        } else {
            break;
        }
    }
    out
}

fn is_request_builder_call(call: Node, content: &str) -> bool {
    let Some(function) = call
        .child_by_field_name("function")
        .or_else(|| first_child(call))
    else {
        return false;
    };
    if function.kind() != "navigation_expression" {
        return false;
    }
    last_identifier_text(function, content) == Some("Builder")
        && navigation_roots_at_request(function, content)
}

/// True only for chains rooted at the exact `Request` identifier
/// (`Request.Builder`, `okhttp3.Request.Builder`); any other root stays silent.
fn navigation_roots_at_request(nav: Node, content: &str) -> bool {
    let mut node = nav;
    loop {
        if node.kind() == "navigation_expression" {
            if let Some(id) = last_identifier_text(node, content) {
                if id == "Request" {
                    return node_text(content, node) == Some("okhttp3.Request");
                }
                if id == "Builder"
                    && let Some(recv) = first_child(node)
                {
                    node = recv;
                    continue;
                }
            }
            if let Some(recv) = first_child(node) {
                node = recv;
                continue;
            }
        }
        if node.kind() == "simple_identifier" || node.kind() == "identifier" {
            return node_text(content, node) == Some("Request");
        }
        return false;
    }
}

fn is_chain_terminal(call: Node) -> bool {
    let Some(parent) = call.parent() else {
        return true;
    };
    if parent.kind() == "navigation_expression"
        && let Some(grand) = parent.parent()
        && grand.kind() == "call_expression"
    {
        let is_receiver = first_child(parent) == Some(call)
            && grand
                .child_by_field_name("function")
                .or_else(|| first_child(grand))
                == Some(parent);
        return !is_receiver;
    }
    true
}

fn retrofit_annotation(
    annotation: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let name = annotation_name(annotation, content)?;
    let verb = retrofit_verb(name)?;
    let target_path = annotation_static_path(annotation, content)?;
    client_fact(
        language,
        tree,
        file_path,
        content,
        annotation.start_byte(),
        annotation.end_byte(),
        "retrofit",
        target_path,
        verb,
        "attested",
        None,
    )
}

/// Trailing identifier of the annotation constructor — `GET` for both `@GET`
/// and `@retrofit2.http.GET` — ignoring argument values.
fn annotation_name<'a>(annotation: Node<'a>, content: &'a str) -> Option<&'a str> {
    let mut last: Option<Node> = None;
    let mut stack = vec![annotation];
    while let Some(node) = stack.pop() {
        if matches!(node.kind(), "simple_identifier" | "identifier")
            && last.is_none_or(|best| node.start_byte() > best.start_byte())
        {
            last = Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "value_arguments"
                || child.kind() == "parenthesized_expression"
                || child.kind() == "string_literal"
            {
                continue;
            }
            stack.push(child);
        }
    }
    last.and_then(|node| node_text(content, node))
}

fn annotation_static_path<'a>(annotation: Node, content: &'a str) -> Option<&'a str> {
    let mut stack = vec![annotation];
    while let Some(node) = stack.pop() {
        if node.kind() == "value_argument" || node.kind() == "string_literal" {
            if let Some(path) = static_route_arg(node, content, StaticArgLang::Kotlin) {
                return Some(path);
            }
            if node.kind() == "value_argument" {
                let mut c = node.walk();
                for child in node.children(&mut c) {
                    if let Some(path) = static_route_arg(child, content, StaticArgLang::Kotlin) {
                        return Some(path);
                    }
                }
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    None
}

fn spring_webclient_request(
    call: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    // Emit on .uri(static) when chain has proven_webclient.<verb>().uri(...)
    let function = call
        .child_by_field_name("function")
        .or_else(|| first_child(call))?;
    if function.kind() != "navigation_expression" {
        return None;
    }
    if last_identifier_text(function, content)? != "uri" {
        return None;
    }
    let args = child_of_kind(call, "value_arguments")?;
    let url_arg = first_named_argument_value(args)?;
    let target_path = static_route_arg(url_arg, content, StaticArgLang::Kotlin)?;

    let receiver = first_child(function)?;
    if receiver.kind() != "call_expression" {
        return None;
    }
    let recv_fn = receiver
        .child_by_field_name("function")
        .or_else(|| first_child(receiver))?;
    if recv_fn.kind() != "navigation_expression" {
        return None;
    }
    let method = last_identifier_text(recv_fn, content)?;
    let verb = verb_for_lower_method(method)?;
    let root = first_child(recv_fn)?;
    if !spring_client_receiver_proven(root, call, content, "WebClient") {
        return None;
    }
    client_fact(
        language,
        tree,
        file_path,
        content,
        call.start_byte(),
        call.end_byte(),
        "spring_webclient",
        target_path,
        verb,
        "attested",
        None,
    )
}

fn spring_resttemplate_request(
    call: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Option<StructuralFact> {
    let function = call
        .child_by_field_name("function")
        .or_else(|| first_child(call))?;
    if function.kind() != "navigation_expression" {
        return None;
    }
    let method = last_identifier_text(function, content)?;
    let root = first_child(function)?;
    if !spring_client_receiver_proven(root, call, content, "RestTemplate") {
        return None;
    }
    let args = child_of_kind(call, "value_arguments")?;
    if method == "exchange" {
        let url_arg = first_named_argument_value(args)?;
        let target_path = static_route_arg(url_arg, content, StaticArgLang::Kotlin)?;
        let method_arg = nth_named_argument_value(args, 1)?;
        let verb = http_method_enum(method_arg, content)?;
        return client_fact(
            language,
            tree,
            file_path,
            content,
            call.start_byte(),
            call.end_byte(),
            "spring_resttemplate",
            target_path,
            verb,
            "attested",
            None,
        );
    }
    let verb = resttemplate_verb(method)?;
    let url_arg = first_named_argument_value(args)?;
    let target_path = static_route_arg(url_arg, content, StaticArgLang::Kotlin)?;
    client_fact(
        language,
        tree,
        file_path,
        content,
        call.start_byte(),
        call.end_byte(),
        "spring_resttemplate",
        target_path,
        verb,
        "attested",
        None,
    )
}

/// Receiver is a typed parameter/property of `type_name`, or a direct
/// `TypeName.create()` / `TypeName.builder()` constructor call. Unrelated
/// fluent roots stay silent even when the Spring import is present.
fn spring_client_receiver_proven(
    receiver: Node,
    from: Node,
    content: &str,
    type_name: &str,
) -> bool {
    match receiver.kind() {
        "simple_identifier" | "identifier" => {
            let Some(name) = node_text(content, receiver) else {
                return false;
            };
            ident_is_kotlin_typed_param(name, from, content, type_name)
                || spring_local_is_client(name, from, content, type_name)
        }
        "call_expression" => call_is_spring_ctor_chain(receiver, content, type_name),
        "navigation_expression" => {
            // Fully qualified TypeName without call — not a receiver value.
            false
        }
        _ => false,
    }
}

/// `TypeName.create()` / `TypeName.builder()` with the exact type as root.
/// Anchored exactly: `AcmeWebClient.create()` must not prove a `WebClient`
/// receiver.
fn call_is_spring_ctor_chain(call: Node, content: &str, type_name: &str) -> bool {
    let Some(function) = call
        .child_by_field_name("function")
        .or_else(|| first_child(call))
    else {
        return false;
    };
    if function.kind() != "navigation_expression" {
        return false;
    }
    let Some(ctor) = last_identifier_text(function, content) else {
        return false;
    };
    if ctor != "create" && ctor != "builder" {
        return false;
    }
    let Some(root) = first_child(function) else {
        return false;
    };
    last_identifier_text(root, content) == Some(type_name)
        || node_text(content, root) == Some(type_name)
}

/// Same-file local consistently assigned from the exact Spring client
/// constructor: `val web = WebClient.create(...)` / `val rest = RestTemplate()`.
fn spring_local_is_client(name: &str, from: Node, content: &str, type_name: &str) -> bool {
    let mut root = from;
    while let Some(parent) = root.parent() {
        root = parent;
    }
    let mut proven = false;
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "property_declaration"
            && property_declared_name(node, content) == Some(name)
        {
            if property_initializer_is_spring_client(node, content, type_name) {
                proven = true;
            } else {
                return false;
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    proven
}

fn property_initializer_is_spring_client(property: Node, content: &str, type_name: &str) -> bool {
    let Some(initializer) = child_of_kind(property, "call_expression") else {
        return false;
    };
    let Some(function) = initializer
        .child_by_field_name("function")
        .or_else(|| first_child(initializer))
    else {
        return false;
    };
    match function.kind() {
        "simple_identifier" | "identifier" => node_text(content, function) == Some(type_name),
        "navigation_expression" => call_is_spring_ctor_chain(initializer, content, type_name),
        _ => false,
    }
}

fn ident_is_kotlin_typed_param(name: &str, from: Node, content: &str, type_name: &str) -> bool {
    let mut cursor = Some(from);
    while let Some(node) = cursor {
        if matches!(
            node.kind(),
            "function_declaration" | "class_body" | "primary_constructor" | "class_declaration"
        ) && function_or_class_has_typed_param(node, name, content, type_name)
        {
            return true;
        }
        cursor = node.parent();
    }
    false
}

fn function_or_class_has_typed_param(
    scope: Node,
    name: &str,
    content: &str,
    type_name: &str,
) -> bool {
    // Class-level scopes may prove primary-constructor / class-parameter
    // bindings, but never sibling or nested method parameters.
    let class_scope = matches!(
        scope.kind(),
        "class_body" | "class_declaration" | "primary_constructor"
    );
    let mut stack = vec![scope];
    while let Some(node) = stack.pop() {
        if matches!(node.kind(), "parameter" | "class_parameter")
            && let Some(param_text) = node_text(content, node)
            && let Some((param_name, param_type)) = param_text.split_once(':')
        {
            // Anchor both edges of the declared type: `WebClientFactory` or
            // `WebClient.Builder` must never prove a `WebClient` receiver.
            let base: String = param_type
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '_')
                .collect();
            if param_name.split_whitespace().last() == Some(name)
                && (base == type_name || base.ends_with(&format!(".{type_name}")))
            {
                return true;
            }
        }
        // Stop descending into nested function bodies / nested class bodies.
        if node != scope
            && matches!(
                node.kind(),
                "function_body" | "class_body" | "lambda_literal"
            )
        {
            continue;
        }
        // From a class scope, do not scan method parameter lists at all.
        if class_scope && node != scope && node.kind() == "function_declaration" {
            continue;
        }
        let mut child_cursor = node.walk();
        for child in node.children(&mut child_cursor) {
            stack.push(child);
        }
    }
    false
}

fn http_method_enum(node: Node, content: &str) -> Option<&'static str> {
    let text = node_text(content, node)?;
    let name = text.rsplit('.').next().unwrap_or(text);
    verb_for_token(name)
}

fn first_named_argument_value(value_arguments: Node) -> Option<Node> {
    nth_named_argument_value(value_arguments, 0)
}

fn nth_named_argument_value(value_arguments: Node, index: usize) -> Option<Node> {
    let mut cursor = value_arguments.walk();
    let mut i = 0;
    for value_argument in value_arguments.children(&mut cursor) {
        if value_argument.kind() != "value_argument" {
            continue;
        }
        if i == index {
            let mut arg_cursor = value_argument.walk();
            return value_argument
                .children(&mut arg_cursor)
                .find(|child| child.is_named());
        }
        i += 1;
    }
    None
}

fn last_identifier_text<'a>(node: Node, content: &'a str) -> Option<&'a str> {
    let mut cursor = node.walk();
    let mut last = None;
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "identifier" | "simple_identifier") {
            last = Some(child);
        }
    }
    last.and_then(|child| node_text(content, child))
}

fn first_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor).next()
}
