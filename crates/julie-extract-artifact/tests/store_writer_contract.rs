use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
use julie_extract_artifact::store::{
    StoreConnectionFactory, StoreLayout, StoreVersionState, StoreWriteRequest, StoreWriter,
    StoreWriterError,
};
use julie_extract_artifact::store::{StoreFileVersion, StoreLevel, StoreProjectionError};
use julie_extract_artifact::writer::ArtifactWriter;

const CREATED_AT: &str = "2026-08-07T12:00:00Z";

#[test]
fn store_projection_refuses_non_indexed_or_unhashed_files() {
    let mut failed = fixture_file("src/lib.rs", "rust", "blake3:aaa");
    failed.status = FileStatus::FailedPreserved;
    assert!(matches!(
        StoreFileVersion::try_from_artifact_file(1, failed),
        Err(StoreProjectionError::FileNotIndexed(
            FileStatus::FailedPreserved
        ))
    ));

    let mut unhashed = fixture_file("src/lib.rs", "rust", "");
    unhashed.content_hash.clear();
    assert!(matches!(
        StoreFileVersion::try_from_artifact_file(1, unhashed),
        Err(StoreProjectionError::MissingContentHash)
    ));
}

#[test]
fn relationship_claims_promote_shared_reference_sites_to_l1() {
    let mut file = fixture_file("src/lib.rs", "rust", "blake3:aaa");
    file.identifiers = vec![
        identifier("identifier-shared", "site-shared", 10),
        identifier("identifier-only", "site-identifier", 20),
    ];
    file.relationships = vec![relationship("relationship-shared", "site-shared")];
    file.pending_relationships = vec![pending_relationship("pending-only", "site-pending")];

    let version = StoreFileVersion::try_from_artifact_file(1, file).unwrap();
    let l1_ids = version
        .reference_sites(StoreLevel::L1)
        .iter()
        .map(|site| site.reference_site_id.as_str())
        .collect::<Vec<_>>();
    let l2_ids = version
        .reference_sites(StoreLevel::L2)
        .iter()
        .map(|site| site.reference_site_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(l1_ids, ["site-shared", "site-pending"]);
    assert_eq!(l2_ids, ["site-identifier"]);
    let shared = &version.reference_sites(StoreLevel::L1)[0];
    assert_eq!(shared.start_line, Some(10));
    assert_eq!(shared.level, 1);
    assert_eq!(version.row_counts(StoreLevel::L1).reference_sites, 2);
    assert_eq!(version.row_counts(StoreLevel::L2).reference_sites, 1);
}

#[test]
fn metadata_json_is_preserved_byte_for_byte_in_every_projection() {
    let mut file = fixture_file("src/lib.rs", "rust", "blake3:aaa");
    file.metadata_json = Some(r#"{"z":1,"a":{"y":2,"x":3}}"#.to_string());
    file.symbols[0].metadata_json = Some(r#"{"last":true,"first":false}"#.to_string());

    let version = StoreFileVersion::try_from_artifact_file(1, file.clone()).unwrap();

    assert_eq!(version.file_metadata_json(), file.metadata_json.as_deref());
    assert_eq!(
        version.artifact_file().symbols[0].metadata_json,
        file.symbols[0].metadata_json
    );
}

#[test]
fn stamped_identity_is_reused_and_other_identity_components_allocate_versions() {
    let store = TestStore::new("identity");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let first = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();

    assert!(
        writer
            .lookup_version(first.path(), first.content_hash(), 1, StoreLevel::L1)
            .unwrap()
            .is_none()
    );
    let created = writer
        .write_level(&request("request-1"), &first, StoreLevel::L1)
        .unwrap();
    let reused = writer
        .write_level(&request("request-2"), &first, StoreLevel::L1)
        .unwrap();

    assert_eq!(created.state, StoreVersionState::Created);
    assert_eq!(reused.state, StoreVersionState::Reused);
    assert_eq!(created.version_id, reused.version_id);
    assert_eq!(created.completion_sequence, reused.completion_sequence);
    assert_eq!(reused.counts.total(), 0);

    let changed_hash = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/lib.rs", "rust", "blake3:bbb"),
    )
    .unwrap();
    let changed_path = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/other.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    let changed_epoch = StoreFileVersion::try_from_artifact_file(
        2,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    let ids = [changed_hash, changed_path, changed_epoch]
        .iter()
        .enumerate()
        .map(|(index, version)| {
            if version.extraction_epoch() == 2 {
                writer.stage_capability_snapshot(2, capability_snapshot());
            }
            writer
                .write_level(
                    &request(&format!("request-other-{index}")),
                    version,
                    StoreLevel::L1,
                )
                .unwrap()
                .version_id
        })
        .collect::<Vec<_>>();

    assert!(
        ids.iter()
            .all(|version_id| *version_id != created.version_id)
    );
    assert_eq!(
        ids.iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );
}

#[test]
fn incomplete_version_resumes_without_allocating_a_second_identity() {
    let store = TestStore::new("resume");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let version = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    writer
        .connection()
        .execute(
            "INSERT INTO file_versions
             (path, content_hash, extraction_epoch, language, content_bytes, line_count, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                version.path(),
                version.content_hash(),
                version.extraction_epoch(),
                version.artifact_file().language,
                version.artifact_file().content_bytes,
                version.artifact_file().line_count,
                version.file_metadata_json(),
            ],
        )
        .unwrap();
    let incomplete_id = writer.connection().last_insert_rowid();

    assert!(
        writer
            .lookup_version(version.path(), version.content_hash(), 1, StoreLevel::L1)
            .unwrap()
            .is_none()
    );
    let resumed = writer
        .write_level(&request("request-resume"), &version, StoreLevel::L1)
        .unwrap();

    assert_eq!(resumed.state, StoreVersionState::Incomplete);
    assert_eq!(resumed.version_id, incomplete_id);
    assert!(resumed.completion_sequence > 0);
    assert_eq!(table_count(writer.connection(), "file_versions"), 1);
}

#[test]
fn incomplete_resume_rejects_language_mismatch_without_mutation() {
    assert_immutable_resume_conflict("language = 'csharp'", "resume-language-conflict");
}

#[test]
fn incomplete_resume_rejects_byte_different_metadata_without_mutation() {
    assert_immutable_resume_conflict(
        r#"metadata_json = '{"language": "fixture"}'"#,
        "resume-metadata-conflict",
    );
}

fn assert_immutable_resume_conflict(mutation: &str, name: &str) {
    let store = TestStore::new(name);
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let version = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    let completed = writer
        .write_level(&request("request-seed"), &version, StoreLevel::L1)
        .unwrap();
    writer
        .connection()
        .execute_batch(&format!(
            "UPDATE file_versions SET complete_l1 = NULL, {mutation} WHERE version_id = {}",
            completed.version_id
        ))
        .unwrap();
    let rows_before = snapshot_version_tables(
        writer.connection(),
        completed.version_id,
        &["file_versions", "symbols", "reference_sites"],
    );
    let log_before = selected_rows(
        writer.connection(),
        "store_log",
        &table_columns(writer.connection(), "store_log")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        None,
    );

    let error = writer
        .write_level(&request("request-resume"), &version, StoreLevel::L1)
        .unwrap_err();

    assert!(matches!(
        error,
        StoreWriterError::ImmutableFileConflict { version_id }
            if version_id == completed.version_id
    ));
    assert_eq!(
        snapshot_version_tables(
            writer.connection(),
            completed.version_id,
            &["file_versions", "symbols", "reference_sites"],
        ),
        rows_before
    );
    assert_eq!(
        selected_rows(
            writer.connection(),
            "store_log",
            &table_columns(writer.connection(), "store_log")
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            None,
        ),
        log_before
    );
}

#[test]
fn retained_versions_can_reuse_local_ids_and_composite_parents() {
    let store = TestStore::new("composite-ids");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let first = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    let second = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/lib.rs", "rust", "blake3:bbb"),
    )
    .unwrap();

    let first_result = writer
        .write_level(&request("request-first"), &first, StoreLevel::L1)
        .unwrap();
    let second_result = writer
        .write_level(&request("request-second"), &second, StoreLevel::L1)
        .unwrap();

    assert_ne!(first_result.version_id, second_result.version_id);
    assert_eq!(table_count(writer.connection(), "symbols"), 2);
    assert!(foreign_key_failures(writer.connection()).is_empty());
}

#[test]
fn stamp_failure_rolls_back_the_last_row_and_leaves_the_version_incomplete() {
    let store = TestStore::new("atomic-stamp");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let version = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    writer
        .connection()
        .execute_batch(
            "CREATE TRIGGER refuse_l1_stamp
             BEFORE UPDATE OF complete_l1 ON file_versions
             BEGIN
               SELECT RAISE(ABORT, 'stamp refused');
             END;",
        )
        .unwrap();

    let error = writer
        .write_level(&request("request-atomic"), &version, StoreLevel::L1)
        .unwrap_err();
    assert!(error.to_string().contains("stamp refused"));
    assert_eq!(table_count(writer.connection(), "file_versions"), 0);
    assert_eq!(table_count(writer.connection(), "symbols"), 0);
    assert_eq!(table_count(writer.connection(), "store_log"), 0);
}

#[test]
fn multi_language_versions_persist_every_row_family_at_its_contract_level() {
    let store = TestStore::new("all-row-families");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let versions = [
        fully_populated_file("src/lib.rs", "rust", "blake3:rust"),
        fully_populated_file("src/Program.cs", "csharp", "blake3:csharp"),
    ]
    .into_iter()
    .map(|file| StoreFileVersion::try_from_artifact_file(1, file).unwrap())
    .collect::<Vec<_>>();

    for (index, version) in versions.iter().enumerate() {
        let l1 = writer
            .write_level(
                &request(&format!("request-{index}-l1")),
                version,
                StoreLevel::L1,
            )
            .unwrap();
        let l2 = writer
            .write_level(
                &request(&format!("request-{index}-l2")),
                version,
                StoreLevel::L2,
            )
            .unwrap();
        let l3 = writer
            .write_level(
                &request(&format!("request-{index}-l3")),
                version,
                StoreLevel::L3,
            )
            .unwrap();

        let mut expected_l1 = version.row_counts(StoreLevel::L1);
        if index == 0 {
            expected_l1.parser_inventory = 1;
            expected_l1.language_capabilities = 1;
            expected_l1.language_capability_fixtures = 1;
            expected_l1.language_capability_gaps = 1;
        }
        assert_eq!(l1.counts, expected_l1);
        assert_eq!(l2.counts, version.row_counts(StoreLevel::L2));
        assert_eq!(l3.counts, version.row_counts(StoreLevel::L3));
    }

    let expected_counts = [
        ("file_versions", 2),
        ("symbols", 4),
        ("symbol_annotations", 2),
        ("reference_sites", 6),
        ("identifiers", 4),
        ("relationships", 2),
        ("pending_relationships", 2),
        ("type_facts", 2),
        ("type_argument_usages", 2),
        ("type_arguments", 4),
        ("literals", 2),
        ("source_regions", 2),
        ("structural_facts", 2),
        ("complexity_metrics", 2),
        ("parse_diagnostics", 2),
        ("store_log", 6),
    ];
    for (table, expected) in expected_counts {
        assert_eq!(table_count(writer.connection(), table), expected, "{table}");
    }
    assert_eq!(
        table_count_where(writer.connection(), "reference_sites", "level = 1"),
        4
    );
    assert_eq!(
        table_count_where(writer.connection(), "reference_sites", "level = 2"),
        2
    );
    assert!(foreign_key_failures(writer.connection()).is_empty());
}

#[test]
fn l2_resume_replaces_only_identifier_sites_and_preserves_l1_bytes() {
    let store = TestStore::new("l2-resume");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let version = StoreFileVersion::try_from_artifact_file(
        1,
        fully_populated_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    let l1 = writer
        .write_level(&request("request-l1"), &version, StoreLevel::L1)
        .unwrap();
    let l1_before = reference_site_rows(writer.connection(), l1.version_id, 1);
    writer
        .connection()
        .execute(
            "INSERT INTO reference_sites
             (version_id, reference_site_id, path, language, containing_symbol_id,
              start_line, start_column, end_line, end_column, start_byte, end_byte,
              is_exact, provenance, level)
             VALUES (?1, 'stale-l2', 'src/lib.rs', 'rust', NULL,
                     NULL, NULL, NULL, NULL, NULL, NULL, 0, 'spanless', 2)",
            [l1.version_id],
        )
        .unwrap();

    let resumed = writer
        .write_level(&request("request-l2"), &version, StoreLevel::L2)
        .unwrap();

    assert_eq!(resumed.state, StoreVersionState::Incomplete);
    assert_eq!(
        reference_site_rows(writer.connection(), l1.version_id, 1),
        l1_before
    );
    assert_eq!(
        reference_site_rows(writer.connection(), l1.version_id, 2)
            .into_iter()
            .map(|row| row[0].clone())
            .collect::<Vec<_>>(),
        [rusqlite::types::Value::Text("site-identifier".to_string())]
    );
    assert!(
        writer
            .lookup_version(version.path(), version.content_hash(), 1, StoreLevel::L2)
            .unwrap()
            .is_some()
    );
}

#[test]
fn l3_resume_replaces_only_l3_rows_and_preserves_earlier_levels_byte_for_byte() {
    let store = TestStore::new("l3-resume");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let version = StoreFileVersion::try_from_artifact_file(
        1,
        fully_populated_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    let l1 = writer
        .write_level(&request("request-l1"), &version, StoreLevel::L1)
        .unwrap();
    writer
        .write_level(&request("request-l2"), &version, StoreLevel::L2)
        .unwrap();
    let earlier_before = snapshot_version_tables(
        writer.connection(),
        l1.version_id,
        &[
            "symbols",
            "symbol_annotations",
            "reference_sites",
            "identifiers",
            "relationships",
            "pending_relationships",
            "type_facts",
            "complexity_metrics",
            "parse_diagnostics",
        ],
    );
    writer
        .connection()
        .execute(
            "INSERT INTO structural_facts
             (version_id, structural_fact_id, path, language, pattern_id, capture_name,
              node_kind, containing_symbol_id, start_line, start_column, end_line, end_column,
              start_byte, end_byte, confidence, metadata_json)
             VALUES (?1, 'stale-fact', 'src/lib.rs', 'rust', 'stale', 'stale', 'stale',
                     NULL, 1, 0, 1, 1, 0, 1, 1.0, NULL)",
            [l1.version_id],
        )
        .unwrap();

    let resumed = writer
        .write_level(&request("request-l3"), &version, StoreLevel::L3)
        .unwrap();

    assert_eq!(resumed.state, StoreVersionState::Incomplete);
    assert_eq!(
        snapshot_version_tables(
            writer.connection(),
            l1.version_id,
            &[
                "symbols",
                "symbol_annotations",
                "reference_sites",
                "identifiers",
                "relationships",
                "pending_relationships",
                "type_facts",
                "complexity_metrics",
                "parse_diagnostics",
            ],
        ),
        earlier_before
    );
    assert_eq!(
        table_count_where(
            writer.connection(),
            "structural_facts",
            "structural_fact_id = 'stale-fact'"
        ),
        0
    );
}

#[test]
fn complete_store_rows_match_v3_extraction_payloads_before_resolution() {
    let file = fully_populated_file("src/lib.rs", "rust", "blake3:aaa");
    let mut legacy = ArtifactWriter::open_in_memory(artifact_metadata()).unwrap();
    legacy
        .write_scan(artifact_revision(), std::slice::from_ref(&file))
        .unwrap();

    let store = TestStore::new("v3-equivalence");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let version = StoreFileVersion::try_from_artifact_file(1, file).unwrap();
    let l1 = writer
        .write_level(&request("request-l1"), &version, StoreLevel::L1)
        .unwrap();
    writer
        .write_level(&request("request-l2"), &version, StoreLevel::L2)
        .unwrap();
    writer
        .write_level(&request("request-l3"), &version, StoreLevel::L3)
        .unwrap();

    let file_columns = [
        "path",
        "language",
        "content_hash",
        "content_bytes",
        "line_count",
        "metadata_json",
    ];
    assert_eq!(
        selected_rows(
            writer.connection(),
            "file_versions",
            &file_columns,
            Some(("version_id", l1.version_id)),
        ),
        selected_rows(legacy.connection(), "files", &file_columns, None)
    );
    for table in [
        "symbols",
        "symbol_annotations",
        "reference_sites",
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
        let columns = table_columns(writer.connection(), table)
            .into_iter()
            .filter(|column| column != "version_id" && column != "level")
            .collect::<Vec<_>>();
        let legacy_columns = table_columns(legacy.connection(), table);
        assert!(
            columns.iter().all(|column| legacy_columns.contains(column)),
            "{table} store payload columns must all exist in v3"
        );
        let column_refs = columns.iter().map(String::as_str).collect::<Vec<_>>();
        assert_eq!(
            selected_rows(
                writer.connection(),
                table,
                &column_refs,
                Some(("version_id", l1.version_id)),
            ),
            selected_rows(legacy.connection(), table, &column_refs, None),
            "{table}"
        );
    }
}

#[test]
fn duplicate_structural_fact_ids_keep_first_payload_and_remain_version_local() {
    let mut file = fully_populated_file("src/lib.rs", "rust", "blake3:aaa");
    let mut duplicate = file.structural_facts[0].clone();
    duplicate.pattern_id = "fixture.later-loser.v1".to_string();
    duplicate.metadata_json = Some(r#"{"winner":false}"#.to_string());
    file.structural_facts.push(duplicate);

    let mut legacy = ArtifactWriter::open_in_memory(artifact_metadata()).unwrap();
    legacy
        .write_scan(artifact_revision(), std::slice::from_ref(&file))
        .unwrap();

    let store = TestStore::new("duplicate-structural-facts");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let first = StoreFileVersion::try_from_artifact_file(1, file.clone()).unwrap();
    let first_l1 = writer
        .write_level(&request("request-first-l1"), &first, StoreLevel::L1)
        .unwrap();
    writer
        .write_level(&request("request-first-l2"), &first, StoreLevel::L2)
        .unwrap();
    let first_l3 = writer
        .write_level(&request("request-first-l3"), &first, StoreLevel::L3)
        .unwrap();

    assert_eq!(first.row_counts(StoreLevel::L3).structural_facts, 1);
    assert_eq!(first_l3.counts.structural_facts, 1);
    let columns = table_columns(writer.connection(), "structural_facts")
        .into_iter()
        .filter(|column| column != "version_id")
        .collect::<Vec<_>>();
    let column_refs = columns.iter().map(String::as_str).collect::<Vec<_>>();
    assert_eq!(
        selected_rows(
            writer.connection(),
            "structural_facts",
            &column_refs,
            Some(("version_id", first_l1.version_id)),
        ),
        selected_rows(legacy.connection(), "structural_facts", &column_refs, None)
    );
    assert_eq!(
        writer
            .connection()
            .query_row(
                "SELECT pattern_id, metadata_json FROM structural_facts WHERE version_id = ?1",
                [first_l1.version_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .unwrap(),
        (
            "fixture.pattern.v1".to_string(),
            Some(r#"{"fact":true}"#.to_string())
        )
    );

    let mut next_file = file.clone();
    next_file.content_hash = "blake3:bbb".to_string();
    let next = StoreFileVersion::try_from_artifact_file(1, next_file).unwrap();
    writer
        .write_level(&request("request-next-l1"), &next, StoreLevel::L1)
        .unwrap();
    writer
        .write_level(&request("request-next-l2"), &next, StoreLevel::L2)
        .unwrap();
    writer
        .write_level(&request("request-next-l3"), &next, StoreLevel::L3)
        .unwrap();
    assert_eq!(table_count(writer.connection(), "structural_facts"), 2);
}

#[test]
fn later_levels_refuse_to_run_before_their_predecessor_stamp() {
    let store = TestStore::new("level-order");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let version = StoreFileVersion::try_from_artifact_file(
        1,
        fully_populated_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();

    let l2_error = writer
        .write_level(&request("request-l2"), &version, StoreLevel::L2)
        .unwrap_err();
    assert!(l2_error.to_string().contains("L1"));

    writer
        .write_level(&request("request-l1"), &version, StoreLevel::L1)
        .unwrap();
    let l3_error = writer
        .write_level(&request("request-l3"), &version, StoreLevel::L3)
        .unwrap_err();
    assert!(l3_error.to_string().contains("L2"));
}

#[test]
fn l2_stamp_failure_rolls_back_only_l2_and_keeps_l1_complete() {
    let store = TestStore::new("l2-atomic-stamp");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let version = StoreFileVersion::try_from_artifact_file(
        1,
        fully_populated_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    let l1 = writer
        .write_level(&request("request-l1"), &version, StoreLevel::L1)
        .unwrap();
    writer
        .connection()
        .execute_batch(
            "CREATE TRIGGER refuse_l2_stamp
             BEFORE UPDATE OF complete_l2 ON file_versions
             BEGIN
               SELECT RAISE(ABORT, 'l2 stamp refused');
             END;",
        )
        .unwrap();

    let error = writer
        .write_level(&request("request-l2"), &version, StoreLevel::L2)
        .unwrap_err();

    assert!(error.to_string().contains("l2 stamp refused"));
    assert_eq!(table_count(writer.connection(), "identifiers"), 0);
    assert_eq!(
        table_count_where(writer.connection(), "reference_sites", "level = 2"),
        0
    );
    let stamps = writer
        .connection()
        .query_row(
            "SELECT complete_l1, complete_l2 FROM file_versions WHERE version_id = ?1",
            [l1.version_id],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .unwrap();
    assert_eq!(stamps, (Some(l1.completion_sequence), None));
}

#[test]
fn capability_rows_are_epoch_global_and_not_duplicated_by_requests() {
    let store = TestStore::new("epoch-capabilities");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let first = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    let second = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/other.rs", "rust", "blake3:bbb"),
    )
    .unwrap();
    let next_epoch = StoreFileVersion::try_from_artifact_file(
        2,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();

    let first_result = writer
        .write_level(&request("request-cap-1"), &first, StoreLevel::L1)
        .unwrap();
    let second_result = writer
        .write_level(&request("request-cap-2"), &second, StoreLevel::L1)
        .unwrap();
    writer.stage_capability_snapshot(2, capability_snapshot());
    let next_epoch_result = writer
        .write_level(&request("request-cap-3"), &next_epoch, StoreLevel::L1)
        .unwrap();

    assert_eq!(first_result.counts.parser_inventory, 1);
    assert_eq!(second_result.counts.parser_inventory, 0);
    assert_eq!(next_epoch_result.counts.parser_inventory, 1);
    for table in [
        "parser_inventory",
        "language_capabilities",
        "language_capability_fixtures",
        "language_capability_gaps",
    ] {
        assert_eq!(table_count(writer.connection(), table), 2, "{table}");
    }
}

#[test]
fn uninitialized_epoch_rejects_missing_empty_and_stale_capability_snapshots() {
    let store = TestStore::new("capability-required");
    let mut writer = store.writer();
    let epoch_two = StoreFileVersion::try_from_artifact_file(
        2,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();

    let missing = writer
        .write_level(&request("request-missing"), &epoch_two, StoreLevel::L1)
        .unwrap_err();
    assert!(matches!(
        missing,
        StoreWriterError::CapabilitySnapshotRequired {
            extraction_epoch: 2
        }
    ));

    writer.stage_capability_snapshot(1, capability_snapshot());
    let stale = writer
        .write_level(&request("request-stale"), &epoch_two, StoreLevel::L1)
        .unwrap_err();
    assert!(matches!(
        stale,
        StoreWriterError::CapabilitySnapshotEpochMismatch {
            staged_epoch: 1,
            requested_epoch: 2
        }
    ));

    writer.stage_capability_snapshot(
        2,
        ArtifactCapabilitySnapshot {
            parser_inventory: Vec::new(),
            languages: Vec::new(),
        },
    );
    let empty = writer
        .write_level(&request("request-empty"), &epoch_two, StoreLevel::L1)
        .unwrap_err();
    assert!(matches!(
        empty,
        StoreWriterError::EmptyCapabilitySnapshot {
            extraction_epoch: 2
        }
    ));
    assert_eq!(table_count(writer.connection(), "file_versions"), 0);
    assert_eq!(table_count(writer.connection(), "store_log"), 0);
}

#[test]
fn initialized_epoch_needs_no_restaging_but_next_epoch_does() {
    let store = TestStore::new("capability-epoch-lifecycle");
    let mut writer = store.writer();
    let first = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    writer.stage_capability_snapshot(1, capability_snapshot());
    writer
        .write_level(&request("request-first"), &first, StoreLevel::L1)
        .unwrap();

    let second = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/other.rs", "rust", "blake3:bbb"),
    )
    .unwrap();
    writer
        .write_level(&request("request-second"), &second, StoreLevel::L1)
        .unwrap();

    let next_epoch = StoreFileVersion::try_from_artifact_file(
        2,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    let error = writer
        .write_level(
            &request("request-next-missing"),
            &next_epoch,
            StoreLevel::L1,
        )
        .unwrap_err();
    assert!(matches!(
        error,
        StoreWriterError::CapabilitySnapshotRequired {
            extraction_epoch: 2
        }
    ));

    writer.stage_capability_snapshot(2, capability_snapshot());
    writer
        .write_level(&request("request-next"), &next_epoch, StoreLevel::L1)
        .unwrap();
    assert_eq!(table_count(writer.connection(), "language_capabilities"), 2);
}

#[test]
fn writer_selects_bulk_and_routine_wal_autocheckpoints_per_request() {
    let store = TestStore::new("wal-autocheckpoint");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let first = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    writer
        .write_level(
            &StoreWriteRequest::bulk("request-bulk", CREATED_AT),
            &first,
            StoreLevel::L1,
        )
        .unwrap();
    assert_eq!(pragma_i64(writer.connection(), "wal_autocheckpoint"), 8_000);

    let second = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/other.rs", "rust", "blake3:bbb"),
    )
    .unwrap();
    writer
        .write_level(&request("request-routine"), &second, StoreLevel::L1)
        .unwrap();
    assert_eq!(pragma_i64(writer.connection(), "wal_autocheckpoint"), 1_000);
}

#[test]
fn projection_applies_v3_validity_filters_and_spanless_normalization() {
    let mut file = fully_populated_file("src/lib.rs", "rust", "blake3:aaa");
    file.symbol_annotations.push(ArtifactSymbolAnnotation {
        annotation_id: "invalid-annotation".to_string(),
        symbol_id: "missing-symbol".to_string(),
        annotation: "invalid".to_string(),
        annotation_key: "invalid".to_string(),
        raw_text: None,
        carrier: None,
        metadata_json: None,
    });
    file.relationships.push(ArtifactRelationship {
        relationship_id: "invalid-relationship".to_string(),
        to_symbol_id: "missing-symbol".to_string(),
        ..relationship("invalid-relationship", "site-identifier")
    });
    file.pending_relationships
        .push(ArtifactPendingRelationship {
            pending_relationship_id: "invalid-pending".to_string(),
            from_symbol_id: "missing-symbol".to_string(),
            ..pending_relationship("invalid-pending", "site-identifier")
        });
    file.type_facts.push(ArtifactTypeFact {
        type_fact_id: "invalid-type-fact".to_string(),
        symbol_id: "missing-symbol".to_string(),
        resolved_type: "Missing".to_string(),
        generic_params_json: None,
        constraints_json: None,
        is_inferred: false,
        metadata_json: None,
    });
    file.type_argument_usages.push(ArtifactTypeArgumentUsage {
        usage_id: "invalid-usage".to_string(),
        identifier_id: "missing-identifier".to_string(),
        metadata_json: None,
    });
    file.literals[0].containing_symbol_id = Some("missing-symbol".to_string());
    file.source_regions[0].containing_symbol_id = Some("missing-symbol".to_string());
    file.structural_facts[0].containing_symbol_id = Some("missing-symbol".to_string());
    file.complexity_metrics[0].symbol_id = Some("missing-symbol".to_string());
    file.identifiers[1].site_is_exact = false;
    file.identifiers[1].site_provenance = ReferenceSiteProvenance::Spanless;

    let version = StoreFileVersion::try_from_artifact_file(1, file).unwrap();
    let projected = version.artifact_file();

    assert_eq!(projected.symbol_annotations.len(), 1);
    assert_eq!(projected.relationships.len(), 1);
    assert_eq!(projected.pending_relationships.len(), 1);
    assert_eq!(projected.type_facts.len(), 1);
    assert_eq!(projected.type_argument_usages.len(), 1);
    assert_eq!(projected.literals[0].containing_symbol_id, None);
    assert_eq!(projected.source_regions[0].containing_symbol_id, None);
    assert_eq!(projected.structural_facts[0].containing_symbol_id, None);
    assert_eq!(projected.complexity_metrics[0].symbol_id, None);
    let spanless = &version.reference_sites(StoreLevel::L2)[0];
    assert_eq!(spanless.reference_site_id, "site-identifier");
    assert_eq!(
        (
            spanless.start_line,
            spanless.start_column,
            spanless.end_line,
            spanless.end_column,
            spanless.start_byte,
            spanless.end_byte,
        ),
        (None, None, None, None, None, None)
    );
    assert!(!spanless.is_exact);
    assert_eq!(spanless.provenance, "spanless");
}

#[test]
fn writer_open_reaps_retired_reference_resolution_gap_rows() {
    let store = TestStore::new("retired-resolution-gaps");
    {
        let mut writer = store.writer();
        let mut legacy = capability_snapshot();
        legacy.languages[0]
            .gaps
            .push(ArtifactLanguageCapabilityGapRow {
                gap_id: "rust-tier3-receiver".to_string(),
                capability: "reference_resolution.tier3_receiver".to_string(),
                status: CapabilityGapStatus::Open,
                reason: "legacy".to_string(),
                required_closure: "none".to_string(),
                evidence: serde_json::json!({}),
            });
        writer.stage_capability_snapshot(1, legacy);
        let first = StoreFileVersion::try_from_artifact_file(
            1,
            fixture_file("src/lib.rs", "rust", "blake3:aaa"),
        )
        .unwrap();
        writer
            .write_level(&request("request-legacy"), &first, StoreLevel::L1)
            .unwrap();
        assert_eq!(
            table_count(writer.connection(), "language_capability_gaps"),
            2
        );
    }

    let mut writer = store.writer();
    assert_eq!(
        table_count(writer.connection(), "language_capability_gaps"),
        1
    );
    writer.stage_capability_snapshot(1, capability_snapshot());
    let second = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/other.rs", "rust", "blake3:bbb"),
    )
    .unwrap();
    writer
        .write_level(&request("request-current"), &second, StoreLevel::L1)
        .unwrap();
}

#[test]
fn conflicting_capability_snapshot_for_an_existing_epoch_is_rejected() {
    let store = TestStore::new("capability-conflict");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let first = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    writer
        .write_level(&request("request-first"), &first, StoreLevel::L1)
        .unwrap();

    let mut conflicting = capability_snapshot();
    conflicting.parser_inventory[0].parser_package = "other-parser".to_string();
    conflicting.languages[0].parser_package = "other-parser".to_string();
    writer.stage_capability_snapshot(1, conflicting);
    let second = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/other.rs", "rust", "blake3:bbb"),
    )
    .unwrap();

    let error = writer
        .write_level(&request("request-second"), &second, StoreLevel::L1)
        .unwrap_err();

    assert!(error.to_string().contains("capability snapshot conflict"));
    assert_eq!(table_count(writer.connection(), "file_versions"), 1);
    assert_eq!(table_count(writer.connection(), "language_capabilities"), 1);
}

fn fixture_file(path: &str, language: &str, content_hash: &str) -> ArtifactFile {
    ArtifactFile {
        file_id: format!("file-{language}"),
        path: path.to_string(),
        language: language.to_string(),
        content_hash: content_hash.to_string(),
        content_bytes: 42,
        line_count: Some(3),
        indexed_at: "2026-08-07T12:00:00Z".to_string(),
        status: FileStatus::Indexed,
        metadata_json: Some(r#"{"language":"fixture"}"#.to_string()),
        symbols: vec![ArtifactSymbol {
            symbol_id: "symbol-root".to_string(),
            name: "root".to_string(),
            kind: "function".to_string(),
            start_line: 1,
            end_line: 3,
            end_byte: 42,
            ..ArtifactSymbol::default()
        }],
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

fn fully_populated_file(path: &str, language: &str, content_hash: &str) -> ArtifactFile {
    let mut file = fixture_file(path, language, content_hash);
    file.symbols.push(ArtifactSymbol {
        symbol_id: "symbol-child".to_string(),
        name: "child".to_string(),
        kind: "function".to_string(),
        parent_symbol_id: Some("symbol-root".to_string()),
        start_line: 2,
        end_line: 2,
        start_byte: 10,
        end_byte: 20,
        ..ArtifactSymbol::default()
    });
    file.symbol_annotations = vec![ArtifactSymbolAnnotation {
        annotation_id: "annotation-1".to_string(),
        symbol_id: "symbol-root".to_string(),
        annotation: "test".to_string(),
        annotation_key: "test".to_string(),
        raw_text: Some("#[test]".to_string()),
        carrier: Some("attribute".to_string()),
        metadata_json: Some(r#"{"annotation":true}"#.to_string()),
    }];
    file.identifiers = vec![
        identifier("identifier-shared", "site-shared", 10),
        identifier("identifier-only", "site-identifier", 20),
    ];
    file.relationships = vec![ArtifactRelationship {
        to_symbol_id: "symbol-child".to_string(),
        ..relationship("relationship-shared", "site-shared")
    }];
    file.pending_relationships = vec![pending_relationship("pending-only", "site-pending")];
    file.type_facts = vec![ArtifactTypeFact {
        type_fact_id: "type-fact-1".to_string(),
        symbol_id: "symbol-root".to_string(),
        resolved_type: "Fixture".to_string(),
        generic_params_json: Some("[]".to_string()),
        constraints_json: Some("[]".to_string()),
        is_inferred: false,
        metadata_json: Some(r#"{"type":true}"#.to_string()),
    }];
    file.type_argument_usages = vec![ArtifactTypeArgumentUsage {
        usage_id: "usage-1".to_string(),
        identifier_id: "identifier-only".to_string(),
        metadata_json: Some(r#"{"usage":true}"#.to_string()),
    }];
    file.type_arguments = vec![
        ArtifactTypeArgument {
            type_argument_id: "type-argument-parent".to_string(),
            usage_id: "usage-1".to_string(),
            parent_type_argument_id: None,
            ordinal: 0,
            type_name: "Vec".to_string(),
        },
        ArtifactTypeArgument {
            type_argument_id: "type-argument-child".to_string(),
            usage_id: "usage-1".to_string(),
            parent_type_argument_id: Some("type-argument-parent".to_string()),
            ordinal: 1,
            type_name: "String".to_string(),
        },
    ];
    file.literals = vec![ArtifactLiteral {
        literal_id: "literal-1".to_string(),
        literal_text: "fixture".to_string(),
        kind: "string".to_string(),
        carrier: Some("argument".to_string()),
        arg_position: 0,
        containing_symbol_id: Some("symbol-root".to_string()),
        start_line: 2,
        start_column: 1,
        end_line: 2,
        end_column: 8,
        start_byte: 10,
        end_byte: 17,
        confidence: 1.0,
        metadata_json: Some(r#"{"literal":true}"#.to_string()),
    }];
    file.source_regions = vec![ArtifactSourceRegion {
        source_region_id: "region-1".to_string(),
        kind: "doc_comment".to_string(),
        containing_symbol_id: Some("symbol-root".to_string()),
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 9,
        start_byte: 0,
        end_byte: 9,
        metadata_json: Some(r#"{"region":true}"#.to_string()),
    }];
    file.structural_facts = vec![ArtifactStructuralFact {
        structural_fact_id: "fact-1".to_string(),
        pattern_id: "fixture.pattern.v1".to_string(),
        capture_name: "fixture".to_string(),
        node_kind: "function".to_string(),
        containing_symbol_id: Some("symbol-root".to_string()),
        start_line: 1,
        start_column: 0,
        end_line: 3,
        end_column: 0,
        start_byte: 0,
        end_byte: 42,
        confidence: 1.0,
        metadata_json: Some(r#"{"fact":true}"#.to_string()),
    }];
    file.complexity_metrics = vec![ArtifactComplexityMetric {
        complexity_metric_id: "complexity-1".to_string(),
        scope: "symbol".to_string(),
        symbol_id: Some("symbol-root".to_string()),
        algorithm_id: "julie-ast-complexity-v1".to_string(),
        covered_lines: 3,
        covered_bytes: 42,
        decision_count: 1,
        loop_count: 1,
        max_nesting_depth: 1,
        parameter_count: Some(0),
        start_line: 1,
        start_column: 0,
        end_line: 3,
        end_column: 0,
        start_byte: 0,
        end_byte: 42,
        metadata_json: Some(r#"{"complexity":true}"#.to_string()),
    }];
    file.parse_diagnostics = vec![ArtifactParseDiagnostic {
        diagnostic_id: "diagnostic-1".to_string(),
        kind: "warning".to_string(),
        message: Some("fixture warning".to_string()),
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 1,
        start_byte: 0,
        end_byte: 1,
        metadata_json: Some(r#"{"diagnostic":true}"#.to_string()),
    }];
    file
}

fn capability_snapshot() -> ArtifactCapabilitySnapshot {
    ArtifactCapabilitySnapshot {
        parser_inventory: vec![ArtifactParserInventoryRow {
            language: "rust".to_string(),
            parser_package: "tree-sitter-rust".to_string(),
            parser_version: Some("1.0.0".to_string()),
            grammar_version: Some("1".to_string()),
            source: Some("built-in".to_string()),
            metadata: Some(serde_json::json!({"z": 1, "a": 2})),
        }],
        languages: vec![ArtifactLanguageCapabilityRow {
            language: "rust".to_string(),
            parser_package: "tree-sitter-rust".to_string(),
            extensions: vec![".rs".to_string()],
            dependency_status: "available".to_string(),
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
            kind_coverage: serde_json::json!({"function": true}),
            fixtures: vec![ArtifactLanguageCapabilityFixtureRow {
                fixture_name: "basic".to_string(),
                source_path: "fixtures/rust/basic.rs".to_string(),
                expected_path: "fixtures/rust/basic.json".to_string(),
            }],
            gaps: vec![ArtifactLanguageCapabilityGapRow {
                gap_id: "rust-gap".to_string(),
                capability: "fixture".to_string(),
                status: CapabilityGapStatus::Exception,
                reason: "fixture".to_string(),
                required_closure: "none".to_string(),
                evidence: serde_json::json!({"accepted": true}),
            }],
        }],
    }
}

fn identifier(identifier_id: &str, reference_site_id: &str, start_line: i64) -> ArtifactIdentifier {
    ArtifactIdentifier {
        identifier_id: identifier_id.to_string(),
        reference_site_id: reference_site_id.to_string(),
        name: identifier_id.to_string(),
        kind: "call".to_string(),
        containing_symbol_id: Some("symbol-root".to_string()),
        start_line,
        start_column: 1,
        end_line: start_line,
        end_column: 4,
        start_byte: start_line * 10,
        end_byte: start_line * 10 + 3,
        site_is_exact: true,
        site_provenance: ReferenceSiteProvenance::TargetToken,
        confidence: 1.0,
        code_context: None,
        metadata_json: Some(r#"{"source":"identifier"}"#.to_string()),
    }
}

fn relationship(relationship_id: &str, reference_site_id: &str) -> ArtifactRelationship {
    ArtifactRelationship {
        relationship_id: relationship_id.to_string(),
        reference_site_id: reference_site_id.to_string(),
        from_symbol_id: "symbol-root".to_string(),
        to_symbol_id: "symbol-root".to_string(),
        kind: "calls".to_string(),
        start_line: Some(30),
        start_column: Some(1),
        end_line: Some(30),
        end_column: Some(4),
        start_byte: Some(300),
        end_byte: Some(303),
        site_is_exact: true,
        site_provenance: ReferenceSiteProvenance::TargetToken,
        confidence: 1.0,
        metadata_json: Some(r#"{"source":"relationship"}"#.to_string()),
    }
}

fn pending_relationship(
    pending_relationship_id: &str,
    reference_site_id: &str,
) -> ArtifactPendingRelationship {
    ArtifactPendingRelationship {
        pending_relationship_id: pending_relationship_id.to_string(),
        reference_site_id: reference_site_id.to_string(),
        from_symbol_id: "symbol-root".to_string(),
        caller_scope_symbol_id: Some("symbol-root".to_string()),
        kind: "calls".to_string(),
        target_display_name: "missing".to_string(),
        target_terminal_name: "missing".to_string(),
        target_receiver: None,
        target_namespace_json: "[]".to_string(),
        target_import_context: None,
        start_line: 31,
        start_column: Some(1),
        end_line: Some(31),
        end_column: Some(4),
        start_byte: Some(310),
        end_byte: Some(313),
        site_is_exact: true,
        site_provenance: ReferenceSiteProvenance::TargetToken,
        confidence: 1.0,
        metadata_json: Some(r#"{"source":"pending"}"#.to_string()),
    }
}

#[test]
fn capability_snapshot_sync_moves_store_meta_to_the_written_epoch() {
    let store = TestStore::new("meta-epoch");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let version = StoreFileVersion::try_from_artifact_file(
        1,
        fixture_file("src/lib.rs", "rust", "blake3:aaa"),
    )
    .unwrap();
    writer
        .write_level(&request("request-1"), &version, StoreLevel::L1)
        .unwrap();

    let connection = rusqlite::Connection::open(store.layout.store_db()).unwrap();
    let recorded: String = connection
        .query_row(
            "SELECT value FROM store_meta WHERE key='extraction_identity_epoch'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(recorded, "1");
}

fn request(request_id: &str) -> StoreWriteRequest {
    StoreWriteRequest::routine(request_id, CREATED_AT)
}

fn table_count(connection: &rusqlite::Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn table_count_where(connection: &rusqlite::Connection, table: &str, condition: &str) -> i64 {
    connection
        .query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE {condition}"),
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn pragma_i64(connection: &rusqlite::Connection, pragma: &str) -> i64 {
    connection
        .query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))
        .unwrap()
}

fn foreign_key_failures(connection: &rusqlite::Connection) -> Vec<()> {
    connection
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |_| Ok(()))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn reference_site_rows(
    connection: &rusqlite::Connection,
    version_id: i64,
    level: i64,
) -> Vec<Vec<rusqlite::types::Value>> {
    let mut statement = connection
        .prepare(
            "SELECT reference_site_id, path, language, containing_symbol_id,
                    start_line, start_column, end_line, end_column, start_byte, end_byte,
                    is_exact, provenance, level
             FROM reference_sites
             WHERE version_id = ?1 AND level = ?2
             ORDER BY reference_site_id",
        )
        .unwrap();
    statement
        .query_map(rusqlite::params![version_id, level], |row| {
            (0..13)
                .map(|index| row.get(index))
                .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn snapshot_version_tables(
    connection: &rusqlite::Connection,
    version_id: i64,
    tables: &[&str],
) -> Vec<(String, Vec<Vec<rusqlite::types::Value>>)> {
    tables
        .iter()
        .map(|table| {
            let columns = table_columns(connection, table);
            let column_refs = columns.iter().map(String::as_str).collect::<Vec<_>>();
            (
                (*table).to_string(),
                selected_rows(
                    connection,
                    table,
                    &column_refs,
                    Some(("version_id", version_id)),
                ),
            )
        })
        .collect()
}

fn table_columns(connection: &rusqlite::Connection, table: &str) -> Vec<String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap();
    statement
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn selected_rows(
    connection: &rusqlite::Connection,
    table: &str,
    columns: &[&str],
    filter: Option<(&str, i64)>,
) -> Vec<Vec<rusqlite::types::Value>> {
    let projection = columns.join(", ");
    let (sql, parameter) = match filter {
        Some((column, value)) => (
            format!("SELECT {projection} FROM {table} WHERE {column} = ?1"),
            Some(value),
        ),
        None => (format!("SELECT {projection} FROM {table}"), None),
    };
    let mut statement = connection.prepare(&sql).unwrap();
    let collect = |row: &rusqlite::Row<'_>| {
        (0..columns.len())
            .map(|index| row.get(index))
            .collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>()
    };
    let mut rows = match parameter {
        Some(value) => statement
            .query_map([value], collect)
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap(),
        None => statement
            .query_map([], collect)
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap(),
    };
    rows.sort_by_key(|row| format!("{row:?}"));
    rows
}

fn artifact_metadata() -> ArtifactMetadata {
    ArtifactMetadata {
        artifact_id: "artifact-store-equivalence".to_string(),
        root_path: "/repo".to_string(),
        binary_version: "julie-extract 2.30.0".to_string(),
        hash_algorithm: "blake3".to_string(),
        parser_inventory_fingerprint: "sha256:parser".to_string(),
        capability_snapshot_fingerprint: "sha256:capability".to_string(),
        created_at: CREATED_AT.to_string(),
        updated_at: CREATED_AT.to_string(),
    }
}

fn artifact_revision() -> RevisionInput {
    RevisionInput {
        operation: WriteOperation::Scan,
        mode: Some(WriteMode::Incremental),
        started_at: CREATED_AT.to_string(),
        completed_at: "2026-08-07T12:00:01Z".to_string(),
        binary_version: "julie-extract 2.30.0".to_string(),
        input_root: Some("/repo".to_string()),
    }
}

struct TestStore {
    path: PathBuf,
    layout: StoreLayout,
}

impl TestStore {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "julie-store-writer-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        let layout = StoreLayout::create(&path, "family-writer", "2.30.0", 7).unwrap();
        Self { path, layout }
    }

    fn writer(&self) -> StoreWriter {
        let factory = StoreConnectionFactory::new(self.layout.clone(), "family-writer", "2.30.0");
        StoreWriter::open(&factory).unwrap()
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.path());
    }
}
