use std::time::Duration;

use julie_extract_artifact::metadata::ArtifactMetadata;
use julie_extract_artifact::model::{
    ArtifactFile, ArtifactIdentifier, ArtifactPendingRelationship, ArtifactSymbol,
    ArtifactTypeFact, FileStatus, RevisionInput, WriteMode, WriteOperation,
};
use julie_extract_artifact::writer::ArtifactWriter;

#[test]
fn tiny_fixture_batch_uses_one_commit_and_stays_inside_tripwire_budget() {
    let mut writer = ArtifactWriter::open_in_memory(metadata()).unwrap();
    let files = (0..250)
        .map(|index| file_with_symbol(index, 2))
        .collect::<Vec<_>>();

    let started = std::time::Instant::now();
    let result = writer.write_scan(revision(), &files).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(
        result.transactions_committed, 1,
        "writer must not commit per file or per row"
    );
    assert_eq!(result.files_changed, 250);
    assert_eq!(result.rows_written.files, 250);
    assert_eq!(result.rows_written.symbols, 500);
    assert!(
        elapsed < Duration::from_millis(750),
        "tiny fixture writer tripwire exceeded budget: {elapsed:?}"
    );
}

#[test]
fn child_row_batch_avoids_per_file_statement_prepare_overhead() {
    let mut writer = ArtifactWriter::open_in_memory(metadata()).unwrap();
    let files = (0..3_000)
        .map(|index| file_with_child_rows(index))
        .collect::<Vec<_>>();

    let started = std::time::Instant::now();
    let result = writer.write_scan(revision(), &files).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(result.transactions_committed, 1);
    assert_eq!(result.files_changed, 3_000);
    assert_eq!(result.rows_written.files, 3_000);
    assert_eq!(result.rows_written.symbols, 9_000);
    assert_eq!(result.rows_written.identifiers, 36_000);
    assert_eq!(result.rows_written.pending_relationships, 12_000);
    assert_eq!(result.rows_written.type_facts, 9_000);
    assert!(
        elapsed < Duration::from_millis(1_250),
        "child-row writer tripwire exceeded budget: {elapsed:?}"
    );
}

fn metadata() -> ArtifactMetadata {
    ArtifactMetadata {
        artifact_id: "artifact-writer-perf-test".to_string(),
        root_path: "/repo".to_string(),
        binary_version: "julie-extract 0.1.0".to_string(),
        hash_algorithm: "blake3".to_string(),
        parser_inventory_fingerprint: "sha256:parser".to_string(),
        capability_snapshot_fingerprint: "sha256:cap".to_string(),
        created_at: "2026-05-31T19:20:00Z".to_string(),
        updated_at: "2026-05-31T19:20:00Z".to_string(),
    }
}

fn revision() -> RevisionInput {
    RevisionInput {
        operation: WriteOperation::Scan,
        mode: Some(WriteMode::Incremental),
        started_at: "2026-05-31T19:20:00Z".to_string(),
        completed_at: "2026-05-31T19:20:01Z".to_string(),
        binary_version: "julie-extract 0.1.0".to_string(),
        input_root: Some("/repo".to_string()),
    }
}

fn file_with_child_rows(index: usize) -> ArtifactFile {
    let mut file = file_with_symbol(index, 3);
    file.identifiers = (0..12)
        .map(|identifier_index| ArtifactIdentifier {
            identifier_id: format!("file-{index}-identifier-{identifier_index}"),
            name: format!("identifier_{index}_{identifier_index}"),
            containing_symbol_id: Some(format!("file-{index}-symbol-0")),
            target_symbol_id: Some(format!("file-{index}-symbol-1")),
            start_line: (identifier_index + 1) as i64,
            end_line: (identifier_index + 1) as i64,
            start_byte: (identifier_index * 8) as i64,
            end_byte: (identifier_index * 8 + 4) as i64,
            ..ArtifactIdentifier::default()
        })
        .collect();
    file.pending_relationships = (0..4)
        .map(|pending_index| ArtifactPendingRelationship {
            pending_relationship_id: format!("file-{index}-pending-{pending_index}"),
            from_symbol_id: format!("file-{index}-symbol-0"),
            caller_scope_symbol_id: Some(format!("file-{index}-symbol-0")),
            target_display_name: format!("externalTarget{pending_index}"),
            target_terminal_name: format!("externalTarget{pending_index}"),
            start_line: (pending_index + 1) as i64,
            ..ArtifactPendingRelationship::default()
        })
        .collect();
    file.type_facts = (0..3)
        .map(|type_index| ArtifactTypeFact {
            type_fact_id: format!("file-{index}-type-{type_index}"),
            symbol_id: format!("file-{index}-symbol-{type_index}"),
            resolved_type: format!("Type{type_index}"),
            generic_params_json: None,
            constraints_json: None,
            is_inferred: true,
            metadata_json: None,
        })
        .collect();
    file
}

fn file_with_symbol(index: usize, symbol_count: usize) -> ArtifactFile {
    ArtifactFile {
        file_id: format!("file-{index}"),
        path: format!("src/file_{index}.rs"),
        language: "rust".to_string(),
        content_hash: format!("hash-{index}"),
        content_bytes: 64,
        line_count: Some(6),
        indexed_at: "2026-05-31T19:20:00Z".to_string(),
        status: FileStatus::Indexed,
        metadata_json: None,
        symbols: (0..symbol_count)
            .map(|symbol_index| ArtifactSymbol {
                symbol_id: format!("file-{index}-symbol-{symbol_index}"),
                name: format!("symbol_{index}_{symbol_index}"),
                kind: "function".to_string(),
                start_line: (symbol_index + 1) as i64,
                end_line: (symbol_index + 1) as i64,
                start_byte: (symbol_index * 8) as i64,
                end_byte: (symbol_index * 8 + 4) as i64,
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
