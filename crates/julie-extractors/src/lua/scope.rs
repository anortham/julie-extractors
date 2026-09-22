//! Lexical scope helpers: local bindings, table-path owners, and function values.

use crate::base::{BaseExtractor, NormalizedSpan, Symbol, SymbolKind};
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

fn declares_local(base: &BaseExtractor, node: Node, name: &str) -> bool {
    match node.kind() {
        "variable_declaration" => {
            let list = crate::base::find_child_by_type(&node, "variable_list").or_else(|| {
                crate::base::find_child_by_type(&node, "assignment_statement").and_then(
                    |assignment| crate::base::find_child_by_type(&assignment, "variable_list"),
                )
            });
            list.is_some_and(|list| has_name_field(base, list, name))
        }
        "function_declaration" => {
            base.get_node_text(&node).trim_start().starts_with("local")
                && node
                    .child_by_field_name("name")
                    .is_some_and(|n| n.kind() == "identifier" && base.get_node_text(&n) == name)
        }
        "parameters" | "for_numeric_clause" => has_name_field(base, node, name),
        "for_generic_clause" => crate::base::find_child_by_type(&node, "variable_list")
            .is_some_and(|list| has_name_field(base, list, name)),
        _ => false,
    }
}

fn has_name_field(base: &BaseExtractor, node: Node, name: &str) -> bool {
    let mut cursor = node.walk();
    node.children_by_field_name("name", &mut cursor)
        .any(|child| child.kind() == "identifier" && base.get_node_text(&child) == name)
}

/// Resolve a table expression (`M`, `M.config`, `self`) to the symbol that owns it.
pub(super) fn resolve_table_symbol_id(
    base: &BaseExtractor,
    table: Node,
    symbols: &[Symbol],
) -> Option<String> {
    match table.kind() {
        "identifier" => {
            let name = base.get_node_text(&table);
            if name == "self"
                && let Some(owner_table) = enclosing_colon_owner_table(table)
            {
                return resolve_table_symbol_id(base, owner_table, symbols);
            }
            resolve_binding(&name, table.start_byte() as u32, symbols).map(|s| s.id.clone())
        }
        "dot_index_expression" => {
            let parent_id =
                resolve_table_symbol_id(base, table.child_by_field_name("table")?, symbols)?;
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

fn enclosing_colon_owner_table(mut node: Node) -> Option<Node> {
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
