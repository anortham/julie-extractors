use julie_extract_artifact::metadata::ArtifactMetadata;
use julie_extract_artifact::model::{
    ArtifactCapabilityFlags, ArtifactCapabilitySnapshot, ArtifactComplexityMetric, ArtifactFile,
    ArtifactIdentifier, ArtifactLanguageCapabilityFixtureRow, ArtifactLanguageCapabilityGapRow,
    ArtifactLanguageCapabilityRow, ArtifactLiteral, ArtifactParseDiagnostic,
    ArtifactParserInventoryRow, ArtifactPendingRelationship, ArtifactRelationship,
    ArtifactSourceRegion, ArtifactStructuralFact, ArtifactSymbol, ArtifactSymbolAnnotation,
    ArtifactTypeArgument, ArtifactTypeArgumentUsage, ArtifactTypeFact, CapabilityGapStatus,
    FileStatus, ReferenceSiteProvenance, RevisionInput, WriteMode, WriteOperation,
};
use julie_extract_artifact::resolution_store::{ResolutionCounts, record_pending_resolution};
use julie_extract_artifact::writer::{
    ArtifactFileSpool, ArtifactWriteError, ArtifactWriter, ResolutionHookError,
    ResolutionScopeInput,
};
use rusqlite::{Connection, limits::Limit};
use serde_json::json;
use std::path::PathBuf;

#[test]
fn writer_canonicalizes_shared_exact_reference_sites_across_evidence_rows() {
    let mut writer = open_writer();
    let mut file = file_with_symbols(
        "file-a",
        "src/a.rs",
        "blake3:a",
        ["caller", "target", "other"],
    );
    let reference_site_id = "site-file-a-10-16".to_string();

    file.identifiers.push(ArtifactIdentifier {
        identifier_id: "identifier-a".to_string(),
        reference_site_id: reference_site_id.clone(),
        name: "target".to_string(),
        kind: "call".to_string(),
        containing_symbol_id: Some("file-a-symbol-0".to_string()),
        target_symbol_id: Some("file-a-symbol-1".to_string()),
        start_line: 2,
        start_column: 4,
        end_line: 2,
        end_column: 10,
        start_byte: 10,
        end_byte: 16,
        site_is_exact: true,
        site_provenance: ReferenceSiteProvenance::TargetToken,
        ..ArtifactIdentifier::default()
    });
    file.relationships.push(ArtifactRelationship {
        relationship_id: "relationship-a".to_string(),
        reference_site_id: reference_site_id.clone(),
        from_symbol_id: "file-a-symbol-0".to_string(),
        to_symbol_id: "file-a-symbol-1".to_string(),
        kind: "calls".to_string(),
        start_line: Some(2),
        start_column: Some(4),
        end_line: Some(2),
        end_column: Some(10),
        start_byte: Some(10),
        end_byte: Some(16),
        site_is_exact: true,
        site_provenance: ReferenceSiteProvenance::TargetToken,
        ..ArtifactRelationship::default()
    });
    file.relationships.push(ArtifactRelationship {
        relationship_id: "relationship-b".to_string(),
        reference_site_id: reference_site_id.clone(),
        from_symbol_id: "file-a-symbol-0".to_string(),
        to_symbol_id: "file-a-symbol-2".to_string(),
        kind: "uses".to_string(),
        start_line: Some(2),
        start_column: Some(4),
        end_line: Some(2),
        end_column: Some(10),
        start_byte: Some(10),
        end_byte: Some(16),
        site_is_exact: true,
        site_provenance: ReferenceSiteProvenance::TargetToken,
        ..ArtifactRelationship::default()
    });
    file.pending_relationships
        .push(ArtifactPendingRelationship {
            pending_relationship_id: "pending-a".to_string(),
            reference_site_id: reference_site_id.clone(),
            from_symbol_id: "file-a-symbol-0".to_string(),
            caller_scope_symbol_id: Some("file-a-symbol-0".to_string()),
            kind: "calls".to_string(),
            target_display_name: "target".to_string(),
            target_terminal_name: "target".to_string(),
            start_line: 2,
            start_column: Some(4),
            end_line: Some(2),
            end_column: Some(10),
            start_byte: Some(10),
            end_byte: Some(16),
            site_is_exact: true,
            site_provenance: ReferenceSiteProvenance::TargetToken,
            ..ArtifactPendingRelationship::default()
        });

    let result = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file],
        )
        .unwrap();

    assert_eq!(result.rows_written.reference_sites, 1);
    assert_eq!(result.rows_written.relationships, 2);
    assert_eq!(
        writer
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM reference_sites
                 WHERE reference_site_id = ?1
                   AND file_id = 'file-a'
                   AND start_byte = 10
                   AND end_byte = 16
                   AND is_exact = 1
                   AND provenance = 'target_token'",
                [&reference_site_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    for table in ["identifiers", "pending_relationships"] {
        let query = format!("SELECT reference_site_id FROM {table}");
        assert_eq!(
            writer
                .connection()
                .query_row(&query, [], |row| row.get::<_, String>(0))
                .unwrap(),
            reference_site_id
        );
    }
    assert_eq!(
        writer
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM relationships WHERE reference_site_id = ?1",
                [&reference_site_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        2
    );
}

#[test]
fn writer_rejects_conflicting_physical_data_for_one_reference_site_id() {
    let mut writer = open_writer();
    let mut file = file_with_symbols("file-a", "src/a.rs", "blake3:a", ["caller", "target"]);

    for (identifier_id, start_byte, end_byte) in
        [("identifier-a", 10, 16), ("identifier-b", 20, 26)]
    {
        file.identifiers.push(ArtifactIdentifier {
            identifier_id: identifier_id.to_string(),
            reference_site_id: "site-conflict".to_string(),
            name: "target".to_string(),
            kind: "call".to_string(),
            containing_symbol_id: Some("file-a-symbol-0".to_string()),
            target_symbol_id: Some("file-a-symbol-1".to_string()),
            start_line: 2,
            start_column: start_byte,
            end_line: 2,
            end_column: end_byte,
            start_byte,
            end_byte,
            site_is_exact: true,
            site_provenance: ReferenceSiteProvenance::TargetToken,
            ..ArtifactIdentifier::default()
        });
    }

    let error = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file],
        )
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("reference_site identity conflict"),
        "{error}"
    );
}

#[test]
fn writer_module_does_not_own_capability_snapshot_sync_helpers() {
    let writer_source = include_str!("../src/writer.rs");
    for forbidden_definition in [
        "fn load_parser_inventory_keys(",
        "fn load_language_capability_keys(",
        "fn load_language_capability_fixture_keys(",
        "fn load_language_capability_gap_keys(",
        "fn sync_optional_capability_snapshot_in_tx(",
        "fn sync_capability_snapshot_in_tx(",
        "fn upsert_parser_inventory(",
        "fn upsert_language_capability(",
        "fn upsert_language_capability_fixture(",
        "fn upsert_language_capability_gap(",
        "fn bool_int(",
        "fn json_string(",
    ] {
        assert!(
            !writer_source.contains(forbidden_definition),
            "writer.rs still owns capability snapshot sync helper {forbidden_definition}"
        );
    }
}

#[test]
fn writer_module_does_not_own_row_inserter_helpers() {
    let writer_source = include_str!("../src/writer.rs");
    for forbidden_definition in [
        "struct FileRowInserters",
        "struct ChildRowInserters",
        "fn insert_file_row(",
        "fn insert_revision_file_change_row(",
        "fn insert_symbol_rows(",
        "fn update_symbol_parent_rows(",
        "fn insert_symbol_annotations(",
        "fn insert_identifiers(",
        "fn insert_relationships(",
        "fn insert_pending_relationships(",
        "fn insert_type_facts(",
        "fn insert_type_argument_usages(",
        "fn insert_type_arguments(",
        "fn insert_literals(",
        "fn insert_source_regions(",
        "fn insert_structural_facts(",
        "fn insert_complexity_metrics(",
        "fn insert_parse_diagnostics(",
        "fn insert_parse_diagnostics_rows(",
        "fn is_preserved_failure(",
        "fn is_preserved_failure_update(",
        "fn update_failed_preserved_file(",
        "fn replace_parse_diagnostics(",
        "struct SymbolLookup",
        "fn load_symbol_lookup(",
        "fn collect_file_symbol_ids(",
        "fn collect_requested_symbol_ids(",
        "fn load_symbol_lookup_for_requested_ids(",
        "fn load_existing_symbol_ids_for_requested_ids(",
        "fn valid_symbol_id(",
        "struct IdentifierLookup",
        "struct TypeArgumentUsageLookup",
    ] {
        assert!(
            !writer_source.contains(forbidden_definition),
            "writer.rs still owns row inserter helper {forbidden_definition}"
        );
    }
}

#[test]
fn path_writer_uses_bulk_sqlite_pragmas() {
    let temp_dir = unique_temp_dir("path-writer-pragmas");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let db_path = temp_dir.join("artifact.sqlite");

    let writer = ArtifactWriter::open_path(&db_path, artifact_metadata()).unwrap();

    assert_eq!(
        pragma_text(writer.connection(), "journal_mode").to_lowercase(),
        "wal"
    );
    assert_eq!(pragma_i64(writer.connection(), "synchronous"), 1);
    assert_eq!(pragma_i64(writer.connection(), "temp_store"), 2);
    assert_eq!(pragma_i64(writer.connection(), "cache_size"), -131_072);

    drop(writer);
    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn path_writer_truncates_wal_after_successful_write() {
    let temp_dir = unique_temp_dir("path-writer-wal-checkpoint");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let db_path = temp_dir.join("artifact.sqlite");
    let wal_path = wal_path(&db_path);

    let mut writer = ArtifactWriter::open_path(&db_path, artifact_metadata()).unwrap();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_with_all_rows("file-a", "src/a.rs", "hash-a")],
        )
        .unwrap();

    let wal_bytes = std::fs::metadata(&wal_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    assert_eq!(
        wal_bytes, 0,
        "successful writes must checkpoint and truncate the WAL sidecar"
    );
    assert_eq!(count(writer.connection(), "files"), 1);

    drop(writer);
    std::fs::remove_dir_all(temp_dir).unwrap();
}

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
    assert_eq!(result.rows_written.source_regions, 1);
    assert_eq!(result.rows_written.structural_facts, 1);
    assert_eq!(result.rows_written.complexity_metrics, 1);
    assert_eq!(result.rows_written.parse_diagnostics, 1);
    assert_eq!(count(writer.connection(), "symbol_annotations"), 1);
    assert_eq!(count(writer.connection(), "identifiers"), 1);
    assert_eq!(count(writer.connection(), "relationships"), 1);
    assert_eq!(count(writer.connection(), "pending_relationships"), 1);
    assert_eq!(count(writer.connection(), "type_facts"), 1);
    assert_eq!(count(writer.connection(), "type_argument_usages"), 1);
    assert_eq!(count(writer.connection(), "type_arguments"), 1);
    assert_eq!(count(writer.connection(), "literals"), 1);
    assert_eq!(count(writer.connection(), "source_regions"), 1);
    assert_eq!(count(writer.connection(), "structural_facts"), 1);
    assert_eq!(count(writer.connection(), "complexity_metrics"), 1);
    assert_eq!(count(writer.connection(), "parse_diagnostics"), 1);
}

#[test]
fn scan_dedupes_duplicate_structural_fact_ids_before_insert() {
    let mut writer = open_writer();
    let mut file = file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"]);
    let mut fact = ArtifactStructuralFact {
        structural_fact_id: "duplicate-structural-fact".to_string(),
        pattern_id: "rust.unsafe_block.v1".to_string(),
        capture_name: "first".to_string(),
        node_kind: "unsafe_block".to_string(),
        containing_symbol_id: Some("file-a-symbol-0".to_string()),
        start_line: 2,
        start_column: 4,
        end_line: 4,
        end_column: 5,
        start_byte: 32,
        end_byte: 80,
        confidence: 1.0,
        metadata_json: Some(r#"{"first":true}"#.to_string()),
    };
    file.structural_facts.push(fact.clone());
    fact.capture_name = "second".to_string();
    fact.metadata_json = Some(r#"{"second":true}"#.to_string());
    file.structural_facts.push(fact);

    let result = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file],
        )
        .unwrap();

    assert_eq!(result.rows_written.structural_facts, 1);
    assert_eq!(count(writer.connection(), "structural_facts"), 1);
    assert_eq!(
        structural_fact_captures(writer.connection()),
        vec!["first".to_string()],
        "writer-level dedupe should keep the first deterministic fact for an id"
    );
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
        reference_site_id: "site-missing-from-symbol".to_string(),
        from_symbol_id: "missing-symbol".to_string(),
        to_symbol_id: "file-a-symbol-0".to_string(),
        kind: "calls".to_string(),
        confidence: 1.0,
        ..ArtifactRelationship::default()
    });
    file.pending_relationships
        .push(ArtifactPendingRelationship {
            pending_relationship_id: "missing-pending-from-symbol".to_string(),
            reference_site_id: "site-missing-pending-from-symbol".to_string(),
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
        reference_site_id: "site-cross-file-relationship".to_string(),
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
fn spooled_scan_allows_relationships_to_symbols_later_in_spool() {
    let mut writer = open_writer();
    let mut file_a = file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"]);
    file_a.relationships.push(ArtifactRelationship {
        relationship_id: "cross-file-relationship".to_string(),
        reference_site_id: "site-cross-file-relationship".to_string(),
        from_symbol_id: "file-a-symbol-0".to_string(),
        to_symbol_id: "file-b-symbol-0".to_string(),
        kind: "calls".to_string(),
        start_line: Some(1),
        confidence: 1.0,
        ..ArtifactRelationship::default()
    });
    let file_b = file_with_symbols("file-b", "src/b.rs", "hash-b", ["beta"]);
    let snapshot_paths = vec![file_a.path.clone(), file_b.path.clone()];
    let temp_dir = unique_temp_dir("spooled-cross-file");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut spool = ArtifactFileSpool::create(temp_dir.join("files.jsonl")).unwrap();
    spool.push(&file_a).unwrap();
    spool.push(&file_b).unwrap();

    let result = writer
        .write_scan_spooled(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &snapshot_paths,
            &mut spool,
        )
        .unwrap();

    assert_eq!(result.transactions_committed, 1);
    assert_eq!(result.files_changed, 2);
    assert_eq!(result.rows_written.relationships, 1);
    assert_eq!(count(writer.connection(), "relationships"), 1);
    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn scan_resolves_identifier_containing_and_target_symbol_ids() {
    // Guards the identifier FK write: containing/target must resolve to existing symbol ids
    // (same-file AND cross-file within one scan), and references to symbols present in no file
    // must persist as NULL — never a dangling id (which foreign_keys=ON would reject). This
    // pins the resolved values, which the row-count tests never read back.
    let mut writer = open_writer();
    let mut file_a = file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"]);
    file_a.identifiers.push(ArtifactIdentifier {
        identifier_id: "id-resolved".to_string(),
        reference_site_id: "site-id-resolved".to_string(),
        name: "beta".to_string(),
        kind: "call".to_string(),
        containing_symbol_id: Some("file-a-symbol-0".to_string()),
        target_symbol_id: Some("file-b-symbol-0".to_string()),
        ..ArtifactIdentifier::default()
    });
    file_a.identifiers.push(ArtifactIdentifier {
        identifier_id: "id-dangling".to_string(),
        reference_site_id: "site-id-dangling".to_string(),
        name: "ghost".to_string(),
        kind: "call".to_string(),
        containing_symbol_id: Some("does-not-exist".to_string()),
        target_symbol_id: Some("also-missing".to_string()),
        ..ArtifactIdentifier::default()
    });
    let file_b = file_with_symbols("file-b", "src/b.rs", "hash-b", ["beta"]);

    let result = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_a, file_b],
        )
        .unwrap();

    assert_eq!(result.rows_written.identifiers, 2);
    assert_eq!(
        identifier_refs(writer.connection(), "id-resolved"),
        (
            Some("file-a-symbol-0".to_string()),
            Some("file-b-symbol-0".to_string())
        ),
        "valid containing/target must resolve, including cross-file within the same scan"
    );
    assert_eq!(
        identifier_refs(writer.connection(), "id-dangling"),
        (None, None),
        "references to nonexistent symbols must persist as NULL, never a dangling id"
    );
}

#[test]
fn spooled_scan_resolves_identifier_containing_and_target_symbol_ids() {
    // Same guard for the spooled production cold-scan path.
    let mut writer = open_writer();
    let mut file_a = file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"]);
    file_a.identifiers.push(ArtifactIdentifier {
        identifier_id: "id-resolved".to_string(),
        reference_site_id: "site-id-resolved".to_string(),
        name: "beta".to_string(),
        kind: "call".to_string(),
        containing_symbol_id: Some("file-a-symbol-0".to_string()),
        target_symbol_id: Some("file-b-symbol-0".to_string()),
        ..ArtifactIdentifier::default()
    });
    file_a.identifiers.push(ArtifactIdentifier {
        identifier_id: "id-dangling".to_string(),
        reference_site_id: "site-id-dangling".to_string(),
        name: "ghost".to_string(),
        kind: "call".to_string(),
        containing_symbol_id: Some("does-not-exist".to_string()),
        target_symbol_id: None,
        ..ArtifactIdentifier::default()
    });
    let file_b = file_with_symbols("file-b", "src/b.rs", "hash-b", ["beta"]);
    let snapshot_paths = vec![file_a.path.clone(), file_b.path.clone()];
    let temp_dir = unique_temp_dir("spooled-identifier-refs");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut spool = ArtifactFileSpool::create(temp_dir.join("files.jsonl")).unwrap();
    spool.push(&file_a).unwrap();
    spool.push(&file_b).unwrap();

    let result = writer
        .write_scan_spooled(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &snapshot_paths,
            &mut spool,
        )
        .unwrap();

    assert_eq!(result.rows_written.identifiers, 2);
    assert_eq!(
        identifier_refs(writer.connection(), "id-resolved"),
        (
            Some("file-a-symbol-0".to_string()),
            Some("file-b-symbol-0".to_string())
        ),
        "spooled path must resolve containing/target, including cross-file within one spool"
    );
    assert_eq!(
        identifier_refs(writer.connection(), "id-dangling"),
        (None, None),
        "spooled path must null references to nonexistent symbols"
    );
    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn spooled_scan_resolves_symbol_parent_ids() {
    // Guards parent_symbol_id resolution: a nested symbol whose parent exists must persist that
    // parent, a symbol whose parent is present in no file must persist NULL, and a top-level
    // symbol stays NULL. Pins the resolved values that the row-count tests never read back.
    let mut writer = open_writer();
    let mut file = file_with_symbols(
        "file-a",
        "src/a.rs",
        "hash-a",
        ["parent", "child", "orphan"],
    );
    file.symbols[1].parent_symbol_id = Some("file-a-symbol-0".to_string());
    file.symbols[2].parent_symbol_id = Some("does-not-exist".to_string());
    let snapshot_paths = vec![file.path.clone()];
    let temp_dir = unique_temp_dir("spooled-symbol-parent");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut spool = ArtifactFileSpool::create(temp_dir.join("files.jsonl")).unwrap();
    spool.push(&file).unwrap();

    let result = writer
        .write_scan_spooled(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &snapshot_paths,
            &mut spool,
        )
        .unwrap();

    assert_eq!(result.rows_written.symbols, 3);
    assert_eq!(
        symbol_parent(writer.connection(), "file-a-symbol-1"),
        Some("file-a-symbol-0".to_string()),
        "a nested symbol must resolve its parent"
    );
    assert_eq!(
        symbol_parent(writer.connection(), "file-a-symbol-2"),
        None,
        "a parent present in no file must persist as NULL"
    );
    assert_eq!(
        symbol_parent(writer.connection(), "file-a-symbol-0"),
        None,
        "a top-level symbol has no parent"
    );
    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn spooled_scan_allows_child_symbols_before_parent_symbols() {
    let mut writer = open_writer();
    let mut file = file_with_symbols("file-a", "src/a.rs", "hash-a", ["child", "parent"]);
    file.symbols[0].parent_symbol_id = Some("file-a-symbol-1".to_string());

    let snapshot_paths = vec![file.path.clone()];
    let temp_dir = unique_temp_dir("spooled-child-before-parent");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut spool = ArtifactFileSpool::create(temp_dir.join("files.jsonl")).unwrap();
    spool.push(&file).unwrap();

    writer
        .write_scan_spooled(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &snapshot_paths,
            &mut spool,
        )
        .unwrap();

    assert_eq!(
        symbol_parent(writer.connection(), "file-a-symbol-0").as_deref(),
        Some("file-a-symbol-1")
    );
    assert_eq!(foreign_key_violation_count(writer.connection()), 0);
    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn spooled_scan_restores_foreign_keys_enforcement_on_success() {
    // The spooled path may defer FK checks inside the transaction, but the connection-level FK
    // enforcement setting must stay ON for the durable artifact and subsequent writes.
    let mut writer = open_writer();
    assert_eq!(pragma_i64(writer.connection(), "foreign_keys"), 1);

    let file = file_with_all_rows("file-a", "src/a.rs", "hash-a");
    let snapshot_paths = vec![file.path.clone()];
    let temp_dir = unique_temp_dir("spooled-fk-success");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut spool = ArtifactFileSpool::create(temp_dir.join("files.jsonl")).unwrap();
    spool.push(&file).unwrap();

    writer
        .write_scan_spooled(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &snapshot_paths,
            &mut spool,
        )
        .unwrap();

    assert_eq!(
        pragma_i64(writer.connection(), "foreign_keys"),
        1,
        "foreign-key enforcement must be restored after a successful spooled write"
    );
    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn spooled_scan_restores_foreign_keys_enforcement_on_error() {
    // Early-return error paths must not leave the connection-level FK setting changed.
    let mut writer = open_writer();
    assert_eq!(pragma_i64(writer.connection(), "foreign_keys"), 1);

    let file = file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"]);
    let temp_dir = unique_temp_dir("spooled-fk-error");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut spool = ArtifactFileSpool::create(temp_dir.join("files.jsonl")).unwrap();
    spool.push(&file).unwrap();

    let error = writer
        .write_scan_spooled(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[],
            &mut spool,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        ArtifactWriteError::SnapshotMissingSpooledPath { .. }
    ));
    assert_eq!(
        pragma_i64(writer.connection(), "foreign_keys"),
        1,
        "foreign-key enforcement must be restored even when the write returns an error"
    );
    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn spooled_scan_deletes_files_missing_from_snapshot_paths() {
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

    let file_b = file_with_symbols("file-b", "src/b.rs", "hash-b", ["wrong"]);
    let snapshot_paths = vec![file_b.path.clone()];
    let temp_dir = unique_temp_dir("spooled-deleted");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut spool = ArtifactFileSpool::create(temp_dir.join("files.jsonl")).unwrap();
    spool.push(&file_b).unwrap();

    let result = writer
        .write_scan_spooled(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &snapshot_paths,
            &mut spool,
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
    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn spooled_scan_deleting_cross_file_symbol_keeps_references_valid() {
    let mut writer = open_writer();
    let mut file_a = file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"]);
    let file_b = file_with_symbols("file-b", "src/b.rs", "hash-b", ["beta"]);
    file_a.identifiers.push(ArtifactIdentifier {
        identifier_id: "file-a-identifier-to-b".to_string(),
        reference_site_id: "site-file-a-identifier-to-b".to_string(),
        name: "beta".to_string(),
        kind: "call".to_string(),
        target_symbol_id: Some("file-b-symbol-0".to_string()),
        ..ArtifactIdentifier::default()
    });
    file_a.relationships.push(ArtifactRelationship {
        relationship_id: "file-a-relationship-to-b".to_string(),
        reference_site_id: "site-file-a-relationship-to-b".to_string(),
        from_symbol_id: "file-a-symbol-0".to_string(),
        to_symbol_id: "file-b-symbol-0".to_string(),
        kind: "calls".to_string(),
        confidence: 1.0,
        ..ArtifactRelationship::default()
    });
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_a.clone(), file_b],
        )
        .unwrap();

    let snapshot_paths = vec![file_a.path.clone()];
    let temp_dir = unique_temp_dir("spooled-cross-file-delete");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut spool = ArtifactFileSpool::create(temp_dir.join("files.jsonl")).unwrap();
    spool.push(&file_a).unwrap();

    writer
        .write_scan_spooled(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &snapshot_paths,
            &mut spool,
        )
        .unwrap();

    assert_eq!(foreign_key_violation_count(writer.connection()), 0);
    assert_eq!(
        identifier_refs(writer.connection(), "file-a-identifier-to-b"),
        (None, None),
        "deleted target symbols must be nulled on surviving identifiers"
    );
    assert_eq!(
        count(writer.connection(), "relationships"),
        0,
        "relationships to deleted symbols must cascade away"
    );
    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn spooled_scan_rejects_spooled_files_missing_from_snapshot_paths() {
    let mut writer = open_writer();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"])],
        )
        .unwrap();

    let file_a = file_with_symbols("file-a", "src/a.rs", "hash-a2", ["alpha_v2"]);
    let temp_dir = unique_temp_dir("spooled-missing-snapshot-path");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut spool = ArtifactFileSpool::create(temp_dir.join("files.jsonl")).unwrap();
    spool.push(&file_a).unwrap();

    let error = writer
        .write_scan_spooled(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[],
            &mut spool,
        )
        .unwrap_err();

    assert!(matches!(
        error,
        ArtifactWriteError::SnapshotMissingSpooledPath { ref path } if path == "src/a.rs"
    ));
    assert_eq!(
        symbols_for_path(writer.connection(), "src/a.rs"),
        vec!["alpha"]
    );
    assert_eq!(count(writer.connection(), "extraction_revisions"), 1);
    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn scan_batches_symbol_lookup_under_sqlite_variable_limit() {
    let mut writer = open_writer();
    writer
        .connection()
        .set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 64)
        .unwrap();
    let mut file = file_with_many_symbols("wide", "src/wide.rs", "hash-wide", 65);
    file.relationships = (1..65)
        .map(|index| ArtifactRelationship {
            relationship_id: format!("wide-relationship-{index}"),
            reference_site_id: format!("site-wide-relationship-{index}"),
            from_symbol_id: "wide-symbol-0".to_string(),
            to_symbol_id: format!("wide-symbol-{index}"),
            kind: "calls".to_string(),
            start_line: Some(index as i64),
            confidence: 1.0,
            ..ArtifactRelationship::default()
        })
        .collect();

    let result = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file],
        )
        .unwrap();

    assert_eq!(result.rows_written.symbols, 65);
    assert_eq!(result.rows_written.relationships, 64);
    assert_eq!(count(writer.connection(), "relationships"), 64);
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

#[test]
fn capability_snapshot_sync_writes_static_rows_once() {
    let mut writer = open_writer();
    let snapshot = one_language_capability_snapshot();

    let first = writer.sync_capability_snapshot(&snapshot).unwrap();

    assert_eq!(first.parser_inventory, 1);
    assert_eq!(first.language_capabilities, 1);
    assert_eq!(first.language_capability_fixtures, 1);
    assert_eq!(first.language_capability_gaps, 1);
    assert_eq!(count(writer.connection(), "parser_inventory"), 1);
    assert_eq!(count(writer.connection(), "language_capabilities"), 1);
    assert_eq!(
        count(writer.connection(), "language_capability_fixtures"),
        1
    );
    assert_eq!(count(writer.connection(), "language_capability_gaps"), 1);
    assert_eq!(count(writer.connection(), "extraction_revisions"), 0);
    let stored_kind_coverage: String = writer
        .connection()
        .query_row(
            "SELECT kind_coverage_json FROM language_capabilities WHERE language = 'rust'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let stored_kind_coverage: serde_json::Value =
        serde_json::from_str(&stored_kind_coverage).unwrap();
    assert_eq!(
        stored_kind_coverage, snapshot.languages[0].kind_coverage,
        "SQLite kind_coverage_json must preserve the exact kind_coverage object"
    );

    let second = writer.sync_capability_snapshot(&snapshot).unwrap();

    assert!(!second.has_rows());
    assert_eq!(count(writer.connection(), "parser_inventory"), 1);
    assert_eq!(count(writer.connection(), "language_capabilities"), 1);
    assert_eq!(
        count(writer.connection(), "language_capability_fixtures"),
        1
    );
    assert_eq!(count(writer.connection(), "language_capability_gaps"), 1);
    assert_eq!(count(writer.connection(), "extraction_revisions"), 0);
}

#[test]
fn resolution_hook_runs_in_transaction_and_folds_counts() {
    // INVARIANT: the hook fires inside the write transaction AFTER row writes (it
    // can reference the pending row + target symbol just inserted this scan) and
    // BEFORE the revision counts are finalized (its overlay writes fold into
    // counts_json). write_scan is a Full-scope path.
    let mut writer = open_writer();
    let file = file_with_pending("file-a", "src/a.rs", "hash-a");

    let mut fired = 0;
    let result = writer
        .write_scan_with_resolution(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            std::slice::from_ref(&file),
            |tx: &rusqlite::Transaction<'_>, scope: &ResolutionScopeInput| {
                fired += 1;
                assert!(scope.is_full_scan, "write_scan is a Full-scope path");
                assert_eq!(scope.changed_file_ids, vec!["file-a".to_string()]);
                record_pending_resolution(
                    tx,
                    "file-a-pending-1",
                    "file-a-symbol-1",
                    2,
                    0.9,
                    "test",
                    current_revision(tx),
                )
                .map_err(|err| ResolutionHookError::new(err.to_string()))?;
                Ok(ResolutionCounts {
                    pending_resolutions: 1,
                    identifier_resolutions: 0,
                })
            },
        )
        .unwrap();

    assert_eq!(fired, 1, "the hook must fire exactly once");
    assert_eq!(result.resolution.counts.pending_resolutions, 1);
    assert!(result.resolution.failed.is_none());
    // The overlay row persisted, proving the hook ran inside the committed tx and
    // could see the pending/symbol rows written earlier in the same tx.
    assert_eq!(count(writer.connection(), "pending_resolutions"), 1);
    // Folded before update_revision_counts.
    assert_eq!(
        revision_counts_pending_resolutions(writer.connection()),
        1,
        "hook overlay writes must fold into the revision counts_json"
    );
}

#[test]
fn resolution_hook_scope_includes_old_names_on_full_rescan() {
    // INVARIANT: touched_symbol_names carries OLD DB names of a rewritten file,
    // including a symbol the rewrite removed (the incoming file cannot supply it).
    let mut writer = open_writer();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_with_symbols(
                "file-a",
                "src/a.rs",
                "hash-a",
                ["Foo", "Bar"],
            )],
        )
        .unwrap();

    let mut captured = None;
    writer
        .write_scan_with_resolution(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_with_symbols("file-a", "src/a.rs", "hash-a2", ["Bar"])],
            |_tx: &rusqlite::Transaction<'_>, scope: &ResolutionScopeInput| {
                captured = Some(sorted_names(scope));
                Ok(ResolutionCounts::default())
            },
        )
        .unwrap();

    let names = captured.expect("hook must fire on a rewrite");
    assert!(
        names.contains(&"Foo".to_string()),
        "old removed symbol must be in the touched set, got {names:?}"
    );
    assert!(names.contains(&"Bar".to_string()), "got {names:?}");
}

#[test]
fn resolution_hook_scope_includes_old_names_on_update_and_delete() {
    // INVARIANT: update and delete (Delta paths) both seed touched_symbol_names
    // from OLD DB rows of the affected file.
    let mut writer = open_writer();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_with_symbols(
                "file-a",
                "src/a.rs",
                "hash-a",
                ["Foo", "Bar"],
            )],
        )
        .unwrap();

    let mut updated = None;
    writer
        .write_update_with_resolution(
            revision(WriteOperation::Update, Some(WriteMode::SingleFile)),
            &file_with_symbols("file-a", "src/a.rs", "hash-a2", ["Bar"]),
            |_tx: &rusqlite::Transaction<'_>, scope: &ResolutionScopeInput| {
                assert!(!scope.is_full_scan, "write_update is a Delta path");
                updated = Some(sorted_names(scope));
                Ok(ResolutionCounts::default())
            },
        )
        .unwrap();
    let names = updated.expect("hook must fire on update");
    assert!(names.contains(&"Foo".to_string()), "got {names:?}");
    assert!(names.contains(&"Bar".to_string()), "got {names:?}");

    let mut deleted = None;
    writer
        .delete_file_with_resolution(
            revision(WriteOperation::Delete, Some(WriteMode::SingleFile)),
            "src/a.rs",
            |_tx: &rusqlite::Transaction<'_>, scope: &ResolutionScopeInput| {
                assert!(!scope.is_full_scan, "delete_file is a Delta path");
                assert_eq!(scope.changed_file_ids, vec!["file-a".to_string()]);
                deleted = Some(sorted_names(scope));
                Ok(ResolutionCounts::default())
            },
        )
        .unwrap();
    // After the update, the file holds only "Bar"; delete sees the old row names.
    assert_eq!(
        deleted.expect("hook must fire on delete"),
        vec!["Bar".to_string()]
    );
}

#[test]
fn resolution_hook_error_is_non_fatal_and_rolls_back_overlay() {
    // INVARIANT: a hook error never rolls back the scan. Its overlay writes are
    // discarded (savepoint), the message is surfaced, and the counts stay zero.
    let mut writer = open_writer();
    let file = file_with_pending("file-a", "src/a.rs", "hash-a");

    let result = writer
        .write_scan_with_resolution(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            std::slice::from_ref(&file),
            |tx: &rusqlite::Transaction<'_>, _scope: &ResolutionScopeInput| {
                // Write an overlay row, THEN fail — the savepoint must discard it.
                record_pending_resolution(
                    tx,
                    "file-a-pending-1",
                    "file-a-symbol-1",
                    2,
                    0.9,
                    "test",
                    current_revision(tx),
                )
                .map_err(|err| ResolutionHookError::new(err.to_string()))?;
                Err(ResolutionHookError::new("resolver boom"))
            },
        )
        .unwrap();

    // The scan itself committed.
    assert_eq!(count(writer.connection(), "extraction_revisions"), 1);
    assert_eq!(
        symbols_for_path(writer.connection(), "src/a.rs"),
        vec!["alpha", "target"]
    );
    // The failure is surfaced and the counts are zeroed.
    assert_eq!(result.resolution.failed.as_deref(), Some("resolver boom"));
    assert_eq!(result.resolution.counts, ResolutionCounts::default());
    // The hook's overlay write was rolled back — the row stays unresolved.
    assert_eq!(count(writer.connection(), "pending_resolutions"), 0);
    assert_eq!(
        revision_counts_pending_resolutions(writer.connection()),
        0,
        "a failed hook must leave the revision counts truthful (zero)"
    );
}

#[test]
fn resolution_hook_fires_in_spooled_and_unsupported_paths() {
    // INVARIANT: both spooled scan paths (Full) and remove_unsupported_file
    // (Delta) run the hook with the correct scope.
    let mut writer = open_writer();
    let file = file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"]);
    let snapshot_paths = vec![file.path.clone()];
    let temp_dir = unique_temp_dir("resolution-spooled");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let mut spool = ArtifactFileSpool::create(temp_dir.join("files.jsonl")).unwrap();
    spool.push(&file).unwrap();

    let mut spooled = None;
    writer
        .write_scan_spooled_with_resolution(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &snapshot_paths,
            &mut spool,
            |_tx: &rusqlite::Transaction<'_>, scope: &ResolutionScopeInput| {
                spooled = Some((
                    scope.is_full_scan,
                    scope.changed_file_ids.clone(),
                    sorted_names(scope),
                ));
                Ok(ResolutionCounts::default())
            },
        )
        .unwrap();
    let (full, ids, names) = spooled.expect("spooled hook must fire");
    assert!(full, "write_scan_spooled is a Full-scope path");
    assert_eq!(ids, vec!["file-a".to_string()]);
    assert!(names.contains(&"alpha".to_string()), "got {names:?}");

    // Preserving-missing spooled variant: a rewrite carries old + new names.
    let file2 = file_with_symbols("file-a", "src/a.rs", "hash-a2", ["alpha_v2"]);
    let snapshot_paths2 = vec![file2.path.clone()];
    let mut spool2 = ArtifactFileSpool::create(temp_dir.join("files2.jsonl")).unwrap();
    spool2.push(&file2).unwrap();
    let mut preserved = None;
    writer
        .write_scan_spooled_preserving_missing_paths_with_resolution(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &snapshot_paths2,
            &[],
            &mut spool2,
            |_tx: &rusqlite::Transaction<'_>, scope: &ResolutionScopeInput| {
                assert!(scope.is_full_scan);
                preserved = Some(sorted_names(scope));
                Ok(ResolutionCounts::default())
            },
        )
        .unwrap();
    let names2 = preserved.expect("preserving-missing hook must fire");
    assert!(
        names2.contains(&"alpha".to_string()),
        "old name, got {names2:?}"
    );
    assert!(
        names2.contains(&"alpha_v2".to_string()),
        "new name, got {names2:?}"
    );

    // remove_unsupported_file is a Delta path that seeds old names.
    let mut unsupported = None;
    writer
        .remove_unsupported_file_with_resolution(
            revision(WriteOperation::Update, Some(WriteMode::SingleFile)),
            "src/a.rs",
            |_tx: &rusqlite::Transaction<'_>, scope: &ResolutionScopeInput| {
                unsupported = Some((scope.is_full_scan, sorted_names(scope)));
                Ok(ResolutionCounts::default())
            },
        )
        .unwrap();
    let (full3, names3) = unsupported.expect("remove_unsupported hook must fire");
    assert!(!full3, "remove_unsupported_file is a Delta path");
    assert_eq!(names3, vec!["alpha_v2".to_string()]);

    std::fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn hookless_write_leaves_resolution_outcome_empty() {
    // INVARIANT: existing hookless callers see an empty resolution outcome and no
    // overlay rows — the no-op delegation preserves current behavior.
    let mut writer = open_writer();
    let result = writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental)),
            &[file_with_symbols("file-a", "src/a.rs", "hash-a", ["alpha"])],
        )
        .unwrap();

    assert_eq!(result.resolution.counts, ResolutionCounts::default());
    assert!(result.resolution.failed.is_none());
    assert_eq!(count(writer.connection(), "pending_resolutions"), 0);
    assert_eq!(count(writer.connection(), "identifier_resolutions"), 0);
}

fn file_with_pending(file_id: &str, path: &str, hash: &str) -> ArtifactFile {
    let mut file = file_with_symbols(file_id, path, hash, ["alpha", "target"]);
    file.pending_relationships
        .push(ArtifactPendingRelationship {
            pending_relationship_id: format!("{file_id}-pending-1"),
            reference_site_id: format!("{file_id}-pending-site-1"),
            from_symbol_id: format!("{file_id}-symbol-0"),
            kind: "uses".to_string(),
            target_display_name: "target".to_string(),
            target_terminal_name: "target".to_string(),
            ..ArtifactPendingRelationship::default()
        });
    file
}

fn sorted_names(scope: &ResolutionScopeInput) -> Vec<String> {
    let mut names = scope
        .touched_symbol_names
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn current_revision(tx: &rusqlite::Transaction<'_>) -> i64 {
    tx.query_row(
        "SELECT MAX(revision_id) FROM extraction_revisions",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

fn revision_counts_pending_resolutions(conn: &Connection) -> i64 {
    let counts_json: String = conn
        .query_row(
            "SELECT counts_json FROM extraction_revisions ORDER BY revision_id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&counts_json).unwrap();
    value["pending_resolutions"].as_i64().unwrap()
}

fn open_writer() -> ArtifactWriter {
    ArtifactWriter::open_in_memory(artifact_metadata()).unwrap()
}

fn artifact_metadata() -> ArtifactMetadata {
    ArtifactMetadata {
        artifact_id: "artifact-writer-test".to_string(),
        root_path: "/repo".to_string(),
        binary_version: "julie-extract 0.1.0".to_string(),
        hash_algorithm: "blake3".to_string(),
        parser_inventory_fingerprint: "sha256:parser".to_string(),
        capability_snapshot_fingerprint: "sha256:cap".to_string(),
        created_at: "2026-05-31T19:20:00Z".to_string(),
        updated_at: "2026-05-31T19:20:00Z".to_string(),
    }
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()))
}

fn wal_path(db_path: &std::path::Path) -> PathBuf {
    let mut path = db_path.as_os_str().to_os_string();
    path.push("-wal");
    PathBuf::from(path)
}

fn pragma_i64(connection: &Connection, name: &str) -> i64 {
    connection
        .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
        .unwrap()
}

fn pragma_text(connection: &Connection, name: &str) -> String {
    connection
        .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
        .unwrap()
}

fn one_language_capability_snapshot() -> ArtifactCapabilitySnapshot {
    ArtifactCapabilitySnapshot {
        parser_inventory: vec![ArtifactParserInventoryRow {
            language: "rust".to_string(),
            parser_package: "tree-sitter-rust".to_string(),
            parser_version: Some("0.24.2".to_string()),
            grammar_version: None,
            source: Some("capability_snapshot".to_string()),
            metadata: Some(json!({"dependency_status": "current"})),
        }],
        languages: vec![ArtifactLanguageCapabilityRow {
            language: "rust".to_string(),
            parser_package: "tree-sitter-rust".to_string(),
            extensions: vec!["rs".to_string()],
            dependency_status: "current".to_string(),
            target_capabilities: ArtifactCapabilityFlags {
                symbols: true,
                relationships: true,
                pending_relationships: true,
                identifiers: true,
                types: true,
            },
            actual_capabilities: ArtifactCapabilityFlags {
                symbols: true,
                relationships: true,
                pending_relationships: true,
                identifiers: true,
                types: true,
            },
            kind_coverage: json!({
                "symbols": {"supported": ["function"], "not_applicable": [], "open_gaps": []},
                "relationships": {"supported": ["calls"], "not_applicable": [], "open_gaps": []},
                "identifiers": {"supported": ["call"], "not_applicable": [], "open_gaps": []},
                "body_spans": {"supported": ["function"], "not_applicable": [], "open_gaps": []},
                "test_detection": {
                    "supported": [],
                    "not_applicable": [],
                    "open_gaps": [
                        {
                            "kind": "test_case",
                            "reason": "No registered rust golden expected artifact currently emits is_test=true.",
                            "required_closure": "determine language-native applicability for test_case, then either record source-backed not_applicable or register a golden fixture that emits is_test=true",
                            "planned_closure_task": "docs/plans/2026-07-09-test-detection-golden-closure-implementation-plan.md"
                        },
                        {
                            "kind": "test_container",
                            "reason": "No registered rust golden expected artifact currently emits test_container=true.",
                            "required_closure": "determine language-native applicability for test_container, then either record source-backed not_applicable or register a golden fixture that emits test_container=true",
                            "planned_closure_task": "docs/plans/2026-07-09-test-detection-golden-closure-implementation-plan.md"
                        },
                        {
                            "kind": "test_lifecycle",
                            "reason": "No registered rust golden expected artifact currently emits test_lifecycle=true.",
                            "required_closure": "determine language-native applicability for test_lifecycle, then either record source-backed not_applicable or register a golden fixture that emits test_lifecycle=true",
                            "planned_closure_task": "docs/plans/2026-07-09-test-detection-golden-closure-implementation-plan.md"
                        }
                    ]
                }
            }),
            fixtures: vec![ArtifactLanguageCapabilityFixtureRow {
                fixture_name: "basic".to_string(),
                source_path: "fixtures/extraction/rust/basic/source.rs".to_string(),
                expected_path: "fixtures/extraction/rust/basic/expected.json".to_string(),
            }],
            gaps: vec![ArtifactLanguageCapabilityGapRow {
                gap_id: "rust:types".to_string(),
                capability: "types".to_string(),
                status: CapabilityGapStatus::Open,
                reason: "test gap".to_string(),
                required_closure: "add fixture evidence".to_string(),
                evidence: json!({"fixture": "basic"}),
            }],
        }],
    }
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
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        parse_diagnostics: Vec::new(),
    }
}

fn file_with_many_symbols(
    file_id: &str,
    path: &str,
    hash: &str,
    symbol_count: usize,
) -> ArtifactFile {
    ArtifactFile {
        file_id: file_id.to_string(),
        path: path.to_string(),
        language: "rust".to_string(),
        content_hash: hash.to_string(),
        content_bytes: 32,
        line_count: Some(symbol_count as i64),
        indexed_at: "2026-05-31T19:20:00Z".to_string(),
        status: FileStatus::Indexed,
        metadata_json: None,
        symbols: (0..symbol_count)
            .map(|index| ArtifactSymbol {
                symbol_id: format!("{file_id}-symbol-{index}"),
                name: format!("symbol_{index}"),
                kind: "function".to_string(),
                signature: Some(format!("fn symbol_{index}()")),
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
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
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
        reference_site_id: format!("{file_id}-identifier-site-1"),
        name: "beta".to_string(),
        kind: "call".to_string(),
        containing_symbol_id: Some(format!("{file_id}-symbol-0")),
        target_symbol_id: Some(format!("{file_id}-symbol-1")),
        ..ArtifactIdentifier::default()
    });
    file.relationships.push(ArtifactRelationship {
        relationship_id: format!("{file_id}-relationship-1"),
        reference_site_id: format!("{file_id}-relationship-site-1"),
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
            reference_site_id: format!("{file_id}-pending-site-1"),
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
    file.source_regions.push(ArtifactSourceRegion {
        source_region_id: format!("{file_id}-region-1"),
        kind: "comment".to_string(),
        containing_symbol_id: Some(format!("{file_id}-symbol-0")),
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 12,
        start_byte: 0,
        end_byte: 12,
        metadata_json: Some(r#"{"source_region":true}"#.to_string()),
    });
    file.structural_facts.push(ArtifactStructuralFact {
        structural_fact_id: format!("{file_id}-structural-fact-1"),
        pattern_id: "rust.unsafe_block.v1".to_string(),
        capture_name: "unsafe_block".to_string(),
        node_kind: "unsafe_block".to_string(),
        containing_symbol_id: Some(format!("{file_id}-symbol-0")),
        start_line: 2,
        start_column: 4,
        end_line: 4,
        end_column: 5,
        start_byte: 32,
        end_byte: 80,
        confidence: 1.0,
        metadata_json: Some(r#"{"pattern_version":1,"query_family":"safety"}"#.to_string()),
    });
    file.complexity_metrics.push(ArtifactComplexityMetric {
        complexity_metric_id: format!("{file_id}-complexity-symbol-1"),
        scope: "symbol".to_string(),
        symbol_id: Some(format!("{file_id}-symbol-0")),
        algorithm_id: "julie-ast-complexity-v1".to_string(),
        covered_lines: 3,
        covered_bytes: 48,
        decision_count: 1,
        loop_count: 1,
        max_nesting_depth: 2,
        parameter_count: Some(2),
        start_line: 1,
        start_column: 0,
        end_line: 3,
        end_column: 1,
        start_byte: 0,
        end_byte: 48,
        metadata_json: Some(r#"{"metric_version":1}"#.to_string()),
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
        "source_regions",
        "structural_facts",
        "complexity_metrics",
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

fn foreign_key_violation_count(conn: &Connection) -> usize {
    conn.prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |_| Ok(()))
        .unwrap()
        .count()
}

fn structural_fact_captures(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT capture_name FROM structural_facts ORDER BY structural_fact_id")
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn symbol_parent(conn: &Connection, symbol_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT parent_symbol_id FROM symbols WHERE symbol_id = ?1",
        [symbol_id],
        |row| row.get(0),
    )
    .unwrap()
}

fn identifier_refs(conn: &Connection, identifier_id: &str) -> (Option<String>, Option<String>) {
    conn.query_row(
        "SELECT containing_symbol_id, target_symbol_id FROM identifiers WHERE identifier_id = ?1",
        [identifier_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
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
