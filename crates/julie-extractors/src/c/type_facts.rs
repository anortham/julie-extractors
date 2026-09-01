use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use tree_sitter::Node;

pub(super) const C_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["struct", "union", "enum", "const", "volatile"],
    generic_open: &[],
};

pub(super) fn record_declared_from_declaration(
    base: &mut BaseExtractor,
    symbol_id: &str,
    decl: Node,
    declarator: Node,
) {
    if contains_function_declarator(declarator) {
        return;
    }
    let Some(type_node) = decl.child_by_field_name("type") else {
        return;
    };
    let Some(name_node) = base_type_name_node(type_node) else {
        return;
    };
    let mut base_name = base.get_node_text(&name_node);
    let (stars, array_declared, array_count) = declarator_decorations(base, declarator);
    for _ in 0..array_count {
        base_name.push_str("[]");
    }
    let mut declared = declared_prefix(base, decl);
    if stars > 0 {
        declared.push(' ');
        declared.push_str(&"*".repeat(stars));
    }
    declared.push_str(&array_declared);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_name,
        &declared,
        &C_TYPE_NAME_RULES,
        false,
    );
}

fn base_type_name_node(node: Node) -> Option<Node> {
    match node.kind() {
        "primitive_type" | "type_identifier" | "sized_type_specifier" => Some(node),
        "struct_specifier" | "union_specifier" | "enum_specifier" => {
            node.child_by_field_name("name")
        }
        _ => None,
    }
}

fn declared_prefix(base: &BaseExtractor, decl: Node) -> String {
    let mut parts = Vec::new();
    let mut cursor = decl.walk();
    for child in decl.children(&mut cursor) {
        match child.kind() {
            "type_qualifier"
            | "primitive_type"
            | "type_identifier"
            | "sized_type_specifier"
            | "struct_specifier"
            | "union_specifier"
            | "enum_specifier" => {
                parts.push(base.get_node_text(&child));
            }
            _ => {}
        }
    }
    parts.join(" ")
}

fn contains_function_declarator(node: Node) -> bool {
    let mut node = Some(node);
    while let Some(current) = node {
        if current.kind() == "function_declarator" {
            return true;
        }
        node = nested_declarator(current);
    }
    false
}

fn declarator_decorations(base: &BaseExtractor, node: Node) -> (usize, String, usize) {
    let mut stars = 0;
    let mut array_suffix = String::new();
    let mut array_count = 0;
    let mut node = Some(node);
    while let Some(current) = node {
        match current.kind() {
            "pointer_declarator" => stars += 1,
            "array_declarator" => {
                array_count += 1;
                array_suffix.push('[');
                if let Some(size) = current.child_by_field_name("size") {
                    array_suffix.push_str(&base.get_node_text(&size));
                }
                array_suffix.push(']');
            }
            _ => {}
        }
        node = nested_declarator(current);
    }
    (stars, array_suffix, array_count)
}

fn nested_declarator(node: Node) -> Option<Node> {
    match node.kind() {
        "init_declarator"
        | "pointer_declarator"
        | "array_declarator"
        | "parenthesized_declarator"
        | "function_declarator" => node.child_by_field_name("declarator"),
        _ => None,
    }
}
