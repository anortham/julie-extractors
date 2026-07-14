use std::collections::HashMap;

use regex::Regex;
use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

use super::attach_containing_symbols;
use super::span::NormalizedSpan;
use super::structural_facts::sort_structural_facts;
use super::types::{StructuralFact, Symbol, stable_location_id};
use crate::sql::helpers::normalize_sql_identifier;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const TABLE_DEFINITION_PATTERN_ID: &str = "sql.table_definition.v1";
const VIEW_DEFINITION_PATTERN_ID: &str = "sql.view_definition.v1";
const TRIGGER_DEFINITION_PATTERN_ID: &str = "sql.trigger_definition.v1";
const COLUMN_DEFINITION_PATTERN_ID: &str = "sql.column_definition.v1";
const CONSTRAINT_PATTERN_ID: &str = "sql.constraint.v1";
const FOREIGN_KEY_PATTERN_ID: &str = "sql.foreign_key.v1";
const SELECT_QUERY_PATTERN_ID: &str = "sql.select_query.v1";
const CTE_PATTERN_ID: &str = "sql.cte.v1";
const JOIN_PATTERN_ID: &str = "sql.join.v1";
const TRANSACTION_PATTERN_ID: &str = "sql.transaction.v1";
const INDEX_DEFINITION_PATTERN_ID: &str = "sql.index_definition.v1";
const UPDATE_STATEMENT_PATTERN_ID: &str = "sql.update_statement.v1";
const MERGE_STATEMENT_PATTERN_ID: &str = "sql.merge_statement.v1";

#[cfg(all(test, feature = "test-capability-matrix"))]
const SQL_STRUCTURAL_PATTERN_IDS: &[&str] = &[
    COLUMN_DEFINITION_PATTERN_ID,
    CONSTRAINT_PATTERN_ID,
    CTE_PATTERN_ID,
    FOREIGN_KEY_PATTERN_ID,
    INDEX_DEFINITION_PATTERN_ID,
    JOIN_PATTERN_ID,
    MERGE_STATEMENT_PATTERN_ID,
    SELECT_QUERY_PATTERN_ID,
    TABLE_DEFINITION_PATTERN_ID,
    TRANSACTION_PATTERN_ID,
    TRIGGER_DEFINITION_PATTERN_ID,
    UPDATE_STATEMENT_PATTERN_ID,
    VIEW_DEFINITION_PATTERN_ID,
];

static ERROR_TRIGGER_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"(?i)CREATE\s+TRIGGER\s+([a-zA-Z_][a-zA-Z0-9_]*)").unwrap()
});
static ERROR_TRIGGER_DETAILS_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(
        r"(?i)CREATE\s+TRIGGER\s+[a-zA-Z_][a-zA-Z0-9_]*\s+(BEFORE|AFTER)\s+(INSERT|UPDATE|DELETE)\s+ON\s+([a-zA-Z_][a-zA-Z0-9_]*)",
    )
    .unwrap()
});

pub fn collect_sql_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    if language != "sql" {
        return Vec::new();
    }

    let mut facts = Vec::new();
    collect_sql_node(tree.root_node(), file_path, content, &mut facts, 0);
    attach_containing_symbols(&mut facts, symbols);
    sort_structural_facts(&mut facts);
    facts
}

#[cfg(all(test, feature = "test-capability-matrix"))]
pub(crate) fn sql_structural_fact_pattern_ids_for_language(
    language: &str,
) -> &'static [&'static str] {
    match language {
        "sql" => SQL_STRUCTURAL_PATTERN_IDS,
        _ => &[],
    }
}

fn collect_sql_node(
    node: Node<'_>,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "create_table" => {
            if let Some(fact) = table_definition_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "create_view" => {
            if let Some(fact) = view_definition_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "create_trigger" => {
            if let Some(fact) = trigger_definition_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "create_index" => {
            if let Some(fact) = index_definition_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "column_definition" => {
            if let Some(fact) = column_definition_fact(file_path, content, node) {
                facts.push(fact);
            }
            if has_child_kind(node, "keyword_references")
                && let Some(fact) = foreign_key_fact(file_path, content, node)
            {
                facts.push(fact);
            }
        }
        "constraint" => {
            if has_child_kind(node, "keyword_foreign")
                && let Some(fact) = foreign_key_fact(file_path, content, node)
            {
                facts.push(fact);
            } else if let Some(fact) = constraint_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "select" => {
            if let Some(fact) = select_query_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "cte" => {
            if let Some(fact) = cte_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "join" => {
            if let Some(fact) = join_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "transaction" => {
            if let Some(fact) = transaction_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "update" => {
            if let Some(fact) = update_statement_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "merge_statement" => {
            if let Some(fact) = merge_statement_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        "ERROR" => {
            if let Some(fact) = trigger_definition_from_error_fact(file_path, content, node) {
                facts.push(fact);
            }
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_sql_node(child, file_path, content, facts, child_depth);
    }
}

fn count_column_definitions(node: Node<'_>) -> usize {
    find_child(node, "column_definitions")
        .map(|definitions| count_direct_children(definitions, "column_definition"))
        .unwrap_or(0)
}

fn count_table_constraints(node: Node<'_>) -> usize {
    find_child(node, "column_definitions")
        .map(|definitions| {
            let direct = count_direct_children(definitions, "constraint");
            let mut cursor = definitions.walk();
            direct
                + definitions
                    .children(&mut cursor)
                    .filter(|child| child.kind() == "constraints")
                    .map(|constraints| count_direct_children(constraints, "constraint"))
                    .sum::<usize>()
        })
        .unwrap_or(0)
}

fn table_definition_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let (schema_name, table_name) = object_reference_parts(content, find_object_reference(node)?)?;
    let mut metadata = base_metadata("schema_structure");
    insert_string(&mut metadata, "table_name", &table_name);
    if let Some(schema) = schema_name {
        insert_string(&mut metadata, "schema_name", &schema);
    }
    metadata.insert(
        "column_count".to_string(),
        Value::Number(Number::from(count_column_definitions(node))),
    );
    metadata.insert(
        "constraint_count".to_string(),
        Value::Number(Number::from(count_table_constraints(node))),
    );
    Some(fact_for_node(
        file_path,
        TABLE_DEFINITION_PATTERN_ID,
        "create_table",
        node,
        metadata,
    ))
}

fn view_definition_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let (schema_name, view_name) = object_reference_parts(content, find_object_reference(node)?)?;
    let source_tables = find_child(node, "create_query")
        .map(|query| collect_source_tables_in_node(query, content))
        .unwrap_or_default();
    let mut metadata = base_metadata("schema_structure");
    insert_string(&mut metadata, "view_name", &view_name);
    if let Some(schema) = schema_name {
        insert_string(&mut metadata, "schema_name", &schema);
    }
    metadata.insert(
        "source_table_count".to_string(),
        Value::Number(Number::from(source_tables.len())),
    );
    metadata.insert(
        "source_tables".to_string(),
        Value::Array(
            source_tables
                .into_iter()
                .map(Value::String)
                .collect::<Vec<_>>(),
        ),
    );
    Some(fact_for_node(
        file_path,
        VIEW_DEFINITION_PATTERN_ID,
        "create_view",
        node,
        metadata,
    ))
}

fn trigger_definition_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let (schema_name, trigger_name) =
        object_reference_parts(content, find_object_reference(node)?)?;
    let timing = first_child_kind(node, &["keyword_before", "keyword_after"])
        .map(|kind| kind.strip_prefix("keyword_").unwrap_or(&kind).to_string());
    let event = first_child_kind(
        node,
        &[
            "keyword_insert",
            "keyword_update",
            "keyword_delete",
            "keyword_truncate",
        ],
    )
    .map(|kind| kind.strip_prefix("keyword_").unwrap_or(&kind).to_string());
    let target_table = find_child(node, "keyword_on")
        .and_then(|keyword| find_descendant_after(node, "object_reference", keyword.end_byte(), 0))
        .and_then(|reference| object_reference_parts(content, reference))
        .map(|(_, table)| table);

    let mut metadata = base_metadata("schema_structure");
    insert_string(&mut metadata, "trigger_name", &trigger_name);
    if let Some(schema) = schema_name {
        insert_string(&mut metadata, "schema_name", &schema);
    }
    if let Some(timing) = timing {
        insert_string(&mut metadata, "timing", &timing);
    }
    if let Some(event) = event {
        insert_string(&mut metadata, "event", &event);
    }
    if let Some(table) = target_table {
        insert_string(&mut metadata, "target_table", &table);
    }
    Some(fact_for_node(
        file_path,
        TRIGGER_DEFINITION_PATTERN_ID,
        "create_trigger",
        node,
        metadata,
    ))
}

fn trigger_definition_from_error_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let text = node_text(content, node)?;
    let captures = ERROR_TRIGGER_RE.captures(text)?;
    let trigger_name = captures.get(1)?.as_str().to_string();
    let mut metadata = base_metadata("schema_structure");
    insert_string(&mut metadata, "trigger_name", &trigger_name);
    if let Some(details) = ERROR_TRIGGER_DETAILS_RE.captures(text) {
        if let Some(timing) = details.get(1) {
            insert_string(&mut metadata, "timing", timing.as_str());
        }
        if let Some(event) = details.get(2) {
            insert_string(&mut metadata, "event", event.as_str());
        }
        if let Some(table) = details.get(3) {
            insert_string(&mut metadata, "target_table", table.as_str());
        }
    }
    Some(fact_for_node(
        file_path,
        TRIGGER_DEFINITION_PATTERN_ID,
        "error_trigger",
        node,
        metadata,
    ))
}

fn index_definition_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let index_name = node
        .child_by_field_name("name")
        .or_else(|| find_child(node, "identifier"))
        .and_then(|name_node| node_text(content, name_node).map(normalize_sql_identifier))?;
    let table_name = find_child(node, "object_reference")
        .and_then(|reference| object_reference_parts(content, reference))
        .map(|(_, table)| table);
    let mut metadata = base_metadata("schema_structure");
    insert_string(&mut metadata, "index_name", &index_name);
    if let Some(table) = table_name {
        insert_string(&mut metadata, "table_name", &table);
    }
    metadata.insert(
        "unique".to_string(),
        Value::Bool(has_child_kind(node, "keyword_unique")),
    );
    Some(fact_for_node(
        file_path,
        INDEX_DEFINITION_PATTERN_ID,
        "create_index",
        node,
        metadata,
    ))
}

fn column_definition_fact(
    file_path: &str,
    content: &str,
    node: Node<'_>,
) -> Option<StructuralFact> {
    let column_name = node
        .child_by_field_name("name")
        .or_else(|| find_child(node, "identifier"))
        .and_then(|name_node| node_text(content, name_node).map(normalize_sql_identifier))?;
    let type_name = column_type_name(content, node);
    let table_name = ancestor_of_kind(node, "create_table")
        .and_then(find_object_reference)
        .and_then(|reference| object_reference_parts(content, reference))
        .map(|(_, table)| table);

    let mut metadata = base_metadata("schema_structure");
    insert_string(&mut metadata, "column_name", &column_name);
    if let Some(type_name) = type_name {
        insert_string(&mut metadata, "type_name", &type_name);
    }
    if let Some(table) = table_name {
        insert_string(&mut metadata, "table_name", &table);
    }
    metadata.insert(
        "nullable".to_string(),
        Value::Bool(!column_is_not_null(node)),
    );
    metadata.insert(
        "has_default".to_string(),
        Value::Bool(has_child_kind(node, "keyword_default")),
    );
    Some(fact_for_node(
        file_path,
        COLUMN_DEFINITION_PATTERN_ID,
        "column_definition",
        node,
        metadata,
    ))
}

fn constraint_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let constraint_type = constraint_type_name(node)?;
    let constraint_name = node
        .child_by_field_name("name")
        .or_else(|| find_child(node, "identifier"))
        .and_then(|name_node| node_text(content, name_node).map(normalize_sql_identifier));
    let table_name = ancestor_of_kind(node, "create_table")
        .and_then(find_object_reference)
        .and_then(|reference| object_reference_parts(content, reference))
        .map(|(_, table)| table);
    let column_names = constraint_column_names(content, node);

    let mut metadata = base_metadata("schema_structure");
    insert_string(&mut metadata, "constraint_type", constraint_type);
    if let Some(name) = constraint_name {
        insert_string(&mut metadata, "constraint_name", &name);
    }
    if let Some(table) = table_name {
        insert_string(&mut metadata, "table_name", &table);
    }
    if !column_names.is_empty() {
        metadata.insert(
            "column_names".to_string(),
            Value::Array(column_names.into_iter().map(Value::String).collect()),
        );
    }
    Some(fact_for_node(
        file_path,
        CONSTRAINT_PATTERN_ID,
        "constraint",
        node,
        metadata,
    ))
}

fn foreign_key_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let table_name = ancestor_of_kind(node, "create_table")
        .and_then(find_object_reference)
        .and_then(|reference| object_reference_parts(content, reference))
        .map(|(_, table)| table);
    let column_names = constraint_column_names(content, node);
    let references = find_child(node, "object_reference")
        .or_else(|| {
            find_child(node, "keyword_references")
                .and_then(|_| find_descendant(node, "object_reference"))
        })
        .and_then(|reference| object_reference_parts(content, reference))?;
    let referenced_columns = referenced_column_names(content, node);

    let mut metadata = base_metadata("schema_structure");
    if let Some(table) = table_name {
        insert_string(&mut metadata, "table_name", &table);
    }
    if !column_names.is_empty() {
        metadata.insert(
            "column_names".to_string(),
            Value::Array(column_names.into_iter().map(Value::String).collect()),
        );
    }
    insert_string(&mut metadata, "referenced_table", &references.1);
    if let Some(schema) = references.0 {
        insert_string(&mut metadata, "referenced_schema", &schema);
    }
    if !referenced_columns.is_empty() {
        metadata.insert(
            "referenced_columns".to_string(),
            Value::Array(referenced_columns.into_iter().map(Value::String).collect()),
        );
    }
    Some(fact_for_node(
        file_path,
        FOREIGN_KEY_PATTERN_ID,
        "foreign_key",
        node,
        metadata,
    ))
}

fn select_query_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let from_node = query_from_node(node);
    let flags = select_clause_flags(content, node.start_byte());
    let projection_count = count_select_projections(node);
    let source_count = from_node
        .map(|from| count_direct_children(from, "relation") + count_direct_children(from, "join"))
        .unwrap_or(0);

    let mut metadata = base_metadata("query_structure");
    metadata.insert(
        "projection_count".to_string(),
        Value::Number(Number::from(projection_count)),
    );
    metadata.insert(
        "source_count".to_string(),
        Value::Number(Number::from(source_count)),
    );
    metadata.insert("has_where".to_string(), Value::Bool(flags.has_where));
    metadata.insert("has_group_by".to_string(), Value::Bool(flags.has_group_by));
    metadata.insert("has_order_by".to_string(), Value::Bool(flags.has_order_by));
    Some(fact_for_node(
        file_path,
        SELECT_QUERY_PATTERN_ID,
        "select",
        node,
        metadata,
    ))
}

#[derive(Default)]
struct SelectClauseFlags {
    has_where: bool,
    has_group_by: bool,
    has_order_by: bool,
}

fn select_clause_flags(content: &str, select_start: usize) -> SelectClauseFlags {
    let base_depth = paren_depth_before(content, select_start);
    let mut flags = SelectClauseFlags::default();
    let mut depth = base_depth;
    let mut cursor = select_start;
    while cursor < content.len() {
        let Some(ch) = content[cursor..].chars().next() else {
            break;
        };
        match ch {
            '\'' | '"' => {
                cursor = skip_sql_string(content, cursor, ch);
                continue;
            }
            '(' => depth += 1,
            ')' => {
                if depth == base_depth {
                    break;
                }
                depth = depth.saturating_sub(1);
            }
            ';' if depth == base_depth => break,
            _ => {}
        }
        if depth == base_depth {
            if sql_keyword_at(content, cursor, "where") {
                flags.has_where = true;
            } else if sql_keyword_at(content, cursor, "group") {
                flags.has_group_by = true;
            } else if sql_keyword_at(content, cursor, "order") {
                flags.has_order_by = true;
            }
        }
        cursor += ch.len_utf8();
    }
    flags
}

fn paren_depth_before(content: &str, end: usize) -> usize {
    let mut depth = 0usize;
    let mut cursor = 0usize;
    while cursor < end {
        let Some(ch) = content[cursor..].chars().next() else {
            break;
        };
        match ch {
            '\'' | '"' => {
                cursor = skip_sql_string(content, cursor, ch);
                continue;
            }
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        cursor += ch.len_utf8();
    }
    depth
}

fn skip_sql_string(content: &str, start: usize, quote: char) -> usize {
    let mut cursor = start + quote.len_utf8();
    while cursor < content.len() {
        let Some(ch) = content[cursor..].chars().next() else {
            break;
        };
        cursor += ch.len_utf8();
        if ch == quote {
            if content[cursor..].starts_with(quote) {
                cursor += quote.len_utf8();
                continue;
            }
            break;
        }
    }
    cursor
}

fn sql_keyword_at(content: &str, start: usize, keyword: &str) -> bool {
    let Some(candidate) = content.get(start..start + keyword.len()) else {
        return false;
    };
    candidate.eq_ignore_ascii_case(keyword)
        && content[..start]
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
        && content[start + keyword.len()..]
            .chars()
            .next()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
}

fn cte_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let cte_name = node
        .child_by_field_name("argument")
        .or_else(|| find_child(node, "identifier"))
        .and_then(|name_node| node_text(content, name_node).map(normalize_sql_identifier))?;
    let recursive = query_clause_container(node)
        .and_then(|container| node_text(content, container))
        .is_some_and(|text| text.to_ascii_uppercase().contains("RECURSIVE"));

    let mut metadata = base_metadata("query_structure");
    insert_string(&mut metadata, "cte_name", &cte_name);
    metadata.insert("recursive".to_string(), Value::Bool(recursive));
    Some(fact_for_node(
        file_path,
        CTE_PATTERN_ID,
        "cte",
        node,
        metadata,
    ))
}

fn join_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let join_type = join_type_name(node);
    let left_table = join_left_table(content, node);
    let right_table =
        find_child(node, "relation").and_then(|relation| relation_table_name(content, relation));

    let mut metadata = base_metadata("query_structure");
    insert_string(&mut metadata, "join_type", &join_type);
    if let Some(left) = left_table {
        insert_string(&mut metadata, "left_table", &left);
    }
    if let Some(right) = right_table {
        insert_string(&mut metadata, "right_table", &right);
    }
    Some(fact_for_node(
        file_path,
        JOIN_PATTERN_ID,
        "join",
        node,
        metadata,
    ))
}

fn transaction_fact(file_path: &str, _content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let transaction_kind = if has_child_kind(node, "keyword_begin") {
        "begin"
    } else if has_child_kind(node, "keyword_commit") {
        "commit"
    } else if has_child_kind(node, "keyword_rollback") {
        "rollback"
    } else {
        "transaction"
    };

    let mut metadata = base_metadata("transaction_structure");
    insert_string(&mut metadata, "transaction_kind", transaction_kind);
    Some(fact_for_node(
        file_path,
        TRANSACTION_PATTERN_ID,
        "transaction",
        node,
        metadata,
    ))
}

fn update_statement_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let table_name = find_child(node, "relation")
        .or_else(|| find_child(node, "object_reference"))
        .and_then(|relation| relation_table_name(content, relation));
    let mut metadata = base_metadata("mutation_structure");
    if let Some(table) = table_name {
        insert_string(&mut metadata, "table_name", &table);
    }
    metadata.insert(
        "has_where".to_string(),
        Value::Bool(has_child_kind(node, "where")),
    );
    Some(fact_for_node(
        file_path,
        UPDATE_STATEMENT_PATTERN_ID,
        "update",
        node,
        metadata,
    ))
}

fn merge_statement_fact(file_path: &str, content: &str, node: Node<'_>) -> Option<StructuralFact> {
    let target_table = node
        .child_by_field_name("target")
        .and_then(|target| object_reference_parts(content, target))
        .map(|(_, table)| table)?;
    let source = node.child_by_field_name("source")?;
    if source.kind() != "merge_values_source" {
        return None;
    }

    let mut has_when_matched = false;
    let mut has_when_not_matched = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() != "when_clause" || !has_child_kind(child, "keyword_matched") {
            continue;
        }
        if has_child_kind(child, "keyword_not") {
            has_when_not_matched = true;
        } else {
            has_when_matched = true;
        }
    }

    let mut metadata = base_metadata("mutation_structure");
    insert_string(&mut metadata, "target_table", &target_table);
    insert_string(&mut metadata, "source_kind", "values");
    metadata.insert(
        "has_when_matched".to_string(),
        Value::Bool(has_when_matched),
    );
    metadata.insert(
        "has_when_not_matched".to_string(),
        Value::Bool(has_when_not_matched),
    );
    Some(fact_for_node(
        file_path,
        MERGE_STATEMENT_PATTERN_ID,
        "merge",
        node,
        metadata,
    ))
}

fn constraint_type_name(node: Node<'_>) -> Option<&'static str> {
    if has_child_kind(node, "keyword_primary") {
        Some("primary_key")
    } else if has_child_kind(node, "keyword_foreign") {
        Some("foreign_key")
    } else if has_child_kind(node, "keyword_unique") {
        Some("unique")
    } else if has_child_kind(node, "keyword_check") {
        Some("check")
    } else if has_child_kind(node, "keyword_index") {
        Some("index")
    } else {
        None
    }
}

fn join_type_name(node: Node<'_>) -> String {
    for kind in [
        "keyword_inner",
        "keyword_left",
        "keyword_right",
        "keyword_full",
        "keyword_cross",
    ] {
        if has_child_kind(node, kind) {
            return kind.strip_prefix("keyword_").unwrap_or(kind).to_string();
        }
    }
    "inner".to_string()
}

fn column_is_not_null(node: Node<'_>) -> bool {
    has_child_kind(node, "not_null")
        || has_child_kind(node, "not_null_constraint")
        || (has_child_kind(node, "keyword_not") && has_child_kind(node, "keyword_null"))
        || has_child_kind(node, "keyword_primary")
        || has_child_kind(node, "primary_key")
}

fn column_type_name(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("type")
        .or_else(|| find_child(node, "data_type"))
        .or_else(|| {
            find_child(node, "identifier").filter(|&first| {
                node.child_by_field_name("name")
                    .is_some_and(|name| first != name)
            })
        })
        .and_then(|type_node| {
            node_text(content, type_node)
                .map(str::trim)
                .map(str::to_string)
        })
        .or_else(|| {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if matches!(
                    child.kind(),
                    "int"
                        | "bigint"
                        | "text"
                        | "varchar"
                        | "boolean"
                        | "decimal"
                        | "keyword_int"
                        | "keyword_text"
                        | "keyword_varchar"
                        | "keyword_boolean"
                ) {
                    return node_text(content, child).map(str::trim).map(str::to_string);
                }
            }
            None
        })
}

fn constraint_column_names(content: &str, node: Node<'_>) -> Vec<String> {
    if let Some(columns) = find_child(node, "ordered_columns") {
        return collect_ordered_column_names(content, columns);
    }
    find_child(node, "identifier_list")
        .map(|list| collect_identifier_names(content, list))
        .unwrap_or_default()
}

fn collect_ordered_column_names(content: &str, node: Node<'_>) -> Vec<String> {
    let mut names = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "column"
            && let Some(identifier) = find_child(child, "identifier")
            && let Some(name) = node_text(content, identifier)
        {
            names.push(normalize_sql_identifier(name));
        }
    }
    names
}

fn referenced_column_names(content: &str, node: Node<'_>) -> Vec<String> {
    let mut names = Vec::new();
    let mut after_reference = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "keyword_references" => after_reference = true,
            "object_reference" if after_reference => {}
            "identifier" if after_reference => {
                if let Some(name) = node_text(content, child) {
                    names.push(normalize_sql_identifier(name));
                }
            }
            _ => {}
        }
    }
    names
}

fn collect_identifier_names(content: &str, node: Node<'_>) -> Vec<String> {
    if node.kind() == "identifier" {
        return node_text(content, node)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(normalize_sql_identifier)
            .into_iter()
            .collect();
    }

    let mut names = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier"
            && let Some(name) = node_text(content, child)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            names.push(normalize_sql_identifier(name));
        }
    }
    names
}

fn collect_source_tables_in_node(node: Node<'_>, content: &str) -> Vec<String> {
    let mut tables = Vec::new();
    collect_relation_names(node, content, &mut tables, 0);
    tables.sort();
    tables.dedup();
    tables
}

fn collect_relation_names(node: Node<'_>, content: &str, tables: &mut Vec<String>, depth: u32) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if (node.kind() == "relation" || node.kind() == "object_reference")
        && let Some(name) = relation_table_name(content, node)
    {
        tables.push(name);
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_relation_names(child, content, tables, child_depth);
    }
}

fn relation_table_name(content: &str, node: Node<'_>) -> Option<String> {
    if node.kind() == "relation" {
        return find_child(node, "object_reference")
            .and_then(|reference| object_reference_parts(content, reference))
            .map(|(_, table)| table);
    }
    object_reference_parts(content, node).map(|(_, table)| table)
}

fn object_reference_parts(content: &str, node: Node<'_>) -> Option<(Option<String>, String)> {
    if let Some(schema_node) = node.child_by_field_name("schema") {
        let schema = normalize_sql_identifier(node_text(content, schema_node)?);
        let name_node = node
            .child_by_field_name("name")
            .or_else(|| find_child(node, "identifier"))?;
        let name = normalize_sql_identifier(node_text(content, name_node)?);
        return Some((Some(schema), name));
    }

    let identifiers = collect_identifier_names(content, node);
    match identifiers.as_slice() {
        [name] => Some((None, name.clone())),
        [schema, name] => Some((Some(schema.clone()), name.clone())),
        _ => node_text(content, node).map(|text| (None, normalize_sql_identifier(text))),
    }
}

fn count_select_projections(node: Node<'_>) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "select_expression" {
            count += count_select_expression_projections(child);
        }
    }
    count
}

fn count_select_expression_projections(node: Node<'_>) -> usize {
    let terms = count_direct_children(node, "term");
    let all_fields = count_direct_children(node, "all_fields");
    terms.max(all_fields).max(1)
}

fn query_from_node(select_node: Node<'_>) -> Option<Node<'_>> {
    let mut node = select_node;
    while let Some(parent) = node.parent() {
        if let Some(from) = find_descendant_after(parent, "from", select_node.end_byte(), 0) {
            return Some(from);
        }
        if matches!(parent.kind(), "statement" | "create_query") {
            break;
        }
        node = parent;
    }
    None
}

fn query_clause_container(mut node: Node<'_>) -> Option<Node<'_>> {
    loop {
        match node.kind() {
            "statement" | "create_query" => return Some(node),
            _ => node = node.parent()?,
        }
    }
}

fn enclosing_from_node(mut node: Node<'_>) -> Option<Node<'_>> {
    while let Some(parent) = node.parent() {
        if parent.kind() == "from" {
            return Some(parent);
        }
        node = parent;
    }
    None
}

fn join_left_table(content: &str, join_node: Node<'_>) -> Option<String> {
    let from = enclosing_from_node(join_node)?;
    let mut previous_table =
        find_child(from, "relation").and_then(|relation| relation_table_name(content, relation));
    let mut cursor = from.walk();
    for child in from.children(&mut cursor) {
        if same_node(child, join_node) {
            return previous_table;
        }
        match child.kind() {
            "relation" => {
                previous_table = relation_table_name(content, child);
            }
            "join" => {
                previous_table = find_child(child, "relation")
                    .and_then(|relation| relation_table_name(content, relation));
            }
            _ => {}
        }
    }
    previous_table
}

fn same_node(left: Node<'_>, right: Node<'_>) -> bool {
    left.start_byte() == right.start_byte() && left.end_byte() == right.end_byte()
}

fn ancestor_of_kind<'a>(mut node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    while let Some(parent) = node.parent() {
        if parent.kind() == kind {
            return Some(parent);
        }
        node = parent;
    }
    None
}

fn find_object_reference(node: Node<'_>) -> Option<Node<'_>> {
    find_child(node, "object_reference")
}

fn find_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|&child| child.kind() == kind)
}

fn find_descendant<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    find_descendant_at_depth(node, kind, 0)
}

fn find_descendant_at_depth<'a>(node: Node<'a>, kind: &str, depth: u32) -> Option<Node<'a>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    if node.kind() == kind {
        return Some(node);
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_descendant_at_depth(child, kind, child_depth) {
            return Some(found);
        }
    }
    None
}

fn find_descendant_after<'a>(
    node: Node<'a>,
    kind: &str,
    after_byte: usize,
    depth: u32,
) -> Option<Node<'a>> {
    if !should_visit_tree_depth(depth) {
        return None;
    }
    let child_depth = child_tree_depth(depth)?;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.end_byte() <= after_byte {
            continue;
        }
        if child.kind() == kind && child.start_byte() >= after_byte {
            return Some(child);
        }
        if let Some(found) = find_descendant_after(child, kind, after_byte, child_depth) {
            return Some(found);
        }
    }
    None
}

fn first_child_kind(node: Node<'_>, kinds: &[&str]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if kinds.contains(&child.kind()) {
            return Some(child.kind().to_string());
        }
    }
    None
}

fn has_child_kind(node: Node<'_>, child_kind: &str) -> bool {
    has_child_kind_at_depth(node, child_kind, 0)
}

fn has_child_kind_at_depth(node: Node<'_>, child_kind: &str, depth: u32) -> bool {
    if !should_visit_tree_depth(depth) {
        return false;
    }
    if node.kind() == child_kind {
        return true;
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return false;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if has_child_kind_at_depth(child, child_kind, child_depth) {
            return true;
        }
    }
    false
}

fn count_direct_children(node: Node<'_>, child_kind: &str) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.kind() == child_kind)
        .count()
}

fn node_text<'a>(content: &'a str, node: Node<'_>) -> Option<&'a str> {
    content.get(node.start_byte()..node.end_byte())
}

fn fact_for_node(
    file_path: &str,
    pattern_id: &str,
    capture_name: &str,
    node: Node<'_>,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    fact_for_span(
        file_path,
        pattern_id,
        capture_name,
        node.kind(),
        NormalizedSpan::from_node(&node),
        metadata,
    )
}

fn fact_for_span(
    file_path: &str,
    pattern_id: &str,
    capture_name: &str,
    node_kind: &str,
    span: NormalizedSpan,
    metadata: HashMap<String, Value>,
) -> StructuralFact {
    StructuralFact {
        id: stable_location_id(file_path, &format!("{pattern_id}:{capture_name}"), span),
        file_path: file_path.to_string(),
        language: "sql".to_string(),
        pattern_id: pattern_id.to_string(),
        capture_name: capture_name.to_string(),
        node_kind: node_kind.to_string(),
        containing_symbol_id: None,
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        confidence: 1.0,
        metadata: Some(metadata),
    }
}

fn base_metadata(query_family: &str) -> HashMap<String, Value> {
    HashMap::from([
        (
            "pattern_version".to_string(),
            Value::Number(Number::from(1)),
        ),
        (
            "query_family".to_string(),
            Value::String(query_family.to_string()),
        ),
    ])
}

fn insert_string(metadata: &mut HashMap<String, Value>, key: &str, value: &str) {
    metadata.insert(key.to_string(), Value::String(value.to_string()));
}
