//! Kotlin HTTP client-request facts (`http.client_request.v1`) for Ktor, OkHttp,
//! Retrofit, Spring WebClient, and RestTemplate.
//!
//! Silence (design §4.4, M2): only static string-literal URLs produce a fact.

use tree_sitter::{Node, Tree};

use super::super::helpers::{child_of_kind, node_text};
use super::super::static_arg::{StaticArgLang, static_route_arg};
use super::client_fact;
use crate::base::types::StructuralFact;

const KTOR_NEEDLE: &str = "io.ktor.client";
const OKHTTP_NEEDLE: &str = "okhttp3.Request";
const RETROFIT_NEEDLE: &str = "retrofit2.http.";
const WEBCLIENT_NEEDLE: &str = "org.springframework.web.reactive.function.client.WebClient";
const RESTTEMPLATE_NEEDLE: &str = "org.springframework.web.client.RestTemplate";

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

pub(super) fn collect_kotlin_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let ktor = content.contains(KTOR_NEEDLE);
    let okhttp = content.contains(OKHTTP_NEEDLE) || content.contains("okhttp3.Request.Builder");
    let retrofit = content.contains(RETROFIT_NEEDLE);
    let webclient = content.contains(WEBCLIENT_NEEDLE);
    let resttemplate = content.contains(RESTTEMPLATE_NEEDLE);
    if !ktor && !okhttp && !retrofit && !webclient && !resttemplate {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        ktor,
        okhttp,
        retrofit,
        webclient,
        resttemplate,
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
    ktor: bool,
    okhttp: bool,
    retrofit: bool,
    webclient: bool,
    resttemplate: bool,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if node.kind() == "call_expression" {
        if ktor && let Some(fact) = ktor_request(node, language, tree, file_path, content) {
            facts.push(fact);
        }
        if okhttp && let Some(fact) = okhttp_request(node, language, tree, file_path, content) {
            facts.push(fact);
        }
        if webclient
            && let Some(fact) = spring_webclient_request(node, language, tree, file_path, content)
        {
            facts.push(fact);
        }
        if resttemplate
            && let Some(fact) =
                spring_resttemplate_request(node, language, tree, file_path, content)
        {
            facts.push(fact);
        }
    }
    if retrofit
        && node.kind() == "annotation"
        && let Some(fact) = retrofit_annotation(node, language, tree, file_path, content)
    {
        facts.push(fact);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            child,
            ktor,
            okhttp,
            retrofit,
            webclient,
            resttemplate,
            language,
            tree,
            file_path,
            content,
            facts,
        );
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
    let verb = verb_for_method(method)?;
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
                    {
                        let p = static_route_arg(arg, content, StaticArgLang::Kotlin)?;
                        target_path = Some(p)
                    }
                }
            }
            "get" | "post" | "put" | "patch" | "delete" | "head" => {
                if let Some(v) = verb_for_method(method) {
                    verb = v;
                    verb_source = "attested";
                }
            }
            "method" => {
                if let Some(args) = child_of_kind(*c, "value_arguments")
                    && let Some(arg) = first_named_argument_value(args)
                {
                    {
                        let lit = static_route_arg(arg, content, StaticArgLang::Kotlin)?;
                        verb = verb_for_method(lit)?;
                        verb_source = "attested";
                    }
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

fn navigation_roots_at_request(nav: Node, content: &str) -> bool {
    // Request.Builder or okhttp3.Request.Builder
    let mut node = nav;
    loop {
        if node.kind() == "navigation_expression" {
            if let Some(id) = last_identifier_text(node, content) {
                if id == "Request" {
                    return true;
                }
                if id == "Builder" {
                    // continue to receiver
                    if let Some(recv) = first_child(node) {
                        node = recv;
                        continue;
                    }
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
        // okhttp3.Request as nested navigations
        if let Some(text) = node_text(content, node) {
            return text.ends_with("Request") || text.contains("Request");
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
    // path from first string arg
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

fn annotation_name<'a>(annotation: Node, content: &'a str) -> Option<&'a str> {
    // Prefer last identifier in the annotation constructor
    let mut last = None;
    let mut stack = vec![annotation];
    while let Some(node) = stack.pop() {
        if matches!(node.kind(), "simple_identifier" | "identifier") {
            last = node_text(content, node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            // don't walk into argument values for name
            if child.kind() == "value_arguments"
                || child.kind() == "parenthesized_expression"
                || child.kind() == "string_literal"
            {
                continue;
            }
            stack.push(child);
        }
    }
    last
}

fn annotation_static_path<'a>(annotation: Node, content: &'a str) -> Option<&'a str> {
    let mut stack = vec![annotation];
    while let Some(node) = stack.pop() {
        if node.kind() == "value_argument" || node.kind() == "string_literal" {
            if let Some(path) = static_route_arg(node, content, StaticArgLang::Kotlin) {
                return Some(path);
            }
            // try children of value_argument
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

    // receiver should be call_expression of .get()/.post()/...
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
    let verb = verb_for_method(method)?;
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
        }
        "call_expression" => {
            let Some(function) = receiver
                .child_by_field_name("function")
                .or_else(|| first_child(receiver))
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
                || node_text(content, root).is_some_and(|t| t.ends_with(type_name))
        }
        "navigation_expression" => {
            // Fully qualified TypeName without call — not a receiver value.
            false
        }
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
        if node.kind() == "parameter"
            && let Some(param_text) = node_text(content, node)
        {
            // Conservative: parameter text looks like `name: TypeName` (optionally
            // with annotations/defaults). Avoid matching substring type names alone.
            let trimmed = param_text.trim_start();
            if (trimmed == name
                || trimmed.starts_with(&format!("{name}:"))
                || trimmed.starts_with(&format!("{name} :")))
                && (param_text.contains(&format!(": {type_name}"))
                    || param_text.contains(&format!(":{type_name}"))
                    || param_text.contains(&format!(": {type_name}?"))
                    || param_text.contains(&format!(":{type_name}?")))
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
    // HttpMethod.GET or GET
    let text = node_text(content, node)?;
    let name = text.rsplit('.').next().unwrap_or(text);
    verb_for_method(name)
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
