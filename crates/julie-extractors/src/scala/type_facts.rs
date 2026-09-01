use crate::base::types::{TypeNameRules, strip_type_decorations};
use crate::base::{BaseExtractor, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) const SCALA_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['['],
};

const DECLARED_TYPE_METADATA_KEYS: [&str; 2] = ["returnType", "propertyType"];

/// Base type names for symbols whose metadata carries declared type text.
/// Shapes with no single base name (tuples, function types, compound types)
/// record nothing.
pub(super) fn metadata_base_types(symbols: &[Symbol]) -> HashMap<String, String> {
    symbols
        .iter()
        .filter_map(|symbol| {
            let declared = declared_type_metadata(symbol)?;
            Some((symbol.id.clone(), base_type_name_from_text(declared)?))
        })
        .collect()
}

fn declared_type_metadata(symbol: &Symbol) -> Option<&str> {
    let metadata = symbol.metadata.as_ref()?;
    DECLARED_TYPE_METADATA_KEYS
        .iter()
        .find_map(|key| metadata.get(*key).and_then(serde_json::Value::as_str))
}

fn base_type_name_from_text(declared: &str) -> Option<String> {
    let name = strip_type_decorations(declared, &SCALA_TYPE_NAME_RULES);
    let is_qualified_name = !name.is_empty() && name.split('.').all(is_type_name_segment);
    is_qualified_name.then_some(name)
}

fn is_type_name_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

pub(super) fn record_initializer_type(base: &mut BaseExtractor, symbol_id: &str, value: Node) {
    match value.kind() {
        "instance_expression" => {
            if let Some(type_node) = instance_type_node(value) {
                record_type_node(base, symbol_id, type_node, true);
            }
        }
        "call_expression" => {
            if let Some(class_name) = same_file_constructor_class(base, value) {
                base.record_declared_type_fact(
                    symbol_id,
                    &class_name,
                    &SCALA_TYPE_NAME_RULES,
                    true,
                );
            }
        }
        _ => {}
    }
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    let Some(name_node) = base_type_name_node(type_node) else {
        return;
    };
    let base_name = base.get_node_text(&name_node);
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &SCALA_TYPE_NAME_RULES,
        is_inferred,
    );
}

fn base_type_name_node(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        match node.kind() {
            "type_identifier" | "identifier" => return Some(node),
            "generic_type" => {
                node = node.child_by_field_name("type")?;
            }
            "stable_type_identifier" => {
                return last_named_type_identifier(node);
            }
            _ => return None,
        }
    }
}

fn last_named_type_identifier(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| matches!(child.kind(), "type_identifier" | "identifier"))
        .last()
}

fn instance_type_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|child| {
        matches!(
            child.kind(),
            "type_identifier" | "generic_type" | "stable_type_identifier"
        )
    })
}

fn same_file_constructor_class(base: &BaseExtractor, value: Node) -> Option<String> {
    let callee = call_callee(value)?;
    if callee.kind() != "identifier" {
        return None;
    }
    let name = base.get_node_text(&callee);
    same_file_has_class(file_root(value), &name, base, 0).then_some(name)
}

fn call_callee(value: Node) -> Option<Node> {
    let callee = value.child_by_field_name("function")?;
    if callee.kind() == "generic_function" {
        callee.child_by_field_name("function")
    } else {
        Some(callee)
    }
}

fn file_root(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

fn same_file_has_class(node: Node, name: &str, base: &BaseExtractor, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.kind() == "class_definition"
        && node
            .child_by_field_name("name")
            .is_some_and(|name_node| base.get_node_text(&name_node) == name)
    {
        return true;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if same_file_has_class(child, name, base, child_depth) {
            return true;
        }
    }
    false
}
