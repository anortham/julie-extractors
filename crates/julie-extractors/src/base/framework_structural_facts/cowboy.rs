//! Cowboy dispatch tables.
//!
//! `cowboy_router:compile/1` takes a static routing table:
//! `[{Host, [{PathMatch, Handler, Opts}]}]`. A host entry may add a constraint
//! list (`{Host, Constraints, Paths}`) and so may a path entry
//! (`{PathMatch, Constraints, Handler, Opts}`). Each path entry becomes one
//! `cowboy.route.v1` fact. Cowboy dispatches every HTTP method to the handler,
//! so the fact carries no verb.

use tree_sitter::{Node, Tree};

use super::COWBOY_ROUTE_PATTERN_ID;
use super::helpers::{insert_string, node_text};
use super::scan::{RouteFactSpec, route_fact};
use crate::base::http_boundary::ParamFlavor;
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) fn collect_cowboy_routes(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    if !content.contains("cowboy_router") {
        return Vec::new();
    }
    let mut facts = Vec::new();
    walk(
        tree.root_node(),
        language,
        tree,
        file_path,
        content,
        &mut facts,
        0,
    );
    facts
}

fn walk(
    node: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if let Some((_, args)) =
        remote_call(node, content, "cowboy_router").filter(|(function, _)| *function == "compile")
        && let Some(table) = args.first().filter(|table| table.kind() == "list")
    {
        for host_entry in named_children(*table) {
            emit_host(host_entry, language, tree, file_path, content, facts);
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in named_children(node) {
        walk(
            child,
            language,
            tree,
            file_path,
            content,
            facts,
            child_depth,
        );
    }
}

fn emit_host(
    host_entry: Node,
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    if host_entry.kind() != "tuple" {
        return;
    }
    let elements = named_children(host_entry);
    let (Some(host), Some(paths)) = (elements.first(), elements.last()) else {
        return;
    };
    if elements.len() < 2 || paths.kind() != "list" {
        return;
    }
    let host = static_term_text(*host, content);
    for path_entry in named_children(*paths) {
        let elements = named_children(path_entry);
        if path_entry.kind() != "tuple" || !(3..=4).contains(&elements.len()) {
            continue;
        }
        let Some(path) = static_term_text(elements[0], content) else {
            continue;
        };
        let handler = elements[elements.len() - 2];
        let handler = (handler.kind() == "atom")
            .then(|| node_text(content, handler).map(unquote_atom))
            .flatten();
        let spec = RouteFactSpec {
            framework: "cowboy",
            pattern_id: COWBOY_ROUTE_PATTERN_ID,
            capture_name: "route",
            api_style: "dispatch_table",
            route_template: &path,
            verb: None,
            verb_source: None,
            flavor: ParamFlavor::Colon,
            prefix: None,
            prefix_key: None,
        };
        let fact = route_fact(
            language,
            tree,
            file_path,
            content,
            path_entry.start_byte(),
            path_entry.end_byte(),
            spec,
            |metadata| {
                if let Some(host) = &host {
                    insert_string(metadata, "host", host);
                }
                if let Some(handler) = &handler {
                    insert_string(metadata, "handler_module", handler);
                }
            },
        );
        facts.extend(fact);
    }
}

/// `(function, arguments)` of a `module:function(...)` call whose module atom
/// is `module`.
pub(super) fn remote_call<'tree>(
    node: Node<'tree>,
    content: &str,
    module: &str,
) -> Option<(String, Vec<Node<'tree>>)> {
    if node.kind() != "remote" {
        return None;
    }
    let module_atom = node
        .child_by_field_name("module")?
        .child_by_field_name("module")
        .filter(|atom| atom.kind() == "atom")?;
    if unquote_atom(node_text(content, module_atom)?) != module {
        return None;
    }
    let call = node
        .child_by_field_name("fun")
        .filter(|call| call.kind() == "call")?;
    let function = call
        .child_by_field_name("expr")
        .filter(|atom| atom.kind() == "atom")?;
    let args = call.child_by_field_name("args")?;
    Some((
        unquote_atom(node_text(content, function)?),
        named_children(args),
    ))
}

/// The text of a static string term: `"text"`, `<<"text">>`, or an atom such
/// as the `'_'` host wildcard. A string with an escape sequence is
/// not static.
pub(super) fn static_term_text(node: Node, content: &str) -> Option<String> {
    match node.kind() {
        "atom" => Some(unquote_atom(node_text(content, node)?)),
        "string" => {
            let text = node_text(content, node)?;
            let inner = text.strip_prefix('"')?.strip_suffix('"')?;
            (!inner.contains('\\')).then(|| inner.to_string())
        }
        "binary" => {
            let [element] = named_children(node).try_into().ok()?;
            let string = element
                .child_by_field_name("element")
                .filter(|string| string.kind() == "string" && element.named_child_count() == 1)?;
            static_term_text(string, content)
        }
        _ => None,
    }
}

pub(super) fn named_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn unquote_atom(text: &str) -> String {
    text.strip_prefix('\'')
        .and_then(|inner| inner.strip_suffix('\''))
        .unwrap_or(text)
        .to_string()
}
