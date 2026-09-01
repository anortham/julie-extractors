use crate::base::BaseExtractor;
use crate::base::types::TypeNameRules;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["*const", "*", "?", "[]const", "[]"],
    generic_open: &['('],
};

pub(super) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

pub(super) fn record_initializer_type(base: &mut BaseExtractor, symbol_id: &str, value: Node) {
    match value.kind() {
        "struct_initializer" => {
            let Some(type_node) = struct_initializer_type(value) else {
                return;
            };
            record_inferred_same_file_container(base, symbol_id, type_node, value);
        }
        "call_expression" => {
            let Some(function) = value.child_by_field_name("function") else {
                return;
            };
            if function.kind() != "field_expression" {
                return;
            }
            let Some(member) = function.child_by_field_name("member") else {
                return;
            };
            if base.get_node_text(&member) != "init" {
                return;
            }
            let Some(object) = function.child_by_field_name("object") else {
                return;
            };
            if object.kind() != "identifier" {
                return;
            }
            record_inferred_same_file_container(base, symbol_id, object, value);
        }
        _ => {}
    }
}

pub(super) fn nearest_callable_ancestor(mut node: Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "function_declaration" | "function_definition" | "test_declaration" => return true,
            _ => current = parent.parent(),
        }
        node = parent;
        let _ = node;
    }
    false
}

pub(super) fn self_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let function = node.child_by_field_name("function")?;
    if function.kind() != "field_expression" {
        return None;
    }
    let object = function.child_by_field_name("object")?;
    if object.kind() != "identifier" {
        return None;
    }
    let receiver_name = base.get_node_text(&object);
    let func_decl = enclosing_function(node)?;
    if !super::helpers::is_inside_struct(func_decl) {
        return None;
    }
    let first_param = first_parameter(func_decl)?;
    let param_name = first_param.child_by_field_name("name")?;
    if base.get_node_text(&param_name) != receiver_name {
        return None;
    }
    let type_node = first_param.child_by_field_name("type")?;
    if is_this_type(base, type_node) {
        return enclosing_container_name(base, func_decl);
    }
    let name_node = base_type_name_node(type_node)?;
    let name = base.get_node_text(&name_node);
    same_file_container(base, file_root(node), &name).then_some(name)
}

fn record_inferred_same_file_container(
    base: &mut BaseExtractor,
    symbol_id: &str,
    type_node: Node,
    from: Node,
) {
    let Some(name_node) = base_type_name_node(type_node) else {
        return;
    };
    let name = base.get_node_text(&name_node);
    if !same_file_container(base, file_root(from), &name) {
        return;
    }
    record_type_node(base, symbol_id, type_node, true);
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
        &TYPE_NAME_RULES,
        is_inferred,
    );
}

fn base_type_name_node(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        match node.kind() {
            "identifier" | "builtin_type" => return Some(node),
            "pointer_type" | "nullable_type" => {
                node = inner_type_child(node)?;
            }
            "parenthesized_expression" => {
                node = node.named_child(0)?;
            }
            "call_expression" => {
                let function = node.child_by_field_name("function")?;
                if function.kind() == "identifier" {
                    return Some(function);
                }
                return None;
            }
            "array_type" | "slice_type" | "field_expression" => return None,
            _ => return None,
        }
    }
}

fn inner_type_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find(|child| {
        matches!(
            child.kind(),
            "identifier"
                | "builtin_type"
                | "pointer_type"
                | "nullable_type"
                | "call_expression"
                | "parenthesized_expression"
                | "builtin_function"
                | "field_expression"
                | "array_type"
                | "slice_type"
        )
    })
}

fn struct_initializer_type(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() != "initializer_list")
}

fn is_this_type(base: &BaseExtractor, node: Node) -> bool {
    let mut node = node;
    loop {
        match node.kind() {
            "pointer_type" | "nullable_type" => {
                let Some(inner) = inner_type_child(node) else {
                    return false;
                };
                node = inner;
            }
            "parenthesized_expression" => {
                let Some(inner) = node.named_child(0) else {
                    return false;
                };
                node = inner;
            }
            "builtin_function" => {
                return builtin_identifier(base, node)
                    .map(|name| name == "@This")
                    .unwrap_or(false);
            }
            _ => return false,
        }
    }
}

fn builtin_identifier(base: &BaseExtractor, node: Node) -> Option<String> {
    let mut cursor = node.walk();
    let ident = node
        .named_children(&mut cursor)
        .find(|child| child.kind() == "builtin_identifier")?;
    Some(base.get_node_text(&ident))
}

fn enclosing_function(mut node: Node) -> Option<Node> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(
            parent.kind(),
            "function_declaration" | "function_definition"
        ) {
            return Some(parent);
        }
        current = parent.parent();
        node = parent;
        let _ = node;
    }
    None
}

fn first_parameter(func_decl: Node) -> Option<Node> {
    let mut cursor = func_decl.walk();
    let params = func_decl
        .named_children(&mut cursor)
        .find(|child| child.kind() == "parameters")?;
    let mut param_cursor = params.walk();
    params
        .named_children(&mut param_cursor)
        .find(|child| child.kind() == "parameter")
}

fn enclosing_container_name(base: &BaseExtractor, mut node: Node) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(
            parent.kind(),
            "struct_declaration" | "union_declaration" | "enum_declaration"
        ) {
            if let Some(decl) = parent.parent()
                && decl.kind() == "variable_declaration"
            {
                let mut cursor = decl.walk();
                if let Some(name) = decl
                    .named_children(&mut cursor)
                    .find(|child| child.kind() == "identifier")
                {
                    return Some(base.get_node_text(&name));
                }
            }
            return None;
        }
        current = parent.parent();
        node = parent;
        let _ = node;
    }
    None
}

fn file_root(mut node: Node) -> Node {
    while let Some(parent) = node.parent() {
        node = parent;
    }
    node
}

fn same_file_container(base: &BaseExtractor, root: Node, name: &str) -> bool {
    find_container_declaration(root, name, base, 0)
}

fn find_container_declaration(node: Node, name: &str, base: &BaseExtractor, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.kind() == "variable_declaration" {
        let mut cursor = node.walk();
        let ident = node
            .named_children(&mut cursor)
            .find(|child| child.kind() == "identifier");
        if let Some(ident) = ident
            && base.get_node_text(&ident) == name
        {
            let mut kind_cursor = node.walk();
            if node.named_children(&mut kind_cursor).any(|child| {
                matches!(
                    child.kind(),
                    "struct_declaration" | "union_declaration" | "enum_declaration"
                )
            }) {
                return true;
            }
        }
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if find_container_declaration(child, name, base, child_depth) {
            return true;
        }
    }
    false
}

pub(super) fn initializer_node(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    let eq = children.iter().position(|child| child.kind() == "=")?;
    children[eq + 1..].iter().copied().find(|child| child.is_named())
}

pub(super) fn has_keyword(node: Node, keyword: &str) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| child.kind() == keyword)
}
