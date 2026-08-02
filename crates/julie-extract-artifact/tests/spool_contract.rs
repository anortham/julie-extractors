use std::path::PathBuf;

use julie_extract_artifact::model::{
    ArtifactComplexityMetric, ArtifactFile, ArtifactIdentifier, ArtifactLiteral,
    ArtifactParseDiagnostic, ArtifactPendingRelationship, ArtifactRelationship,
    ArtifactSourceRegion, ArtifactStructuralFact, ArtifactSymbol, ArtifactSymbolAnnotation,
    ArtifactTypeArgument, ArtifactTypeArgumentUsage, ArtifactTypeFact, FileStatus,
    ReferenceSiteProvenance,
};
use julie_extract_artifact::writer::ArtifactFileSpool;

const IDENTIFIER_BYTE_CEILING: u64 = 200;

#[test]
fn spool_roundtrip_preserves_every_artifact_file_field() {
    let file = fully_populated_file();
    let spool_path = temp_path("roundtrip");
    let mut spool = ArtifactFileSpool::create(&spool_path).unwrap();
    spool.push(&file).unwrap();
    spool.push(&minimal_file()).unwrap();
    spool.finish().unwrap();

    let decoded = spool
        .iter()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(decoded, vec![file, minimal_file()]);
    std::fs::remove_file(&spool_path).ok();
}

#[test]
fn spool_reports_pushed_record_count() {
    let spool_path = temp_path("len");
    let mut spool = ArtifactFileSpool::create(&spool_path).unwrap();
    assert!(spool.is_empty());
    spool.push(&minimal_file()).unwrap();
    spool.push(&minimal_file()).unwrap();
    spool.finish().unwrap();

    assert_eq!(spool.len(), 2);
    assert_eq!(spool.iter().unwrap().count(), 2);
    std::fs::remove_file(&spool_path).ok();
}

#[test]
fn dense_identifier_spool_stays_under_the_bytes_per_row_ceiling() {
    let file = dense_identifier_file(5_000);
    let identifier_rows = file.identifiers.len() as u64;
    let spool_path = temp_path("density");
    let mut spool = ArtifactFileSpool::create(&spool_path).unwrap();
    spool.push(&file).unwrap();
    spool.finish().unwrap();

    let spool_bytes = std::fs::metadata(&spool_path).unwrap().len();
    let bytes_per_row = spool_bytes / identifier_rows;
    std::fs::remove_file(&spool_path).ok();

    assert!(
        bytes_per_row < IDENTIFIER_BYTE_CEILING,
        "spool spent {bytes_per_row} B per identifier row ({spool_bytes} B for {identifier_rows} rows); ceiling is {IDENTIFIER_BYTE_CEILING} B"
    );
}

fn dense_identifier_file(identifiers: usize) -> ArtifactFile {
    let symbols = (0..identifiers / 25)
        .map(|index| ArtifactSymbol {
            symbol_id: hash_id(index as u64),
            name: format!("Method{index}"),
            kind: "method".to_string(),
            signature: Some(format!("public void Method{index}(int value)")),
            start_line: index as i64 * 20 + 1,
            end_line: index as i64 * 20 + 18,
            start_byte: index as i64 * 640,
            end_byte: index as i64 * 640 + 600,
            is_test: true,
            ..ArtifactSymbol::default()
        })
        .collect::<Vec<_>>();

    let identifiers = (0..identifiers)
        .map(|index| {
            let start_byte = index as i64 * 24;
            ArtifactIdentifier {
                identifier_id: hash_id(1_000_000 + index as u64),
                reference_site_id: format!("reference_site-{}", hash_id(2_000_000 + index as u64)),
                name: format!("local{}", index % 40),
                kind: if index % 3 == 0 {
                    "call".to_string()
                } else {
                    "variable_ref".to_string()
                },
                containing_symbol_id: Some(hash_id((index / 25) as u64)),
                target_symbol_id: None,
                start_line: index as i64 / 4 + 1,
                start_column: (index % 60) as i64,
                end_line: index as i64 / 4 + 1,
                end_column: (index % 60) as i64 + 6,
                start_byte,
                end_byte: start_byte + 6,
                site_is_exact: true,
                site_provenance: ReferenceSiteProvenance::TargetToken,
                confidence: 1.0,
                code_context: None,
                metadata_json: None,
            }
        })
        .collect::<Vec<_>>();

    ArtifactFile {
        file_id: format!("file-{}", hash_id(7)),
        path: "src/tests/JIT/Directed/cmov/Generated.cs".to_string(),
        language: "csharp".to_string(),
        content_hash: format!("blake3:{}", hash_id(9)),
        content_bytes: 4_000_000,
        line_count: Some(120_000),
        indexed_at: "2026-08-02T00:00:00Z".to_string(),
        status: FileStatus::Indexed,
        metadata_json: None,
        symbols,
        identifiers,
        symbol_annotations: Vec::new(),
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

fn hash_id(seed: u64) -> String {
    format!("{seed:032x}")
}

fn minimal_file() -> ArtifactFile {
    ArtifactFile {
        file_id: "file-minimal".to_string(),
        path: "src/minimal.rs".to_string(),
        language: "rust".to_string(),
        content_hash: "blake3:minimal".to_string(),
        content_bytes: 12,
        line_count: None,
        indexed_at: "2026-08-02T00:00:00Z".to_string(),
        status: FileStatus::Unsupported,
        metadata_json: None,
        symbols: Vec::new(),
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

fn fully_populated_file() -> ArtifactFile {
    ArtifactFile {
        file_id: "file-a".to_string(),
        path: "src/a.rs".to_string(),
        language: "rust".to_string(),
        content_hash: "blake3:aaaa".to_string(),
        content_bytes: 2_048,
        line_count: Some(64),
        indexed_at: "2026-08-02T12:00:00Z".to_string(),
        status: FileStatus::FailedPreserved,
        metadata_json: Some(r#"{"file":true}"#.to_string()),
        symbols: vec![ArtifactSymbol {
            symbol_id: "sym-alpha".to_string(),
            name: "alpha".to_string(),
            kind: "function".to_string(),
            signature: Some("fn alpha() -> u32".to_string()),
            doc_comment: Some("Alpha docs".to_string()),
            visibility: Some("public".to_string()),
            parent_symbol_id: Some("sym-parent".to_string()),
            start_line: 4,
            start_column: 0,
            end_line: 9,
            end_column: 1,
            start_byte: 40,
            end_byte: 120,
            body_start_line: Some(4),
            body_start_column: Some(18),
            body_end_line: Some(9),
            body_end_column: Some(1),
            body_start_byte: Some(58),
            body_end_byte: Some(120),
            body_hash: Some("md5:bbbb".to_string()),
            semantic_group: Some("alpha-group".to_string()),
            confidence: Some(0.75),
            content_type: Some("documentation".to_string()),
            is_test: true,
            test_container: true,
            test_lifecycle: true,
            metadata_json: Some(r#"{"symbol":true}"#.to_string()),
        }],
        symbol_annotations: vec![ArtifactSymbolAnnotation {
            annotation_id: "ann-alpha".to_string(),
            symbol_id: "sym-alpha".to_string(),
            annotation: "route".to_string(),
            annotation_key: "route".to_string(),
            raw_text: Some("#[route]".to_string()),
            carrier: Some("attribute".to_string()),
            metadata_json: Some(r#"{"annotation":true}"#.to_string()),
        }],
        identifiers: vec![ArtifactIdentifier {
            identifier_id: "ident-beta".to_string(),
            reference_site_id: "site-beta".to_string(),
            name: "beta".to_string(),
            kind: "call".to_string(),
            containing_symbol_id: Some("sym-alpha".to_string()),
            target_symbol_id: Some("sym-beta".to_string()),
            start_line: 5,
            start_column: 4,
            end_line: 5,
            end_column: 8,
            start_byte: 64,
            end_byte: 68,
            site_is_exact: true,
            site_provenance: ReferenceSiteProvenance::TargetToken,
            confidence: 0.95,
            code_context: Some("  beta();".to_string()),
            metadata_json: Some(r#"{"identifier":true}"#.to_string()),
        }],
        relationships: vec![ArtifactRelationship {
            relationship_id: "rel-alpha-beta".to_string(),
            reference_site_id: "site-beta".to_string(),
            from_symbol_id: "sym-alpha".to_string(),
            to_symbol_id: "sym-beta".to_string(),
            kind: "calls".to_string(),
            start_line: Some(5),
            start_column: Some(4),
            end_line: Some(5),
            end_column: Some(8),
            start_byte: Some(64),
            end_byte: Some(68),
            site_is_exact: true,
            site_provenance: ReferenceSiteProvenance::TargetToken,
            confidence: 0.9,
            metadata_json: Some(r#"{"relationship":true}"#.to_string()),
        }],
        pending_relationships: vec![ArtifactPendingRelationship {
            pending_relationship_id: "pending-external".to_string(),
            reference_site_id: "site-pending".to_string(),
            from_symbol_id: "sym-alpha".to_string(),
            caller_scope_symbol_id: Some("sym-alpha".to_string()),
            kind: "uses".to_string(),
            target_display_name: "crate::external::Thing".to_string(),
            target_terminal_name: "Thing".to_string(),
            target_receiver: Some("external".to_string()),
            target_namespace_json: r#"["crate","external"]"#.to_string(),
            target_import_context: Some("use crate::external".to_string()),
            start_line: 6,
            start_column: Some(4),
            end_line: Some(6),
            end_column: Some(12),
            start_byte: Some(80),
            end_byte: Some(88),
            site_is_exact: false,
            site_provenance: ReferenceSiteProvenance::Spanless,
            confidence: 0.4,
            metadata_json: Some(r#"{"pending":true}"#.to_string()),
        }],
        type_facts: vec![ArtifactTypeFact {
            type_fact_id: "type-fact-alpha".to_string(),
            symbol_id: "sym-alpha".to_string(),
            resolved_type: "Result<T>".to_string(),
            generic_params_json: Some(r#"["T"]"#.to_string()),
            constraints_json: Some(r#"["T: Clone"]"#.to_string()),
            is_inferred: true,
            metadata_json: Some(r#"{"type_fact":true}"#.to_string()),
        }],
        type_argument_usages: vec![ArtifactTypeArgumentUsage {
            usage_id: "usage-beta".to_string(),
            identifier_id: "ident-beta".to_string(),
            metadata_json: Some(r#"{"usage":true}"#.to_string()),
        }],
        type_arguments: vec![ArtifactTypeArgument {
            type_argument_id: "type-arg-beta".to_string(),
            usage_id: "usage-beta".to_string(),
            parent_type_argument_id: Some("type-arg-root".to_string()),
            ordinal: 1,
            type_name: "String".to_string(),
        }],
        literals: vec![ArtifactLiteral {
            literal_id: "literal-alpha".to_string(),
            literal_text: "hello".to_string(),
            kind: "string".to_string(),
            carrier: Some("argument".to_string()),
            arg_position: 2,
            containing_symbol_id: Some("sym-alpha".to_string()),
            start_line: 7,
            start_column: 8,
            end_line: 7,
            end_column: 15,
            start_byte: 96,
            end_byte: 103,
            confidence: 0.8,
            metadata_json: Some(r#"{"literal":true}"#.to_string()),
        }],
        source_regions: vec![ArtifactSourceRegion {
            source_region_id: "region-alpha".to_string(),
            kind: "doc_comment".to_string(),
            containing_symbol_id: Some("sym-alpha".to_string()),
            start_line: 3,
            start_column: 0,
            end_line: 3,
            end_column: 12,
            start_byte: 24,
            end_byte: 36,
            metadata_json: Some(r#"{"region":true}"#.to_string()),
        }],
        structural_facts: vec![ArtifactStructuralFact {
            structural_fact_id: "fact-alpha".to_string(),
            pattern_id: "sql.select".to_string(),
            capture_name: "table".to_string(),
            node_kind: "identifier".to_string(),
            containing_symbol_id: Some("sym-alpha".to_string()),
            start_line: 8,
            start_column: 2,
            end_line: 8,
            end_column: 9,
            start_byte: 110,
            end_byte: 117,
            confidence: 0.6,
            metadata_json: Some(r#"{"fact":true}"#.to_string()),
        }],
        complexity_metrics: vec![ArtifactComplexityMetric {
            complexity_metric_id: "metric-alpha".to_string(),
            scope: "symbol".to_string(),
            symbol_id: Some("sym-alpha".to_string()),
            algorithm_id: "julie-ast-complexity-v1".to_string(),
            covered_lines: 6,
            covered_bytes: 80,
            decision_count: 3,
            loop_count: 1,
            max_nesting_depth: 2,
            parameter_count: Some(1),
            start_line: 4,
            start_column: 0,
            end_line: 9,
            end_column: 1,
            start_byte: 40,
            end_byte: 120,
            metadata_json: Some(r#"{"metric":true}"#.to_string()),
        }],
        parse_diagnostics: vec![ArtifactParseDiagnostic {
            diagnostic_id: "diag-alpha".to_string(),
            kind: "error".to_string(),
            message: Some("unexpected token".to_string()),
            start_line: 2,
            start_column: 1,
            end_line: 2,
            end_column: 4,
            start_byte: 10,
            end_byte: 13,
            metadata_json: Some(r#"{"diagnostic":true}"#.to_string()),
        }],
    }
}

fn temp_path(name: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "julie-extract-scan-spool-{}-{unique}-{name}.jsonl",
        std::process::id()
    ))
}
