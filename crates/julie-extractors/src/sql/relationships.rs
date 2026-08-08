//! Relationship extraction (foreign keys, joins, table references).
//!
//! Handles extraction of relationships between tables and other objects:
//! - Foreign key relationships
//! - JOIN operations
//! - Table references in queries

use crate::base::{
    BaseExtractor, Relationship, RelationshipKind, StructuredPendingRelationship, Symbol,
    SymbolKind, UnresolvedTarget,
};
use crate::sql::helpers::normalize_sql_identifier;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract relationships recursively from tree
pub(super) fn extract_relationships_internal(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "create_view" => {
            super::schema_relationships::extract_view_source_relationships(
                base,
                node,
                symbols,
                relationships,
            );
        }
        "create_trigger" => {
            super::schema_relationships::extract_trigger_target_relationship(
                base,
                node,
                symbols,
                relationships,
            );
        }
        "ERROR" => {
            super::schema_relationships::extract_error_relationships(
                base,
                node,
                symbols,
                relationships,
            );
        }
        "constraint" => {
            // Check if this is a foreign key constraint
            let has_foreign = base.find_child_by_type(&node, "keyword_foreign");
            if has_foreign.is_some() {
                extract_foreign_key_relationship(base, node, symbols, relationships);
            }
        }
        "foreign_key_constraint" | "references_clause" => {
            extract_foreign_key_relationship(base, node, symbols, relationships);
        }
        // Inline REFERENCES on a column definition: tree-sitter-sequel does
        // not wrap the references in a dedicated `references_clause` node — it
        // attaches `keyword_references` + `object_reference` directly under
        // `column_definition`. Phase 3.1 needs to catch this shape so that
        // cross-schema FKs (e.g., `REFERENCES other_schema.users(id)`) emit
        // structured pending relationships.
        "column_definition"
            if base
                .find_child_by_type(&node, "keyword_references")
                .is_some() =>
        {
            extract_foreign_key_relationship(base, node, symbols, relationships);
        }
        // Plain SELECT/FROM table references are not emitted as top-level edges.
        // CREATE VIEW handles its own FROM dependencies at the view symbol boundary.
        "select_statement" | "from_clause" => {}
        "join" | "join_clause" => {
            extract_join_relationships(base, node, symbols, relationships);
        }
        _ => {}
    }

    // Recursively visit children
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in node.children(&mut node.walk()) {
        extract_relationships_internal(base, child, symbols, relationships, child_depth);
    }
}

/// Extract foreign key relationship from FOREIGN KEY constraint
pub(super) fn extract_foreign_key_relationship(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let references_keyword = base.find_child_by_type(&node, "keyword_references");
    if references_keyword.is_none() {
        return;
    }

    let object_ref_node = base.find_child_by_type(&node, "object_reference");
    let (referenced_table, referenced_table_parts) = if let Some(obj_ref) = object_ref_node {
        let parts = object_reference_parts(base, obj_ref);
        let Some(name) = parts.last().cloned() else {
            return;
        };
        (name, parts)
    } else {
        let Some(name_node) = base
            .find_child_by_type(&node, "table_name")
            .or_else(|| base.find_child_by_type(&node, "identifier"))
        else {
            return;
        };
        let name = normalize_sql_identifier(&base.get_node_text(&name_node));
        (name.clone(), vec![name])
    };
    let referenced_table_qualified = referenced_table_parts.join(".");

    // Find the source table (parent of this foreign key)
    let mut current_node = node.parent();
    while let Some(current) = current_node {
        if current.kind() == "create_table" {
            break;
        }
        current_node = current.parent();
    }

    let current_node = match current_node {
        Some(node) => node,
        None => return,
    };

    let source_object_ref_node = base.find_child_by_type(&current_node, "object_reference");
    let source_table = if let Some(obj_ref) = source_object_ref_node {
        let Some(name) = object_reference_name(base, obj_ref) else {
            return;
        };
        name
    } else {
        let Some(name_node) = base
            .find_child_by_type(&current_node, "identifier")
            .or_else(|| base.find_child_by_type(&current_node, "table_name"))
        else {
            return;
        };
        normalize_sql_identifier(&base.get_node_text(&name_node))
    };

    // Find corresponding symbols
    let source_symbol = symbols
        .iter()
        .find(|s| s.name == source_table && s.kind == SymbolKind::Class);
    let target_symbol = symbols
        .iter()
        .find(|s| s.name == referenced_table && s.kind == SymbolKind::Class);

    let line_number = node.start_position().row as u32 + 1;

    match (source_symbol, target_symbol) {
        (Some(source_symbol), Some(target_symbol)) => {
            let mut metadata = HashMap::new();
            metadata.insert(
                "targetTable".to_string(),
                Value::String(referenced_table.clone()),
            );
            metadata.insert("sourceTable".to_string(), Value::String(source_table));
            metadata.insert(
                "relationshipType".to_string(),
                Value::String("foreign_key".to_string()),
            );
            metadata.insert("isExternal".to_string(), Value::Bool(false));

            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    source_symbol.id,
                    target_symbol.id,
                    RelationshipKind::References,
                    node.start_position().row
                ),
                from_symbol_id: source_symbol.id.clone(),
                to_symbol_id: target_symbol.id.clone(),
                kind: RelationshipKind::References,
                file_path: base.file_path.clone(),
                line_number,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 1.0,
                metadata: Some(metadata),
            });
        }
        (Some(source_symbol), None) => {
            let (terminal_name, namespace_path) = match referenced_table_parts.as_slice() {
                [] => return,
                [name] => (name.clone(), Vec::new()),
                _ => (
                    referenced_table_parts
                        .last()
                        .expect("split produces at least one element")
                        .clone(),
                    referenced_table_parts[..referenced_table_parts.len() - 1].to_vec(),
                ),
            };
            let target = UnresolvedTarget {
                display_name: referenced_table_qualified.clone(),
                terminal_name,
                receiver: None,
                namespace_path,
                import_context: None,
            };
            let pending = StructuredPendingRelationship::new(
                source_symbol.id.clone(),
                target,
                Some(source_symbol.id.clone()),
                RelationshipKind::References,
                base.file_path.clone(),
                line_number,
                1.0,
            );
            base.add_structured_pending_relationship(pending);
        }
        _ => {
            // No source table in scope (malformed AST): emit nothing rather
            // than fabricate a synthetic edge. The capability matrix's
            // negative-case test requires this branch to exist.
        }
    }
}

/// Extract JOIN relationships
fn first_child_by_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    for i in 0..node.child_count() {
        let child = node.child(i as u32)?;
        if child.kind() == kind {
            return Some(child);
        }
    }
    None
}

fn table_symbol_from_relation<'a>(
    base: &BaseExtractor,
    relation_node: Node,
    symbols: &'a [Symbol],
) -> Option<(&'a Symbol, String)> {
    let object_reference = first_child_by_kind(relation_node, "object_reference")?;
    let table_name = object_reference_name(base, object_reference)?;
    let table_symbol = symbols
        .iter()
        .find(|s| s.name == table_name && s.kind == SymbolKind::Class)?;

    Some((table_symbol, table_name))
}

fn object_reference_name(base: &BaseExtractor, object_reference: Node) -> Option<String> {
    object_reference_parts(base, object_reference).pop()
}

fn object_reference_parts(base: &BaseExtractor, object_reference: Node) -> Vec<String> {
    let mut parts = ["database", "schema", "name"]
        .into_iter()
        .filter_map(|field| object_reference.child_by_field_name(field))
        .map(|node| normalize_sql_identifier(&base.get_node_text(&node)))
        .collect::<Vec<_>>();
    if parts.is_empty()
        && let Some(identifier) = first_child_by_kind(object_reference, "identifier")
    {
        parts.push(normalize_sql_identifier(&base.get_node_text(&identifier)));
    }
    parts
}

fn enclosing_from_node(mut node: Node) -> Option<Node> {
    while let Some(parent) = node.parent() {
        if parent.kind() == "from" {
            return Some(parent);
        }
        node = parent;
    }
    None
}

pub(super) fn extract_join_relationships(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(from_node) = enclosing_from_node(node) else {
        return;
    };
    let Some(source_relation) = first_child_by_kind(from_node, "relation") else {
        return;
    };
    let Some((source_symbol, _source_table_name)) =
        table_symbol_from_relation(base, source_relation, symbols)
    else {
        return;
    };
    let Some(target_relation) = first_child_by_kind(node, "relation") else {
        return;
    };
    let Some((target_symbol, target_table_name)) =
        table_symbol_from_relation(base, target_relation, symbols)
    else {
        return;
    };

    // Create a join relationship from the FROM-side table to the joined table.
    let mut metadata = HashMap::new();
    metadata.insert("joinType".to_string(), Value::String("join".to_string()));
    metadata.insert("tableName".to_string(), Value::String(target_table_name));

    relationships.push(Relationship {
        id: format!(
            "{}_{}_{:?}_{}",
            source_symbol.id,
            target_symbol.id,
            RelationshipKind::Joins,
            node.start_position().row
        ),
        from_symbol_id: source_symbol.id.clone(),
        to_symbol_id: target_symbol.id.clone(),
        kind: RelationshipKind::Joins,
        file_path: base.file_path.clone(),
        line_number: node.start_position().row as u32 + 1,
        span: Some(crate::base::NormalizedSpan::from_node(&node)),
        reference_site_is_exact: false,
        confidence: 0.9,
        metadata: Some(metadata),
    });
}
