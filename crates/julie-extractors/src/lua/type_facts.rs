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
    let table = colon_method_index(function_node)?.child_by_field_name("table")?;
    let owner = match table.kind() {
        "identifier" => table,
        "dot_index_expression" => table.child_by_field_name("field")?,
        _ => return None,
    };
    Some(base.get_node_text(&owner))
}

pub(super) fn colon_method_name_node(function_node: Node) -> Option<Node> {
    colon_method_index(function_node)?.child_by_field_name("method")
}

fn colon_method_index(function_node: Node) -> Option<Node> {
    function_node
        .child_by_field_name("name")
        .filter(|name| name.kind() == "method_index_expression")
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
    if node.kind() == "variable_declaration" {
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
        if let Some(symbol) = symbol_for_name_node(symbols, &name, name_node)
            .filter(|symbol| symbol.kind != SymbolKind::Class)
        {
            base.record_declared_type_fact(&symbol.id, &type_name, &TYPE_NAME_RULES, true);
        }
    }
}

fn constructor_type_name(base: &BaseExtractor, expression: Node) -> Option<String> {
    if expression.kind() != "function_call" {
        return None;
    }
    let name = expression.child_by_field_name("name")?;
    let member_field = match name.kind() {
        "dot_index_expression" => Some("field"),
        "method_index_expression" => Some("method"),
        _ => None,
    };
    if let Some(member_field) = member_field {
        let member = name.child_by_field_name(member_field)?;
        if base.get_node_text(&member) != "new" {
            return None;
        }
        return identifier_table_name(base, name);
    }
    if name.kind() == "identifier" && base.get_node_text(&name) != "setmetatable" {
        return Some(base.get_node_text(&name));
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

const ANNOTATION_TYPE_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &[],
    generic_open: &['<'],
};

/// Record declared type facts from LuaLS / LuaCATS doc annotations:
/// `---@param name Type` on parameters, `---@return Type` on the function, and
/// `---@type Type` on variables and fields.
pub(super) fn record_annotation_facts(base: &mut BaseExtractor, symbols: &[Symbol]) {
    for symbol in symbols {
        let Some(doc) = symbol.doc_comment.as_deref() else {
            continue;
        };
        for (tag, rest) in annotation_tags(doc) {
            match (tag, &symbol.kind) {
                ("param", SymbolKind::Function | SymbolKind::Method) => {
                    let Some((name, type_text)) = rest.split_once(char::is_whitespace) else {
                        continue;
                    };
                    let name = name.trim_end_matches('?');
                    let optional = rest
                        .split_whitespace()
                        .next()
                        .is_some_and(|n| n.ends_with('?'));
                    let parameter = symbols.iter().find(|candidate| {
                        candidate.name == name
                            && candidate.parent_id.as_deref() == Some(symbol.id.as_str())
                    });
                    if let (Some(parameter), Some(type_text)) =
                        (parameter, leading_type(type_text.trim_start()))
                    {
                        let declared = if optional {
                            format!("{type_text}?")
                        } else {
                            type_text.to_string()
                        };
                        record_annotated_type(base, &parameter.id, &declared);
                    }
                }
                ("return", SymbolKind::Function | SymbolKind::Method)
                | ("type", SymbolKind::Variable | SymbolKind::Field | SymbolKind::Constant) => {
                    if let Some(type_text) = leading_type(rest) {
                        record_annotated_type(base, &symbol.id, type_text);
                    }
                }
                _ => {}
            }
        }
    }
}

fn annotation_tags(doc: &str) -> impl Iterator<Item = (&str, &str)> {
    doc.lines().filter_map(|line| {
        let rest = line.trim_start().trim_start_matches('-').trim_start();
        let rest = rest.strip_prefix('@')?;
        let (tag, rest) = rest.split_once(char::is_whitespace)?;
        Some((tag, rest.trim()))
    })
}

/// The type expression at the start of `text`: brackets and parentheses nest,
/// and the first top-level whitespace (or `#` comment marker) ends it.
fn leading_type(text: &str) -> Option<&str> {
    let mut depth = 0i32;
    for (index, ch) in text.char_indices() {
        match ch {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => depth -= 1,
            '#' if depth == 0 => return non_empty(&text[..index]),
            c if c.is_whitespace() && depth == 0 => {
                if text[..index].ends_with(',') || text[..index].ends_with(':') {
                    continue;
                }
                return non_empty(&text[..index]);
            }
            _ => {}
        }
    }
    non_empty(text)
}

fn non_empty(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty()).then_some(text)
}

/// Record one annotated type. A union drops `nil`; a union of several other
/// types records nothing, and a `fun(...)` type records `function`.
fn record_annotated_type(base: &mut BaseExtractor, symbol_id: &str, declared: &str) {
    let mut depth = 0i32;
    let mut members = Vec::new();
    let mut start = 0;
    for (index, ch) in declared.char_indices() {
        match ch {
            '(' | '<' | '[' | '{' => depth += 1,
            ')' | '>' | ']' | '}' => depth -= 1,
            '|' if depth == 0 => {
                members.push(declared[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    members.push(declared[start..].trim());
    let non_nil: Vec<&str> = members
        .into_iter()
        .filter(|member| !member.is_empty() && *member != "nil")
        .collect();
    let [single] = non_nil.as_slice() else {
        return;
    };
    let base_text = if single.starts_with("fun(") {
        "function"
    } else {
        single
    };
    base.record_declared_type_fact_with_declared(
        symbol_id,
        base_text,
        declared,
        &ANNOTATION_TYPE_RULES,
        false,
    );
}
