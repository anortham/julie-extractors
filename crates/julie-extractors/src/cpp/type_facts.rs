//! Declared-type fact recording for C++.

use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["const", "volatile", "struct", "class"],
    generic_open: &['<'],
};

pub(super) fn record_variable_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    declaration: Node,
    declarator: Node,
) {
    let Some(type_node) = declaration.child_by_field_name("type") else {
        return;
    };
    if is_auto_type(type_node) {
        let value = match declarator.kind() {
            "init_declarator" => declarator.child_by_field_name("value"),
            _ => None,
        };
        if let Some(name) =
            value.and_then(|value| inferred_constructor_name(base, value, declaration))
        {
            base.record_declared_type_fact_with_declared(
                symbol_id,
                &name,
                &name,
                &TYPE_NAME_RULES,
                true,
            );
        }
        return;
    }
    record_stated_type(base, symbol_id, declaration, type_node, Some(declarator));
}

pub(super) fn record_parameter_fact(base: &mut BaseExtractor, symbol_id: &str, param_node: Node) {
    let Some(type_node) = param_node.child_by_field_name("type") else {
        return;
    };
    if is_auto_type(type_node) {
        return;
    }
    record_stated_type(
        base,
        symbol_id,
        param_node,
        type_node,
        param_node.child_by_field_name("declarator"),
    );
}

pub(super) fn record_field_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    field_node: Node,
    declarator: Option<Node>,
) {
    let Some(type_node) = field_node.child_by_field_name("type") else {
        return;
    };
    if is_auto_type(type_node) {
        return;
    }
    record_stated_type(base, symbol_id, field_node, type_node, declarator);
}

fn record_stated_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    container: Node,
    type_node: Node,
    declarator: Option<Node>,
) {
    if declarator.is_some_and(|declarator| contains_function_declarator(declarator, 0)) {
        return;
    }
    let Some(base_name) = structural_base_name(base, type_node, 0) else {
        return;
    };
    let declared = declared_type_text(base, container, type_node, declarator);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &TYPE_NAME_RULES,
        false,
    );
}

fn is_auto_type(type_node: Node) -> bool {
    matches!(type_node.kind(), "placeholder_type_specifier" | "auto")
}

fn structural_base_name(base: &BaseExtractor, node: Node, depth: u32) -> Option<String> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let mut node = node;
    loop {
        match node.kind() {
            "type_identifier" | "identifier" | "primitive_type" | "namespace_identifier" => {
                return Some(base.get_node_text(&node));
            }
            "sized_type_specifier" => {
                return single_word_sized_type(node).map(|node| base.get_node_text(&node));
            }
            "template_type" => {
                node = node.child_by_field_name("name")?;
            }
            "qualified_identifier" => {
                let child_depth = child_tree_depth(depth)?;
                let scope = node.child_by_field_name("scope")?;
                let name = node.child_by_field_name("name")?;
                let scope_text = structural_base_name(base, scope, child_depth)
                    .unwrap_or_else(|| base.get_node_text(&scope));
                let name_text = structural_base_name(base, name, child_depth)?;
                return Some(format!("{scope_text}::{name_text}"));
            }
            "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier" => {
                let name = node
                    .children(&mut node.walk())
                    .find(|child| child.kind() == "type_identifier")?;
                return Some(base.get_node_text(&name));
            }
            _ => return None,
        }
    }
}

fn declared_type_text(
    base: &BaseExtractor,
    container: Node,
    type_node: Node,
    declarator: Option<Node>,
) -> String {
    let mut start = type_node.start_byte();
    let mut end = type_node.end_byte();
    let mut cursor = container.walk();
    for child in container.children(&mut cursor) {
        if child.kind() == "type_qualifier" {
            start = start.min(child.start_byte());
            end = end.max(child.end_byte());
        }
    }
    let mut declared = base.content[start..end].to_string();
    if let Some(declarator) = declarator {
        declared.push_str(&decoration_suffix(base, declarator, 0));
    }
    declared
}

fn single_word_sized_type(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let words = node
        .children(&mut cursor)
        .filter(|child| child.kind() != "type_qualifier")
        .count();
    (words == 1).then_some(node)
}

fn contains_function_declarator(node: Node, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.kind() == "function_declarator" {
        return true;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    match node.kind() {
        "pointer_declarator"
        | "reference_declarator"
        | "array_declarator"
        | "parenthesized_declarator"
        | "init_declarator" => node
            .child_by_field_name("declarator")
            .or_else(|| node.named_child(0))
            .is_some_and(|inner| contains_function_declarator(inner, child_depth)),
        _ => false,
    }
}

fn decoration_suffix(base: &BaseExtractor, node: Node, depth: u32) -> String {
    if !should_visit_tree_depth(depth) {
        return String::new();
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return String::new();
    };
    match node.kind() {
        "pointer_declarator" => {
            let inner = node
                .child_by_field_name("declarator")
                .map(|inner| decoration_suffix(base, inner, child_depth))
                .unwrap_or_default();
            format!("*{inner}")
        }
        "reference_declarator" => {
            let kind = reference_kind(base, node);
            let inner = node
                .named_child(0)
                .map(|inner| decoration_suffix(base, inner, child_depth))
                .unwrap_or_default();
            format!("{kind}{inner}")
        }
        "init_declarator" => node
            .child_by_field_name("declarator")
            .map(|inner| decoration_suffix(base, inner, child_depth))
            .unwrap_or_default(),
        "parenthesized_declarator" | "array_declarator" => node
            .child_by_field_name("declarator")
            .or_else(|| node.named_child(0))
            .map(|inner| decoration_suffix(base, inner, child_depth))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn reference_kind(base: &BaseExtractor, node: Node) -> &'static str {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match base.get_node_text(&child).as_str() {
            "&&" => return "&&",
            "&" => return "&",
            _ => {}
        }
    }
    "&"
}

fn inferred_constructor_name(base: &BaseExtractor, value: Node, origin: Node) -> Option<String> {
    match value.kind() {
        "call_expression" => {
            let function = value.child_by_field_name("function")?;
            if function.kind() != "identifier" {
                return None;
            }
            let name = base.get_node_text(&function);
            same_file_defines_type(base, origin, &name).then_some(name)
        }
        "new_expression" => {
            let type_node = value.child_by_field_name("type")?;
            if type_node.kind() == "qualified_identifier" {
                return None;
            }
            let name = structural_base_name(base, type_node, 0)?;
            same_file_defines_type(base, origin, &name).then_some(name)
        }
        _ => None,
    }
}

fn same_file_defines_type(base: &BaseExtractor, node: Node, name: &str) -> bool {
    find_named_type(file_root(node), base, name, 0)
}

fn file_root(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

fn find_named_type(node: Node, base: &BaseExtractor, name: &str, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if matches!(
        node.kind(),
        "class_specifier" | "struct_specifier" | "union_specifier"
    ) {
        let found = node
            .children(&mut node.walk())
            .any(|child| child.kind() == "type_identifier" && base.get_node_text(&child) == name);
        if found {
            return true;
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if find_named_type(child, base, name, child_depth) {
            return true;
        }
    }
    false
}
