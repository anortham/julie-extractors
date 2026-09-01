use super::helpers::{find_child_by_type, get_node_text};
use super::type_facts;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) fn extract_parameter_symbols(
    base: &mut BaseExtractor,
    callable_node: Node,
    callable_id: &str,
) -> Vec<Symbol> {
    let Some(list) = formal_parameter_list(callable_node) else {
        return Vec::new();
    };
    let class_node = enclosing_class_node(callable_node);
    let mut symbols = Vec::new();
    for param_node in collect_formal_parameters(list) {
        let Some((name_node, span_node, type_node)) = parameter_parts(param_node, class_node)
        else {
            continue;
        };
        let name = get_node_text(&name_node);
        if name.is_empty() {
            continue;
        }
        let signature = get_node_text(&span_node);
        let metadata = HashMap::from([(
            "role".to_string(),
            serde_json::Value::String("parameter".to_string()),
        )]);
        let symbol = base.create_symbol(
            &span_node,
            name,
            SymbolKind::Variable,
            SymbolOptions {
                signature: Some(signature),
                parent_id: Some(callable_id.to_string()),
                metadata: Some(metadata),
                ..Default::default()
            },
        );
        if let Some(type_node) = type_node {
            type_facts::record_declared_type(base, &symbol.id, type_node);
        }
        symbols.push(symbol);
    }
    symbols
}
fn formal_parameter_list(node: Node) -> Option<Node> {
    if node.kind() == "formal_parameter_list" {
        return Some(node);
    }
    if let Some(parameters) = node.child_by_field_name("parameters") {
        if parameters.kind() == "formal_parameter_list" {
            return Some(parameters);
        }
        if let Some(list) = find_child_by_type(&parameters, "formal_parameter_list") {
            return Some(list);
        }
    }
    if let Some(list) = find_child_by_type(&node, "formal_parameter_list") {
        return Some(list);
    }
    for kind in [
        "function_signature",
        "method_signature",
        "constructor_signature",
        "factory_constructor_signature",
        "constant_constructor_signature",
    ] {
        if let Some(child) = find_child_by_type(&node, kind)
            && let Some(list) = formal_parameter_list(child)
        {
            return Some(list);
        }
    }
    node.child_by_field_name("signature")
        .and_then(formal_parameter_list)
}

fn collect_formal_parameters(list: Node) -> Vec<Node> {
    let mut parameters = Vec::new();
    let mut cursor = list.walk();
    for child in list.named_children(&mut cursor) {
        match child.kind() {
            "formal_parameter" => parameters.push(child),
            "optional_formal_parameters" => {
                let mut inner = child.walk();
                for param in child.named_children(&mut inner) {
                    if param.kind() == "formal_parameter" {
                        parameters.push(param);
                    }
                }
            }
            _ => {}
        }
    }
    parameters
}

fn parameter_parts<'a>(
    param_node: Node<'a>,
    class_node: Option<Node<'a>>,
) -> Option<(Node<'a>, Node<'a>, Option<Node<'a>>)> {
    if let Some(constructor_param) = find_child_by_type(&param_node, "constructor_param") {
        let name_node = find_child_by_type(&constructor_param, "identifier")?;
        let type_node = declared_type_node(constructor_param).or_else(|| {
            class_node
                .and_then(|class_node| field_type_in_class(class_node, &get_node_text(&name_node)))
        });
        return Some((name_node, param_node, type_node));
    }
    if let Some(super_param) = find_child_by_type(&param_node, "super_formal_parameter") {
        let name_node = find_child_by_type(&super_param, "identifier")?;
        return Some((name_node, param_node, declared_type_node(super_param)));
    }
    let name_node = param_node
        .child_by_field_name("name")
        .filter(|node| node.kind() == "identifier")
        .or_else(|| find_child_by_type(&param_node, "identifier"))?;
    Some((name_node, param_node, declared_type_node(param_node)))
}

fn declared_type_node(node: Node<'_>) -> Option<Node<'_>> {
    node.child_by_field_name("type")
        .or_else(|| find_child_by_type(&node, "type"))
}

fn enclosing_class_node(mut node: Node<'_>) -> Option<Node<'_>> {
    while let Some(parent) = node.parent() {
        if matches!(parent.kind(), "class_definition" | "class_declaration") {
            return Some(parent);
        }
        node = parent;
    }
    None
}
fn field_type_in_class<'a>(class_node: Node<'a>, field_name: &str) -> Option<Node<'a>> {
    let body = find_child_by_type(&class_node, "class_body")?;
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        let declaration = match child.kind() {
            "declaration" => child,
            "class_member" => match find_child_by_type(&child, "declaration") {
                Some(declaration) => declaration,
                None => continue,
            },
            _ => continue,
        };
        if !declaration_has_name(declaration, field_name) {
            continue;
        }
        return declared_type_node(declaration);
    }
    None
}

fn declaration_has_name(declaration: Node, field_name: &str) -> bool {
    let mut cursor = declaration.walk();
    for child in declaration.named_children(&mut cursor) {
        if matches!(
            child.kind(),
            "initialized_identifier_list" | "identifier_list" | "initialized_identifier"
        ) && identifier_list_has_name(child, field_name, 0)
        {
            return true;
        }
    }
    false
}

fn identifier_list_has_name(node: Node, field_name: &str, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.kind() == "identifier" {
        return get_node_text(&node) == field_name;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "identifier" if get_node_text(&child) == field_name => return true,
            "initialized_identifier" | "initialized_identifier_list" | "identifier_list"
                if identifier_list_has_name(child, field_name, child_depth) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}
