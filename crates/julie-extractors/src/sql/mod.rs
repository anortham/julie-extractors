//! SQL language extractor module.
//!
//! This module provides comprehensive SQL symbol extraction for cross-platform code intelligence.
//! It's organized into logical submodules for maintainability:
//!
//! - **helpers.rs**: Regex patterns and utility functions
//! - **schemas.rs**: Table, view, index, trigger extraction
//! - **routines.rs**: Stored procedures and functions
//! - **constraints.rs**: Column and table constraints
//! - **relationships.rs**: Foreign keys and joins
//! - **error_handling.rs**: ERROR node processing
//! - **views.rs**: View columns and SELECT alias extraction
//! - **identifiers.rs**: Identifier usage extraction
//!
//! This enables full-stack symbol tracing from frontend -> API -> database schema.

mod aliases;
mod body_spans;
pub(crate) mod complexity_metrics;
mod constraints;
pub(crate) mod doc_comments;
mod error_handling;
pub(crate) mod helpers;
mod identifiers;
mod references;
mod relationships;
mod routines;
mod schema_relationships;
mod schemas;
mod test_detection;
mod types;
mod views;

use crate::base::{
    BaseExtractor, ContainingSymbolIndex, Identifier, PendingRelationship, Relationship,
    StructuredPendingRelationship, Symbol,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Tree;

/// SQL language extractor that handles SQL-specific constructs for cross-language tracing:
/// - Table definitions (CREATE TABLE)
/// - Column definitions and constraints
/// - Stored procedures and functions
/// - Views and triggers
/// - Indexes and foreign keys
/// - Query patterns and table references
pub struct SqlExtractor {
    pub(crate) base: BaseExtractor,
    routine_variables: HashMap<String, std::collections::HashSet<String>>,
}

impl SqlExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            routine_variables: HashMap::new(),
        }
    }

    /// Pending relationships accumulated during extraction. SQL emits these
    /// for cross-file/cross-schema FK targets (Phase 3.1).
    pub fn get_pending_relationships(&self) -> Vec<PendingRelationship> {
        self.base.get_pending_relationships()
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals.
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    /// Structured pending relationships with full `UnresolvedTarget` shape
    /// (terminal_name, namespace_path, etc.) for cross-file FK references.
    pub fn get_structured_pending_relationships(&self) -> Vec<StructuredPendingRelationship> {
        self.base.get_structured_pending_relationships()
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let pgtap_context = test_detection::PgTapContext::from_tree(&self.base, tree);
        self.visit_node(tree.root_node(), &mut symbols, None, 0, &pgtap_context);
        for column in symbols.iter_mut().filter(|symbol| {
            matches!(
                symbol.kind,
                crate::base::SymbolKind::Field | crate::base::SymbolKind::Variable
            )
        }) {
            column.body_span = None;
            column.body_hash = None;
        }
        test_detection::mark_pgtap_schema_containers(&pgtap_context, &mut symbols);
        doc_comments::apply_catalog_comments(&self.base, tree.root_node(), &mut symbols);
        self.record_declared_types(tree.root_node(), &symbols);
        let containing_symbols = self.base.containing_symbol_index(&symbols);
        self.walk_for_string_literals(tree.root_node(), &containing_symbols, 0);
        symbols
    }

    fn walk_for_string_literals(
        &mut self,
        node: tree_sitter::Node,
        containing_symbols: &ContainingSymbolIndex<'_>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        if matches!(node.kind(), "string" | "string_literal" | "literal") {
            let containing_symbol_id = containing_symbols
                .find(node)
                .map(|symbol| symbol.id.clone());
            if let Some(text) = self.decode_sql_string_literal(&node) {
                let carrier = self.sql_literal_carrier(&node);
                self.base
                    .record_literal(&node, text, carrier, 0, containing_symbol_id);
            }
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_string_literals(child, containing_symbols, child_depth);
        }
    }

    fn sql_literal_carrier(&self, node: &tree_sitter::Node) -> Option<String> {
        if let Some(carrier) = self.dynamic_sql_carrier(node) {
            return Some(carrier);
        }
        if self.literal_is_in_default_clause(node) {
            return Some("DEFAULT".to_string());
        }
        if let Some(column_def) = self.find_ancestor(node, "column_definition") {
            let name_node = self
                .base
                .find_child_by_type(&column_def, "identifier")
                .or_else(|| self.base.find_child_by_type(&column_def, "column_name"));
            if let Some(name_node) = name_node {
                return Some(helpers::normalize_sql_identifier(
                    &self.base.get_node_text(&name_node),
                ));
            }
        }
        if let Some(parent) = node.parent() {
            match parent.kind() {
                "insert" | "insert_statement" | "values" => {
                    return Some("INSERT".to_string());
                }
                "list"
                    if parent
                        .parent()
                        .is_some_and(|insert| insert.kind() == "insert") =>
                {
                    return Some("INSERT".to_string());
                }
                _ => {}
            }
        }
        Some("value".to_string())
    }

    /// `EXEC(N'...')` carries `EXEC`; a string argument of `EXEC proc 'x'`
    /// carries the procedure name, so `sp_executesql` strings classify as SQL.
    fn dynamic_sql_carrier(&self, node: &tree_sitter::Node) -> Option<String> {
        let parent = node.parent()?;
        match parent.kind() {
            "parenthesized_expression" => parent
                .parent()
                .filter(|execute| execute.kind() == "execute_statement")
                .filter(|execute| {
                    execute
                        .child_by_field_name("command")
                        .is_some_and(|command| command.id() == parent.id())
                })
                .map(|_| "EXEC".to_string()),
            "execute_statement" => {
                let reference = self.base.find_child_by_type(&parent, "object_reference")?;
                let parts = references::object_reference_parts(&self.base, reference);
                parts.last().cloned()
            }
            _ => None,
        }
    }

    fn find_ancestor<'a>(
        &self,
        node: &tree_sitter::Node<'a>,
        kind: &str,
    ) -> Option<tree_sitter::Node<'a>> {
        let mut current = node.parent();
        while let Some(parent) = current {
            if parent.kind() == kind {
                return Some(parent);
            }
            current = parent.parent();
        }
        None
    }

    fn literal_is_in_default_clause(&self, node: &tree_sitter::Node) -> bool {
        let column_def = self.find_ancestor(node, "column_definition");
        let Some(column_def) = column_def else {
            return false;
        };
        let mut cursor = column_def.walk();
        let mut saw_default = false;
        for child in column_def.children(&mut cursor) {
            if child.kind() == "keyword_default" {
                saw_default = true;
            }
            if descendant_contains(child, *node) {
                return saw_default;
            }
        }
        false
    }

    fn decode_sql_string_literal(&self, node: &tree_sitter::Node) -> Option<String> {
        if node.kind() == "literal" {
            helpers::sql_string_literal_text(&self.base.get_node_text(node))
                .map(|text| text.trim().to_string())
        } else {
            self.base.decode_string_literal(node)
        }
    }

    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let mut relationships = Vec::new();
        relationships::extract_relationships_internal(
            &mut self.base,
            tree.root_node(),
            symbols,
            &mut relationships,
            0,
        );
        references::extract_object_reference_relationships(
            &mut self.base,
            tree.root_node(),
            symbols,
            &mut relationships,
        );
        relationships
    }

    /// Object kinds for tables, views, and procedures. Declared types from the
    /// grammar's type nodes are recorded on the base during extraction and win
    /// over these rows.
    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        let mut types = HashMap::new();
        for symbol in symbols {
            let flag = |key: &str| {
                symbol
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get(key))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            };
            let inferred = if flag("isTable") {
                Some("TABLE".to_string())
            } else if flag("isView") {
                Some("VIEW".to_string())
            } else if flag("isStoredProcedure") {
                Some("PROCEDURE".to_string())
            } else if flag("extractedFromError") {
                symbol
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get("returnType"))
                    .and_then(|v| v.as_str())
                    .filter(|ty| !ty.is_empty())
                    .map(str::to_string)
                    .or_else(|| {
                        let signature = symbol.signature.as_deref()?;
                        helpers::SQL_TYPE_RE
                            .find(signature)
                            .map(|found| found.as_str().to_uppercase())
                    })
            } else {
                None
            };
            if let Some(inferred) = inferred {
                types.insert(symbol.id.clone(), inferred);
            }
        }
        types
    }

    /// Extract all identifier usages (function calls, member access, etc.)
    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        let containing_symbols = self.base.containing_symbol_index(symbols);
        self.routine_variables = identifiers::routine_variables(symbols);
        self.walk_tree_for_identifiers(tree.root_node(), &containing_symbols, 0);
        self.base.identifiers.clone()
    }

    /// Main node visiting dispatch function
    fn visit_node(
        &mut self,
        node: tree_sitter::Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<&str>,
        depth: u32,
        pgtap_context: &test_detection::PgTapContext,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        let mut symbol: Option<Symbol> = None;

        match node.kind() {
            "create_table" => {
                symbol = schemas::extract_table_definition(&mut self.base, node, parent_id);
            }
            "create_procedure"
            | "create_function"
            | "create_function_statement"
            | "alter_procedure"
            | "alter_function" => {
                symbol = routines::extract_stored_procedure(
                    &mut self.base,
                    node,
                    parent_id,
                    pgtap_context,
                );
            }
            "create_view" | "create_materialized_view" => {
                symbol = schemas::extract_view(&mut self.base, node, parent_id);
            }
            "create_index" => {
                symbol = schemas::extract_index(&mut self.base, node, parent_id);
            }
            "create_trigger" => {
                symbol = schemas::extract_trigger(&mut self.base, node, parent_id);
            }
            "cte" => {
                symbol = schemas::extract_cte(&mut self.base, node, parent_id);
            }
            "create_schema" => {
                symbol = schemas::extract_schema(&mut self.base, node, parent_id);
            }
            "create_sequence" => {
                symbol = schemas::extract_sequence(&mut self.base, node, parent_id);
            }
            "create_domain" => {
                symbol = schemas::extract_domain(&mut self.base, node, parent_id);
            }
            "create_type" => {
                symbol = schemas::extract_type(&mut self.base, node, parent_id);
            }
            "alter_table" => {
                constraints::extract_constraints_from_alter_table(&mut self.base, node, symbols);
            }
            "select" if views::alias_owner_kind(node) == Some("cte") => {
                self.extract_select_aliases(node, symbols, parent_id);
            }
            "ERROR" => {
                // Remember symbol count before extraction
                let symbols_before = symbols.len();

                error_handling::extract_multiple_from_error_node(
                    &mut self.base,
                    node,
                    symbols,
                    parent_id,
                );

                // Check if any view symbols were added and extract their columns
                for i in symbols_before..symbols.len() {
                    let symbol_ref = &symbols[i].clone(); // Clone to avoid borrow issues
                    if symbol_ref
                        .metadata
                        .as_ref()
                        .and_then(|m| m.get("isView"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        self.extract_view_columns_from_error_node(node, symbols, &symbol_ref.id);
                    }
                }
            }
            _ => {}
        }

        if let Some(mut symbol) = symbol {
            if node.kind() != "ERROR" {
                record_declared_schema(&self.base, node, &mut symbol);
            }
            symbols.push(symbol.clone());

            // Extract additional child symbols for specific node types
            match node.kind() {
                "create_table" => {
                    constraints::extract_table_columns(
                        &mut self.base,
                        node,
                        symbols,
                        Some(&symbol.id),
                    );
                    constraints::extract_table_constraints(
                        &mut self.base,
                        node,
                        symbols,
                        &symbol.id,
                    );
                }
                "create_view" | "create_materialized_view" => {
                    self.extract_view_columns(node, symbols, &symbol.id);
                }
                "ERROR" => {
                    let metadata = &symbol.metadata;
                    if metadata
                        .as_ref()
                        .and_then(|m| m.get("isView"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        self.extract_view_columns_from_error_node(node, symbols, &symbol.id);
                    }
                    if metadata
                        .as_ref()
                        .and_then(|m| m.get("isStoredProcedure"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                        || metadata
                            .as_ref()
                            .and_then(|m| m.get("isFunction"))
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                    {
                        routines::extract_parameters_from_error_node(
                            &mut self.base,
                            node,
                            symbols,
                            &symbol.id,
                        );
                    }
                }
                "create_procedure"
                | "create_function"
                | "create_function_statement"
                | "alter_procedure"
                | "alter_function" => {
                    routines::extract_parameters_from_routine_node(
                        &mut self.base,
                        node,
                        symbols,
                        &symbol.id,
                    );
                    routines::extract_bare_tsql_parameters(
                        &mut self.base,
                        node,
                        symbols,
                        &symbol.id,
                    );
                    routines::extract_declare_variables(&mut self.base, node, symbols, &symbol.id);
                }
                _ => {}
            }

            // Continue with this symbol as parent
            let new_parent_id = Some(symbol.id.as_str());
            let Some(child_depth) = child_tree_depth(depth) else {
                return;
            };
            for child in node.children(&mut node.walk()) {
                self.visit_node(child, symbols, new_parent_id, child_depth, pgtap_context);
            }
        } else {
            // No symbol extracted, continue with current parent
            let Some(child_depth) = child_tree_depth(depth) else {
                return;
            };
            for child in node.children(&mut node.walk()) {
                self.visit_node(child, symbols, parent_id, child_depth, pgtap_context);
            }
        }
    }
}

/// Record the schema a declaration names (`CREATE TABLE billing.accounts`) so
/// references can tell same-named objects in different schemas apart.
fn record_declared_schema(base: &BaseExtractor, node: tree_sitter::Node, symbol: &mut Symbol) {
    let Some(reference) = base.find_child_by_type(&node, "object_reference") else {
        return;
    };
    let Some(schema) = reference.child_by_field_name("schema") else {
        return;
    };
    symbol.metadata.get_or_insert_with(HashMap::new).insert(
        "schema".to_string(),
        serde_json::Value::String(helpers::normalize_sql_identifier(
            &base.get_node_text(&schema),
        )),
    );
}

fn descendant_contains(ancestor: tree_sitter::Node, target: tree_sitter::Node) -> bool {
    descendant_contains_with_depth(ancestor, target, 0)
}

fn descendant_contains_with_depth(
    ancestor: tree_sitter::Node,
    target: tree_sitter::Node,
    depth: u32,
) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }

    if ancestor.id() == target.id() {
        return true;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = ancestor.walk();
    for child in ancestor.children(&mut cursor) {
        if descendant_contains_with_depth(child, target, child_depth) {
            return true;
        }
    }
    false
}
