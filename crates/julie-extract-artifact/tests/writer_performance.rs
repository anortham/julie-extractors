//! Structural batching contract for `ArtifactWriter`: one commit per write and
//! every child row family persisted at scale.
//!
//! These assertions are deliberately hardware-independent. Wall-clock budgets
//! live in the feature-gated `writer_perf.rs` harness, never here — a timing
//! assertion in the default suite fails on whichever runner happens to be slow
//! that day, which is exactly what it did on shared CI while passing locally.

use julie_extract_artifact::metadata::ArtifactMetadata;
use julie_extract_artifact::model::{
    ArtifactComplexityMetric, ArtifactFile, ArtifactIdentifier, ArtifactPendingRelationship,
    ArtifactSymbol, ArtifactTypeFact, FileStatus, RevisionInput, WriteMode, WriteOperation,
};
use julie_extract_artifact::writer::ArtifactWriter;

#[test]
fn tiny_fixture_batch_persists_every_file_in_one_commit() {
    let mut writer = ArtifactWriter::open_in_memory(metadata()).unwrap();
    let files = (0..250)
        .map(|index| file_with_symbol(index, 2))
        .collect::<Vec<_>>();

    let result = writer.write_scan(revision(), &files).unwrap();

    assert_eq!(
        result.transactions_committed, 1,
        "writer must not commit per file or per row"
    );
    assert_eq!(result.files_changed, 250);
    assert_eq!(result.rows_written.files, 250);
    assert_eq!(result.rows_written.symbols, 500);
}

#[test]
fn child_row_batch_persists_every_child_row_family_in_one_commit() {
    let mut writer = ArtifactWriter::open_in_memory(metadata()).unwrap();
    let files = (0..3_000).map(file_with_child_rows).collect::<Vec<_>>();

    let result = writer.write_scan(revision(), &files).unwrap();

    assert_eq!(result.transactions_committed, 1);
    assert_eq!(result.files_changed, 3_000);
    assert_eq!(result.rows_written.files, 3_000);
    assert_eq!(result.rows_written.symbols, 9_000);
    assert_eq!(result.rows_written.reference_sites, 48_000);
    assert_eq!(result.rows_written.identifiers, 36_000);
    assert_eq!(result.rows_written.pending_relationships, 12_000);
    assert_eq!(result.rows_written.type_facts, 9_000);
    assert_eq!(result.rows_written.complexity_metrics, 9_000);
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
            reference_site_id: format!("file-{index}-identifier-site-{identifier_index}"),
            name: format!("identifier_{index}_{identifier_index}"),
            containing_symbol_id: Some(format!("file-{index}-symbol-0")),
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
            reference_site_id: format!("file-{index}-pending-site-{pending_index}"),
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
    file.complexity_metrics = (0..3)
        .map(|metric_index| ArtifactComplexityMetric {
            complexity_metric_id: format!("file-{index}-complexity-{metric_index}"),
            scope: "symbol".to_string(),
            symbol_id: Some(format!("file-{index}-symbol-{metric_index}")),
            algorithm_id: "julie-ast-complexity-v1".to_string(),
            covered_lines: 3,
            covered_bytes: 64,
            decision_count: 1,
            loop_count: 1,
            max_nesting_depth: 2,
            parameter_count: Some(2),
            start_line: (metric_index + 1) as i64,
            end_line: (metric_index + 3) as i64,
            start_byte: (metric_index * 8) as i64,
            end_byte: (metric_index * 8 + 64) as i64,
            ..ArtifactComplexityMetric::default()
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
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        parse_diagnostics: Vec::new(),
    }
}
