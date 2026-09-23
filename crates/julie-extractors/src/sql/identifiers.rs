//! Identifier extraction for SQL files.
//!
//! Handles walking the AST to extract identifier usages including:
//! - Function/procedure invocations
//! - Column references
//! - Qualified names (schema.table.column)

use crate::base::{ContainingSymbolIndex, IdentifierKind, Symbol, SymbolKind};
use crate::sql::helpers::normalize_sql_identifier;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};

use super::{SqlExtractor, aliases};

/// Parameter and local variable names of each routine, lowercased.
pub(super) fn routine_variables(symbols: &[Symbol]) -> HashMap<String, HashSet<String>> {
    let routines: HashSet<&str> = symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Function)
        .map(|symbol| symbol.id.as_str())
        .collect();
    let mut variables: HashMap<String, HashSet<String>> = HashMap::new();
    for symbol in symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Variable)
    {
        if let Some(routine) = symbol
            .parent_id
            .as_deref()
            .filter(|parent| routines.contains(parent))
        {
            variables
                .entry(routine.to_string())
                .or_default()
                .insert(symbol.name.to_ascii_lowercase());
        }
    }
    variables
}

/// An `EXEC proc @Name = value` argument name belongs to the callee.
fn is_named_argument(field: tree_sitter::Node) -> bool {
    field.parent().is_some_and(|assignment| {
        assignment.kind() == "binary_expression"
            && assignment
                .child_by_field_name("left")
                .is_some_and(|left| left.id() == field.id())
            && assignment
                .parent()
                .is_some_and(|parent| parent.kind() == "execute_statement")
    })
}

fn is_variable_declaration(parent: &str) -> bool {
    matches!(
        parent,
        "function_argument" | "var_declaration" | "function_declaration" | "declare_statement"
    )
}

impl SqlExtractor {
    /// Recursively walk tree extracting identifiers from each node
    pub(super) fn walk_tree_for_identifiers(
        &mut self,
        node: tree_sitter::Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        self.extract_identifier_from_node(node, containing_symbols);

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_tree_for_identifiers(child, containing_symbols, child_depth);
        }
    }

    /// Extract identifier from a single node based on its kind
    fn extract_identifier_from_node(
        &mut self,
        node: tree_sitter::Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) {
        match node.kind() {
            "object_reference" => {
                if let Some(name_node) = node.child_by_field_name("name")
                    && self.base.get_node_text(&name_node).starts_with('@')
                {
                    if node.parent().is_some_and(|parent| parent.kind() != "field") {
                        let name = normalize_sql_identifier(&self.base.get_node_text(&name_node));
                        let scope = self.find_containing_symbol_id(node, containing_symbols);
                        self.base.create_identifier(
                            &name_node,
                            name,
                            IdentifierKind::VariableRef,
                            scope,
                        );
                    }
                    return;
                }
                let kind = match super::references::object_reference_role(node) {
                    Some(
                        super::references::ObjectReferenceRole::Table
                        | super::references::ObjectReferenceRole::TriggerTarget,
                    ) => IdentifierKind::TypeUsage,
                    Some(super::references::ObjectReferenceRole::Call)
                        if node
                            .parent()
                            .is_some_and(|parent| parent.kind() != "invocation") =>
                    {
                        IdentifierKind::Call
                    }
                    _ => return,
                };
                if let Some(name_node) = super::references::object_reference_name_node(node) {
                    let name = normalize_sql_identifier(&self.base.get_node_text(&name_node));
                    let containing_symbol_id =
                        self.find_containing_symbol_id(node, containing_symbols);
                    self.base
                        .create_identifier(&name_node, name, kind, containing_symbol_id);
                }
            }
            "invocation" => {
                let name_node = if let Some(obj_ref) =
                    self.base.find_child_by_type(&node, "object_reference")
                {
                    obj_ref
                        .child_by_field_name("name")
                        .or_else(|| self.base.find_child_by_type(&obj_ref, "identifier"))
                } else {
                    self.base.find_child_by_type(&node, "identifier")
                };

                if let Some(name_node) = name_node {
                    let name = normalize_sql_identifier(&self.base.get_node_text(&name_node));
                    let containing_symbol_id =
                        self.find_containing_symbol_id(node, containing_symbols);

                    self.base.create_identifier(
                        &name_node,
                        name,
                        IdentifierKind::Call,
                        containing_symbol_id,
                    );
                }
            }

            "identifier"
                if self.base.get_node_text(&node).starts_with('@')
                    && node.parent().is_some_and(|parent| {
                        !matches!(parent.kind(), "field" | "object_reference" | "ERROR")
                            && !is_variable_declaration(parent.kind())
                    }) =>
            {
                let name = normalize_sql_identifier(&self.base.get_node_text(&node));
                let scope = self.find_containing_symbol_id(node, containing_symbols);
                self.base
                    .create_identifier(&node, name, IdentifierKind::VariableRef, scope);
            }

            "column"
                if node.parent().is_some_and(|list| list.kind() == "list")
                    && let Some(name_node) = self.base.find_child_by_type(&node, "identifier") =>
            {
                let name = normalize_sql_identifier(&self.base.get_node_text(&name_node));
                let scope = self.find_containing_symbol_id(node, containing_symbols);
                let table = node
                    .parent()
                    .and_then(|list| aliases::write_target_table(&self.base, list));
                self.base.create_identifier_with_receiver_type(
                    &name_node,
                    name,
                    IdentifierKind::MemberAccess,
                    scope,
                    table,
                );
            }

            "identifier" => {
                if let Some(reference) = node.parent()
                    && reference.kind() == "object_reference"
                    && reference
                        .child_by_field_name("name")
                        .is_some_and(|name| name.id() == node.id())
                    && reference
                        .parent()
                        .and_then(|parent| parent.child_by_field_name("custom_type"))
                        .is_some_and(|ty| ty.id() == reference.id())
                {
                    let name = normalize_sql_identifier(&self.base.get_node_text(&node));
                    let scope = self.find_containing_symbol_id(node, containing_symbols);
                    self.base
                        .create_identifier(&node, name, IdentifierKind::TypeUsage, scope);
                    return;
                }
                if let Some(next_sibling) = node.next_sibling()
                    && next_sibling.kind() == "function_arguments"
                {
                    let name = normalize_sql_identifier(&self.base.get_node_text(&node));
                    let containing_symbol_id =
                        self.find_containing_symbol_id(node, containing_symbols);

                    self.base.create_identifier(
                        &node,
                        name,
                        IdentifierKind::Call,
                        containing_symbol_id,
                    );
                    return;
                }

                if let Some(parent) = node.parent() {
                    match parent.kind() {
                        "select_expression" | "where_clause" | "having_clause" => {
                            let name = normalize_sql_identifier(&self.base.get_node_text(&node));
                            let containing_symbol_id =
                                self.find_containing_symbol_id(node, containing_symbols);

                            self.base.create_identifier(
                                &node,
                                name,
                                IdentifierKind::MemberAccess,
                                containing_symbol_id,
                            );
                        }
                        _ => {}
                    }
                }
            }

            "field" => {
                if let Some(parent) = node.parent()
                    && (parent.kind() == "table_reference" || parent.kind() == "qualified_name")
                {
                    return;
                }

                let name_node = node.child_by_field_name("name").unwrap_or(node);
                let name = normalize_sql_identifier(&self.base.get_node_text(&name_node));
                let containing_symbol_id = self.find_containing_symbol_id(node, containing_symbols);
                let qualified = node
                    .named_children(&mut node.walk())
                    .any(|child| child.kind() == "object_reference");
                if name.starts_with('@') {
                    if !is_named_argument(node) {
                        self.base.create_identifier(
                            &name_node,
                            name,
                            IdentifierKind::VariableRef,
                            containing_symbol_id,
                        );
                    }
                    return;
                }
                if !qualified
                    && containing_symbol_id
                        .as_ref()
                        .and_then(|routine| self.routine_variables.get(routine))
                        .is_some_and(|variables| variables.contains(&name.to_ascii_lowercase()))
                {
                    self.base.create_identifier(
                        &name_node,
                        name,
                        IdentifierKind::VariableRef,
                        containing_symbol_id,
                    );
                    return;
                }
                let table = aliases::column_table(&self.base, node);
                self.base.create_identifier_with_receiver_type(
                    &name_node,
                    name,
                    IdentifierKind::MemberAccess,
                    containing_symbol_id,
                    table,
                );
            }

            "qualified_name" => {
                let mut rightmost_identifier = None;
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "identifier" {
                        rightmost_identifier = Some(child);
                    }
                }

                if let Some(name_node) = rightmost_identifier {
                    let name = normalize_sql_identifier(&self.base.get_node_text(&name_node));
                    let containing_symbol_id =
                        self.find_containing_symbol_id(node, containing_symbols);

                    self.base.create_identifier(
                        &name_node,
                        name,
                        IdentifierKind::MemberAccess,
                        containing_symbol_id,
                    );
                }
            }

            _ => {}
        }
    }

    /// Find the ID of the symbol that contains this node
    fn find_containing_symbol_id(
        &self,
        node: tree_sitter::Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
    ) -> Option<String> {
        containing_symbols.find(node).map(|s| s.id.clone())
    }
}
