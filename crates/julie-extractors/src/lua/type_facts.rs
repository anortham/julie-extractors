use crate::base::types::TypeNameRules;
use crate::base::{BaseExtractor, Symbol, SymbolKind};
use crate::lua::helpers;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashSet;
use tree_sitter::Node;

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &[],
};

pub(super) fn record_declared_owner_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    owner_name: &str,
) {
    base.record_declared_type_fact(symbol_id, owner_name, &TYPE_NAME_RULES, false);
}

pub(super) fn record_inferred_constructor_facts(
    base: &mut BaseExtractor,
    root: Node,
    symbols: &[Symbol],
) {
    let class_names: HashSet<String> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Class)
        .map(|symbol| symbol.name.clone())
        .collect();
    walk_constructor_facts(base, root, symbols, &class_names, 0);
}

pub(super) fn colon_method_owner_name(base: &BaseExtractor, function_node: Node) -> Option<String> {
    let name = function_node.child_by_field_name("name")?;
    if name.kind() != "method_index_expression" {
        return None;
    }
    identifier_table_name(base, name)
}

pub(super) fn enclosing_colon_owner_name(base: &BaseExtractor, mut node: Node) -> Option<String> {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "function_declaration" | "function_definition_statement"
        ) {
            return colon_method_owner_name(base, parent);
        }
        node = parent;
    }
    None
}

pub(super) fn call_receiver_type(base: &BaseExtractor, node: Node) -> Option<String> {
    let index = match node.kind() {
        "function_call" => node.child_by_field_name("name")?,
        "method_index_expression" | "dot_index_expression" => node,
        _ => return None,
    };
    if identifier_table_name(base, index)?.as_str() != "self" {
        return None;
    }
    enclosing_colon_owner_name(base, node)
}

fn identifier_table_name(base: &BaseExtractor, index: Node) -> Option<String> {
    let table = index.child_by_field_name("table")?;
    if table.kind() != "identifier" {
        return None;
    }
    Some(base.get_node_text(&table))
}

fn walk_constructor_facts(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    class_names: &HashSet<String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if matches!(
        node.kind(),
        "variable_declaration" | "local_variable_declaration"
    ) {
        record_declaration_constructor_facts(base, node, symbols, class_names);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_constructor_facts(base, child, symbols, class_names, child_depth);
    }
}

fn record_declaration_constructor_facts(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    class_names: &HashSet<String>,
) {
    let Some(assignment) = helpers::find_child_by_type(&node, "assignment_statement") else {
        return;
    };
    let Some(variable_list) = helpers::find_child_by_type(&assignment, "variable_list") else {
        return;
    };
    let expressions: Vec<Node> = helpers::find_child_by_type(&assignment, "expression_list")
        .map(super::variables::collect_expression_nodes)
        .unwrap_or_default();

    let mut cursor = variable_list.walk();
    let variables: Vec<Node> = variable_list
        .children(&mut cursor)
        .filter(|child| child.kind() == "variable" || child.kind() == "identifier")
        .collect();

    for (index, var_node) in variables.iter().enumerate() {
        let name_node = if var_node.kind() == "identifier" {
            Some(*var_node)
        } else {
            helpers::find_child_by_type(var_node, "identifier")
        };
        let Some(name_node) = name_node else {
            continue;
        };
        let Some(expression) = expressions.get(index).copied() else {
            continue;
        };
        let Some(type_name) = constructor_type_name(base, expression) else {
            continue;
        };
        if !class_names.contains(&type_name) {
            continue;
        }
        let name = base.get_node_text(&name_node);
        if let Some(symbol) = symbol_for_name_node(symbols, &name, name_node) {
            base.record_declared_type_fact(&symbol.id, &type_name, &TYPE_NAME_RULES, true);
        }
    }
}

fn constructor_type_name(base: &BaseExtractor, expression: Node) -> Option<String> {
    if expression.kind() != "function_call" {
        return None;
    }
    let name = expression.child_by_field_name("name")?;
    if name.kind() == "dot_index_expression" {
        let field = name.child_by_field_name("field")?;
        if base.get_node_text(&field) != "new" {
            return None;
        }
        return identifier_table_name(base, name);
    }
    if name.kind() == "identifier" && base.get_node_text(&name) == "setmetatable" {
        let arguments = expression.child_by_field_name("arguments")?;
        let mut cursor = arguments.walk();
        let args: Vec<Node> = arguments.named_children(&mut cursor).collect();
        if args.len() >= 2
            && args[0].kind() == "table_constructor"
            && args[1].kind() == "identifier"
        {
            return Some(base.get_node_text(&args[1]));
        }
    }
    None
}

fn symbol_for_name_node<'a>(
    symbols: &'a [Symbol],
    name: &str,
    name_node: Node,
) -> Option<&'a Symbol> {
    let start_line = name_node.start_position().row as u32 + 1;
    let start_column = name_node.start_position().column as u32;
    symbols.iter().find(|symbol| {
        symbol.name == name
            && symbol.start_line == start_line
            && symbol.start_column == start_column
    })
}
