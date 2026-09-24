//! Ruby's lexical rule for a bare identifier: it is a local variable when a
//! binding for the name appears earlier in the same scope, and a receiverless
//! method call otherwise. tree-sitter-ruby does not track locals, so a bare
//! `subtotal` is an `identifier` whether it reads a local or calls a method.
//!
//! A `def`, `class`, `module`, or the program starts a new scope. A block or
//! lambda sees the enclosing scope's bindings, and its own bindings stay
//! inside it.

use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

struct Binding {
    name: String,
    start_byte: usize,
    /// Byte ranges of the blocks between the binding and its scope root.
    block_ranges: Vec<(usize, usize)>,
}

/// Caches the bindings of each scope root, keyed by node id.
#[derive(Default)]
pub(super) struct LocalBindings {
    scopes: HashMap<usize, Vec<Binding>>,
}

impl LocalBindings {
    /// Whether a value-position `identifier` is a receiverless method call.
    pub(super) fn is_method_call(&mut self, content: &str, identifier: Node) -> bool {
        let name = &content[identifier.byte_range()];
        if matches!(
            name,
            "private" | "protected" | "public" | "module_function" | "__method__"
        ) {
            return false;
        }
        let Some(root) = scope_root(identifier) else {
            return false;
        };
        let bindings = self
            .scopes
            .entry(root.id())
            .or_insert_with(|| collect_bindings(content, root));
        let position = identifier.start_byte();
        !bindings.iter().any(|binding| {
            binding.name == name
                && binding.start_byte <= position
                && binding
                    .block_ranges
                    .iter()
                    .all(|(start, end)| *start <= position && position < *end)
        })
    }
}

fn is_scope_root(kind: &str) -> bool {
    matches!(
        kind,
        "method" | "singleton_method" | "class" | "module" | "singleton_class" | "program"
    )
}

fn is_block(kind: &str) -> bool {
    matches!(kind, "block" | "do_block" | "lambda")
}

fn scope_root(node: Node) -> Option<Node> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if is_scope_root(candidate.kind()) {
            return Some(candidate);
        }
        current = candidate.parent();
    }
    None
}

fn collect_bindings(content: &str, root: Node) -> Vec<Binding> {
    let mut bindings = Vec::new();
    let mut blocks = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        visit(content, child, &mut blocks, &mut bindings, 0);
    }
    bindings
}

fn visit(
    content: &str,
    node: Node,
    blocks: &mut Vec<(usize, usize)>,
    bindings: &mut Vec<Binding>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) || is_scope_root(node.kind()) {
        return;
    }
    for name in bound_names(content, node) {
        bindings.push(Binding {
            name,
            start_byte: node.start_byte(),
            block_ranges: blocks.clone(),
        });
    }
    let opens_block = is_block(node.kind());
    if opens_block {
        blocks.push((node.start_byte(), node.end_byte()));
    }
    if let Some(child_depth) = child_tree_depth(depth) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            visit(content, child, blocks, bindings, child_depth);
        }
    }
    if opens_block {
        blocks.pop();
    }
}

/// The locals `node` introduces: an identifier that `binds`, the key of a
/// `{name:}` pattern, or a named group of a regex literal matched with `=~`.
fn bound_names(content: &str, node: Node) -> Vec<String> {
    match node.kind() {
        "identifier" if binds(node) => vec![content[node.byte_range()].to_string()],
        "keyword_pattern" if node.child_by_field_name("value").is_none() => node
            .child_by_field_name("key")
            .map(|key| {
                content[key.byte_range()]
                    .trim_matches(|c| matches!(c, '"' | '\'' | ':'))
                    .to_string()
            })
            .into_iter()
            .collect(),
        "binary" => node
            .child_by_field_name("left")
            .filter(|left| left.kind() == "regex")
            .filter(|_| {
                node.child_by_field_name("operator")
                    .is_some_and(|operator| &content[operator.byte_range()] == "=~")
            })
            .map(|regex| regex_group_names(&content[regex.byte_range()]))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The names of `(?<name>..)` and `(?'name'..)` groups in a regex literal.
fn regex_group_names(regex: &str) -> Vec<String> {
    regex
        .split("(?")
        .skip(1)
        .filter_map(|group| {
            let (close, rest) = match group.chars().next()? {
                '<' => ('>', &group[1..]),
                '\'' => ('\'', &group[1..]),
                _ => return None,
            };
            let name = &rest[..rest.find(close)?];
            (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
                .then(|| name.to_string())
        })
        .collect()
}

/// Whether an `identifier` introduces a local: an assignment target, a
/// parameter, a rescue variable, a `for` loop variable, or a pattern variable.
fn binds(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_field = |field: &str| {
        parent
            .child_by_field_name(field)
            .is_some_and(|child| child.id() == node.id())
    };
    match parent.kind() {
        "assignment" | "operator_assignment" => is_field("left"),
        "left_assignment_list"
        | "destructured_left_assignment"
        | "rest_assignment"
        | "method_parameters"
        | "block_parameters"
        | "lambda_parameters"
        | "splat_parameter"
        | "hash_splat_parameter"
        | "block_parameter"
        | "exception_variable"
        | "destructured_parameter" => true,
        "optional_parameter" | "keyword_parameter" => is_field("name"),
        "for" | "in_clause" | "match_pattern" | "test_pattern" => is_field("pattern"),
        "array_pattern" | "find_pattern" | "parenthesized_pattern" | "alternative_pattern" => true,
        "as_pattern" => is_field("name"),
        "keyword_pattern" => is_field("value"),
        _ => false,
    }
}
