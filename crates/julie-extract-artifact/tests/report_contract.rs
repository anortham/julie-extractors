use std::collections::{BTreeMap, BTreeSet};

use julie_extract_artifact::reports::{
    ArtifactReport, Report, ReportCode, ReportCounts, ReportDiagnostic, ReportFileRows,
    ReportInput, ReportLanguageProfile, ReportMode, ReportOperation, ReportProfile, ReportRevision,
    ReportStatus, RowDomainCounts, SQLITE_ROW_DOMAINS, ToolReport,
};
use serde_json::json;

#[test]
fn report_serializes_schema_version_and_success_shape() {
    let value = serde_json::to_value(sample_report(ReportStatus::Ok)).unwrap();

    assert_eq!(value["report_schema_version"], 3);
    assert_eq!(value["status"], "ok");
    assert_eq!(value["operation"], "scan");
    assert_eq!(value["mode"], "incremental");
    assert_eq!(value["input"]["db_path"], "/tmp/code.sqlite");
    assert_eq!(value["artifact"]["schema_version"], 3);
    assert_eq!(
        value["artifact"]["jsonl_schema_version"],
        serde_json::Value::Null
    );
    assert_eq!(value["tool"]["binary_name"], "julie-extract");
    assert_eq!(value["revision"]["latest_revision_id"], 7);
    assert!(value.get("profile").is_none());
    assert_eq!(value["errors"], json!([]));
    assert_eq!(value["warnings"], json!([]));
}

#[test]
fn report_profile_serializes_phase_and_language_timings() {
    let mut report = sample_report(ReportStatus::Ok);
    report.profile = Some(ReportProfile {
        total_duration_ms: 42,
        phases: BTreeMap::from([
            ("discovery".to_string(), 3),
            ("extraction_spool".to_string(), 11),
            ("artifact_write".to_string(), 17),
        ]),
        languages: BTreeMap::from([(
            "rust".to_string(),
            ReportLanguageProfile {
                files: 2,
                changed_files: 1,
                unchanged_files: 1,
                failed_files: 0,
                bytes: 128,
                read_duration_ms: 4,
                extract_duration_ms: 7,
                spool_write_duration_ms: 2,
            },
        )]),
    });

    let value = serde_json::to_value(report).unwrap();

    assert_eq!(value["profile"]["total_duration_ms"], 42);
    assert_eq!(value["profile"]["phases"]["discovery"], 3);
    assert_eq!(value["profile"]["phases"]["extraction_spool"], 11);
    assert_eq!(value["profile"]["phases"]["artifact_write"], 17);
    assert_eq!(value["profile"]["languages"]["rust"]["files"], 2);
    assert_eq!(value["profile"]["languages"]["rust"]["changed_files"], 1);
    assert_eq!(value["profile"]["languages"]["rust"]["unchanged_files"], 1);
    assert_eq!(value["profile"]["languages"]["rust"]["bytes"], 128);
    assert_eq!(value["profile"]["languages"]["rust"]["read_duration_ms"], 4);
    assert_eq!(
        value["profile"]["languages"]["rust"]["extract_duration_ms"],
        7
    );
    assert_eq!(
        value["profile"]["languages"]["rust"]["spool_write_duration_ms"],
        2
    );
}

#[test]
fn every_report_status_has_stable_serialized_spelling() {
    let serialized = ReportStatus::ALL
        .iter()
        .map(|status| serde_json::to_value(status).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(
        serialized,
        vec![
            json!("ok"),
            json!("no_change"),
            json!("unsupported"),
            json!("not_found"),
            json!("partial"),
            json!("failed"),
        ]
    );
}

#[test]
fn every_v3_error_code_has_stable_serialized_spelling() {
    let serialized = ReportCode::ERROR_CODES
        .iter()
        .map(|code| serde_json::to_value(code).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(
        serialized,
        vec![
            json!("usage_error"),
            json!("invalid_path"),
            json!("file_outside_root"),
            json!("file_not_found"),
            json!("root_mismatch"),
            json!("schema_migration_required"),
            json!("schema_incompatible"),
            json!("contract_incompatible"),
            json!("db_open_failed"),
            json!("db_write_failed"),
            json!("lock_timeout"),
            json!("unsupported_format"),
            json!("unsupported_file"),
            json!("read_failed"),
            json!("parse_failed"),
            json!("data_loss_guard"),
            json!("export_failed"),
            json!("internal_error"),
        ]
    );

    let diagnostic = ReportDiagnostic {
        code: ReportCode::RootMismatch,
        message: "database root does not match requested root".to_string(),
        path: None,
        root_relative_path: None,
        recoverable: false,
        details: json!({
            "expected_root": "/old",
            "requested_root": "/new"
        }),
    };
    let value = serde_json::to_value(diagnostic).unwrap();

    assert_eq!(value["code"], "root_mismatch");
    assert_eq!(value["details"]["expected_root"], "/old");
    assert_eq!(value["recoverable"], false);
}

#[test]
fn single_file_success_reports_include_absolute_and_root_relative_paths() {
    let mut report = sample_report(ReportStatus::Ok);
    report.operation = ReportOperation::Update;
    report.mode = ReportMode::SingleFile;
    report.input.file_path = Some("/repo/src/lib.rs".to_string());
    report.input.root_relative_path = Some("src/lib.rs".to_string());

    let value = serde_json::to_value(report).unwrap();

    assert_eq!(value["operation"], "update");
    assert_eq!(value["mode"], "single_file");
    assert_eq!(value["input"]["file_path"], "/repo/src/lib.rs");
    assert_eq!(value["input"]["root_relative_path"], "src/lib.rs");
}

#[test]
fn report_row_count_keys_are_exhaustive_for_sqlite_v3() {
    let value = serde_json::to_value(RowDomainCounts::default()).unwrap();
    let actual = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let expected = SQLITE_ROW_DOMAINS.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(actual, expected);
    assert!(
        actual.contains("structural_facts"),
        "SQLite v3 row domains must include structural_facts"
    );
    assert!(
        actual.contains("complexity_metrics"),
        "SQLite v3 row domains must include complexity_metrics"
    );

    let report = sample_report(ReportStatus::Ok);
    let value = serde_json::to_value(report).unwrap();
    let rows_written = value["counts"]["rows_written"].as_object().unwrap();
    let totals = value["counts"]["totals"].as_object().unwrap();

    for domain in SQLITE_ROW_DOMAINS {
        assert!(
            rows_written.contains_key(*domain),
            "rows_written missing {domain}"
        );
        assert!(totals.contains_key(*domain), "totals missing {domain}");
    }
}

#[test]
fn report_counts_include_per_file_row_attribution() {
    let report = sample_report(ReportStatus::Ok);
    let value = serde_json::to_value(report).unwrap();
    let file_rows = value["counts"]["file_rows"].as_array().unwrap();

    assert_eq!(file_rows.len(), 1);
    assert_eq!(file_rows[0]["path"], "src/a.rs");
    assert_eq!(file_rows[0]["language"], "rust");
    assert_eq!(file_rows[0]["status"], "indexed");
    assert_eq!(file_rows[0]["total_rows"], 48);
    assert_eq!(file_rows[0]["rows"]["files"], 1);
    assert_eq!(file_rows[0]["rows"]["symbols"], 4);
    assert_eq!(file_rows[0]["rows"]["identifiers"], 30);
    assert_eq!(file_rows[0]["rows"]["source_regions"], 4);
    assert_eq!(file_rows[0]["rows"]["complexity_metrics"], 2);
    assert_eq!(file_rows[0]["rows"]["artifact_metadata"], 0);
    assert_eq!(value["counts"]["file_rows_truncated"], false);

    let rows = file_rows[0]["rows"].as_object().unwrap();
    for domain in SQLITE_ROW_DOMAINS {
        assert!(rows.contains_key(*domain), "file rows missing {domain}");
    }
}

fn sample_report(status: ReportStatus) -> Report {
    Report {
        status,
        operation: ReportOperation::Scan,
        mode: ReportMode::Incremental,
        input: ReportInput {
            db_path: Some("/tmp/code.sqlite".to_string()),
            root_path: Some("/repo".to_string()),
            file_path: None,
            root_relative_path: None,
            format: None,
            output_path: None,
        },
        artifact: Some(ArtifactReport {
            db_path: "/tmp/code.sqlite".to_string(),
            root_path: "/repo".to_string(),
            artifact_id: "artifact-test-1".to_string(),
            schema_version: 3,
            extract_contract_version: 3,
            sqlite_schema_version: 3,
            jsonl_schema_version: None,
            hash_algorithm: "blake3".to_string(),
            parser_inventory_fingerprint: "sha256:parser".to_string(),
            capability_snapshot_fingerprint: "sha256:cap".to_string(),
        }),
        tool: ToolReport {
            binary_name: "julie-extract".to_string(),
            binary_version: "0.1.0".to_string(),
        },
        revision: Some(ReportRevision {
            latest_revision_id: Some(7),
            created_revision_id: Some(7),
        }),
        counts: ReportCounts {
            files_scanned: 10,
            files_changed: 2,
            files_unchanged: 8,
            files_unsupported: 0,
            files_deleted: 0,
            files_failed: 0,
            rows_written: RowDomainCounts {
                extraction_revisions: 1,
                revision_file_changes: 2,
                files: 2,
                symbols: 12,
                identifiers: 30,
                relationships: 4,
                pending_relationships: 2,
                type_facts: 3,
                literals: 1,
                source_regions: 4,
                structural_facts: 1,
                complexity_metrics: 2,
                ..RowDomainCounts::default()
            },
            totals: RowDomainCounts {
                artifact_metadata: 10,
                parser_inventory: 36,
                language_capabilities: 36,
                language_capability_fixtures: 72,
                extraction_revisions: 7,
                revision_file_changes: 24,
                files: 100,
                symbols: 2400,
                symbol_annotations: 120,
                identifiers: 12000,
                relationships: 900,
                pending_relationships: 80,
                type_facts: 14,
                type_argument_usages: 5,
                type_arguments: 8,
                literals: 6,
                source_regions: 42,
                structural_facts: 12,
                complexity_metrics: 200,
                ..RowDomainCounts::default()
            },
            file_rows_truncated: false,
            file_rows: vec![ReportFileRows {
                path: "src/a.rs".to_string(),
                language: "rust".to_string(),
                status: "indexed".to_string(),
                total_rows: 48,
                rows: RowDomainCounts {
                    files: 1,
                    symbols: 4,
                    identifiers: 30,
                    relationships: 3,
                    type_facts: 1,
                    literals: 1,
                    source_regions: 4,
                    structural_facts: 1,
                    complexity_metrics: 2,
                    parse_diagnostics: 1,
                    ..RowDomainCounts::default()
                },
            }],
        },
        errors: Vec::new(),
        warnings: Vec::new(),
        profile: None,
        languages: None,
    }
}
