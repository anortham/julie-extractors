//! Elixir HTTP client-request facts (`http.client_request.v1`) for Req, Tesla,
//! HTTPoison, Finch, and OTP `:httpc`.
//!
//! Silence (design §4.4, M2): only static string/charlist URLs produce a fact.

use std::collections::HashMap;
use tree_sitter::{Node, Tree};

use super::super::helpers::{child_of_kind, node_text};
use super::super::static_arg::{StaticArgLang, static_route_arg};
use super::client_fact;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

struct ElixirClientRequest<'a> {
    client: &'static str,
    target_path: &'a str,
    verb: &'static str,
    verb_source: &'static str,
    base_url: Option<&'a str>,
}

fn verb_for_method(method: &str) -> Option<&'static str> {
    super::verb_for_token(method.strip_suffix('!').unwrap_or(method))
}

fn atom_verb(atom: &str) -> Option<&'static str> {
    let name = atom.strip_prefix(':').unwrap_or(atom);
    verb_for_method(name)
}

pub(super) fn collect_elixir_http_client_requests(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let req = content.contains("Req");
    let tesla = content.contains("Tesla");
    let httpoison = content.contains("HTTPoison.");
    let finch = content.contains("Finch.");
    let httpc = content.contains(":httpc.");
    if !req && !tesla && !httpoison && !finch && !httpc {
        return Vec::new();
    }
    let mut facts = Vec::new();
    let mut aliases = HashMap::new();
    collect_aliases(tree.root_node(), content, 0, &mut aliases);
    let context = ClientContext {
        aliases,
        tesla_module: None,
    };
    walk(
        tree.root_node(),
        &context,
        language,
        tree,
        file_path,
        content,
        0,
        &mut facts,
    );
    facts
}

/// What a request call needs from its surroundings: the file's `alias`
/// declarations, to resolve a receiver to its real module, and the enclosing
/// `use Tesla` module with its `Tesla.Middleware.BaseUrl` plug, if any.
#[derive(Clone)]
struct ClientContext<'a> {
    aliases: HashMap<&'a str, String>,
    tesla_module: Option<Option<&'a str>>,
}

impl ClientContext<'_> {
    /// The full module name a receiver alias stands for.
    fn resolve(&self, receiver: &str) -> String {
        let (head, rest) = receiver
            .split_once('.')
            .map_or((receiver, None), |(head, rest)| (head, Some(rest)));
        match (self.aliases.get(head), rest) {
            (Some(full), Some(rest)) => format!("{full}.{rest}"),
            (Some(full), None) => full.clone(),
            (None, _) => receiver.to_string(),
        }
    }
}

// ponytail: aliases are collected file-wide, not per lexical scope; two
// modules in one file that alias the same name differently can misresolve.
fn collect_aliases<'a>(
    node: Node,
    content: &'a str,
    depth: u32,
    aliases: &mut HashMap<&'a str, String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if let Some(arguments) = bare_call_arguments(node, content, "alias") {
        let module = first_positional_arg(arguments)
            .filter(|arg| arg.kind() == "alias")
            .and_then(|arg| node_text(content, arg));
        if let Some(module) = module {
            let local = keyword_value(arguments, "as", content)
                .filter(|value| value.kind() == "alias")
                .and_then(|value| node_text(content, value))
                .or_else(|| module.rsplit('.').next());
            if let Some(local) = local {
                aliases.insert(local, module.to_string());
            }
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_aliases(child, content, child_depth, aliases);
    }
}

/// The arguments of a bare `name ...` call, such as `alias X` or `use Tesla`.
fn bare_call_arguments<'a>(node: Node<'a>, content: &str, name: &str) -> Option<Node<'a>> {
    if node.kind() != "call" {
        return None;
    }
    let target = node.child_by_field_name("target")?;
    if target.kind() != "identifier" || node_text(content, target)? != name {
        return None;
    }
    child_of_kind(node, "arguments")
}

fn keyword_value<'a>(arguments: Node<'a>, key: &str, content: &str) -> Option<Node<'a>> {
    let mut cursor = arguments.walk();
    let keywords = arguments
        .named_children(&mut cursor)
        .find(|child| child.kind() == "keywords")?;
    let mut pair_cursor = keywords.walk();
    keywords
        .named_children(&mut pair_cursor)
        .find(|pair| {
            pair.child_by_field_name("key")
                .and_then(|k| node_text(content, k))
                .is_some_and(|text| text.trim().trim_end_matches(':').trim() == key)
        })
        .and_then(|pair| pair.child_by_field_name("value"))
}

/// For a `defmodule` whose body `use`s Tesla, the static URL of its
/// `plug Tesla.Middleware.BaseUrl, "..."`: `Some(None)` when the module has no
/// such plug, `None` when the module is not a Tesla client.
fn tesla_module_base_url<'a>(
    node: Node,
    context: &ClientContext,
    content: &'a str,
) -> Option<Option<&'a str>> {
    bare_call_arguments(node, content, "defmodule")?;
    let body = child_of_kind(node, "do_block")?;
    let mut cursor = body.walk();
    let statements: Vec<Node> = body.named_children(&mut cursor).collect();
    let uses_tesla = statements.iter().any(|statement| {
        bare_call_arguments(*statement, content, "use")
            .and_then(first_positional_arg)
            .and_then(|arg| node_text(content, arg))
            .is_some_and(|module| context.resolve(module) == "Tesla")
    });
    if !uses_tesla {
        return None;
    }
    Some(statements.iter().find_map(|statement| {
        let arguments = bare_call_arguments(*statement, content, "plug")?;
        let middleware = node_text(content, first_positional_arg(arguments)?)?;
        if context.resolve(middleware) != "Tesla.Middleware.BaseUrl" {
            return None;
        }
        static_route_arg(
            nth_positional_arg(arguments, 1)?,
            content,
            StaticArgLang::Elixir,
        )
    }))
}

#[allow(clippy::too_many_arguments)]
fn walk(
    node: Node,
    context: &ClientContext,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    depth: u32,
    facts: &mut Vec<StructuralFact>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    let mut inner = None;
    if let Some(base_url) = tesla_module_base_url(node, context, content) {
        let mut tesla = context.clone();
        tesla.tesla_module = Some(base_url);
        inner = Some(tesla);
    }
    let context = inner.as_ref().unwrap_or(context);

    if node.kind() == "call"
        && let Some(req) = classify_call(node, context, content)
    {
        let target_path = match (&req.base_url, req.target_path.starts_with('/')) {
            (Some(base_url), true) => {
                format!("{}{}", base_url.trim_end_matches('/'), req.target_path)
            }
            _ => req.target_path.to_string(),
        };
        if let Some(fact) = client_fact(
            language,
            tree,
            file_path,
            content,
            node.start_byte(),
            node.end_byte(),
            req.client,
            &target_path,
            req.verb,
            req.verb_source,
            None,
        ) {
            facts.push(fact);
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            child,
            context,
            language,
            tree,
            file_path,
            content,
            child_depth,
            facts,
        );
    }
}

fn classify_call<'a>(
    call: Node<'_>,
    context: &ClientContext<'a>,
    content: &'a str,
) -> Option<ElixirClientRequest<'a>> {
    if let Some(r) = module_client_request(call, context, content) {
        return Some(r);
    }
    if let Some(r) = tesla_module_request(call, context, content) {
        return Some(r);
    }
    httpc_request(call, content)
}

/// A bare `get("/path")` inside a `use Tesla` module, joined to the module's
/// `BaseUrl` plug.
fn tesla_module_request<'a>(
    call: Node<'_>,
    context: &ClientContext<'a>,
    content: &'a str,
) -> Option<ElixirClientRequest<'a>> {
    let base_url = context.tesla_module?;
    let target = call.child_by_field_name("target")?;
    if target.kind() != "identifier" {
        return None;
    }
    let verb = verb_for_method(node_text(content, target)?)?;
    let arguments = child_of_kind(call, "arguments")?;
    let target_path = static_route_arg(
        first_positional_arg(arguments)?,
        content,
        StaticArgLang::Elixir,
    )?;
    Some(ElixirClientRequest {
        client: "tesla",
        target_path,
        verb,
        verb_source: "attested",
        base_url,
    })
}

fn module_client_request<'a>(
    call: Node<'_>,
    context: &ClientContext<'a>,
    content: &'a str,
) -> Option<ElixirClientRequest<'a>> {
    let target = call.child_by_field_name("target")?;
    if target.kind() != "dot" {
        return None;
    }
    let module = target.child_by_field_name("left")?;
    if module.kind() != "alias" {
        return None;
    }
    let module_name = context.resolve(node_text(content, module)?);
    let method = node_text(content, target.child_by_field_name("right")?)?;
    let arguments = child_of_kind(call, "arguments")?;

    match module_name.as_str() {
        "Req" => {
            let verb = verb_for_method(method)?;
            let url_argument = first_positional_arg(arguments)?;
            let target_path = static_route_arg(url_argument, content, StaticArgLang::Elixir)?;
            Some(ElixirClientRequest {
                client: "req",
                target_path,
                verb,
                verb_source: "attested",
                base_url: None,
            })
        }
        "Tesla" => tesla_request(method, arguments, content),
        "HTTPoison" => httpoison_request(method, arguments, content),
        "Finch" => finch_request(method, arguments, content),
        _ => None,
    }
}

fn tesla_request<'a>(
    method: &str,
    arguments: Node<'_>,
    content: &'a str,
) -> Option<ElixirClientRequest<'a>> {
    let verb = verb_for_method(method)?;
    // Tesla.get(url) or Tesla.get!(url) — URL first
    // Tesla.get(client, url, ...) — URL second when first is not a static string
    let arg0 = first_positional_arg(arguments)?;
    if let Some(path) = static_route_arg(arg0, content, StaticArgLang::Elixir) {
        return Some(ElixirClientRequest {
            client: "tesla",
            target_path: path,
            verb,
            verb_source: "attested",
            base_url: None,
        });
    }
    let arg1 = nth_positional_arg(arguments, 1)?;
    let path = static_route_arg(arg1, content, StaticArgLang::Elixir)?;
    Some(ElixirClientRequest {
        client: "tesla",
        target_path: path,
        verb,
        verb_source: "attested",
        base_url: None,
    })
}

fn httpoison_request<'a>(
    method: &str,
    arguments: Node<'_>,
    content: &'a str,
) -> Option<ElixirClientRequest<'a>> {
    let bare = method.strip_suffix('!').unwrap_or(method);
    if bare == "request" {
        let method_arg = first_positional_arg(arguments)?;
        let verb = atom_from_node(method_arg, content).and_then(atom_verb)?;
        let url_arg = nth_positional_arg(arguments, 1)?;
        let target_path = static_route_arg(url_arg, content, StaticArgLang::Elixir)?;
        return Some(ElixirClientRequest {
            client: "httpoison",
            target_path,
            verb,
            verb_source: "attested",
            base_url: None,
        });
    }
    let verb = verb_for_method(method)?;
    let url_arg = first_positional_arg(arguments)?;
    let target_path = static_route_arg(url_arg, content, StaticArgLang::Elixir)?;
    Some(ElixirClientRequest {
        client: "httpoison",
        target_path,
        verb,
        verb_source: "attested",
        base_url: None,
    })
}

fn finch_request<'a>(
    method: &str,
    arguments: Node<'_>,
    content: &'a str,
) -> Option<ElixirClientRequest<'a>> {
    if method != "build" {
        return None;
    }
    let method_arg = first_positional_arg(arguments)?;
    let verb = atom_from_node(method_arg, content).and_then(atom_verb)?;
    let url_arg = nth_positional_arg(arguments, 1)?;
    let target_path = static_route_arg(url_arg, content, StaticArgLang::Elixir)?;
    Some(ElixirClientRequest {
        client: "finch",
        target_path,
        verb,
        verb_source: "attested",
        base_url: None,
    })
}

fn httpc_request<'a>(call: Node<'_>, content: &'a str) -> Option<ElixirClientRequest<'a>> {
    let target = call.child_by_field_name("target")?;
    if target.kind() != "dot" {
        return None;
    }
    let left = target.child_by_field_name("left")?;
    let right = target.child_by_field_name("right")?;
    // Only the `:httpc` atom receiver counts; a variable named `httpc` is not
    // the OTP client (M2).
    if left.kind() != "atom" || node_text(content, left)? != ":httpc" {
        return None;
    }
    if node_text(content, right)? != "request" {
        return None;
    }
    let arguments = child_of_kind(call, "arguments")?;
    let arg0 = first_positional_arg(arguments)?;
    // :httpc.request(url) GET default
    if let Some(path) = static_route_arg(arg0, content, StaticArgLang::Elixir) {
        return Some(ElixirClientRequest {
            client: "httpc",
            target_path: path,
            verb: "GET",
            verb_source: "default",
            base_url: None,
        });
    }
    // :httpc.request(method, {url, headers}, ...)
    let verb = atom_from_node(arg0, content).and_then(atom_verb)?;
    let tuple = nth_positional_arg(arguments, 1)?;
    if tuple.kind() != "tuple" {
        return None;
    }
    let url = first_tuple_element(tuple)?;
    let target_path = static_route_arg(url, content, StaticArgLang::Elixir)?;
    Some(ElixirClientRequest {
        client: "httpc",
        target_path,
        verb,
        verb_source: "attested",
        base_url: None,
    })
}

fn atom_from_node<'a>(node: Node<'_>, content: &'a str) -> Option<&'a str> {
    let text = node_text(content, node)?;
    if node.kind() == "atom" || text.starts_with(':') {
        return Some(text);
    }
    None
}

fn first_tuple_element(tuple: Node) -> Option<Node> {
    let mut cursor = tuple.walk();
    tuple.named_children(&mut cursor).next()
}

fn first_positional_arg(arguments: Node) -> Option<Node> {
    nth_positional_arg(arguments, 0)
}

fn nth_positional_arg(arguments: Node, index: usize) -> Option<Node> {
    let mut cursor = arguments.walk();
    let mut i = 0;
    for child in arguments.named_children(&mut cursor) {
        if child.kind() == "keywords" {
            continue;
        }
        if i == index {
            return Some(child);
        }
        i += 1;
    }
    None
}
