use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.sql", source, Path::new("/repo"))
        .expect("canonical SQL extraction should succeed")
}

fn facts_with_pattern<'a>(
    results: &'a crate::ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .collect()
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

fn metadata_u64(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_u64())
}

fn metadata_bool(fact: &StructuralFact, key: &str) -> Option<bool> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
}

fn metadata_string_array(fact: &StructuralFact, key: &str) -> Option<Vec<String>> {
    fact.metadata.as_ref().and_then(|metadata| {
        metadata.get(key).and_then(|value| {
            value.as_array().map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
        })
    })
}

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/sql/basic/source.sql");

#[test]
fn sql_emits_expected_structural_fact_patterns() {
    let results = extract(FIXTURE_SOURCE);
    let pattern_ids = results
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .collect::<BTreeSet<_>>();

    for pattern_id in [
        "sql.table_definition.v1",
        "sql.column_definition.v1",
        "sql.constraint.v1",
        "sql.foreign_key.v1",
        "sql.view_definition.v1",
        "sql.trigger_definition.v1",
        "sql.index_definition.v1",
        "sql.select_query.v1",
        "sql.cte.v1",
        "sql.join.v1",
        "sql.transaction.v1",
        "sql.update_statement.v1",
    ] {
        assert!(
            pattern_ids.contains(pattern_id),
            "expected pattern `{pattern_id}` to be emitted"
        );
    }
}

#[test]
fn sql_table_column_and_foreign_key_metadata_are_correct() {
    let results = extract(FIXTURE_SOURCE);

    let workers = facts_with_pattern(&results, "sql.table_definition.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "table_name") == Some("workers"))
        .expect("expected workers table fact");
    assert_eq!(metadata_u64(workers, "column_count"), Some(2));
    assert_eq!(
        metadata_str(workers, "query_family"),
        Some("schema_structure")
    );
    assert!(workers.start_byte < workers.end_byte);

    let name_column = facts_with_pattern(&results, "sql.column_definition.v1")
        .into_iter()
        .find(|fact| {
            metadata_str(fact, "column_name") == Some("name")
                && metadata_str(fact, "table_name") == Some("workers")
        })
        .expect("expected workers.name column fact");
    assert_eq!(metadata_str(name_column, "type_name"), Some("TEXT"));
    assert_eq!(metadata_bool(name_column, "nullable"), Some(false));
    assert_eq!(metadata_bool(name_column, "has_default"), Some(true));

    let foreign_key = facts_with_pattern(&results, "sql.foreign_key.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "table_name") == Some("jobs"))
        .expect("expected jobs foreign key fact");
    assert_eq!(
        metadata_str(foreign_key, "referenced_table"),
        Some("workers")
    );
    assert_eq!(
        foreign_key
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("column_names"))
            .and_then(|value| value.as_array())
            .map(|columns| columns.len()),
        Some(1)
    );

    let check_constraint = facts_with_pattern(&results, "sql.constraint.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "constraint_name") == Some("chk_worker_id_positive"))
        .expect("expected jobs check constraint fact");
    assert_eq!(
        metadata_str(check_constraint, "constraint_type"),
        Some("check")
    );
    assert_eq!(metadata_str(check_constraint, "table_name"), Some("jobs"));
}

#[test]
fn sql_view_trigger_query_and_transaction_metadata_are_correct() {
    let results = extract(FIXTURE_SOURCE);

    let view = facts_with_pattern(&results, "sql.view_definition.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "view_name") == Some("active_workers"))
        .expect("expected view fact");
    assert_eq!(metadata_u64(view, "source_table_count"), Some(1));
    assert_eq!(
        metadata_string_array(view, "source_tables"),
        Some(vec!["workers".to_string()])
    );

    let view_select = facts_with_pattern(&results, "sql.select_query.v1")
        .into_iter()
        .find(|fact| {
            fact.start_byte >= view.start_byte
                && fact.end_byte <= view.end_byte
                && metadata_u64(fact, "projection_count") == Some(2)
        })
        .expect("expected view body select fact");
    assert_eq!(metadata_u64(view_select, "projection_count"), Some(2));
    assert_eq!(metadata_u64(view_select, "source_count"), Some(1));
    assert_eq!(metadata_bool(view_select, "has_where"), Some(true));

    let trigger = facts_with_pattern(&results, "sql.trigger_definition.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "trigger_name") == Some("refresh_active_workers"))
        .expect("expected trigger fact");
    assert_eq!(metadata_str(trigger, "timing"), Some("AFTER"));
    assert_eq!(metadata_str(trigger, "event"), Some("INSERT"));
    assert_eq!(metadata_str(trigger, "target_table"), Some("workers"));

    let cte = facts_with_pattern(&results, "sql.cte.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "cte_name") == Some("recent_workers"))
        .expect("expected cte fact");
    assert_eq!(metadata_bool(cte, "recursive"), Some(false));

    let join = facts_with_pattern(&results, "sql.join.v1")
        .into_iter()
        .next()
        .expect("expected join fact");
    assert_eq!(metadata_str(join, "right_table"), Some("workers"));
    assert!(join.start_byte < join.end_byte);

    let outer_select = facts_with_pattern(&results, "sql.select_query.v1")
        .into_iter()
        .find(|fact| metadata_u64(fact, "source_count") == Some(2))
        .expect("expected outer select with join sources");
    assert_eq!(metadata_u64(outer_select, "projection_count"), Some(2));
    assert_eq!(metadata_bool(outer_select, "has_where"), Some(false));

    let transaction = facts_with_pattern(&results, "sql.transaction.v1")
        .into_iter()
        .next()
        .expect("expected transaction fact");
    assert_eq!(metadata_str(transaction, "transaction_kind"), Some("begin"));

    assert!(
        results
            .structural_facts
            .iter()
            .all(|fact| fact.end_byte > fact.start_byte),
        "structural facts must have non-empty spans"
    );
}

#[test]
fn sql_local_counts_are_direct_not_recursive() {
    let source = r#"
CREATE TABLE outer_table (
    id INTEGER PRIMARY KEY,
    nested INTEGER
);

WITH inner_cte AS (
    SELECT id FROM outer_table
)
SELECT id FROM inner_cte;
"#;

    let results = extract(source);
    let table = facts_with_pattern(&results, "sql.table_definition.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "table_name") == Some("outer_table"))
        .expect("expected outer_table fact");
    assert_eq!(metadata_u64(table, "column_count"), Some(2));

    let cte = facts_with_pattern(&results, "sql.cte.v1")
        .into_iter()
        .next()
        .expect("expected cte fact");
    let inner_select = facts_with_pattern(&results, "sql.select_query.v1")
        .into_iter()
        .find(|fact| fact.start_byte >= cte.start_byte && fact.end_byte <= cte.end_byte)
        .expect("expected inner cte select");
    assert_eq!(metadata_u64(inner_select, "source_count"), Some(1));
    assert_eq!(metadata_u64(inner_select, "projection_count"), Some(1));
}

#[test]
fn sql_integer_primary_key_columns_are_not_nullable() {
    let source = r#"
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    email TEXT
);
"#;
    let results = extract(source);
    let id = facts_with_pattern(&results, "sql.column_definition.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "column_name") == Some("id"))
        .expect("id column");
    assert_eq!(metadata_str(id, "type_name"), Some("INTEGER"));
    assert_eq!(metadata_bool(id, "nullable"), Some(false));
}

#[test]
fn sql_subquery_select_counts_are_local_to_the_subquery() {
    let source = r#"
SELECT *
FROM (
    SELECT id
    FROM users
    WHERE active = 1
) active_users
JOIN profiles ON profiles.user_id = active_users.id
ORDER BY profiles.created_at;
"#;
    let results = extract(source);
    let selects = facts_with_pattern(&results, "sql.select_query.v1");
    let subquery = selects
        .iter()
        .copied()
        .find(|fact| {
            metadata_bool(fact, "has_where") == Some(true)
                && metadata_bool(fact, "has_order_by") == Some(false)
        })
        .expect("inner subquery select");
    assert_eq!(metadata_u64(subquery, "source_count"), Some(1));
    assert_eq!(metadata_bool(subquery, "has_where"), Some(true));
    assert_eq!(metadata_bool(subquery, "has_order_by"), Some(false));

    let outer = selects
        .iter()
        .copied()
        .find(|fact| metadata_bool(fact, "has_order_by") == Some(true))
        .expect("outer select");
    assert_eq!(metadata_u64(outer, "source_count"), Some(2));
    assert_eq!(metadata_bool(outer, "has_where"), Some(false));
}

#[test]
fn sql_chained_joins_record_adjacent_left_and_right_tables() {
    let source = r#"
SELECT *
FROM users
JOIN profiles ON profiles.user_id = users.id
JOIN teams ON teams.id = profiles.team_id;
"#;
    let results = extract(source);
    let joins = facts_with_pattern(&results, "sql.join.v1");
    assert_eq!(joins.len(), 2, "{joins:#?}");
    let endpoints = joins
        .iter()
        .map(|fact| {
            (
                metadata_str(fact, "left_table"),
                metadata_str(fact, "right_table"),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        endpoints,
        vec![
            (Some("users"), Some("profiles")),
            (Some("profiles"), Some("teams")),
        ]
    );
}

#[test]
fn sql_with_recursive_detection_is_case_insensitive() {
    let source = r#"
with recursive nums(n) as (
    select 1
)
select n from nums;
"#;
    let results = extract(source);
    let cte = facts_with_pattern(&results, "sql.cte.v1")
        .into_iter()
        .next()
        .expect("cte fact");
    assert_eq!(metadata_bool(cte, "recursive"), Some(true));
}

#[test]
fn sql_tsql_ddl_facts_normalize_names_and_select_trigger_target_after_on() {
    let source = r#"
CREATE TABLE [edr].[Items] (
    [Id] INT,
    CONSTRAINT [PK_Items] PRIMARY KEY ([Id])
);

CREATE TRIGGER [edr].[TR_Items]
    AFTER INSERT ON [edr].[Items]
    FOR EACH ROW
    EXECUTE FUNCTION [edr].[log_item]();
"#;
    let results = extract(source);

    let table = facts_with_pattern(&results, "sql.table_definition.v1")
        .into_iter()
        .next()
        .expect("normalized table fact");
    assert_eq!(metadata_str(table, "schema_name"), Some("edr"));
    assert_eq!(metadata_str(table, "table_name"), Some("Items"));

    let column = facts_with_pattern(&results, "sql.column_definition.v1")
        .into_iter()
        .next()
        .expect("normalized column fact");
    assert_eq!(metadata_str(column, "table_name"), Some("Items"));
    assert_eq!(metadata_str(column, "column_name"), Some("Id"));

    let constraint = facts_with_pattern(&results, "sql.constraint.v1")
        .into_iter()
        .next()
        .expect("normalized constraint fact");
    assert_eq!(
        metadata_str(constraint, "constraint_name"),
        Some("PK_Items")
    );
    assert_eq!(
        metadata_string_array(constraint, "column_names"),
        Some(vec!["Id".to_string()])
    );

    let trigger = facts_with_pattern(&results, "sql.trigger_definition.v1")
        .into_iter()
        .next()
        .expect("normalized trigger fact");
    assert_eq!(metadata_str(trigger, "schema_name"), Some("edr"));
    assert_eq!(metadata_str(trigger, "trigger_name"), Some("TR_Items"));
    assert_eq!(metadata_str(trigger, "target_table"), Some("Items"));
}

#[test]
fn sql_tsql_merge_emits_registered_values_source_fact() {
    let results = extract(
        r#"MERGE [dbo].[Seed] AS t
USING (VALUES (N'alpha', N'Reader')) AS s (Area, Role)
ON t.Area = s.Area AND t.Role = s.Role
WHEN NOT MATCHED THEN INSERT (Area, Role) VALUES (s.Area, s.Role);"#,
    );
    let merge = facts_with_pattern(&results, "sql.merge_statement.v1")
        .into_iter()
        .next()
        .expect("T-SQL MERGE fact");

    assert_eq!(merge.capture_name, "merge");
    assert_eq!(merge.node_kind, "merge_statement");
    assert_eq!(
        metadata_str(merge, "query_family"),
        Some("mutation_structure")
    );
    assert_eq!(metadata_str(merge, "target_table"), Some("Seed"));
    assert_eq!(metadata_str(merge, "source_kind"), Some("values"));
    assert_eq!(metadata_bool(merge, "has_when_matched"), Some(false));
    assert_eq!(metadata_bool(merge, "has_when_not_matched"), Some(true));
    assert!(
        merge
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("source_table"))
            .is_none()
    );
}

#[test]
fn sql_tsql_control_flow_emits_no_structural_facts() {
    let results = extract(include_str!(
        "../../../../../fixtures/extraction/sql/tsql_batch_control/source.sql"
    ));

    assert!(results.parse_diagnostics.is_empty());
    assert!(results.structural_facts.iter().all(|fact| {
        !matches!(
            fact.node_kind.as_str(),
            "go_statement"
                | "set_statement"
                | "if_statement"
                | "begin_end_block"
                | "declare_statement"
                | "throw_statement"
        )
    }));
}
