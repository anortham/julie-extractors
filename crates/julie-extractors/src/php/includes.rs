// PHP Extractor - require/include file imports

use super::PhpExtractor;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// The directive keyword of a `require`, `require_once`, `include`, or
/// `include_once` expression node.
fn include_form(kind: &str) -> Option<&'static str> {
    match kind {
        "require_expression" => Some("require"),
        "require_once_expression" => Some("require_once"),
        "include_expression" => Some("include"),
        "include_once_expression" => Some("include_once"),
        _ => None,
    }
}

/// A file include as an import, named by the included path as C names an
/// `#include`. Metadata `source` holds the path when the argument is a plain
/// string, or `__DIR__ . '/x.php'` (also `dirname(__FILE__)` and
/// `dirname(__DIR__)`), which becomes `./x.php` or `../x.php` with
/// `relative_to: "file"`. Any other argument keeps the row with no `source`,
/// named by the argument's text.
pub(super) fn extract_include(
    extractor: &mut PhpExtractor,
    node: Node,
    parent_id: Option<&str>,
) -> Option<Symbol> {
    let form = include_form(node.kind())?;
    let argument = unwrap_parentheses(node.named_child(0)?);
    let base = extractor.get_base();
    let mut metadata = HashMap::from([(
        "form".to_string(),
        serde_json::Value::String(form.to_string()),
    )]);
    let source = match plain_string(base, argument) {
        Some(path) => Some(path),
        None => {
            let relative = file_relative_path(base, argument);
            if relative.is_some() {
                metadata.insert(
                    "relative_to".to_string(),
                    serde_json::Value::String("file".to_string()),
                );
            }
            relative
        }
    };
    let name = match &source {
        Some(source) => {
            metadata.insert(
                "source".to_string(),
                serde_json::Value::String(source.clone()),
            );
            source.clone()
        }
        None => base.get_node_text(&argument).trim().to_string(),
    };
    if name.is_empty() {
        return None;
    }
    let signature = base.get_node_text(&node);
    let doc_comment = base.find_doc_comment(&node);

    Some(extractor.get_base_mut().create_symbol(
        &node,
        name,
        SymbolKind::Import,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id: parent_id.map(|s| s.to_string()),
            metadata: Some(metadata),
            doc_comment,
            annotations: Vec::new(),
        },
    ))
}

fn unwrap_parentheses(mut node: Node) -> Node {
    while node.kind() == "parenthesized_expression" {
        match node.named_child(0) {
            Some(inner) => node = inner,
            None => break,
        }
    }
    node
}

/// The text of a string with no interpolated variable.
fn plain_string(base: &BaseExtractor, node: Node) -> Option<String> {
    if !matches!(node.kind(), "string" | "encapsed_string") {
        return None;
    }
    let mut cursor = node.walk();
    let interpolated = node
        .named_children(&mut cursor)
        .any(|child| !matches!(child.kind(), "string_content" | "escape_sequence"));
    if interpolated {
        return None;
    }
    base.decode_string_literal(&node)
        .filter(|path| !path.is_empty())
}

/// `__DIR__ . '/lib/x.php'` as `./lib/x.php`, and `dirname(__DIR__) . '/x.php'`
/// as `../x.php`.
fn file_relative_path(base: &BaseExtractor, node: Node) -> Option<String> {
    if node.kind() != "binary_expression" || operator(base, node)? != "." {
        return None;
    }
    let left = unwrap_parentheses(node.child_by_field_name("left")?);
    let folder = match directory_depth(base, left)? {
        0 => ".",
        _ => "..",
    };
    let tail = plain_string(base, unwrap_parentheses(node.child_by_field_name("right")?))?;
    let tail = tail.strip_prefix('/')?;
    if tail.is_empty() {
        return None;
    }
    if folder == "." && tail.starts_with("../") {
        return Some(tail.to_string());
    }
    Some(format!("{folder}/{tail}"))
}

fn operator(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| !child.is_named())
        .map(|operator| base.get_node_text(&operator))
}

/// 0 for the file's own folder (`__DIR__`, `dirname(__FILE__)`), 1 for its
/// parent (`dirname(__DIR__)`).
fn directory_depth(base: &BaseExtractor, node: Node) -> Option<u8> {
    match node.kind() {
        "name" if base.get_node_text(&node) == "__DIR__" => Some(0),
        "function_call_expression" => {
            let function = node.child_by_field_name("function")?;
            if base.get_node_text(&function) != "dirname" {
                return None;
            }
            let arguments = node.child_by_field_name("arguments")?;
            let mut cursor = arguments.walk();
            let mut argument_nodes = arguments.named_children(&mut cursor);
            let only = argument_nodes.next()?;
            if argument_nodes.next().is_some() {
                return None;
            }
            match base.get_node_text(&only).trim() {
                "__FILE__" => Some(0),
                "__DIR__" => Some(1),
                _ => None,
            }
        }
        _ => None,
    }
}
