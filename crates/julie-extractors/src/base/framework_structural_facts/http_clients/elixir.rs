//! Elixir HTTP client-request facts (`http.client_request.v1`) for Req, Tesla,
//! HTTPoison, Finch, and OTP `:httpc`.
//!
//! Silence (design §4.4, M2): only static string/charlist URLs produce a fact.

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
    let req = content.contains("Req.");
    let tesla = content.contains("Tesla.");
    let httpoison = content.contains("HTTPoison.");
    let finch = content.contains("Finch.");
    let httpc = content.contains(":httpc.");
    if !req && !tesla && !httpoison && !finch && !httpc {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        language,
        tree,
        file_path,
        content,
        0,
        &mut facts,
    );
    facts
}

fn walk(
    node: Node,
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

    if node.kind() == "call"
        && let Some(req) = classify_call(node, content)
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
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(
            child,
            language,
            tree,
            file_path,
            content,
            child_depth,
            facts,
        );
    }
}

fn classify_call<'a>(call: Node<'_>, content: &'a str) -> Option<ElixirClientRequest<'a>> {
    if let Some(r) = module_client_request(call, content) {
        return Some(r);
    }
    httpc_request(call, content)
}

fn module_client_request<'a>(call: Node<'_>, content: &'a str) -> Option<ElixirClientRequest<'a>> {
    let target = call.child_by_field_name("target")?;
    if target.kind() != "dot" {
        return None;
    }
    let module = target.child_by_field_name("left")?;
    if module.kind() != "alias" {
        return None;
    }
    let module_name = node_text(content, module)?;
    let method = node_text(content, target.child_by_field_name("right")?)?;
    let arguments = child_of_kind(call, "arguments")?;

    match module_name {
        "Req" => {
            let verb = verb_for_method(method)?;
            let url_argument = first_positional_arg(arguments)?;
            let target_path = static_route_arg(url_argument, content, StaticArgLang::Elixir)?;
            Some(ElixirClientRequest {
                client: "req",
                target_path,
                verb,
                verb_source: "attested",
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
        });
    }
    let arg1 = nth_positional_arg(arguments, 1)?;
    let path = static_route_arg(arg1, content, StaticArgLang::Elixir)?;
    Some(ElixirClientRequest {
        client: "tesla",
        target_path: path,
        verb,
        verb_source: "attested",
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
