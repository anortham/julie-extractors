use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::jsonl::{JSONL_RECORD_KINDS, export_jsonl, export_jsonl_to_path};
use julie_extract_artifact::metadata::{ArtifactMetadata, initialize_metadata};
use julie_extract_artifact::schema::create_schema;
use rusqlite::{Connection, params};
use serde_json::{Value, json};

#[test]
fn full_export_emits_every_kind_in_contract_order_with_snapshot_envelope() {
    let conn = populated_artifact();

    let records = export_records(&conn);
    let mut kinds = records
        .iter()
        .map(|record| record["kind"].as_str().unwrap())
        .collect::<Vec<_>>();
    kinds.dedup();

    assert_eq!(kinds, JSONL_RECORD_KINDS);
    assert_eq!(records[0]["kind"], "artifact");
    assert_eq!(records[0]["record_id"], "artifact-jsonl-test");

    for record in &records {
        assert_eq!(record["jsonl_schema_version"], 3);
        assert_eq!(record["extract_contract_version"], 3);
        assert_eq!(record["op"], "snapshot");
        assert_eq!(record["artifact_id"], "artifact-jsonl-test");
        assert!(
            record.get("record").is_some(),
            "missing payload in {record}"
        );
    }
}

#[test]
fn sqlite_json_text_columns_are_decoded_into_json_values() {
    let conn = populated_artifact();

    let records = export_records(&conn);

    assert_eq!(
        record(&records, "parser_inventory")["metadata"],
        json!({"parser": true})
    );
    assert_eq!(
        record(&records, "language_capability")["extensions"],
        json!(["rs"])
    );
    assert_eq!(
        record(&records, "language_capability")["kind_coverage"]["symbols"]["supported"],
        json!(["function"])
    );
    assert_eq!(
        record(&records, "language_capability_gap")["evidence"],
        json!({"fixture": "basic"})
    );
    assert_eq!(record(&records, "revision")["counts"], json!({"files": 1}));
    assert_eq!(record(&records, "file")["metadata"], json!({"file": true}));
    assert_eq!(
        record(&records, "symbol")["metadata"],
        json!({"symbol": true, "is_test": true, "test_lifecycle": true})
    );
    assert_eq!(record(&records, "symbol")["is_test"], json!(true));
    assert_eq!(record(&records, "symbol")["test_container"], json!(false));
    assert_eq!(record(&records, "symbol")["test_lifecycle"], json!(true));
    assert_eq!(
        record(&records, "pending_relationship")["target"]["namespace"],
        json!(["crate", "external"])
    );
    assert_eq!(
        record(&records, "type_fact")["generic_params"],
        json!(["T"])
    );
    assert_eq!(
        record(&records, "type_fact")["constraints"],
        json!(["T: Clone"])
    );
    assert_eq!(
        record(&records, "literal")["metadata"],
        json!({"literal": true})
    );
    assert_eq!(
        record(&records, "source_region")["metadata"],
        json!({"source_region": true})
    );
    assert_eq!(
        record(&records, "complexity_metric")["metadata"],
        json!({"metric_version": 1})
    );
    assert_eq!(
        record(&records, "structural_fact")["metadata"],
        json!({"pattern_version": 1, "query_family": "safety"})
    );
}

#[test]
fn structural_fact_metadata_exports_stored_json_object_raw() {
    let conn = populated_artifact();
    conn.execute(
        "UPDATE structural_facts SET metadata_json = ?1 WHERE structural_fact_id = 'fact-unsafe'",
        [r#"{"z":1,"a":2}"#],
    )
    .unwrap();

    let output = export_string(&conn);
    assert!(
        output
            .lines()
            .any(|line| line.contains(r#""kind":"structural_fact""#)
                && line.contains(r#""metadata":{"z":1,"a":2}"#)),
        "structural-fact metadata should be validated but emitted from stored JSON text"
    );

    let records = output
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        record(&records, "structural_fact")["metadata"],
        json!({"z": 1, "a": 2})
    );
}

#[test]
fn structural_fact_metadata_export_compacts_raw_object_whitespace() {
    let conn = populated_artifact();
    let metadata = r#"{
  "z": 1,
  "a": 2
}
"#;
    conn.execute(
        "UPDATE structural_facts SET metadata_json = ?1 WHERE structural_fact_id = 'fact-unsafe'",
        [metadata],
    )
    .unwrap();

    let output = export_string(&conn);
    for (index, line) in output.lines().enumerate() {
        serde_json::from_str::<Value>(line).unwrap_or_else(|err| {
            panic!(
                "line {} should be one valid JSONL record: {err}: {line:?}",
                index + 1
            )
        });
    }
    assert!(
        output
            .lines()
            .any(|line| line.contains(r#""kind":"structural_fact""#)
                && line.contains(r#""metadata":{"z":1,"a":2}"#)),
        "structural-fact metadata should be compacted before raw JSONL export"
    );

    let records = output
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        record(&records, "structural_fact")["metadata"],
        json!({"z": 1, "a": 2})
    );
}

#[test]
fn every_record_kind_uses_exact_payload_keys() {
    let conn = populated_artifact();
    let records = export_records(&conn);

    assert_record_keys(
        &records,
        "artifact",
        &[
            "artifact_id",
            "root_path",
            "schema_version",
            "extract_contract_version",
            "sqlite_schema_version",
            "binary_version",
            "hash_algorithm",
            "parser_inventory_fingerprint",
            "capability_snapshot_fingerprint",
            "created_at",
            "updated_at",
        ],
    );
    assert_record_keys(
        &records,
        "parser_inventory",
        &[
            "language",
            "parser_package",
            "parser_version",
            "grammar_version",
            "source",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "language_capability",
        &[
            "language",
            "parser_package",
            "extensions",
            "dependency_status",
            "target_capabilities",
            "actual_capabilities",
            "kind_coverage",
        ],
    );
    assert_record_keys(
        &records,
        "language_capability_fixture",
        &["language", "fixture_name", "source_path", "expected_path"],
    );
    assert_record_keys(
        &records,
        "language_capability_gap",
        &[
            "gap_id",
            "language",
            "capability",
            "status",
            "reason",
            "required_closure",
            "evidence",
        ],
    );
    assert_record_keys(
        &records,
        "revision",
        &[
            "revision_id",
            "parent_revision_id",
            "operation",
            "mode",
            "started_at",
            "completed_at",
            "binary_version",
            "extract_contract_version",
            "sqlite_schema_version",
            "input_root",
            "counts",
        ],
    );
    assert_record_keys(
        &records,
        "revision_file_change",
        &["revision_id", "file_id", "path", "change_kind"],
    );
    assert_record_keys(
        &records,
        "file",
        &[
            "file_id",
            "path",
            "language",
            "content_hash",
            "content_bytes",
            "line_count",
            "indexed_at",
            "last_revision_id",
            "status",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "symbol",
        &[
            "symbol_id",
            "file_id",
            "path",
            "language",
            "name",
            "kind",
            "signature",
            "doc_comment",
            "visibility",
            "parent_symbol_id",
            "span",
            "body_span",
            "body_hash",
            "semantic_group",
            "confidence",
            "content_type",
            "is_test",
            "test_container",
            "test_lifecycle",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "symbol_annotation",
        &[
            "annotation_id",
            "symbol_id",
            "annotation",
            "annotation_key",
            "raw_text",
            "carrier",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "identifier",
        &[
            "identifier_id",
            "file_id",
            "path",
            "language",
            "name",
            "kind",
            "containing_symbol_id",
            "target_symbol_id",
            "span",
            "confidence",
            "code_context",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "relationship",
        &[
            "relationship_id",
            "from_symbol_id",
            "to_symbol_id",
            "file_id",
            "path",
            "kind",
            "span",
            "confidence",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "pending_relationship",
        &[
            "pending_relationship_id",
            "from_symbol_id",
            "caller_scope_symbol_id",
            "file_id",
            "path",
            "kind",
            "target",
            "site",
            "confidence",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "type_fact",
        &[
            "type_fact_id",
            "symbol_id",
            "language",
            "resolved_type",
            "generic_params",
            "constraints",
            "is_inferred",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "type_argument_usage",
        &[
            "usage_id",
            "identifier_id",
            "file_id",
            "path",
            "language",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "type_argument",
        &[
            "type_argument_id",
            "usage_id",
            "parent_type_argument_id",
            "ordinal",
            "type_name",
        ],
    );
    assert_record_keys(
        &records,
        "literal",
        &[
            "literal_id",
            "file_id",
            "path",
            "language",
            "literal_text",
            "kind",
            "carrier",
            "arg_position",
            "containing_symbol_id",
            "span",
            "confidence",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "source_region",
        &[
            "source_region_id",
            "file_id",
            "path",
            "language",
            "kind",
            "containing_symbol_id",
            "span",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "structural_fact",
        &[
            "structural_fact_id",
            "file_id",
            "path",
            "language",
            "pattern_id",
            "capture_name",
            "node_kind",
            "containing_symbol_id",
            "span",
            "confidence",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "complexity_metric",
        &[
            "complexity_metric_id",
            "file_id",
            "path",
            "language",
            "scope",
            "symbol_id",
            "algorithm_id",
            "covered_lines",
            "covered_bytes",
            "decision_count",
            "loop_count",
            "max_nesting_depth",
            "parameter_count",
            "span",
            "metadata",
        ],
    );
    assert_record_keys(
        &records,
        "parse_diagnostic",
        &[
            "diagnostic_id",
            "file_id",
            "path",
            "language",
            "kind",
            "message",
            "span",
            "metadata",
        ],
    );
}

#[test]
fn full_export_is_deterministic_for_same_artifact() {
    let conn = populated_artifact();

    let first = export_string(&conn);
    let second = export_string(&conn);

    assert_eq!(first, second);
}

#[test]
fn buffered_export_uses_bounded_write_calls() {
    const TEST_BUFFER_BYTES: usize = 64 * 1024;

    let conn = populated_artifact();
    let mut writer = CountingWriter::default();

    let summary = export_jsonl(&conn, &mut writer).unwrap();

    let output = String::from_utf8(writer.bytes.clone()).unwrap();
    let records = output
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let max_expected_write_calls = writer.bytes_written.div_ceil(TEST_BUFFER_BYTES).max(1);

    assert_eq!(summary.total_records, records.len());
    assert!(records.len() >= JSONL_RECORD_KINDS.len());
    assert_eq!(records[0]["kind"], "artifact");
    assert!(
        writer.write_calls <= max_expected_write_calls,
        "export wrote {} chunks for {} bytes; expected at most {} chunks",
        writer.write_calls,
        writer.bytes_written,
        max_expected_write_calls
    );
}

#[test]
fn failed_path_export_removes_incomplete_output_file() {
    let conn = populated_artifact();
    conn.execute("UPDATE files SET metadata_json = '{'", [])
        .unwrap();
    let output_path = unique_temp_path("julie-jsonl-failed-export.jsonl");
    let _ = fs::remove_file(&output_path);

    let error = export_jsonl_to_path(&conn, &output_path).unwrap_err();

    assert!(
        error.to_string().contains("metadata_json"),
        "unexpected error: {error}"
    );
    assert!(
        !output_path.exists(),
        "failed export must not leave a complete output file"
    );
    let _ = fs::remove_file(output_path.with_extension("tmp"));
}

fn export_records(conn: &Connection) -> Vec<Value> {
    export_string(conn)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn export_string(conn: &Connection) -> String {
    let mut bytes = Vec::new();
    export_jsonl(conn, &mut bytes).unwrap();
    String::from_utf8(bytes).unwrap()
}

fn record<'a>(records: &'a [Value], kind: &str) -> &'a Value {
    &records
        .iter()
        .find(|record| record["kind"] == kind)
        .unwrap_or_else(|| panic!("missing record kind {kind}"))["record"]
}

fn assert_record_keys(records: &[Value], kind: &str, expected: &[&str]) {
    let record = record(records, kind);
    let actual = record
        .as_object()
        .unwrap_or_else(|| panic!("{kind} payload is not an object"))
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(actual, expected, "{kind} payload keys drifted");
}

#[derive(Default)]
struct CountingWriter {
    bytes: Vec<u8>,
    bytes_written: usize,
    write_calls: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_calls += 1;
        self.bytes_written += buf.len();
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn populated_artifact() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    create_schema(&conn).unwrap();
    initialize_metadata(
        &conn,
        &ArtifactMetadata {
            artifact_id: "artifact-jsonl-test".to_string(),
            root_path: "/repo".to_string(),
            binary_version: "julie-extract 0.1.0".to_string(),
            hash_algorithm: "blake3".to_string(),
            parser_inventory_fingerprint: "sha256:parser".to_string(),
            capability_snapshot_fingerprint: "sha256:cap".to_string(),
            created_at: "2026-05-31T20:35:00Z".to_string(),
            updated_at: "2026-05-31T20:35:01Z".to_string(),
        },
    )
    .unwrap();
    insert_capability_rows(&conn);
    insert_extraction_rows(&conn);
    conn
}

fn insert_capability_rows(conn: &Connection) {
    conn.execute(
        "INSERT INTO parser_inventory
         (language, parser_package, parser_version, grammar_version, source, metadata_json)
         VALUES ('rust', 'tree-sitter-rust', '0.24.2', '1', 'crate', ?1)",
        [r#"{"parser":true}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO language_capabilities
         (language, parser_package, extensions_json, dependency_status,
          target_symbols, target_relationships, target_pending_relationships,
          target_identifiers, target_types, actual_symbols, actual_relationships,
          actual_pending_relationships, actual_identifiers, actual_types,
          kind_coverage_json)
         VALUES ('rust', 'tree-sitter-rust', ?1, 'bundled',
          1, 1, 1, 1, 1, 1, 1, 1, 1, 1, ?2)",
        params![
            r#"["rs"]"#,
            r#"{
              "symbols":{"supported":["function"],"not_applicable":[],"open_gaps":[]},
              "relationships":{"supported":["calls"],"not_applicable":[],"open_gaps":[]},
              "identifiers":{"supported":["call"],"not_applicable":[],"open_gaps":[]},
              "body_spans":{"supported":["function"],"not_applicable":[],"open_gaps":[]}
            }"#
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO language_capability_fixtures
         (language, fixture_name, source_path, expected_path)
         VALUES ('rust', 'basic', 'fixtures/rust/basic.rs', 'fixtures/extraction/rust/basic.json')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO language_capability_gaps
         (gap_id, language, capability, status, reason, required_closure, evidence_json)
         VALUES ('gap-rust-1', 'rust', 'relationships', 'open', 'needs fixture',
                 'add fixture', ?1)",
        [r#"{"fixture":"basic"}"#],
    )
    .unwrap();
}

fn insert_extraction_rows(conn: &Connection) {
    conn.execute(
        "INSERT INTO extraction_revisions
         (revision_id, parent_revision_id, operation, mode, started_at, completed_at,
          binary_version, extract_contract_version, sqlite_schema_version, input_root, counts_json)
         VALUES (1, NULL, 'scan', 'incremental', '2026-05-31T20:35:00Z',
                 '2026-05-31T20:35:01Z', 'julie-extract 0.1.0', 1, 1, '/repo', ?1)",
        [r#"{"files":1}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO revision_file_changes (revision_id, file_id, path, change_kind)
         VALUES (1, 'file-a', 'src/a.rs', 'inserted')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO files
         (file_id, path, language, content_hash, content_bytes, line_count, indexed_at,
          last_revision_id, status, metadata_json)
         VALUES ('file-a', 'src/a.rs', 'rust', 'blake3:file-a', 64, 6,
                 '2026-05-31T20:35:00Z', 1, 'indexed', ?1)",
        [r#"{"file":true}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO symbols
         (symbol_id, file_id, path, language, name, kind, signature, doc_comment, visibility,
          parent_symbol_id, start_line, start_column, end_line, end_column, start_byte, end_byte,
          body_start_line, body_start_column, body_end_line, body_end_column, body_start_byte,
          body_end_byte, body_hash, semantic_group, confidence, content_type, is_test,
          test_container, test_lifecycle, metadata_json)
         VALUES ('sym-alpha', 'file-a', 'src/a.rs', 'rust', 'alpha', 'function',
          'fn alpha()', NULL, 'public', NULL, 1, 0, 3, 1, 0, 32, 2, 0, 3, 1,
          12, 32, 'body-hash', 'function', 1.0, 'code', 1, 0, 1, ?1)",
        [r#"{"symbol":true,"is_test":true,"test_lifecycle":true}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO symbols
         (symbol_id, file_id, path, language, name, kind, signature, doc_comment, visibility,
          parent_symbol_id, start_line, start_column, end_line, end_column, start_byte, end_byte,
          body_start_line, body_start_column, body_end_line, body_end_column, body_start_byte,
          body_end_byte, body_hash, semantic_group, confidence, content_type, is_test,
          test_container, test_lifecycle, metadata_json)
         VALUES ('sym-beta', 'file-a', 'src/a.rs', 'rust', 'beta', 'function',
          'fn beta()', NULL, 'private', 'sym-alpha', 4, 0, 5, 1, 33, 50,
          NULL, NULL, NULL, NULL, NULL, NULL, NULL, 'function', 1.0, 'code', 0, 0, 0, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO symbol_annotations
         (annotation_id, symbol_id, annotation, annotation_key, raw_text, carrier, metadata_json)
         VALUES ('ann-alpha', 'sym-alpha', 'route', 'route', '#[route]', 'attribute', ?1)",
        [r#"{"annotation":true}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO identifiers
         (identifier_id, file_id, path, language, name, kind, containing_symbol_id,
          target_symbol_id, start_line, start_column, end_line, end_column, start_byte,
          end_byte, confidence, code_context, metadata_json)
         VALUES ('ident-beta', 'file-a', 'src/a.rs', 'rust', 'beta', 'call',
                 'sym-alpha', 'sym-beta', 2, 4, 2, 8, 16, 20, 0.95,
                 'beta();', ?1)",
        [r#"{"identifier":true}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO relationships
         (relationship_id, from_symbol_id, to_symbol_id, file_id, path, kind, start_line,
          start_column, end_line, end_column, start_byte, end_byte, confidence, metadata_json)
         VALUES ('rel-alpha-beta', 'sym-alpha', 'sym-beta', 'file-a', 'src/a.rs', 'calls',
                 2, 4, 2, 8, 16, 20, 0.95, ?1)",
        [r#"{"relationship":true}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO pending_relationships
         (pending_relationship_id, from_symbol_id, caller_scope_symbol_id, file_id, path, kind,
          target_display_name, target_terminal_name, target_receiver, target_namespace_json,
          target_import_context, start_line, start_column, end_line, end_column, start_byte,
          end_byte, confidence, metadata_json)
         VALUES ('pending-external', 'sym-alpha', 'sym-alpha', 'file-a', 'src/a.rs', 'uses',
                 'crate::external::Thing', 'Thing', 'external', ?1, 'use crate::external',
                 2, 4, NULL, NULL, 16, NULL, 0.4, ?2)",
        params![r#"["crate","external"]"#, r#"{"pending":true}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO type_facts
         (type_fact_id, symbol_id, language, resolved_type, generic_params_json,
          constraints_json, is_inferred, metadata_json)
         VALUES ('type-fact-alpha', 'sym-alpha', 'rust', 'Result<T>', ?1, ?2, 1, ?3)",
        params![r#"["T"]"#, r#"["T: Clone"]"#, r#"{"type_fact":true}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO type_argument_usages
         (usage_id, identifier_id, file_id, path, language, metadata_json)
         VALUES ('usage-beta', 'ident-beta', 'file-a', 'src/a.rs', 'rust', ?1)",
        [r#"{"usage":true}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO type_arguments
         (type_argument_id, usage_id, parent_type_argument_id, ordinal, type_name)
         VALUES ('type-arg-string', 'usage-beta', NULL, 0, 'String')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO literals
         (literal_id, file_id, path, language, literal_text, kind, carrier, arg_position,
          containing_symbol_id, start_line, start_column, end_line, end_column, start_byte,
          end_byte, confidence, metadata_json)
         VALUES ('literal-route', 'file-a', 'src/a.rs', 'rust', '/api/users', 'route',
                 'route', 0, 'sym-alpha', 2, 10, 2, 22, 22, 34, 1.0, ?1)",
        [r#"{"literal":true}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO source_regions
         (source_region_id, file_id, path, language, kind, containing_symbol_id,
          start_line, start_column, end_line, end_column, start_byte, end_byte,
          metadata_json)
         VALUES ('region-comment', 'file-a', 'src/a.rs', 'rust', 'comment',
                 'sym-alpha', 1, 0, 1, 14, 0, 14, ?1)",
        [r#"{"source_region":true}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO complexity_metrics
         (complexity_metric_id, file_id, path, language, scope, symbol_id, algorithm_id,
          covered_lines, covered_bytes, decision_count, loop_count, max_nesting_depth,
          parameter_count, start_line, start_column, end_line, end_column, start_byte, end_byte,
          metadata_json)
         VALUES ('complexity-alpha', 'file-a', 'src/a.rs', 'rust', 'symbol', 'sym-alpha',
                 'julie-ast-complexity-v1', 3, 48, 1, 1, 2, 2, 1, 0, 3, 1, 0, 48, ?1)",
        [r#"{"metric_version":1}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO structural_facts
         (structural_fact_id, file_id, path, language, pattern_id, capture_name, node_kind,
          containing_symbol_id, start_line, start_column, end_line, end_column, start_byte,
          end_byte, confidence, metadata_json)
         VALUES ('fact-unsafe', 'file-a', 'src/a.rs', 'rust', 'rust.unsafe_block.v1',
                 'unsafe_block', 'unsafe_block', 'sym-alpha', 3, 4, 5, 5, 36, 58, 1.0, ?1)",
        [r#"{"pattern_version":1,"query_family":"safety"}"#],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO parse_diagnostics
         (diagnostic_id, file_id, path, language, kind, message, start_line, start_column,
          end_line, end_column, start_byte, end_byte, metadata_json)
         VALUES ('diag-a', 'file-a', 'src/a.rs', 'rust', 'error', 'recoverable',
                 6, 0, 6, 1, 60, 61, ?1)",
        [r#"{"diagnostic":true}"#],
    )
    .unwrap();
}

fn unique_temp_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{}-{nanos}-{name}", std::process::id()))
}
