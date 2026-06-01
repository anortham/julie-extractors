use julie_extract_artifact::metadata::ArtifactMetadata;
use julie_extract_artifact::model::{
    ArtifactFile, ArtifactIdentifier, ArtifactLiteral, ArtifactParseDiagnostic,
    ArtifactPendingRelationship, ArtifactRelationship, ArtifactSymbol, ArtifactSymbolAnnotation,
    ArtifactTypeArgument, ArtifactTypeArgumentUsage, ArtifactTypeFact, FileStatus, RevisionInput,
    WriteMode, WriteOperation,
};
use julie_extract_artifact::writer::{ArtifactWriteError, ArtifactWriter};
use rusqlite::Connection;

#[test]
fn scan_batch_writes_multiple_files_in_one_transaction() {
    let mut writer = open_writer();

    let result = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[
                file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha", "helper"]),
                file_with_symbols("file-b", "src/b.rs", "hash-b", ["beta"]),
            ],
        )
        .unwrap();

    assert_eq!(result.transactions_committed, 1);
    assert_eq!(result.files_changed, 2);
    assert_eq!(result.files_skipped, 0);
    assert_eq!(result.rows_written.files, 2);
    assert_eq!(result.rows_written.symbols, 3);
    assert_eq!(count(writer.connection(), "extraction_revisions"), 1);
    assert_eq!(count(writer.connection(), "files"), 2);
    assert_eq!(count(writer.connection(), "symbols"), 3);
    assert!(index_exists(writer.connection(), "idx_symbols_name_kind"));
    assert_eq!(
        revision_changes(writer.connection()),
        vec![
            ("src/a.rs".to_string(), "inserted".to_string()),
            ("src/b.rs".to_string(), "inserted".to_string())
        ]
    );
}

#[test]
fn scan_deletes_files_missing_from_the_current_source_snapshot() {
    let mut writer = open_writer();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[
                file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"]),
                file_with_symbols("file-b", "src/b.rs", "hash-b", ["beta"]),
            ],
        )
        .unwrap();

    let result = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_with_symbols("file-b", "src/b.rs", "hash-b", ["wrong"])],
        )
        .unwrap();

    assert_eq!(result.transactions_committed, 1);
    assert_eq!(result.files_changed, 1);
    assert_eq!(result.files_deleted, 1);
    assert_eq!(result.files_skipped, 1);
    assert_eq!(result.rows_written.files, 0);
    assert_eq!(result.rows_written.revision_file_changes, 1);
    assert_eq!(
        symbols_for_path(writer.connection(), "src/a.rs"),
        Vec::<String>::new()
    );
    assert_eq!(
        symbols_for_path(writer.connection(), "src/b.rs"),
        vec!["beta"]
    );
    assert_eq!(
        latest_change(writer.connection(), "src/a.rs"),
        Some("deleted".to_string())
    );
}

#[test]
fn scan_persists_every_normalized_row_family_with_counts() {
    let mut writer = open_writer();

    let result = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_with_all_rows("file-a", "src/a.rs", "hash-a")],
        )
        .unwrap();

    assert_eq!(result.rows_written.files, 1);
    assert_eq!(result.rows_written.symbols, 2);
    assert_eq!(result.rows_written.symbol_annotations, 1);
    assert_eq!(result.rows_written.identifiers, 1);
    assert_eq!(result.rows_written.relationships, 1);
    assert_eq!(result.rows_written.pending_relationships, 1);
    assert_eq!(result.rows_written.type_facts, 1);
    assert_eq!(result.rows_written.type_argument_usages, 1);
    assert_eq!(result.rows_written.type_arguments, 1);
    assert_eq!(result.rows_written.literals, 1);
    assert_eq!(result.rows_written.parse_diagnostics, 1);
    assert_eq!(count(writer.connection(), "symbol_annotations"), 1);
    assert_eq!(count(writer.connection(), "identifiers"), 1);
    assert_eq!(count(writer.connection(), "relationships"), 1);
    assert_eq!(count(writer.connection(), "pending_relationships"), 1);
    assert_eq!(count(writer.connection(), "type_facts"), 1);
    assert_eq!(count(writer.connection(), "type_argument_usages"), 1);
    assert_eq!(count(writer.connection(), "type_arguments"), 1);
    assert_eq!(count(writer.connection(), "literals"), 1);
    assert_eq!(count(writer.connection(), "parse_diagnostics"), 1);
}

#[test]
fn failed_mid_batch_rolls_back_prior_file_writes() {
    let mut writer = open_writer();
    let mut broken = file_with_symbols("file-bad", "src/bad.rs", "hash-bad", ["bad"]);
    broken.symbols[0].symbol_id = "file-a-symbol-0".to_string();

    let error = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[
                file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"]),
                broken,
            ],
        )
        .unwrap_err();

    assert!(matches!(error, ArtifactWriteError::Sqlite(_)));
    assert_eq!(count(writer.connection(), "extraction_revisions"), 0);
    assert_eq!(count(writer.connection(), "files"), 0);
    assert_eq!(count(writer.connection(), "symbols"), 0);
    assert_eq!(count(writer.connection(), "relationships"), 0);
}

#[test]
fn scan_skips_child_rows_with_missing_required_references() {
    let mut writer = open_writer();
    let mut file = file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"]);
    file.symbol_annotations.push(ArtifactSymbolAnnotation {
        annotation_id: "missing-annotation-symbol".to_string(),
        symbol_id: "missing-symbol".to_string(),
        annotation: "Missing".to_string(),
        annotation_key: "missing".to_string(),
        raw_text: None,
        carrier: None,
        metadata_json: None,
    });
    file.relationships.push(ArtifactRelationship {
        relationship_id: "missing-from-symbol".to_string(),
        from_symbol_id: "missing-symbol".to_string(),
        to_symbol_id: "file-a-symbol-0".to_string(),
        kind: "calls".to_string(),
        confidence: 1.0,
        ..ArtifactRelationship::default()
    });
    file.pending_relationships
        .push(ArtifactPendingRelationship {
            pending_relationship_id: "missing-pending-from-symbol".to_string(),
            from_symbol_id: "missing-symbol".to_string(),
            target_display_name: "Missing".to_string(),
            target_terminal_name: "Missing".to_string(),
            ..ArtifactPendingRelationship::default()
        });
    file.type_facts.push(ArtifactTypeFact {
        type_fact_id: "missing-type-symbol".to_string(),
        symbol_id: "missing-symbol".to_string(),
        resolved_type: "Missing".to_string(),
        generic_params_json: None,
        constraints_json: None,
        is_inferred: true,
        metadata_json: None,
    });
    file.type_argument_usages.push(ArtifactTypeArgumentUsage {
        usage_id: "missing-identifier-usage".to_string(),
        identifier_id: "missing-identifier".to_string(),
        metadata_json: None,
    });
    file.type_arguments.push(ArtifactTypeArgument {
        type_argument_id: "missing-identifier-argument".to_string(),
        usage_id: "missing-identifier-usage".to_string(),
        parent_type_argument_id: None,
        ordinal: 0,
        type_name: "Missing".to_string(),
    });

    let result = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file],
        )
        .unwrap();

    assert_eq!(result.rows_written.files, 1);
    assert_eq!(result.rows_written.symbols, 1);
    assert_eq!(result.rows_written.symbol_annotations, 0);
    assert_eq!(result.rows_written.relationships, 0);
    assert_eq!(result.rows_written.pending_relationships, 0);
    assert_eq!(result.rows_written.type_facts, 0);
    assert_eq!(result.rows_written.type_argument_usages, 0);
    assert_eq!(result.rows_written.type_arguments, 0);
    assert_eq!(count(writer.connection(), "symbol_annotations"), 0);
    assert_eq!(count(writer.connection(), "relationships"), 0);
    assert_eq!(count(writer.connection(), "pending_relationships"), 0);
    assert_eq!(count(writer.connection(), "type_facts"), 0);
    assert_eq!(count(writer.connection(), "type_argument_usages"), 0);
    assert_eq!(count(writer.connection(), "type_arguments"), 0);
}

#[test]
fn scan_batch_allows_relationships_to_symbols_later_in_same_batch() {
    let mut writer = open_writer();
    let mut file_a = file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"]);
    file_a.relationships.push(ArtifactRelationship {
        relationship_id: "cross-file-relationship".to_string(),
        from_symbol_id: "file-a-symbol-0".to_string(),
        to_symbol_id: "file-b-symbol-0".to_string(),
        kind: "calls".to_string(),
        start_line: Some(1),
        confidence: 1.0,
        ..ArtifactRelationship::default()
    });
    let file_b = file_with_symbols("file-b", "src/b.rs", "hash-b", ["beta"]);

    let result = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_a, file_b],
        )
        .unwrap();

    assert_eq!(result.rows_written.relationships, 1);
    assert_eq!(count(writer.connection(), "relationships"), 1);
}

#[test]
fn update_replaces_exactly_one_files_rows() {
    let mut writer = open_writer();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[
                file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha", "helper"]),
                file_with_symbols("file-b", "src/b.rs", "hash-b", ["beta"]),
            ],
        )
        .unwrap();

    let result = writer
        .write_update(
            revision(WriteOperation::Update, Some(WriteMode::SingleFile)),
            &file_with_symbols("file-a", "src/a.rs", "hash-a2", ["alpha_v2"]),
        )
        .unwrap();

    assert_eq!(result.transactions_committed, 1);
    assert_eq!(result.files_changed, 1);
    assert_eq!(result.files_skipped, 0);
    assert_eq!(result.rows_written.files, 1);
    assert_eq!(result.rows_written.symbols, 1);
    assert_eq!(
        symbols_for_path(writer.connection(), "src/a.rs"),
        vec!["alpha_v2"]
    );
    assert_eq!(
        symbols_for_path(writer.connection(), "src/b.rs"),
        vec!["beta"]
    );
    assert_eq!(count(writer.connection(), "files"), 2);
}

#[test]
fn update_cleans_old_child_rows_for_replaced_file() {
    let mut writer = open_writer();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[
                file_with_all_rows("file-a", "src/a.rs", "hash-a"),
                file_with_symbols("file-b", "src/b.rs", "hash-b", ["beta"]),
            ],
        )
        .unwrap();

    writer
        .write_update(
            revision(WriteOperation::Update, Some(WriteMode::SingleFile)),
            &file_with_symbols("file-a", "src/a.rs", "hash-a2", ["alpha_v2"]),
        )
        .unwrap();

    assert_eq!(
        symbols_for_path(writer.connection(), "src/a.rs"),
        vec!["alpha_v2"]
    );
    assert_eq!(
        symbols_for_path(writer.connection(), "src/b.rs"),
        vec!["beta"]
    );
    assert_child_tables_empty(writer.connection());
}

#[test]
fn delete_removes_exactly_one_files_rows() {
    let mut writer = open_writer();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[
                file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha", "helper"]),
                file_with_symbols("file-b", "src/b.rs", "hash-b", ["beta"]),
            ],
        )
        .unwrap();

    let result = writer
        .delete_file(
            revision(WriteOperation::Delete, Some(WriteMode::SingleFile)),
            "src/a.rs",
        )
        .unwrap();

    assert_eq!(result.transactions_committed, 1);
    assert_eq!(result.files_changed, 1);
    assert_eq!(result.rows_written.revision_file_changes, 1);
    assert_eq!(
        symbols_for_path(writer.connection(), "src/a.rs"),
        Vec::<String>::new()
    );
    assert_eq!(
        symbols_for_path(writer.connection(), "src/b.rs"),
        vec!["beta"]
    );
    assert_eq!(count(writer.connection(), "files"), 1);
    assert_eq!(
        latest_change(writer.connection(), "src/a.rs"),
        Some("deleted".to_string())
    );
}

#[test]
fn delete_cleans_child_rows_for_one_file_only() {
    let mut writer = open_writer();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[
                file_with_all_rows("file-a", "src/a.rs", "hash-a"),
                file_with_symbols("file-b", "src/b.rs", "hash-b", ["beta"]),
            ],
        )
        .unwrap();

    writer
        .delete_file(
            revision(WriteOperation::Delete, Some(WriteMode::SingleFile)),
            "src/a.rs",
        )
        .unwrap();

    assert_eq!(
        symbols_for_path(writer.connection(), "src/a.rs"),
        Vec::<String>::new()
    );
    assert_eq!(
        symbols_for_path(writer.connection(), "src/b.rs"),
        vec!["beta"]
    );
    assert_child_tables_empty(writer.connection());
}

#[test]
fn unchanged_file_hash_skips_row_churn_before_replacement() {
    let mut writer = open_writer();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"])],
        )
        .unwrap();

    let result = writer
        .write_update(
            revision(WriteOperation::Update, Some(WriteMode::SingleFile)),
            &file_with_symbols("file-a", "src/a.rs", "hash-a", ["would_be_wrong"]),
        )
        .unwrap();

    assert_eq!(result.transactions_committed, 1);
    assert_eq!(result.files_changed, 0);
    assert_eq!(result.files_skipped, 1);
    assert_eq!(result.rows_written.files, 0);
    assert_eq!(
        symbols_for_path(writer.connection(), "src/a.rs"),
        vec!["alpha"]
    );
    assert_eq!(count(writer.connection(), "extraction_revisions"), 1);
}

#[test]
fn force_scan_rewrites_unchanged_hash_rows() {
    let mut writer = open_writer();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"])],
        )
        .unwrap();

    let result = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Force)),
            &[file_with_symbols(
                "file-a",
                "src/a.rs",
                "hash-a",
                ["alpha_v2"],
            )],
        )
        .unwrap();

    assert_eq!(result.transactions_committed, 1);
    assert_eq!(result.files_changed, 1);
    assert_eq!(result.files_skipped, 0);
    assert_eq!(
        symbols_for_path(writer.connection(), "src/a.rs"),
        vec!["alpha_v2"]
    );
    assert_eq!(count(writer.connection(), "extraction_revisions"), 2);
    assert_eq!(
        latest_change(writer.connection(), "src/a.rs"),
        Some("updated".to_string())
    );
}

#[test]
fn data_loss_guard_preserves_known_good_rows_on_parser_failure_evidence() {
    let mut writer = open_writer();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"])],
        )
        .unwrap();

    let failed = ArtifactFile {
        status: FileStatus::FailedPreserved,
        content_hash: "hash-a2".to_string(),
        symbols: Vec::new(),
        ..file_with_symbols("file-a", "src/a.rs", "hash-a2", [])
    };
    let error = writer
        .write_update(
            revision(WriteOperation::Update, Some(WriteMode::SingleFile)),
            &failed,
        )
        .unwrap_err();

    assert!(matches!(
        error,
        ArtifactWriteError::DataLossGuard { ref path, .. } if path == "src/a.rs"
    ));
    assert_eq!(
        symbols_for_path(writer.connection(), "src/a.rs"),
        vec!["alpha"]
    );
    assert_eq!(
        file_hash(writer.connection(), "src/a.rs"),
        Some("hash-a".to_string())
    );
    assert_eq!(count(writer.connection(), "extraction_revisions"), 1);
}

fn open_writer() -> ArtifactWriter {
    ArtifactWriter::open_in_memory(ArtifactMetadata {
        artifact_id: "artifact-writer-test".to_string(),
        root_path: "/repo".to_string(),
        binary_version: "julie-extract 0.1.0".to_string(),
        hash_algorithm: "blake3".to_string(),
        parser_inventory_fingerprint: "sha256:parser".to_string(),
        capability_snapshot_fingerprint: "sha256:cap".to_string(),
        created_at: "2026-05-31T19:20:00Z".to_string(),
        updated_at: "2026-05-31T19:20:00Z".to_string(),
    })
    .unwrap()
}

fn revision(operation: WriteOperation, mode: Option<WriteMode>) -> RevisionInput {
    RevisionInput {
        operation,
        mode,
        started_at: "2026-05-31T19:20:00Z".to_string(),
        completed_at: "2026-05-31T19:20:01Z".to_string(),
        binary_version: "julie-extract 0.1.0".to_string(),
        input_root: Some("/repo".to_string()),
    }
}

fn file_with_symbols<const N: usize>(
    file_id: &str,
    path: &str,
    hash: &str,
    names: [&str; N],
) -> ArtifactFile {
    ArtifactFile {
        file_id: file_id.to_string(),
        path: path.to_string(),
        language: "rust".to_string(),
        content_hash: hash.to_string(),
        content_bytes: 32,
        line_count: Some(3),
        indexed_at: "2026-05-31T19:20:00Z".to_string(),
        status: FileStatus::Indexed,
        metadata_json: None,
        symbols: names
            .into_iter()
            .enumerate()
            .map(|(index, name)| ArtifactSymbol {
                symbol_id: format!("{file_id}-symbol-{index}"),
                name: name.to_string(),
                kind: "function".to_string(),
                signature: Some(format!("fn {name}()")),
                start_line: (index + 1) as i64,
                end_line: (index + 1) as i64,
                start_byte: (index * 10) as i64,
                end_byte: (index * 10 + 5) as i64,
                ..ArtifactSymbol::default()
            })
            .collect(),
        symbol_annotations: Vec::new(),
        identifiers: Vec::new(),
        relationships: Vec::new(),
        pending_relationships: Vec::new(),
        type_facts: Vec::new(),
        type_argument_usages: Vec::new(),
        type_arguments: Vec::new(),
        literals: Vec::new(),
        parse_diagnostics: Vec::new(),
    }
}

fn file_with_all_rows(file_id: &str, path: &str, hash: &str) -> ArtifactFile {
    let mut file = file_with_symbols(file_id, path, hash, ["alpha", "beta"]);
    file.symbol_annotations.push(ArtifactSymbolAnnotation {
        annotation_id: format!("{file_id}-annotation-1"),
        symbol_id: format!("{file_id}-symbol-0"),
        annotation: "route".to_string(),
        annotation_key: "route".to_string(),
        raw_text: Some("#[route]".to_string()),
        carrier: Some("attribute".to_string()),
        metadata_json: None,
    });
    file.identifiers.push(ArtifactIdentifier {
        identifier_id: format!("{file_id}-identifier-1"),
        name: "beta".to_string(),
        kind: "call".to_string(),
        containing_symbol_id: Some(format!("{file_id}-symbol-0")),
        target_symbol_id: Some(format!("{file_id}-symbol-1")),
        ..ArtifactIdentifier::default()
    });
    file.relationships.push(ArtifactRelationship {
        relationship_id: format!("{file_id}-relationship-1"),
        from_symbol_id: format!("{file_id}-symbol-0"),
        to_symbol_id: format!("{file_id}-symbol-1"),
        kind: "calls".to_string(),
        start_line: Some(2),
        confidence: 1.0,
        metadata_json: None,
        ..ArtifactRelationship::default()
    });
    file.pending_relationships
        .push(ArtifactPendingRelationship {
            pending_relationship_id: format!("{file_id}-pending-1"),
            from_symbol_id: format!("{file_id}-symbol-0"),
            kind: "uses".to_string(),
            target_display_name: "External".to_string(),
            target_terminal_name: "External".to_string(),
            target_namespace_json: r#"["crate"]"#.to_string(),
            start_line: 2,
            ..ArtifactPendingRelationship::default()
        });
    file.type_facts.push(ArtifactTypeFact {
        type_fact_id: format!("{file_id}-type-fact-1"),
        symbol_id: format!("{file_id}-symbol-0"),
        resolved_type: "Result<()>".to_string(),
        generic_params_json: Some(r#"["T"]"#.to_string()),
        constraints_json: None,
        is_inferred: true,
        metadata_json: None,
    });
    file.type_argument_usages.push(ArtifactTypeArgumentUsage {
        usage_id: format!("{file_id}-usage-1"),
        identifier_id: format!("{file_id}-identifier-1"),
        metadata_json: None,
    });
    file.type_arguments.push(ArtifactTypeArgument {
        type_argument_id: format!("{file_id}-type-arg-1"),
        usage_id: format!("{file_id}-usage-1"),
        parent_type_argument_id: None,
        ordinal: 0,
        type_name: "String".to_string(),
    });
    file.literals.push(ArtifactLiteral {
        literal_id: format!("{file_id}-literal-1"),
        literal_text: "/api/users".to_string(),
        kind: "route".to_string(),
        carrier: Some("route".to_string()),
        containing_symbol_id: Some(format!("{file_id}-symbol-0")),
        ..ArtifactLiteral::default()
    });
    file.parse_diagnostics.push(ArtifactParseDiagnostic {
        diagnostic_id: format!("{file_id}-diagnostic-1"),
        kind: "error".to_string(),
        message: Some("recoverable".to_string()),
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 1,
        start_byte: 0,
        end_byte: 1,
        metadata_json: None,
    });
    file
}

fn assert_child_tables_empty(conn: &Connection) {
    for table in [
        "symbol_annotations",
        "identifiers",
        "relationships",
        "pending_relationships",
        "type_facts",
        "type_argument_usages",
        "type_arguments",
        "literals",
        "parse_diagnostics",
    ] {
        assert_eq!(count(conn, table), 0, "{table} should be empty");
    }
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

fn symbols_for_path(conn: &Connection, path: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM symbols WHERE path = ?1 ORDER BY name")
        .unwrap();
    stmt.query_map([path], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn file_hash(conn: &Connection, path: &str) -> Option<String> {
    conn.query_row(
        "SELECT content_hash FROM files WHERE path = ?1",
        [path],
        |row| row.get(0),
    )
    .ok()
}

fn revision_changes(conn: &Connection) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare("SELECT path, change_kind FROM revision_file_changes ORDER BY path")
        .unwrap();
    stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn latest_change(conn: &Connection, path: &str) -> Option<String> {
    conn.query_row(
        "SELECT change_kind FROM revision_file_changes \
         WHERE path = ?1 ORDER BY revision_id DESC LIMIT 1",
        [path],
        |row| row.get(0),
    )
    .ok()
}

fn index_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}
