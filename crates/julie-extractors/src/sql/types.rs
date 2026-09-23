//! Declared SQL types: the grammar's type node for columns, parameters, and
//! local variables, and the `RETURNS` clause of functions.

use crate::base::types::TypeNameRules;
use crate::base::{BaseExtractor, Symbol, SymbolKind};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

use super::SqlExtractor;

/// `SETOF orders` resolves to `orders`; `NUMERIC(12,2)` and `TEXT[]` resolve
/// to their base names and keep the full text as `declared`.
const SQL_TYPE_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &["SETOF", "setof", "Setof"],
    generic_open: &['(', '['],
};

impl SqlExtractor {
    pub(super) fn record_declared_types(&mut self, root: Node, symbols: &[Symbol]) {
        let mut declared = HashMap::new();
        collect_declared_types(&self.base, root, &mut declared, 0);
        for symbol in symbols {
            let declared_type = declared
                .get(&(symbol.start_byte, symbol_slot(symbol)))
                .cloned()
                .or_else(|| bare_parameter_type(symbol));
            if let Some(declared_type) = declared_type {
                self.base.record_declared_type_fact(
                    &symbol.id,
                    &declared_type,
                    &SQL_TYPE_RULES,
                    false,
                );
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Slot {
    Column,
    Variable,
    Routine,
    Other,
}

fn symbol_slot(symbol: &Symbol) -> Slot {
    match symbol.kind {
        SymbolKind::Field => Slot::Column,
        SymbolKind::Variable => Slot::Variable,
        SymbolKind::Function => Slot::Routine,
        _ => Slot::Other,
    }
}

fn collect_declared_types(
    base: &BaseExtractor,
    node: Node,
    declared: &mut HashMap<(u32, Slot), String>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    let entry = match node.kind() {
        "column_definition" => column_type(base, node).map(|ty| (Slot::Column, ty)),
        "function_argument" | "function_declaration" | "var_declaration" => {
            named_declaration_type(base, node).map(|ty| (Slot::Variable, ty))
        }
        "create_function" | "alter_function" => {
            routine_return_type(base, &node).map(|ty| (Slot::Routine, ty))
        }
        _ => None,
    };
    if let Some((slot, declared_type)) = entry {
        declared.insert((node.start_byte() as u32, slot), declared_type);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_declared_types(base, child, declared, child_depth);
    }
}

pub(super) fn column_type(base: &BaseExtractor, column: Node) -> Option<String> {
    let mut cursor = column.walk();
    let type_nodes: Vec<Node> = column
        .children_by_field_name("type", &mut cursor)
        .chain(column.child_by_field_name("custom_type"))
        .collect();
    span_text(base, type_nodes.first()?, type_nodes.last()?)
}

/// `name TYPE ...`: the type is the node after the name, plus an array
/// suffix (`TEXT[]`).
pub(super) fn named_declaration_type(base: &BaseExtractor, declaration: Node) -> Option<String> {
    let name = base.find_child_by_type(&declaration, "identifier")?;
    let type_node = name
        .next_named_sibling()
        .filter(|node| !matches!(node.kind(), "literal" | "keyword_default"))?;
    let last = type_node
        .next_named_sibling()
        .filter(|node| node.kind() == "array_size_definition")
        .unwrap_or(type_node);
    span_text(base, &type_node, &last)
}

/// The type after `RETURNS`: `TEXT`, `NUMERIC(12,2)`, `SETOF orders`,
/// `TABLE(id INT, name TEXT)`.
pub(super) fn routine_return_type(base: &BaseExtractor, routine: &Node) -> Option<String> {
    let mut cursor = routine.walk();
    let mut children = routine.children(&mut cursor);
    children.find(|child| child.kind() == "keyword_returns")?;
    let parts: Vec<Node> = children
        .take_while(|child| !ends_return_clause(child.kind()))
        .collect();
    span_text(base, parts.first()?, parts.last()?)
}

fn ends_return_clause(kind: &str) -> bool {
    kind.starts_with("function_")
        || matches!(
            kind,
            "keyword_as" | "keyword_language" | "keyword_begin" | "keyword_with" | "ERROR"
        )
}

fn span_text(base: &BaseExtractor, first: &Node, last: &Node) -> Option<String> {
    let text = base.content.get(first.start_byte()..last.end_byte())?;
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

/// A T-SQL parameter declared without parentheses keeps its declaration as
/// its signature (`@UserId INT = 0 OUTPUT`).
fn bare_parameter_type(symbol: &Symbol) -> Option<String> {
    if symbol.kind != SymbolKind::Variable || !symbol.name.starts_with('@') {
        return None;
    }
    let signature = symbol.signature.as_deref()?;
    let rest = signature.strip_prefix(symbol.name.as_str())?;
    let words: Vec<&str> = rest
        .split('=')
        .next()?
        .split_whitespace()
        .filter(|word| {
            !matches!(
                word.to_ascii_uppercase().as_str(),
                "AS" | "OUT" | "OUTPUT" | "READONLY"
            )
        })
        .collect();
    let rest = words.join(" ");
    (!rest.is_empty()).then_some(rest)
}
