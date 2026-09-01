use crate::base::BaseExtractor;
use crate::base::types::{SymbolKind, TypeNameRules};
use tree_sitter::Node;

use super::idioms::{assignment_name, call_name, positional_string_argument};
use super::text_args::clean_r_name;
use super::RExtractor;

pub(super) const R_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &[],
};

pub(super) fn record_constructor_fact(base: &mut BaseExtractor, symbol_id: &str, class_name: &str) {
    base.record_declared_type_fact(symbol_id, class_name, &R_TYPE_NAME_RULES, true);
}

pub(super) fn same_file_constructor_class(
    extractor: &RExtractor,
    right: Node,
) -> Option<String> {
    if right.kind() != "call" {
        return None;
    }
    let callee = right.child(0)?;
    match callee.kind() {
        "identifier" => {
            let name = clean_r_name(&extractor.base.get_node_text(&callee))?;
            if name == "new" {
                let args = right.child(1)?;
                let class_name = positional_string_argument(extractor, args, 0)?;
                same_file_class(extractor, &class_name)
            } else {
                same_file_class(extractor, &name)
            }
        }
        "extract_operator" => {
            let object = callee.child(0)?;
            if object.kind() != "identifier" {
                return None;
            }
            let method = callee.child(2)?;
            if extractor.base.get_node_text(&method) != "new" {
                return None;
            }
            let class_name = clean_r_name(&extractor.base.get_node_text(&object))?;
            same_file_class(extractor, &class_name)
        }
        _ => None,
    }
}

pub(super) fn self_receiver_type(extractor: &RExtractor, function_node: Node) -> Option<String> {
    if function_node.kind() != "extract_operator" {
        return None;
    }
    let object = function_node.child(0)?;
    if extractor.base.get_node_text(&object) != "self" {
        return None;
    }
    enclosing_r6_class_name(extractor, function_node)
}

fn same_file_class(extractor: &RExtractor, name: &str) -> Option<String> {
    extractor
        .symbols
        .iter()
        .any(|symbol| symbol.name == name && symbol.kind == SymbolKind::Class)
        .then(|| name.to_string())
}

pub(super) fn enclosing_r6_class_name(extractor: &RExtractor, node: Node) -> Option<String> {
    let mut current = node;
    let mut in_public_or_private = false;
    while let Some(parent) = current.parent() {
        if argument_name(extractor, parent)
            .is_some_and(|name| name == "public" || name == "private")
        {
            in_public_or_private = true;
        }
        if in_public_or_private
            && parent.kind() == "call"
            && call_name(extractor, parent).as_deref() == Some("R6Class")
        {
            return r6_class_name(extractor, parent);
        }
        current = parent;
    }
    None
}

pub(super) fn argument_name(extractor: &RExtractor, node: Node) -> Option<String> {
    match node.kind() {
        "argument" => {
            let name_node = node.child_by_field_name("name")?;
            clean_r_name(&extractor.base.get_node_text(&name_node))
        }
        "binary_operator" => assignment_name(extractor, node.child(0)?),
        _ => None,
    }
}

fn r6_class_name(extractor: &RExtractor, call: Node) -> Option<String> {
    if let Some(args) = call.child(1)
        && let Some(name) = positional_string_argument(extractor, args, 0)
    {
        return Some(name);
    }
    let parent = call.parent()?;
    if parent.kind() != "binary_operator" {
        return None;
    }
    assignment_name(extractor, parent.child(0)?)
}
