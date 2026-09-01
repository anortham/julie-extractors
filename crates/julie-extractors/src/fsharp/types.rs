use super::FSharpExtractor;
use super::parameters;
use crate::base::types::TypeNameRules;
use crate::base::{BaseExtractor, Symbol, SymbolKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

pub(super) const FSHARP_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["byref", "inref", "outref"],
    generic_open: &['<'],
};


pub(super) fn collect_types(
    extractor: &mut FSharpExtractor,
    root: Node,
    symbols: &[Symbol],
) -> HashMap<String, String> {
    let mut types = HashMap::new();
    extractor.base.type_info.clear();
    walk(&mut extractor.base, root, symbols, &mut types, 0);
    for (symbol_id, type_info) in &extractor.base.type_info {
        types.insert(symbol_id.clone(), type_info.resolved_type.clone());
    }
    types
}

fn walk(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    types: &mut HashMap<String, String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "function_or_value_defn" => {
            collect_definition_type(base, node, symbols, types);
            parameters::record_parameter_facts(base, node, symbols);
        }
        "record_field" | "union_type_field" => collect_field_type(base, node, symbols, types),
        "member_defn" => {
            collect_member_type(base, node, symbols, types);
            parameters::record_parameter_facts(base, node, symbols);
        }
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
    base: &mut BaseExtractor,
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
        record_named_type(base, &symbol.id, literal, literal, true);
        return;
    }
    if let Some(type_name) = same_file_constructor_type(base, body, symbols) {
        record_named_type(base, &symbol.id, &type_name, &type_name, true);
    }

}

fn collect_field_type(
    base: &mut BaseExtractor,
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
    base: &mut BaseExtractor,
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
    base: &mut BaseExtractor,
    types: &mut HashMap<String, String>,
    symbol: &Symbol,
    node: Node,
) {
    record_type_node(base, &symbol.id, node, false);
    if let Some(type_info) = base.type_info.get(&symbol.id) {
        types.insert(symbol.id.clone(), type_info.resolved_type.clone());
    }
}

pub(super) fn record_type_node(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    is_inferred: bool,
) {
    let Some(base_name) = structural_base_name(base, type_node) else {
        return;
    };
    let declared = base.get_node_text(&type_node);
    record_named_type(base, symbol_id, &base_name, declared.trim(), is_inferred);
}

fn record_named_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    base_name: &str,
    declared: &str,
    is_inferred: bool,
) {
    base.record_declared_type_fact_with_declared(
        symbol_id,
        base_name,
        declared,
        &FSHARP_TYPE_NAME_RULES,
        is_inferred,
    );
}

fn structural_base_name(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut node = node;
    loop {
        match node.kind() {
            "identifier" | "long_identifier" | "simple_type" | "type_argument" => {
                let text = base.get_node_text(&node);
                let text = text.trim();
                if text.is_empty() {
                    return None;
                }
                return Some(text.to_string());
            }
            "paren_type" | "atomic_type" | "flexible_type" | "type_name" => {
                node = first_type_or_named_child(node)?;
            }
            "generic_type" => {
                let name_node = direct_child(node, "long_identifier")?;
                let name = base.get_node_text(&name_node);
                let name = name.trim();
                if matches!(name, "byref" | "inref" | "outref") {
                    node = generic_argument_type(node)?;
                    continue;
                }
                if name.is_empty() {
                    return None;
                }
                return Some(name.to_string());
            }
            "postfix_type" => {
                let ident = last_named_child_of_kind(node, "long_identifier")?;
                let text = base.get_node_text(&ident);
                let text = text.trim();
                if text.is_empty() {
                    return None;
                }
                return Some(text.to_string());
            }
            "static_type" => {
                node = first_type_or_named_child(node)?;
            }
            _ => return None,
        }
    }
}

fn generic_argument_type(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if is_type_node(&child) {
            return Some(child);
        }
        if matches!(child.kind(), "type_attributes" | "types" | "type_attribute") {
            if let Some(inner) = first_type_or_named_child(child) {
                return Some(inner);
            }
        }
    }
    None
}

fn first_type_or_named_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children
        .iter()
        .copied()
        .find(is_type_node)
        .or_else(|| children.into_iter().next())
}

fn last_named_child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();
    children.into_iter().rev().find(|child| child.kind() == kind)
}

fn same_file_constructor_type(
    base: &BaseExtractor,
    body: Node,
    symbols: &[Symbol],
) -> Option<String> {
    let application = match body.kind() {
        "application_expression" => body,
        _ => return None,
    };
    let head = first_named_child(application)?;
    if !matches!(head.kind(), "long_identifier" | "long_identifier_or_op") {
        return None;
    }
    let name = base.get_node_text(&head);
    let name = name.trim();
    if name.is_empty() || name.contains('.') {
        return None;
    }
    symbols
        .iter()
        .any(|symbol| {
            symbol.name == name
                && matches!(
                    symbol.kind,
                    SymbolKind::Class
                        | SymbolKind::Struct
                        | SymbolKind::Union
                        | SymbolKind::Interface
                        | SymbolKind::Enum
                        | SymbolKind::Type
                        | SymbolKind::Delegate
                )
        })
        .then(|| name.to_string())
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
