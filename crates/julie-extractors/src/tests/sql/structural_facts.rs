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
