use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use tree_sitter::Node;

pub(super) const VBNET_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &[],
    generic_open: &['('],
};

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

pub(super) fn record_constructor_fact(base: &mut BaseExtractor, symbol_id: &str, class_name: &str) {
    base.record_declared_type_fact(symbol_id, class_name, &VBNET_TYPE_NAME_RULES, true);
}

pub(super) fn declared_type_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "as_clause" {
            return child.child_by_field_name("type");
        }
    }
    node.child_by_field_name("value")
        .filter(|value| value.kind() == "new_expression")
        .and_then(|value| value.child_by_field_name("type"))
}

pub(super) fn constructor_type_node(initializer: Node) -> Option<Node> {
    match initializer.kind() {
        "new_expression" => initializer.child_by_field_name("type"),
        "element_access" => {
            let object = initializer.child_by_field_name("object")?;
            if object.kind() == "new_expression" {
                object.child_by_field_name("type")
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(super) fn simple_unqualified_name(base: &BaseExtractor, type_node: Node) -> Option<String> {
    if !is_simple_constructed_type(type_node) {
        return None;
    }
    let name_node = base_type_name_node(type_node)?;
    Some(base.get_node_text(&name_node))
}

fn is_simple_constructed_type(node: Node) -> bool {
    match node.kind() {
        "identifier" | "primitive_type" => true,
        "namespace_name" => single_identifier(node).is_some(),
        "array_type" => node
            .child_by_field_name("element")
            .is_some_and(is_simple_constructed_type),
        "nullable_type" => node.named_child(0).is_some_and(is_simple_constructed_type),
        _ => false,
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
        &VBNET_TYPE_NAME_RULES,
        is_inferred,
    );
}

fn base_type_name_node(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        match node.kind() {
            "identifier" | "primitive_type" => return Some(node),
            "namespace_name" => return single_identifier(node),
            "generic_type" => {
                node = named_child_of_kind(node, "namespace_name")?;
            }
            "nullable_type" => {
                node = node.named_child(0)?;
            }
            "array_type" => {
                node = node.child_by_field_name("element")?;
            }
            "new_expression" => {
                node = node.child_by_field_name("type")?;
            }
            _ => return None,
        }
    }
}

fn single_identifier(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let mut identifiers = node
        .children(&mut cursor)
        .filter(|child| child.kind() == "identifier");
    let first = identifiers.next()?;
    if identifiers.next().is_some() {
        return None;
    }
    Some(first)
}

fn named_child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}
