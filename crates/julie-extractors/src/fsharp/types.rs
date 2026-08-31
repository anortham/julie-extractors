use super::FSharpExtractor;
use crate::base::{BaseExtractor, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) fn collect_types(
    extractor: &FSharpExtractor,
    root: Node,
    symbols: &[Symbol],
) -> HashMap<String, String> {
    let mut types = HashMap::new();
    walk(&extractor.base, root, symbols, &mut types, 0);
    types
}

fn walk(
    base: &BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    types: &mut HashMap<String, String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "function_or_value_defn" => collect_definition_type(base, node, symbols, types),
        "record_field" | "union_type_field" => collect_field_type(base, node, symbols, types),
        "member_defn" => collect_member_type(base, node, symbols, types),
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(base, child, symbols, types, child_depth);
    }
}

fn collect_definition_type(
    base: &BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    types: &mut HashMap<String, String>,
) {
    let Some(left) = direct_child(node, "function_declaration_left")
        .or_else(|| direct_child(node, "value_declaration_left"))
    else {
        return;
    };
    let Some(name_node) = declaration_name(left) else {
        return;
    };
    let Some(symbol) = symbol_for_name(symbols, base, &name_node) else {
        return;
    };
    let explicit = direct_type_child_after(node, left);
    if let Some(type_node) = explicit {
        insert_type(base, types, symbol, type_node);
        return;
    }
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    if let Some(literal) = literal_type(body) {
        types.insert(symbol.id.clone(), literal.to_string());
    }
}

fn collect_field_type(
    base: &BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    types: &mut HashMap<String, String>,
) {
    let Some(name_node) = direct_identifier(node) else {
        return;
    };
    let Some(type_node) = direct_type_child(node) else {
        return;
    };
    let Some(symbol) = symbol_for_name(symbols, base, &name_node) else {
        return;
    };
    insert_type(base, types, symbol, type_node);
}

fn collect_member_type(
    base: &BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    types: &mut HashMap<String, String>,
) {
    let Some(definition) = direct_child(node, "method_or_prop_defn") else {
        return;
    };
    let Some(name) = definition.child_by_field_name("name") else {
        return;
    };
    let Some(name_node) = terminal_identifier(name) else {
        return;
    };
    let Some(symbol) = symbol_for_name(symbols, base, &name_node) else {
        return;
    };
    if let Some(type_node) = direct_type_child(definition) {
        insert_type(base, types, symbol, type_node);
    }
}

fn insert_type(
    base: &BaseExtractor,
    types: &mut HashMap<String, String>,
    symbol: &Symbol,
    node: Node,
) {
    let type_name = base.get_node_text(&node).trim().to_string();
    if !type_name.is_empty() {
        types.insert(symbol.id.clone(), type_name);
    }
}

fn literal_type(node: Node) -> Option<&'static str> {
    if node.kind() != "const" {
        return None;
    }
    let child = first_named_child(node)?;
    Some(match child.kind() {
        "string" | "triple_quoted_string" | "verbatim_string" => "string",
        "char" => "char",
        "int" => "int",
        "float" => "float",
        "decimal" => "decimal",
        "bool" => "bool",
        "unit" => "unit",
        _ => return None,
    })
}

fn symbol_for_name<'a>(
    symbols: &'a [Symbol],
    base: &BaseExtractor,
    name_node: &Node,
) -> Option<&'a Symbol> {
    let name_text = base.get_node_text(name_node);
    let name = name_text.trim();
    symbols
        .iter()
        .filter(|symbol| symbol.name == name)
        .filter(|symbol| {
            symbol.start_byte <= name_node.start_byte() as u32
                && symbol.end_byte >= name_node.end_byte() as u32
        })
        .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
}

fn direct_type_child_after<'a>(node: Node<'a>, left: Node<'a>) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    children
        .into_iter()
        .skip_while(|child| child.id() != left.id())
        .skip(1)
        .find(is_type_node)
}

fn direct_type_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(is_type_node)
}

fn declaration_name(node: Node) -> Option<Node> {
    first_identifier(node)
}

fn direct_identifier(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == "identifier")
}

fn first_identifier(node: Node) -> Option<Node> {
    if node.kind() == "identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find_map(first_identifier)
}

fn direct_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn is_type_node(node: &Node) -> bool {
    matches!(
        node.kind(),
        "simple_type"
            | "generic_type"
            | "atomic_type"
            | "compound_type"
            | "constrained_type"
            | "flexible_type"
            | "function_type"
            | "list_type"
            | "paren_type"
            | "postfix_type"
            | "static_type"
            | "struct_type"
            | "tuple_type"
            | "type_name"
            | "types"
    )
}

fn first_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}

fn terminal_identifier(node: Node) -> Option<Node> {
    if node.kind() == "identifier" {
        return Some(node);
    }
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children.into_iter().rev().find_map(terminal_identifier)
}
