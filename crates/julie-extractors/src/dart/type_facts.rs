use super::helpers::find_child_by_type;
use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Node;

pub(super) const DART_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &[],
    generic_open: &['<'],
};

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    let Some(base_name) = base_type_name(base, type_node) else {
        return;
    };
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &DART_TYPE_NAME_RULES,
        false,
    );
}

fn base_type_name(base: &BaseExtractor, type_node: Node) -> Option<String> {
    let container = match type_node.kind() {
        "type" | "nullable_type" => type_node,
        "type_identifier" => return Some(base.get_node_text(&type_node)),
        _ => return None,
    };
    let mut segments = Vec::new();
    let mut cursor = container.walk();
    for child in container.named_children(&mut cursor) {
        match child.kind() {
            "type_identifier" => segments.push(base.get_node_text(&child)),
            "type_arguments" => {}
            _ => return None,
        }
    }
    if segments.is_empty() {
        None
    } else {
        Some(segments.join("."))
    }
}

pub(super) fn record_constructor_fact(base: &mut BaseExtractor, symbol_id: &str, class_name: &str) {
    base.record_declared_type_fact(symbol_id, class_name, &DART_TYPE_NAME_RULES, true);
}

pub(super) fn collect_type_names(base: &BaseExtractor, root: Node) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_type_names_into(base, root, 0, &mut names);
    names
}

fn collect_type_names_into(
    base: &BaseExtractor,
    node: Node,
    depth: u32,
    names: &mut HashSet<String>,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if matches!(
        node.kind(),
        "class_definition" | "class_declaration" | "mixin_declaration" | "enum_declaration"
    ) && let Some(name_node) = node
        .child_by_field_name("name")
        .or_else(|| find_child_by_type(&node, "identifier"))
    {
        names.insert(base.get_node_text(&name_node));
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_type_names_into(base, child, child_depth, names);
    }
}

pub(super) fn inferred_constructor_name(
    base: &BaseExtractor,
    value: Node,
    same_file_types: &HashSet<String>,
) -> Option<String> {
    match value.kind() {
        "new_expression" | "const_object_expression" => {
            let type_node = value.child_by_field_name("type")?;
            let name = constructor_type_name(base, type_node)?;
            same_file_types.contains(&name).then_some(name)
        }
        "call_expression" => {
            let function = value.child_by_field_name("function")?;
            let name = match function.kind() {
                "identifier" => base.get_node_text(&function),
                "member_expression" | "null_aware_member_expression" => {
                    let object = function.child_by_field_name("object")?;
                    if object.kind() != "identifier" {
                        return None;
                    }
                    base.get_node_text(&object)
                }
                _ => return None,
            };
            same_file_types.contains(&name).then_some(name)
        }
        _ => None,
    }
}

fn constructor_type_name(base: &BaseExtractor, type_node: Node) -> Option<String> {
    let declared = base.get_node_text(&type_node);
    let stripped = crate::base::types::strip_type_decorations(&declared, &DART_TYPE_NAME_RULES);
    if stripped.is_empty() || stripped.contains('.') {
        None
    } else {
        Some(stripped)
    }
}
