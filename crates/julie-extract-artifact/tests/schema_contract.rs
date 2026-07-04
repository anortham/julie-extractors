use std::collections::{BTreeMap, BTreeSet};

use julie_extract_artifact::metadata::{
    ArtifactMetadata, REQUIRED_METADATA_KEYS, initialize_metadata, read_metadata,
};
use julie_extract_artifact::reports::SQLITE_ROW_DOMAINS;
use julie_extract_artifact::schema::{SQLITE_SCHEMA_VERSION, create_schema};
use julie_extract_artifact::writer::ArtifactWriter;
use rusqlite::Connection;

#[test]
fn schema_creates_every_sqlite_v3_public_table_with_contract_columns() {
    let conn = open_schema();

    let table_names: BTreeSet<_> = conn
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    for table in expected_tables() {
        assert!(
            table_names.contains(table.name),
            "missing public table {}. Got {table_names:?}",
            table.name
        );
        assert_eq!(
            table_columns(&conn, table.name),
            table.columns,
            "{} columns drifted from sqlite-schema-v3.md",
            table.name
        );
    }

    for forbidden in [
        "search_index",
        "embeddings",
        "workspace_registry",
        "mcp_tools",
        "watcher_state",
        "reference_scores",
        "test_quality",
    ] {
        assert!(
            !table_names.contains(forbidden),
            "Task5 must not copy old Julie internal table {forbidden}"
        );
    }
}

#[test]
fn schema_creates_required_indexes_with_contract_columns() {
    let conn = open_schema();

    for index in expected_indexes() {
        let table: String = conn
            .query_row(
                "SELECT tbl_name FROM sqlite_master WHERE type = 'index' AND name = ?1",
                [index.name],
                |row| row.get(0),
            )
            .unwrap_or_else(|err| panic!("missing required index {}: {err}", index.name));
        assert_eq!(table, index.table, "{} points at wrong table", index.name);
        assert_eq!(
            index_columns(&conn, index.name),
            index.columns,
            "{} columns drifted from sqlite-schema-v3.md",
            index.name
        );
    }
}

#[test]
fn query_plan_uses_required_lookup_indexes() {
    let conn = open_schema();

    assert_query_uses_index(
        &conn,
        "SELECT symbol_id FROM symbols WHERE name = ?1 AND kind = ?2",
        ["function", "function"],
        "idx_symbols_name_kind",
    );
    assert_query_uses_index(
        &conn,
        "SELECT symbol_id FROM symbols WHERE is_test = ?1",
        ["1"],
        "idx_symbols_is_test",
    );
    assert_query_uses_index(
        &conn,
        "SELECT identifier_id FROM identifiers WHERE name = ?1 AND kind = ?2",
        ["load", "call"],
        "idx_identifiers_name_kind",
    );
    assert_query_uses_index(
        &conn,
        "SELECT pending_relationship_id FROM pending_relationships \
         WHERE target_terminal_name = ?1",
        ["User"],
        "idx_pending_terminal",
    );
    assert_query_uses_index(
        &conn,
        "SELECT complexity_metric_id FROM complexity_metrics \
         WHERE scope = ?1 AND language = ?2",
        ["symbol", "rust"],
        "idx_complexity_metrics_scope_language",
    );
}

#[test]
fn query_plan_uses_required_writer_delete_indexes() {
    let conn = open_schema();

    assert_query_plan_contains(
        &conn,
        "DELETE FROM files WHERE path = ?1",
        ["src/a.rs"],
        &[
            "SEARCH files USING INDEX",
            "path=?",
            "idx_symbols_file",
            "idx_identifiers_file",
            "idx_relationships_file",
            "idx_pending_file",
            "idx_type_argument_usages_file",
            "idx_structural_facts_file_span",
        ],
    );
}

#[test]
fn query_plan_uses_jsonl_export_order_indexes() {
    let conn = open_schema();

    assert_query_uses_index(
        &conn,
        "SELECT source_region_id, file_id, path, language, kind, containing_symbol_id,
                start_line, start_column, end_line, end_column, start_byte, end_byte,
                metadata_json
         FROM source_regions
         ORDER BY path, start_byte, end_byte, kind, source_region_id",
        [],
        "idx_source_regions_export_order",
    );
    assert_query_uses_index(
        &conn,
        "SELECT structural_fact_id, file_id, path, language, pattern_id, capture_name,
                node_kind, containing_symbol_id, start_line, start_column, end_line,
                end_column, start_byte, end_byte, confidence, metadata_json
         FROM structural_facts
         ORDER BY path, start_byte, end_byte, pattern_id, capture_name, structural_fact_id",
        [],
        "idx_structural_facts_export_order",
    );
    assert_query_uses_index(
        &conn,
        "SELECT complexity_metric_id, file_id, path, language, scope, symbol_id, algorithm_id,
                covered_lines, covered_bytes, decision_count, loop_count, max_nesting_depth,
                parameter_count, start_line, start_column, end_line, end_column, start_byte,
                end_byte, metadata_json
         FROM complexity_metrics
         ORDER BY path, start_byte, end_byte, scope, symbol_id, complexity_metric_id",
        [],
        "idx_complexity_metrics_export_order",
    );
}

#[test]
fn metadata_required_keys_are_inserted_and_readable() {
    let conn = open_schema();
    let metadata = sample_metadata();

    initialize_metadata(&conn, &metadata).unwrap();
    let rows = read_metadata(&conn).unwrap();

    for key in REQUIRED_METADATA_KEYS {
        assert!(rows.contains_key(*key), "missing metadata key {key}");
    }
    assert_eq!(rows["artifact_id"], "artifact-test-1");
    assert_eq!(rows["root_path"], "/repo");
    assert_eq!(rows["schema_version"], "3");
    assert_eq!(rows["extract_contract_version"], "3");
    assert_eq!(
        rows["sqlite_schema_version"],
        SQLITE_SCHEMA_VERSION.to_string()
    );
    assert_eq!(rows["binary_version"], "julie-extract 0.1.0");
    assert_eq!(rows["hash_algorithm"], "blake3");
    assert_eq!(rows["parser_inventory_fingerprint"], "sha256:parser");
    assert_eq!(rows["capability_snapshot_fingerprint"], "sha256:cap");
    assert_eq!(rows["created_at"], "2026-05-31T17:50:00Z");
    assert_eq!(rows["updated_at"], "2026-05-31T17:50:00Z");
}

#[test]
fn writer_initializes_schema_metadata_and_foreign_key_enforcement() {
    let writer = ArtifactWriter::open_in_memory(sample_metadata()).unwrap();
    let conn = writer.connection();

    assert_eq!(
        read_metadata(conn).unwrap()["artifact_id"],
        "artifact-test-1"
    );
    assert_eq!(table_columns(conn, "files")[0], "file_id TEXT");

    let foreign_keys_enabled: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(foreign_keys_enabled, 1);
}

#[test]
fn report_row_domains_cover_every_sqlite_v3_public_table() {
    let domains = SQLITE_ROW_DOMAINS.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(
        domains.len(),
        SQLITE_ROW_DOMAINS.len(),
        "report row domains must not contain duplicates"
    );

    for table in expected_tables() {
        assert!(
            domains.contains(table.name),
            "report row domains must include {}",
            table.name
        );
    }
}

#[test]
fn contract_docs_define_body_hash_algorithm_and_limits() {
    const SQLITE_V2: &str = include_str!("../../../docs/contracts/sqlite-schema-v2.md");
    const SQLITE_V3: &str = include_str!("../../../docs/contracts/sqlite-schema-v3.md");
    const JSONL_V2: &str = include_str!("../../../docs/contracts/jsonl-v2.md");
    const JSONL_V3: &str = include_str!("../../../docs/contracts/jsonl-v3.md");

    for (name, doc) in [
        ("sqlite-schema-v2.md", SQLITE_V2),
        ("sqlite-schema-v3.md", SQLITE_V3),
        ("jsonl-v2.md", JSONL_V2),
        ("jsonl-v3.md", JSONL_V3),
    ] {
        let searchable = compact_whitespace(doc);
        assert!(
            searchable.contains("julie-normalized-body-md5-v1"),
            "{name} must name the body_hash algorithm"
        );
        assert!(
            searchable.contains("exact normalized-body fingerprint"),
            "{name} must define body_hash as an exact normalized-body fingerprint"
        );
        assert!(
            searchable.contains("ignores whitespace and comments"),
            "{name} must document body_hash normalization inputs"
        );
        assert!(
            searchable.contains("does not encode duplicate severity"),
            "{name} must keep clone ranking out of the extractor contract"
        );
    }
}

#[test]
fn jsonl_v3_docs_list_all_capability_kind_coverage_domains() {
    const JSONL_V3: &str = include_str!("../../../docs/contracts/jsonl-v3.md");
    let searchable = compact_whitespace(JSONL_V3);

    assert!(
        searchable.contains(
            "`kind_coverage`: object with `symbols`, `relationships`, `identifiers`, \
             `body_spans`, `structural_facts`, and `complexity_metrics` domains"
        ),
        "jsonl-v3.md language_capability docs must list every kind_coverage domain"
    );
}

fn compact_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn open_schema() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn).unwrap();
    conn
}

fn sample_metadata() -> ArtifactMetadata {
    ArtifactMetadata {
        artifact_id: "artifact-test-1".to_string(),
        root_path: "/repo".to_string(),
        binary_version: "julie-extract 0.1.0".to_string(),
        hash_algorithm: "blake3".to_string(),
        parser_inventory_fingerprint: "sha256:parser".to_string(),
        capability_snapshot_fingerprint: "sha256:cap".to_string(),
        created_at: "2026-05-31T17:50:00Z".to_string(),
        updated_at: "2026-05-31T17:50:00Z".to_string(),
    }
}

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap_or_else(|err| panic!("failed to inspect table {table}: {err}"));
    stmt.query_map([], |row| {
        let name: String = row.get(1)?;
        let ty: String = row.get(2)?;
        Ok(format!("{name} {ty}"))
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

fn index_columns(conn: &Connection, index: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA index_info({index})"))
        .unwrap_or_else(|err| panic!("failed to inspect index {index}: {err}"));
    stmt.query_map([], |row| row.get::<_, String>(2))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn assert_query_uses_index<const N: usize>(
    conn: &Connection,
    sql: &str,
    params: [&str; N],
    expected_index: &str,
) {
    assert_query_plan_contains(conn, sql, params, &[expected_index]);
}

fn assert_query_plan_contains<const N: usize>(
    conn: &Connection,
    sql: &str,
    params: [&str; N],
    expected_fragments: &[&str],
) {
    let plan_sql = format!("EXPLAIN QUERY PLAN {sql}");
    let mut stmt = conn.prepare(&plan_sql).unwrap();
    let plan = stmt
        .query_map(rusqlite::params_from_iter(params), |row| {
            row.get::<_, String>(3)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    for expected_fragment in expected_fragments {
        assert!(
            plan.contains(expected_fragment),
            "query plan for `{sql}` did not contain {expected_fragment}. Plan:\n{plan}"
        );
    }
}

struct ExpectedTable {
    name: &'static str,
    columns: Vec<&'static str>,
}

struct ExpectedIndex {
    name: &'static str,
    table: &'static str,
    columns: Vec<&'static str>,
}

fn expected_tables() -> Vec<ExpectedTable> {
    let mut tables = BTreeMap::new();
    tables.insert("artifact_metadata", vec!["key TEXT", "value TEXT"]);
    tables.insert(
        "parser_inventory",
        vec![
            "language TEXT",
            "parser_package TEXT",
            "parser_version TEXT",
            "grammar_version TEXT",
            "source TEXT",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "extraction_revisions",
        vec![
            "revision_id INTEGER",
            "parent_revision_id INTEGER",
            "operation TEXT",
            "mode TEXT",
            "started_at TEXT",
            "completed_at TEXT",
            "binary_version TEXT",
            "extract_contract_version INTEGER",
            "sqlite_schema_version INTEGER",
            "input_root TEXT",
            "counts_json TEXT",
        ],
    );
    tables.insert(
        "revision_file_changes",
        vec![
            "revision_id INTEGER",
            "file_id TEXT",
            "path TEXT",
            "change_kind TEXT",
        ],
    );
    tables.insert(
        "files",
        vec![
            "file_id TEXT",
            "path TEXT",
            "language TEXT",
            "content_hash TEXT",
            "content_bytes INTEGER",
            "line_count INTEGER",
            "indexed_at TEXT",
            "last_revision_id INTEGER",
            "status TEXT",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "symbols",
        vec![
            "symbol_id TEXT",
            "file_id TEXT",
            "path TEXT",
            "language TEXT",
            "name TEXT",
            "kind TEXT",
            "signature TEXT",
            "doc_comment TEXT",
            "visibility TEXT",
            "parent_symbol_id TEXT",
            "start_line INTEGER",
            "start_column INTEGER",
            "end_line INTEGER",
            "end_column INTEGER",
            "start_byte INTEGER",
            "end_byte INTEGER",
            "body_start_line INTEGER",
            "body_start_column INTEGER",
            "body_end_line INTEGER",
            "body_end_column INTEGER",
            "body_start_byte INTEGER",
            "body_end_byte INTEGER",
            "body_hash TEXT",
            "semantic_group TEXT",
            "confidence REAL",
            "content_type TEXT",
            "is_test INTEGER",
            "test_container INTEGER",
            "test_lifecycle INTEGER",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "symbol_annotations",
        vec![
            "annotation_id TEXT",
            "symbol_id TEXT",
            "annotation TEXT",
            "annotation_key TEXT",
            "raw_text TEXT",
            "carrier TEXT",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "identifiers",
        vec![
            "identifier_id TEXT",
            "file_id TEXT",
            "path TEXT",
            "language TEXT",
            "name TEXT",
            "kind TEXT",
            "containing_symbol_id TEXT",
            "target_symbol_id TEXT",
            "start_line INTEGER",
            "start_column INTEGER",
            "end_line INTEGER",
            "end_column INTEGER",
            "start_byte INTEGER",
            "end_byte INTEGER",
            "confidence REAL",
            "code_context TEXT",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "relationships",
        vec![
            "relationship_id TEXT",
            "from_symbol_id TEXT",
            "to_symbol_id TEXT",
            "file_id TEXT",
            "path TEXT",
            "kind TEXT",
            "start_line INTEGER",
            "start_column INTEGER",
            "end_line INTEGER",
            "end_column INTEGER",
            "start_byte INTEGER",
            "end_byte INTEGER",
            "confidence REAL",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "pending_relationships",
        vec![
            "pending_relationship_id TEXT",
            "from_symbol_id TEXT",
            "caller_scope_symbol_id TEXT",
            "file_id TEXT",
            "path TEXT",
            "kind TEXT",
            "target_display_name TEXT",
            "target_terminal_name TEXT",
            "target_receiver TEXT",
            "target_namespace_json TEXT",
            "target_import_context TEXT",
            "start_line INTEGER",
            "start_column INTEGER",
            "end_line INTEGER",
            "end_column INTEGER",
            "start_byte INTEGER",
            "end_byte INTEGER",
            "confidence REAL",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "type_facts",
        vec![
            "type_fact_id TEXT",
            "symbol_id TEXT",
            "language TEXT",
            "resolved_type TEXT",
            "generic_params_json TEXT",
            "constraints_json TEXT",
            "is_inferred INTEGER",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "type_argument_usages",
        vec![
            "usage_id TEXT",
            "identifier_id TEXT",
            "file_id TEXT",
            "path TEXT",
            "language TEXT",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "type_arguments",
        vec![
            "type_argument_id TEXT",
            "usage_id TEXT",
            "parent_type_argument_id TEXT",
            "ordinal INTEGER",
            "type_name TEXT",
        ],
    );
    tables.insert(
        "literals",
        vec![
            "literal_id TEXT",
            "file_id TEXT",
            "path TEXT",
            "language TEXT",
            "literal_text TEXT",
            "kind TEXT",
            "carrier TEXT",
            "arg_position INTEGER",
            "containing_symbol_id TEXT",
            "start_line INTEGER",
            "start_column INTEGER",
            "end_line INTEGER",
            "end_column INTEGER",
            "start_byte INTEGER",
            "end_byte INTEGER",
            "confidence REAL",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "source_regions",
        vec![
            "source_region_id TEXT",
            "file_id TEXT",
            "path TEXT",
            "language TEXT",
            "kind TEXT",
            "containing_symbol_id TEXT",
            "start_line INTEGER",
            "start_column INTEGER",
            "end_line INTEGER",
            "end_column INTEGER",
            "start_byte INTEGER",
            "end_byte INTEGER",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "complexity_metrics",
        vec![
            "complexity_metric_id TEXT",
            "file_id TEXT",
            "path TEXT",
            "language TEXT",
            "scope TEXT",
            "symbol_id TEXT",
            "algorithm_id TEXT",
            "covered_lines INTEGER",
            "covered_bytes INTEGER",
            "decision_count INTEGER",
            "loop_count INTEGER",
            "max_nesting_depth INTEGER",
            "parameter_count INTEGER",
            "start_line INTEGER",
            "start_column INTEGER",
            "end_line INTEGER",
            "end_column INTEGER",
            "start_byte INTEGER",
            "end_byte INTEGER",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "structural_facts",
        vec![
            "structural_fact_id TEXT",
            "file_id TEXT",
            "path TEXT",
            "language TEXT",
            "pattern_id TEXT",
            "capture_name TEXT",
            "node_kind TEXT",
            "containing_symbol_id TEXT",
            "start_line INTEGER",
            "start_column INTEGER",
            "end_line INTEGER",
            "end_column INTEGER",
            "start_byte INTEGER",
            "end_byte INTEGER",
            "confidence REAL",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "parse_diagnostics",
        vec![
            "diagnostic_id TEXT",
            "file_id TEXT",
            "path TEXT",
            "language TEXT",
            "kind TEXT",
            "message TEXT",
            "start_line INTEGER",
            "start_column INTEGER",
            "end_line INTEGER",
            "end_column INTEGER",
            "start_byte INTEGER",
            "end_byte INTEGER",
            "metadata_json TEXT",
        ],
    );
    tables.insert(
        "language_capabilities",
        vec![
            "language TEXT",
            "parser_package TEXT",
            "extensions_json TEXT",
            "dependency_status TEXT",
            "target_symbols INTEGER",
            "target_relationships INTEGER",
            "target_pending_relationships INTEGER",
            "target_identifiers INTEGER",
            "target_types INTEGER",
            "actual_symbols INTEGER",
            "actual_relationships INTEGER",
            "actual_pending_relationships INTEGER",
            "actual_identifiers INTEGER",
            "actual_types INTEGER",
            "kind_coverage_json TEXT",
        ],
    );
    tables.insert(
        "language_capability_fixtures",
        vec![
            "language TEXT",
            "fixture_name TEXT",
            "source_path TEXT",
            "expected_path TEXT",
        ],
    );
    tables.insert(
        "language_capability_gaps",
        vec![
            "gap_id TEXT",
            "language TEXT",
            "capability TEXT",
            "status TEXT",
            "reason TEXT",
            "required_closure TEXT",
            "evidence_json TEXT",
        ],
    );

    tables
        .into_iter()
        .map(|(name, columns)| ExpectedTable { name, columns })
        .collect()
}

fn expected_indexes() -> Vec<ExpectedIndex> {
    vec![
        ExpectedIndex {
            name: "idx_files_path",
            table: "files",
            columns: vec!["path"],
        },
        ExpectedIndex {
            name: "idx_files_language",
            table: "files",
            columns: vec!["language"],
        },
        ExpectedIndex {
            name: "idx_symbols_path",
            table: "symbols",
            columns: vec!["path"],
        },
        ExpectedIndex {
            name: "idx_symbols_file",
            table: "symbols",
            columns: vec!["file_id"],
        },
        ExpectedIndex {
            name: "idx_symbols_name_kind",
            table: "symbols",
            columns: vec!["name", "kind"],
        },
        ExpectedIndex {
            name: "idx_symbols_parent",
            table: "symbols",
            columns: vec!["parent_symbol_id"],
        },
        ExpectedIndex {
            name: "idx_symbols_is_test",
            table: "symbols",
            columns: vec!["is_test"],
        },
        ExpectedIndex {
            name: "idx_symbols_test_container",
            table: "symbols",
            columns: vec!["test_container"],
        },
        ExpectedIndex {
            name: "idx_symbols_test_lifecycle",
            table: "symbols",
            columns: vec!["test_lifecycle"],
        },
        ExpectedIndex {
            name: "idx_identifiers_path",
            table: "identifiers",
            columns: vec!["path"],
        },
        ExpectedIndex {
            name: "idx_identifiers_file",
            table: "identifiers",
            columns: vec!["file_id"],
        },
        ExpectedIndex {
            name: "idx_identifiers_name_kind",
            table: "identifiers",
            columns: vec!["name", "kind"],
        },
        ExpectedIndex {
            name: "idx_identifiers_containing",
            table: "identifiers",
            columns: vec!["containing_symbol_id"],
        },
        ExpectedIndex {
            name: "idx_identifiers_target",
            table: "identifiers",
            columns: vec!["target_symbol_id"],
        },
        ExpectedIndex {
            name: "idx_relationships_from",
            table: "relationships",
            columns: vec!["from_symbol_id"],
        },
        ExpectedIndex {
            name: "idx_relationships_to",
            table: "relationships",
            columns: vec!["to_symbol_id"],
        },
        ExpectedIndex {
            name: "idx_relationships_kind",
            table: "relationships",
            columns: vec!["kind"],
        },
        ExpectedIndex {
            name: "idx_relationships_file",
            table: "relationships",
            columns: vec!["file_id"],
        },
        ExpectedIndex {
            name: "idx_pending_terminal",
            table: "pending_relationships",
            columns: vec!["target_terminal_name"],
        },
        ExpectedIndex {
            name: "idx_pending_file",
            table: "pending_relationships",
            columns: vec!["file_id"],
        },
        ExpectedIndex {
            name: "idx_pending_from",
            table: "pending_relationships",
            columns: vec!["from_symbol_id"],
        },
        ExpectedIndex {
            name: "idx_pending_caller_scope",
            table: "pending_relationships",
            columns: vec!["caller_scope_symbol_id"],
        },
        ExpectedIndex {
            name: "idx_type_facts_symbol",
            table: "type_facts",
            columns: vec!["symbol_id"],
        },
        ExpectedIndex {
            name: "idx_symbol_annotations_symbol",
            table: "symbol_annotations",
            columns: vec!["symbol_id"],
        },
        ExpectedIndex {
            name: "idx_type_argument_usages_identifier",
            table: "type_argument_usages",
            columns: vec!["identifier_id"],
        },
        ExpectedIndex {
            name: "idx_type_argument_usages_file",
            table: "type_argument_usages",
            columns: vec!["file_id"],
        },
        ExpectedIndex {
            name: "idx_type_arguments_usage",
            table: "type_arguments",
            columns: vec!["usage_id"],
        },
        ExpectedIndex {
            name: "idx_type_arguments_parent",
            table: "type_arguments",
            columns: vec!["parent_type_argument_id"],
        },
        ExpectedIndex {
            name: "idx_literals_file",
            table: "literals",
            columns: vec!["file_id"],
        },
        ExpectedIndex {
            name: "idx_source_regions_file_span",
            table: "source_regions",
            columns: vec!["file_id", "start_byte", "end_byte"],
        },
        ExpectedIndex {
            name: "idx_source_regions_export_order",
            table: "source_regions",
            columns: vec!["path", "start_byte", "end_byte", "kind", "source_region_id"],
        },
        ExpectedIndex {
            name: "idx_source_regions_kind_file",
            table: "source_regions",
            columns: vec!["kind", "file_id", "start_byte"],
        },
        ExpectedIndex {
            name: "idx_source_regions_symbol",
            table: "source_regions",
            columns: vec!["containing_symbol_id"],
        },
        ExpectedIndex {
            name: "idx_complexity_metrics_file_scope",
            table: "complexity_metrics",
            columns: vec!["file_id", "scope", "start_byte"],
        },
        ExpectedIndex {
            name: "idx_complexity_metrics_export_order",
            table: "complexity_metrics",
            columns: vec![
                "path",
                "start_byte",
                "end_byte",
                "scope",
                "symbol_id",
                "complexity_metric_id",
            ],
        },
        ExpectedIndex {
            name: "idx_complexity_metrics_scope_language",
            table: "complexity_metrics",
            columns: vec!["scope", "language", "path"],
        },
        ExpectedIndex {
            name: "idx_complexity_metrics_symbol",
            table: "complexity_metrics",
            columns: vec!["symbol_id"],
        },
        ExpectedIndex {
            name: "idx_structural_facts_file_span",
            table: "structural_facts",
            columns: vec!["file_id", "start_byte", "end_byte"],
        },
        ExpectedIndex {
            name: "idx_structural_facts_export_order",
            table: "structural_facts",
            columns: vec![
                "path",
                "start_byte",
                "end_byte",
                "pattern_id",
                "capture_name",
                "structural_fact_id",
            ],
        },
        ExpectedIndex {
            name: "idx_structural_facts_pattern_language_path",
            table: "structural_facts",
            columns: vec!["pattern_id", "language", "path"],
        },
        ExpectedIndex {
            name: "idx_structural_facts_symbol",
            table: "structural_facts",
            columns: vec!["containing_symbol_id"],
        },
        ExpectedIndex {
            name: "idx_diagnostics_path",
            table: "parse_diagnostics",
            columns: vec!["path"],
        },
        ExpectedIndex {
            name: "idx_diagnostics_file",
            table: "parse_diagnostics",
            columns: vec!["file_id"],
        },
    ]
}
