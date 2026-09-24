//! Lexical scope helpers: local bindings, table-path owners, and function values.

use crate::base::{BaseExtractor, NormalizedSpan, Symbol, SymbolKind};
use crate::tree_traversal::child_tree_depth;
use std::cell::RefCell;
use std::collections::HashMap;
use tree_sitter::Node;

/// Span from the start of `start` to the end of `end`.
pub(super) fn span_between(start: &Node, end: &Node) -> NormalizedSpan {
    let start_span = NormalizedSpan::from_node(start);
    let end_span = NormalizedSpan::from_node(end);
    NormalizedSpan {
        end_line: end_span.end_line,
        end_column: end_span.end_column,
        end_byte: end_span.end_byte,
        ..start_span
    }
}

/// The `function_definition` an assigned expression evaluates to, if any.
pub(super) fn function_value(expression: Node) -> Option<Node> {
    match expression.kind() {
        "function_definition" => Some(expression),
        "expression_list" => {
            let mut cursor = expression.walk();
            expression
                .named_children(&mut cursor)
                .find(|child| child.kind() == "function_definition")
        }
        _ => None,
    }
}

/// True when `name` at `node` refers to a `local`, a parameter, or a loop variable.
pub(super) fn is_local_binding_in_scope(base: &BaseExtractor, node: Node, name: &str) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        let mut cursor = parent.walk();
        for sibling in parent.children(&mut cursor) {
            if sibling.start_byte() >= current.start_byte() {
                break;
            }
            if declares_local(base, sibling, name) {
                return true;
            }
        }
        current = parent;
    }
    false
}

/// The local declarations of one file, by the scope node that holds them.
/// Each scope's children are read once, so a lookup costs the scope depth,
/// not the number of statements before it.
#[derive(Debug, Default)]
pub(super) struct LocalBindings {
    /// Scope node id, then bound name, then the start byte of each child that
    /// declares it, in source order.
    by_scope: RefCell<HashMap<usize, HashMap<String, Vec<usize>>>>,
}

impl LocalBindings {
    /// Start byte of the innermost declaration that binds `name` at `node`: a
    /// `local`, a `local function` (also inside its own body), a parameter
    /// list, or a loop clause. `None` means `name` is global at `node`.
    pub(super) fn innermost(&self, base: &BaseExtractor, node: Node, name: &str) -> Option<usize> {
        let mut current = node;
        while let Some(parent) = current.parent() {
            let mut by_scope = self.by_scope.borrow_mut();
            let declared = by_scope
                .entry(parent.id())
                .or_insert_with(|| declarations_by_name(base, parent));
            let before = declared.get(name).and_then(|starts| {
                starts
                    .iter()
                    .rev()
                    .find(|start| **start < current.start_byte())
            });
            if let Some(start) = before {
                return Some(*start);
            }
            if parent.kind() == "function_declaration" && declares_local(base, parent, name) {
                return Some(parent.start_byte());
            }
            current = parent;
        }
        None
    }
}

fn declarations_by_name(base: &BaseExtractor, scope: Node) -> HashMap<String, Vec<usize>> {
    let mut declared: HashMap<String, Vec<usize>> = HashMap::new();
    let mut cursor = scope.walk();
    for child in scope.children(&mut cursor) {
        for name in declared_local_names(base, child) {
            declared.entry(name).or_default().push(child.start_byte());
        }
    }
    declared
}

fn declares_local(base: &BaseExtractor, node: Node, name: &str) -> bool {
    if let Some(function_name) = local_function_name(node) {
        return base.get_node_text(&function_name) == name;
    }
    name_list(node).is_some_and(|list| {
        let mut cursor = list.walk();
        list.children_by_field_name("name", &mut cursor)
            .any(|child| child.kind() == "identifier" && base.get_node_text(&child) == name)
    })
}

fn declared_local_names(base: &BaseExtractor, node: Node) -> Vec<String> {
    if let Some(function_name) = local_function_name(node) {
        return vec![base.get_node_text(&function_name)];
    }
    let Some(list) = name_list(node) else {
        return Vec::new();
    };
    let mut cursor = list.walk();
    list.children_by_field_name("name", &mut cursor)
        .filter(|child| child.kind() == "identifier")
        .map(|child| base.get_node_text(&child))
        .collect()
}

/// The identifier a `local function name` declaration binds.
fn local_function_name(node: Node) -> Option<Node> {
    if node.kind() != "function_declaration"
        || !node.child(0).is_some_and(|first| first.kind() == "local")
    {
        return None;
    }
    node.child_by_field_name("name")
        .filter(|name| name.kind() == "identifier")
}

/// The node whose `name` fields hold the names a `local`, parameter list, or
/// loop clause binds.
fn name_list(node: Node) -> Option<Node> {
    match node.kind() {
        "variable_declaration" => {
            crate::base::find_child_by_type(&node, "variable_list").or_else(|| {
                crate::base::find_child_by_type(&node, "assignment_statement").and_then(
                    |assignment| crate::base::find_child_by_type(&assignment, "variable_list"),
                )
            })
        }
        "parameters" | "for_numeric_clause" => Some(node),
        "for_generic_clause" => crate::base::find_child_by_type(&node, "variable_list"),
        _ => None,
    }
}

/// Resolve a table expression (`M`, `M.config`, `self`) to the symbol that owns it.
pub(super) fn resolve_table_symbol_id(
    base: &BaseExtractor,
    table: Node,
    symbols: &[Symbol],
) -> Option<String> {
    resolve_table_symbol_id_at(base, table, symbols, 0)
}

/// `None` past the traversal depth limit, so a very deep chain has no owner.
fn resolve_table_symbol_id_at(
    base: &BaseExtractor,
    table: Node,
    symbols: &[Symbol],
    depth: u32,
) -> Option<String> {
    let depth = child_tree_depth(depth)?;
    match table.kind() {
        "identifier" => {
            let name = base.get_node_text(&table);
            if name == "self"
                && let Some(owner_table) = enclosing_colon_owner_table(table)
            {
                return resolve_table_symbol_id_at(base, owner_table, symbols, depth);
            }
            let binding = resolve_binding(&name, table.start_byte() as u32, symbols)?;
            let instance_class = binding
                .parent_id
                .is_some()
                .then(|| super::classes::instance_metatable_name(binding))
                .flatten()
                .and_then(|class| resolve_binding(class, binding.start_byte, symbols));
            Some(instance_class.unwrap_or(binding).id.clone())
        }
        "dot_index_expression" => {
            let parent_id = resolve_table_symbol_id_at(
                base,
                table.child_by_field_name("table")?,
                symbols,
                depth,
            )?;
            let field = base.get_node_text(&table.child_by_field_name("field")?);
            symbols
                .iter()
                .rev()
                .find(|s| s.name == field && s.parent_id.as_deref() == Some(parent_id.as_str()))
                .map(|s| s.id.clone())
        }
        _ => None,
    }
}

pub(super) fn enclosing_colon_owner_table(mut node: Node) -> Option<Node> {
    while let Some(parent) = node.parent() {
        if matches!(
            parent.kind(),
            "function_declaration" | "function_definition_statement"
        ) {
            return parent
                .child_by_field_name("name")
                .filter(|name| name.kind() == "method_index_expression")?
                .child_by_field_name("table");
        }
        node = parent;
    }
    None
}

/// The owner table of the nearest enclosing colon method, past any nested
/// functions that are not colon methods. Callers rule out a `self` local or
/// parameter first, so `self` here is the method's implicit upvalue.
pub(super) fn outer_colon_owner_table(node: Node) -> Option<Node> {
    std::iter::successors(node.parent(), Node::parent)
        .filter(|ancestor| {
            matches!(
                ancestor.kind(),
                "function_declaration" | "function_definition_statement"
            )
        })
        .find_map(|function| {
            function
                .child_by_field_name("name")
                .filter(|name| name.kind() == "method_index_expression")
        })?
        .child_by_field_name("table")
}

/// The nearest earlier binding named `name` whose scope covers `position`.
pub(super) fn resolve_binding<'a>(
    name: &str,
    position: u32,
    symbols: &'a [Symbol],
) -> Option<&'a Symbol> {
    symbols.iter().rev().find(|s| {
        s.name == name
            && s.start_byte <= position
            && !matches!(s.kind, SymbolKind::Field | SymbolKind::Method)
            && s.parent_id.as_deref().is_none_or(|parent_id| {
                symbols
                    .iter()
                    .find(|p| p.id == parent_id)
                    .is_some_and(|p| p.start_byte <= position && position <= p.end_byte)
            })
    })
}
