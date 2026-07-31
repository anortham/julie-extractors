//! Node helpers shared by the Erlang attribute and definition-form extractors.
//!
//! Node kinds come from `tree-sitter-erlang` 0.20.0 parse trees, not from the
//! Erlang reference manual: attributes are `module_attribute`,
//! `export_attribute`, `export_type_attribute`, `compile_options_attribute`,
//! `record_decl`, `type_alias`, `opaque`, `callback`, `spec`, `pp_define`, and
//! `wild_attribute`; functions are one `fun_decl` per clause.

use crate::base::BaseExtractor;
use tree_sitter::Node;

pub(super) use crate::base::find_child_by_type;

/// A function, type, or callback identity: name plus arity.
pub(super) type NameArity = (String, u32);

pub(super) fn child_named_kinds<'a>(node: &Node<'a>, kind: &str) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == kind)
        .collect()
}

pub(super) fn named_children<'a>(node: &Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

/// Text of the first `atom` child, with any quoting removed.
pub(super) fn first_atom_text(base: &BaseExtractor, node: &Node) -> Option<String> {
    let atom = find_child_by_type(node, "atom")?;
    Some(unquote_atom(&base.get_node_text(&atom)))
}

pub(super) fn unquote_atom(text: &str) -> String {
    text.trim().trim_matches('\'').to_string()
}

/// Number of arguments carried by an `expr_args` or `var_args` node.
pub(super) fn arg_count(args: &Node) -> u32 {
    named_children(args).len() as u32
}

/// `fa` nodes (`open/1`) appear inside `-export` and `-export_type` lists.
pub(super) fn function_arity_entries(base: &BaseExtractor, node: &Node) -> Vec<NameArity> {
    let mut entries = Vec::new();
    collect_function_arity_entries(base, node, &mut entries);
    entries
}

fn collect_function_arity_entries(base: &BaseExtractor, node: &Node, entries: &mut Vec<NameArity>) {
    if node.kind() == "fa" {
        if let Some(entry) = function_arity(base, node) {
            entries.push(entry);
        }
        return;
    }

    for child in named_children(node) {
        collect_function_arity_entries(base, &child, entries);
    }
}

fn function_arity(base: &BaseExtractor, node: &Node) -> Option<NameArity> {
    let name = first_atom_text(base, node)?;
    let arity_node = find_child_by_type(node, "arity")?;
    let integer = find_child_by_type(&arity_node, "integer")?;
    let arity = base.get_node_text(&integer).trim().parse().ok()?;
    Some((name, arity))
}

/// Attribute source without the trailing `.`, used as a symbol signature.
pub(super) fn attribute_signature(base: &BaseExtractor, node: &Node) -> String {
    base.get_node_text(node)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches('.')
        .trim_end()
        .to_string()
}

/// Name of a `wild_attribute` (`-doc "..."` -> `doc`).
pub(super) fn wild_attribute_name(base: &BaseExtractor, node: &Node) -> Option<String> {
    let attr_name = find_child_by_type(node, "attr_name")?;
    first_atom_text(base, &attr_name)
}

/// String payload of a `-doc` / `-moduledoc` attribute, with quotes removed.
pub(super) fn wild_attribute_string(base: &BaseExtractor, node: &Node) -> Option<String> {
    let string = find_child_by_type(node, "string")?;
    let text = base.get_node_text(&string);
    let trimmed = text.trim();
    let unquoted = trimmed
        .strip_prefix("\"\"\"")
        .and_then(|rest| rest.strip_suffix("\"\"\""))
        .or_else(|| {
            trimmed
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .unwrap_or(trimmed);
    let doc = unquoted.trim().to_string();
    (!doc.is_empty()).then_some(doc)
}

/// Attributes that document or annotate the declaration directly below them,
/// nearest first. Comments between the attribute and the declaration are
/// transparent; any other node ends the run. `-moduledoc` documents the module
/// rather than the next declaration, so it terminates the run instead of
/// joining it.
pub(super) fn preceding_attributes<'a>(base: &BaseExtractor, node: &Node<'a>) -> Vec<Node<'a>> {
    let mut attributes = Vec::new();
    let mut current = node.prev_named_sibling();

    while let Some(sibling) = current {
        match sibling.kind() {
            "comment" => {}
            "spec" => attributes.push(sibling),
            "wild_attribute" => {
                if wild_attribute_name(base, &sibling).as_deref() == Some("moduledoc") {
                    break;
                }
                attributes.push(sibling);
            }
            _ => break,
        }
        current = sibling.prev_named_sibling();
    }

    attributes
}
