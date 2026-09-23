//! Table and routine references inside SQL objects.
//!
//! Every `object_reference` that names a table or view (FROM, JOIN, UPDATE,
//! DELETE, INSERT, MERGE, TRUNCATE, CREATE TABLE AS, a trigger's ON target) or
//! a routine (function invocation, EXEC, a trigger's EXECUTE FUNCTION) becomes
//! an edge from the routine, view, trigger, or table that holds it: a
//! relationship when the target is declared in this file, a structured pending
//! relationship otherwise.

use crate::base::{
    BaseExtractor, Relationship, RelationshipKind, StructuredPendingRelationship, Symbol,
    UnresolvedTarget,
};
use crate::sql::helpers::normalize_sql_identifier;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ObjectReferenceRole {
    Table,
    TriggerTarget,
    Call,
}

/// What an `object_reference` names, judged by where it sits.
pub(super) fn object_reference_role(node: Node) -> Option<ObjectReferenceRole> {
    let parent = node.parent()?;
    let previous = node.prev_named_sibling().map(|sibling| sibling.kind());
    match parent.kind() {
        "relation" | "from" | "insert" | "create_index" => Some(ObjectReferenceRole::Table),
        "invocation" | "execute_statement" => Some(ObjectReferenceRole::Call),
        "statement" => {
            let opener = parent.named_child(0)?.kind();
            matches!(opener, "keyword_merge" | "keyword_truncate")
                .then_some(ObjectReferenceRole::Table)
        }
        "create_trigger" => match previous? {
            "keyword_on" => Some(ObjectReferenceRole::TriggerTarget),
            "keyword_function" | "keyword_procedure" => Some(ObjectReferenceRole::Call),
            _ => None,
        },
        _ => None,
    }
}

/// `[database, schema, name]` parts of an object reference, outermost first.
pub(super) fn object_reference_parts(base: &BaseExtractor, node: Node) -> Vec<String> {
    let mut parts = ["database", "schema", "name"]
        .into_iter()
        .filter_map(|field| node.child_by_field_name(field))
        .map(|part| normalize_sql_identifier(&base.get_node_text(&part)))
        .collect::<Vec<_>>();
    if parts.is_empty() {
        let mut cursor = node.walk();
        parts.extend(
            node.named_children(&mut cursor)
                .filter(|child| child.kind() == "identifier")
                .map(|child| normalize_sql_identifier(&base.get_node_text(&child))),
        );
    }
    parts
}

/// The identifier node that names the referenced object.
pub(super) fn object_reference_name_node(node: Node) -> Option<Node> {
    node.child_by_field_name("name").or_else(|| {
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .filter(|child| child.kind() == "identifier")
            .last()
    })
}

pub(super) fn is_table_like(symbol: &Symbol) -> bool {
    metadata_flag(symbol, "isTable") || metadata_flag(symbol, "isView")
}

pub(super) fn is_routine(symbol: &Symbol) -> bool {
    metadata_flag(symbol, "isStoredProcedure") || metadata_flag(symbol, "isFunction")
}

/// The one same-file symbol a qualified or bare reference names. A schema on
/// both sides must agree; an unqualified side matches any schema.
pub(super) fn find_declared_object<'a>(
    symbols: &'a [Symbol],
    parts: &[String],
    accepts: impl Fn(&Symbol) -> bool,
) -> Option<&'a Symbol> {
    let (name, qualifiers) = parts.split_last()?;
    let schema = qualifiers.last();
    let mut matches = symbols.iter().filter(|symbol| {
        symbol.name == *name
            && accepts(symbol)
            && match (schema, metadata_str(symbol, "schema")) {
                (Some(wanted), Some(declared)) => wanted == declared,
                _ => true,
            }
    });
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

pub(super) fn extract_object_reference_relationships(
    base: &mut BaseExtractor,
    root: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    visit(base, root, symbols, relationships, 0);
}

fn visit(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    if node.kind() == "object_reference"
        && let Some(role) = object_reference_role(node)
    {
        record_reference(base, node, role, symbols, relationships);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(base, child, symbols, relationships, child_depth);
    }
}

fn record_reference(
    base: &mut BaseExtractor,
    node: Node,
    role: ObjectReferenceRole,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let Some(owner) = owning_object(symbols, node) else {
        return;
    };
    let parts = object_reference_parts(base, node);
    let Some(terminal_name) = parts.last().cloned() else {
        return;
    };
    if terminal_name.starts_with('@') {
        return;
    }
    let (kind, target) = match role {
        ObjectReferenceRole::Call => (
            RelationshipKind::Calls,
            find_declared_object(symbols, &parts, is_routine),
        ),
        ObjectReferenceRole::Table | ObjectReferenceRole::TriggerTarget => {
            if is_query_local_name(symbols, owner, &terminal_name)
                || (metadata_flag(owner, "isTrigger") && is_trigger_pseudo_table(&terminal_name))
            {
                return;
            }
            (
                RelationshipKind::References,
                find_declared_object(symbols, &parts, is_table_like),
            )
        }
    };
    let line_number = node.start_position().row as u32 + 1;

    let Some(target) = target else {
        let target = UnresolvedTarget {
            display_name: parts.join("."),
            terminal_name,
            receiver: None,
            namespace_path: parts[..parts.len() - 1].to_vec(),
            import_context: None,
        };
        base.add_structured_pending_relationship(StructuredPendingRelationship::new(
            owner.id.clone(),
            target,
            Some(owner.id.clone()),
            kind,
            base.file_path.clone(),
            line_number,
            1.0,
        ));
        return;
    };

    let metadata = (kind == RelationshipKind::References).then(|| {
        HashMap::from([
            (
                "relationshipType".to_string(),
                Value::String(table_relationship_type(owner, role).to_string()),
            ),
            (
                "targetTable".to_string(),
                Value::String(target.name.clone()),
            ),
            ("isExternal".to_string(), Value::Bool(false)),
        ])
    });
    relationships.push(Relationship {
        id: format!(
            "{}_{}_{:?}_{}",
            owner.id,
            target.id,
            kind,
            node.start_byte()
        ),
        from_symbol_id: owner.id.clone(),
        to_symbol_id: target.id.clone(),
        kind,
        file_path: base.file_path.clone(),
        line_number,
        span: Some(crate::base::NormalizedSpan::from_node(&node)),
        reference_site_is_exact: false,
        confidence: 0.95,
        metadata,
    });
}

fn table_relationship_type(owner: &Symbol, role: ObjectReferenceRole) -> &'static str {
    if role == ObjectReferenceRole::TriggerTarget {
        "trigger_target"
    } else if metadata_flag(owner, "isView") {
        "view_source"
    } else if metadata_flag(owner, "isIndex") {
        "index_table"
    } else {
        "table_reference"
    }
}

/// The innermost routine, view, trigger, index, or table whose span holds
/// `node`.
fn owning_object<'a>(symbols: &'a [Symbol], node: Node) -> Option<&'a Symbol> {
    let (start, end) = (node.start_byte() as u32, node.end_byte() as u32);
    symbols
        .iter()
        .filter(|symbol| {
            (is_routine(symbol)
                || is_table_like(symbol)
                || metadata_flag(symbol, "isTrigger")
                || metadata_flag(symbol, "isIndex"))
                && !metadata_flag(symbol, "isCte")
                && symbol.start_byte <= start
                && end <= symbol.end_byte
        })
        .min_by_key(|symbol| symbol.end_byte - symbol.start_byte)
}

/// A CTE declared inside the owning object shadows any table of that name.
fn is_query_local_name(symbols: &[Symbol], owner: &Symbol, name: &str) -> bool {
    symbols.iter().any(|symbol| {
        symbol.name == name
            && metadata_flag(symbol, "isCte")
            && owner.start_byte <= symbol.start_byte
            && symbol.end_byte <= owner.end_byte
    })
}

/// T-SQL `inserted`/`deleted` rows exist only inside a trigger body.
fn is_trigger_pseudo_table(name: &str) -> bool {
    name.eq_ignore_ascii_case("inserted") || name.eq_ignore_ascii_case("deleted")
}

fn metadata_flag(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn metadata_str<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_str)
}
