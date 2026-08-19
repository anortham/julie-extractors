use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use clap::Parser;
use julie_extract_cli::store::args::{StoreCli, StoreCommand, StoreLevelArg, StoreRootCommand};
use julie_extract_cli::store::report::{
    STORE_EXIT_INCOMPATIBLE, STORE_EXIT_OPERATIONAL_FAILURE, STORE_EXIT_SUCCESS, STORE_EXIT_USAGE,
    STORE_REPORT_SCHEMA_VERSION, StoreCommandOutcome, StoreCoordinatorDisposition,
    StoreFailureClass, StoreLevelCompletion, StoreManifestDisposition, StoreManifestReport,
    StoreOutputFormat, StoreOutputStream, StoreReport, StoreRequestState, StoreRequestedLevel,
    StoreRowCounts,
};
use julie_extractors::EXTRACTION_IDENTITY_EPOCH;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};

const FAMILY_ID: &str = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";

fn julie_extract(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .expect("julie-extract should start")
}

#[test]
fn internal_parser_accepts_only_the_final_import_form() {
    let cli = StoreCli::try_parse_from([
        "julie-extract",
        "store",
        "import",
        "--store",
        "/tmp/family",
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        "/tmp/repo",
        "--view",
        "view-main",
        "--level",
        "l1",
        "--request-id",
        "request-1",
        "--idempotency-key",
        "idem-1",
        "--request-timeout-seconds",
        "30",
    ])
    .expect("store import should parse through the internal contract");

    let StoreRootCommand::Store(store) = cli.command;
    let StoreCommand::Import(args) = store.command else {
        panic!("expected import command");
    };
    assert_eq!(args.store, PathBuf::from("/tmp/family"));
    assert_eq!(args.family, "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11");
    assert_eq!(args.root, PathBuf::from("/tmp/repo"));
    assert_eq!(args.view, "view-main");
    assert_eq!(args.level, StoreLevelArg::L1);
    assert_eq!(args.request.request_id.as_deref(), Some("request-1"));
    assert_eq!(args.request.idempotency_key.as_deref(), Some("idem-1"));
    assert_eq!(args.request.request_timeout_seconds, 30);
}

#[test]
fn import_requires_family_root_view_and_store_but_allows_create_family_paths() {
    let base = [
        "julie-extract",
        "store",
        "import",
        "--store",
        "/tmp/not-yet-created-family",
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        "/tmp/source",
        "--view",
        "view-main",
    ];
    let parsed = StoreCli::try_parse_from(base).expect("missing family directory is createable");
    let StoreRootCommand::Store(store) = parsed.command;
    let StoreCommand::Import(args) = store.command else {
        panic!("expected import command");
    };
    assert_eq!(args.level, StoreLevelArg::Full);
    assert_eq!(args.request.request_timeout_seconds, 30);
    assert!(args.request.request_id.is_none());
    assert!(args.request.idempotency_key.is_none());

    for missing in ["--store", "--family", "--root", "--view"] {
        let mut argv = base.to_vec();
        let index = argv.iter().position(|value| *value == missing).unwrap();
        argv.drain(index..=index + 1);
        assert!(
            StoreCli::try_parse_from(argv).is_err(),
            "{missing} is required"
        );
    }
}

#[test]
fn unknown_store_verbs_are_rejected_without_a_public_command() {
    let unknown = StoreCli::try_parse_from(["julie-extract", "store", "not-a-store-verb"]);
    assert!(
        unknown.is_err(),
        "a verb that is not a store command must not parse"
    );
    let unknown_message = unknown.unwrap_err().to_string();

    let resolve = StoreCli::try_parse_from([
        "julie-extract",
        "store",
        "resolve",
        "--store",
        "/tmp/family",
        "--view",
        "view-main",
    ]);
    assert!(
        resolve.is_err(),
        "store resolve must be an unknown subcommand"
    );
    let resolve_message = resolve.unwrap_err().to_string();
    assert!(
        resolve_message.contains("unrecognized subcommand"),
        "store resolve must use the unknown-subcommand path, got {resolve_message}"
    );
    assert!(
        unknown_message.contains("unrecognized subcommand"),
        "unknown store verbs must use the same clap path, got {unknown_message}"
    );

    let output = julie_extract(&[
        "store",
        "resolve",
        "--store",
        "/tmp/family",
        "--view",
        "view-main",
    ]);
    assert_ne!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand"),
        "julie-extract store resolve must exit as an unknown subcommand, got {stderr}"
    );
}

#[test]
fn report_json_is_versioned_and_names_request_scope_and_failure_class() {
    let report = StoreReport::new(
        "request-1",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "view-main",
        StoreRequestState::Failed,
    )
    .with_failure(StoreFailureClass::ViewRootMismatch, "view root differs");
    let outcome = StoreCommandOutcome::failed(report);
    let json: Value = serde_json::from_str(&outcome.render_json()).expect("valid JSON report");

    assert_eq!(json["report_schema_version"], STORE_REPORT_SCHEMA_VERSION);
    assert_eq!(json["request"]["id"], "request-1");
    assert_eq!(json["family_id"], "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11");
    assert_eq!(json["view_id"], "view-main");
    assert_eq!(json["state"], "failed");
    assert_eq!(json["failure_class"], "view_root_mismatch");
    assert_eq!(json["error"]["message"], "view root differs");
    assert_eq!(outcome.exit_code(), STORE_EXIT_OPERATIONAL_FAILURE);
    assert_eq!(
        StoreCommandOutcome::queued(StoreReport::new(
            "request-2",
            "family-1",
            "view-1",
            StoreRequestState::Queued,
        ))
        .exit_code(),
        STORE_EXIT_SUCCESS
    );
    assert_eq!(
        StoreCommandOutcome::usage(StoreReport::new(
            "request-3",
            "family-1",
            "view-1",
            StoreRequestState::Queued,
        ))
        .exit_code(),
        STORE_EXIT_USAGE
    );
    assert_eq!(
        StoreCommandOutcome::incompatible(StoreReport::new(
            "request-4",
            "family-1",
            "view-1",
            StoreRequestState::Queued,
        ))
        .exit_code(),
        STORE_EXIT_INCOMPATIBLE
    );
}

#[test]
fn report_json_snapshot_locks_the_complete_v1_shape() {
    let report = StoreReport::new(
        "request-1",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "view-main",
        StoreRequestState::Queued,
    )
    .with_root("/tmp/repo")
    .with_requested_level(StoreRequestedLevel::Full)
    .with_completion(StoreLevelCompletion {
        l1: true,
        l2: true,
        l3: false,
    })
    .with_manifest(StoreManifestReport {
        generation: Some(7),
        hash: Some("sha256:abcd".to_string()),
        disposition: StoreManifestDisposition::Created,
    })
    .with_row_counts(StoreRowCounts {
        file_versions: 3,
        l1: 4,
        l2: 5,
        l3: 6,
    })
    .with_coordinator(StoreCoordinatorDisposition::Queued)
    .with_idempotency_key("idem-1");
    let actual: Value = serde_json::from_str(&StoreCommandOutcome::queued(report).render_json())
        .expect("valid JSON report");
    let expected = json!({
        "report_schema_version": 1,
        "operation": "import",
        "request": {"id": "request-1", "idempotency_key": "idem-1"},
        "family_id": "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "view_id": "view-main",
        "root": "/tmp/repo",
        "state": "queued",
        "requested_level": "full",
        "completion": {"l1": true, "l2": true, "l3": false},
        "manifest": {
            "generation": 7,
            "hash": "sha256:abcd",
            "disposition": "created"
        },
        "row_counts": {"file_versions": 3, "l1": 4, "l2": 5, "l3": 6},
        "coordinator": "queued",
        "failure_class": "none",
        "error": null
    });
    assert_eq!(actual, expected);
}

#[test]
fn report_human_output_is_stable_and_failures_stay_on_stderr() {
    let report = StoreReport::new("request-1", "family-1", "view-1", StoreRequestState::Queued);
    let outcome = StoreCommandOutcome::queued(report);
    assert_eq!(
        outcome.render_human(),
        "queued\noperation: import\nrequest: request-1\nidempotency_key: none\nfamily: family-1\nview: view-1\nroot: \nstate: queued\nrequested_level: full\ncompletion: - - -\nmanifest: generation=none hash=none disposition=not_published\nrows: file_versions=0 l1=0 l2=0 l3=0\ncoordinator: queued\nfailure_class: none\n"
    );
    let success_plan = outcome.output_plan(false);
    assert_eq!(success_plan.format, StoreOutputFormat::Human);
    assert_eq!(success_plan.stream, StoreOutputStream::Stdout);

    let failed = StoreCommandOutcome::failed(
        StoreReport::new("request-2", "family-1", "view-1", StoreRequestState::Failed)
            .with_failure(StoreFailureClass::ViewNotFound, "view is unknown"),
    );
    let human_failure = failed.output_plan(false);
    assert_eq!(human_failure.format, StoreOutputFormat::Human);
    assert_eq!(human_failure.stream, StoreOutputStream::Stderr);
    assert_eq!(failed.output_plan(true).stream, StoreOutputStream::Stdout);

    let usage = StoreCommandOutcome::usage(StoreReport::new(
        "request-3",
        "family-1",
        "view-1",
        StoreRequestState::Queued,
    ));
    assert_eq!(usage.exit_code(), STORE_EXIT_USAGE);
    assert_eq!(usage.report().state, StoreRequestState::Failed);
    assert_eq!(
        usage.report().failure_class,
        StoreFailureClass::InvalidArguments
    );
    assert_eq!(
        usage.report().error.as_ref().unwrap().class,
        usage.report().failure_class
    );
    assert_eq!(usage.output_plan(false).stream, StoreOutputStream::Stderr);
    assert_eq!(usage.output_plan(true).stream, StoreOutputStream::Stdout);
}

#[test]
fn report_constructors_cannot_emit_an_unclassified_failure() {
    let failed = StoreCommandOutcome::failed(StoreReport::new(
        "request-1",
        "family-1",
        "view-1",
        StoreRequestState::Queued,
    ));
    assert_eq!(failed.report().state, StoreRequestState::Failed);
    assert_ne!(failed.report().failure_class, StoreFailureClass::None);
    assert!(failed.report().error.is_some());

    let classified = StoreReport::new("request-2", "family-1", "view-1", StoreRequestState::Queued)
        .with_failure(StoreFailureClass::ViewNotFound, "view is unknown");
    assert_eq!(classified.state, StoreRequestState::Failed);
    assert_eq!(classified.failure_class, StoreFailureClass::ViewNotFound);

    let unclassified =
        StoreReport::new("request-3", "family-1", "view-1", StoreRequestState::Queued)
            .with_failure(StoreFailureClass::None, "missing classification");
    assert_ne!(unclassified.failure_class, StoreFailureClass::None);
}

#[test]
fn failure_constructors_keep_failure_class_and_error_class_coherent() {
    let incompatible = StoreCommandOutcome::incompatible(StoreReport::new(
        "request-1",
        "family-1",
        "view-1",
        StoreRequestState::Failed,
    ));
    assert_eq!(
        incompatible.report().failure_class,
        StoreFailureClass::StoreIncompatible
    );
    assert_eq!(
        incompatible.report().error.as_ref().unwrap().class,
        StoreFailureClass::StoreIncompatible
    );

    let mut mismatched =
        StoreReport::new("request-2", "family-1", "view-1", StoreRequestState::Queued)
            .with_failure(StoreFailureClass::ViewNotFound, "view is unknown");
    mismatched.error.as_mut().unwrap().class = StoreFailureClass::Internal;
    let failed = StoreCommandOutcome::failed(mismatched);
    assert_eq!(
        failed.report().error.as_ref().unwrap().class,
        failed.report().failure_class
    );
}

#[test]
fn production_store_import_is_exposed_and_reports_json_on_stdout() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    let output = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "l1",
        "--request-id",
        "request-public-store",
        "--idempotency-key",
        "idem-public-store",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["operation"], "import");
    assert_eq!(report["state"], "committed");
}

#[test]
fn parser_enforces_identifier_and_path_bounds() {
    let long_id = "x".repeat(129);
    let long_path = format!("/tmp/{}", "x".repeat(4096));
    for args in [
        vec![
            "julie-extract",
            "store",
            "import",
            "--store",
            "/tmp/family",
            "--family",
            "family-1",
            "--root",
            "/tmp/repo",
            "--view",
            long_id.as_str(),
        ],
        vec![
            "julie-extract",
            "store",
            "import",
            "--store",
            long_path.as_str(),
            "--family",
            "family-1",
            "--root",
            "/tmp/repo",
            "--view",
            "view-1",
        ],
    ] {
        assert!(StoreCli::try_parse_from(args).is_err());
    }
}

#[test]
fn timeout_is_positive_and_bounded() {
    for value in ["0", "86401"] {
        let parsed = StoreCli::try_parse_from([
            "julie-extract",
            "store",
            "import",
            "--store",
            "/tmp/family",
            "--family",
            "family-1",
            "--root",
            "/tmp/repo",
            "--view",
            "view-1",
            "--request-timeout-seconds",
            value,
        ]);
        assert!(parsed.is_err(), "timeout {value} must be rejected");
    }
}

#[test]
fn import_accepts_all_scan_controls_and_json_mode() {
    let cli = StoreCli::try_parse_from([
        "julie-extract",
        "store",
        "import",
        "--store",
        "/tmp/family",
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        "/tmp/repo",
        "--view",
        "view-main",
        "--ignore-file",
        "/tmp/one.ignore",
        "--ignore-file",
        "/tmp/two.ignore",
        "--jobs",
        "4",
        "--spool-dir",
        "/tmp/spool",
        "--progress-file",
        "/tmp/progress.jsonl",
        "--parent-pid",
        "1234",
        "--json",
    ])
    .expect("scan and report controls should parse");
    let StoreRootCommand::Store(store) = cli.command;
    let StoreCommand::Import(args) = store.command else {
        panic!("expected import command");
    };
    assert_eq!(args.scan.ignore_files.len(), 2);
    assert_eq!(args.scan.jobs, 4);
    assert_eq!(
        args.scan.spool_dir.as_deref().unwrap().to_str(),
        Some("/tmp/spool")
    );
    assert_eq!(
        args.scan.progress_file.as_deref().unwrap().to_str(),
        Some("/tmp/progress.jsonl")
    );
    assert_eq!(args.scan.parent_pid, Some(1234));
    assert!(args.json);
}

#[test]
fn parser_rejects_invalid_family_and_nul_values_but_accepts_exact_boundaries() {
    for family in ["family-1", "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d1x"] {
        let parsed = StoreCli::try_parse_from([
            "julie-extract",
            "store",
            "import",
            "--store",
            "/tmp/family",
            "--family",
            family,
            "--root",
            "/tmp/repo",
            "--view",
            "view-1",
        ]);
        assert!(parsed.is_err(), "invalid UUID {family} must be rejected");
    }

    let exact_id = "x".repeat(128);
    let exact_path = format!("/{}", "x".repeat(4095));
    let parsed = StoreCli::try_parse_from([
        "julie-extract",
        "store",
        "import",
        "--store",
        exact_path.as_str(),
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        "/tmp/repo",
        "--view",
        exact_id.as_str(),
    ]);
    assert!(
        parsed.is_ok(),
        "exact UTF-8 byte boundaries should be accepted"
    );

    let nul = "view\0id";
    let parsed = StoreCli::try_parse_from([
        "julie-extract",
        "store",
        "import",
        "--store",
        "/tmp/family",
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        "/tmp/repo",
        "--view",
        nul,
    ]);
    assert!(parsed.is_err(), "NUL identifiers must be rejected");
}

#[test]
fn help_exposes_store_while_legacy_json_contract_stays_unchanged() {
    let help = julie_extract(&["--help"]);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).expect("help must be UTF-8");
    assert!(help.contains("scan"));
    assert!(help.contains("languages"));
    assert!(help.contains("store"));

    let store_help = julie_extract(&["store", "--help"]);
    assert!(store_help.status.success());
    assert!(store_help.stderr.is_empty());
    let store_help = String::from_utf8(store_help.stdout).expect("store help must be UTF-8");
    assert!(store_help.contains("import"));
    assert!(store_help.contains("maintain"));

    let languages = julie_extract(&["languages", "--json"]);
    assert_eq!(languages.status.code(), Some(0));
    assert!(languages.stderr.is_empty());
    let report: Value = serde_json::from_slice(&languages.stdout).expect("legacy JSON report");
    assert_eq!(report["report_schema_version"], 3);
    assert_eq!(report["operation"], "languages");
}

#[test]
fn export_succeeds_on_an_empty_import() {
    let fixture = tempfile::tempdir().unwrap();
    let (store, _root) = seed_empty_store(&fixture, "view-empty", "request-export-empty");
    let artifact = fixture.path().join("empty-export.db");

    let output = julie_extract(&[
        "store",
        "export",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--view",
        "view-empty",
        "--out",
        artifact.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["operation"], "export");
    assert_eq!(report["state"], "committed");
    assert_exported_capability_rows_match_current_fingerprint(&artifact);
}

#[test]
fn export_succeeds_on_an_all_unsupported_import() {
    let fixture = tempfile::tempdir().unwrap();
    let (store, _root) =
        seed_unsupported_store(&fixture, "view-unsupported", "request-export-unsupported");
    let artifact = fixture.path().join("unsupported-export.db");

    let output = julie_extract(&[
        "store",
        "export",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--view",
        "view-unsupported",
        "--out",
        artifact.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["operation"], "export");
    assert_eq!(report["state"], "committed");
    assert_exported_capability_rows_match_current_fingerprint(&artifact);
}

#[test]
fn export_omits_legacy_reference_resolution_capability_gaps() {
    let fixture = tempfile::tempdir().unwrap();
    let (store, _root) = seed_unresolved_store(&fixture, "view-main", "request-export-legacy-gaps");
    let store_db = store.join("gen-001/store.db");
    let connection = Connection::open(&store_db).unwrap();
    connection
        .execute(
            "INSERT INTO language_capability_gaps
             (extraction_epoch,gap_id,language,capability,status,reason,required_closure,evidence_json)
             VALUES (?1,'rust:reference_resolution.tier2_import','rust',
                     'reference_resolution.tier2_import','open','legacy','none','{}')",
            [EXTRACTION_IDENTITY_EPOCH],
        )
        .unwrap();
    drop(connection);

    let artifact = fixture.path().join("legacy-gaps-export.db");
    let output = julie_extract(&[
        "store",
        "export",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--view",
        "view-main",
        "--out",
        artifact.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_exported_capability_rows_match_current_fingerprint(&artifact);
}

#[test]
fn export_succeeds_on_a_store_that_never_resolved() {
    let fixture = tempfile::tempdir().unwrap();
    let (store, _root) = seed_unresolved_store(&fixture, "view-main", "request-export-unresolved");
    let artifact = fixture.path().join("export.db");

    let output = julie_extract(&[
        "store",
        "export",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--view",
        "view-main",
        "--out",
        artifact.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["operation"], "export");
    assert_eq!(report["state"], "committed");
    assert!(report.get("resolution").is_none());
    assert_artifact_has_facts_and_no_resolution(&artifact);
}

#[test]
fn export_then_from_artifact_round_trip_works_without_resolution() {
    let fixture = tempfile::tempdir().unwrap();
    let (store, root) = seed_unresolved_store(&fixture, "view-main", "request-round-trip-seed");
    let artifact = fixture.path().join("export.db");
    let imported = fixture.path().join("imported-store");

    let exported = julie_extract(&[
        "store",
        "export",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--view",
        "view-main",
        "--out",
        artifact.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        exported.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&exported.stdout),
        String::from_utf8_lossy(&exported.stderr)
    );
    assert_artifact_has_facts_and_no_resolution(&artifact);

    let reimported = julie_extract(&[
        "store",
        "import",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--store",
        imported.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--request-id",
        "request-round-trip-import",
        "--idempotency-key",
        "idem-round-trip-import",
        "--json",
    ]);
    assert_eq!(
        reimported.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&reimported.stdout),
        String::from_utf8_lossy(&reimported.stderr)
    );
    let report: Value = serde_json::from_slice(&reimported.stdout).unwrap();
    assert_eq!(report["operation"], "from_artifact");
    assert_eq!(report["state"], "committed");
    assert!(report.get("resolution").is_none());
    assert_store_has_no_bases(&imported);
}

#[test]
fn export_under_concurrent_update_and_gc_keeps_one_generation() {
    let fixture = tempfile::tempdir().unwrap();
    let (store, root) =
        seed_unresolved_store(&fixture, "view-main", "request-export-snapshot-seed");
    let first_hashes = visible_content_hashes(&store, "view-main");
    assert_eq!(first_hashes.len(), 2);
    let artifact = fixture.path().join("snapshot.db");
    let pause = fixture.path().join("export-pause");
    std::fs::create_dir(&pause).unwrap();

    let store_path = store.clone();
    let artifact_path = artifact.clone();
    let pause_path = pause.clone();
    let export = std::thread::spawn(move || {
        Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args([
                "store",
                "export",
                "--store",
                store_path.to_str().unwrap(),
                "--family",
                FAMILY_ID,
                "--view",
                "view-main",
                "--out",
                artifact_path.to_str().unwrap(),
                "--json",
            ])
            .env("JULIE_EXTRACT_STORE_EXPORT_TEST_PAUSE_DIR", &pause_path)
            .output()
            .expect("export should start")
    });

    let ready = pause.join("ready");
    let started = Instant::now();
    while !ready.exists() && started.elapsed() < Duration::from_secs(5) {
        if export.is_finished() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(
        ready.exists(),
        "export must pause at its test hook before the concurrent update runs"
    );
    std::fs::write(root.join("a.rs"), "pub fn a() -> u32 { 2 }\n").unwrap();
    std::fs::write(root.join("b.rs"), "pub fn b() -> u32 { 4 }\n").unwrap();
    for (file, request) in [("a.rs", "request-snap-a"), ("b.rs", "request-snap-b")] {
        let updated = julie_extract(&[
            "store",
            "update",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--file",
            file,
            "--request-id",
            request,
            "--idempotency-key",
            request,
            "--json",
        ]);
        assert_eq!(
            updated.status.code(),
            Some(0),
            "stdout: {} stderr: {}",
            String::from_utf8_lossy(&updated.stdout),
            String::from_utf8_lossy(&updated.stderr)
        );
    }
    let gc = julie_extract(&[
        "store",
        "maintain",
        "gc",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--apply",
        "--json",
    ]);
    assert_eq!(
        gc.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&gc.stdout),
        String::from_utf8_lossy(&gc.stderr)
    );
    std::fs::write(pause.join("continue"), b"continue").unwrap();

    let output = export.join().expect("export thread");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_artifact_has_facts_and_no_resolution(&artifact);
    let exported_hashes = artifact_content_hashes(&artifact);
    assert_eq!(
        exported_hashes, first_hashes,
        "export must keep the snapshot generation, not mix later updates"
    );
}

#[test]
fn from_artifact_binds_a_fact_complete_artifact_without_resolution_metadata() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let artifact = fixture.path().join("scanned.db");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 7 }\n").unwrap();

    let scanned = julie_extract(&[
        "scan",
        "--root",
        root.to_str().unwrap(),
        "--db",
        artifact.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        scanned.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&scanned.stdout),
        String::from_utf8_lossy(&scanned.stderr)
    );
    strip_artifact_resolution(&artifact);
    assert_artifact_has_facts_and_no_resolution(&artifact);

    let imported = julie_extract(&[
        "store",
        "import",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--request-id",
        "request-from-artifact-no-res",
        "--idempotency-key",
        "idem-from-artifact-no-res",
        "--json",
    ]);
    assert_eq!(
        imported.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&imported.stdout),
        String::from_utf8_lossy(&imported.stderr)
    );
    let report: Value = serde_json::from_slice(&imported.stdout).unwrap();
    assert_eq!(report["operation"], "from_artifact");
    assert_eq!(report["state"], "committed");
    assert!(report.get("resolution").is_none());
    assert_store_has_no_bases(&store);
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let generation: i64 = connection
        .query_row(
            "SELECT current_generation FROM views WHERE view_id = 'view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(generation, 1);
}

fn seed_empty_store(
    fixture: &tempfile::TempDir,
    view: &str,
    request_id: &str,
) -> (PathBuf, PathBuf) {
    let root = fixture.path().join(format!("{request_id}-root"));
    let store = fixture.path().join(format!("{request_id}-store"));
    std::fs::create_dir(&root).unwrap();
    import_store(&store, &root, view, request_id);
    (store, root)
}

fn seed_unsupported_store(
    fixture: &tempfile::TempDir,
    view: &str,
    request_id: &str,
) -> (PathBuf, PathBuf) {
    let root = fixture.path().join(format!("{request_id}-root"));
    let store = fixture.path().join(format!("{request_id}-store"));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("notes.txt"), "not a supported language\n").unwrap();
    import_store(&store, &root, view, request_id);
    (store, root)
}

fn import_store(store: &Path, root: &Path, view: &str, request_id: &str) {
    let imported = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--root",
        root.to_str().unwrap(),
        "--view",
        view,
        "--request-id",
        request_id,
        "--idempotency-key",
        request_id,
        "--json",
    ]);
    assert_eq!(
        imported.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&imported.stdout),
        String::from_utf8_lossy(&imported.stderr)
    );
}

fn seed_unresolved_store(
    fixture: &tempfile::TempDir,
    view: &str,
    request_id: &str,
) -> (PathBuf, PathBuf) {
    let root = fixture.path().join(format!("{request_id}-root"));
    let store = fixture.path().join(format!("{request_id}-store"));
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.rs"), "pub fn a() -> u32 { 1 }\n").unwrap();
    std::fs::write(root.join("b.rs"), "pub fn b() -> u32 { 3 }\n").unwrap();
    import_store(&store, &root, view, request_id);
    (store, root)
}

fn assert_exported_capability_rows_match_current_fingerprint(path: &Path) {
    let connection = Connection::open(path).unwrap();
    let parser_inventory: i64 = connection
        .query_row("SELECT COUNT(*) FROM parser_inventory", [], |row| {
            row.get(0)
        })
        .unwrap();
    let languages: i64 = connection
        .query_row("SELECT COUNT(*) FROM language_capabilities", [], |row| {
            row.get(0)
        })
        .unwrap();
    let reference_resolution_gaps: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM language_capability_gaps
             WHERE capability LIKE 'reference_resolution.%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        parser_inventory > 0,
        "export must emit current parser inventory rows"
    );
    assert_eq!(parser_inventory, languages);
    assert_eq!(reference_resolution_gaps, 0);
}

fn assert_artifact_has_facts_and_no_resolution(path: &Path) {
    let connection = Connection::open(path).unwrap();
    let files: i64 = connection
        .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
        .unwrap();
    let symbols: i64 = connection
        .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
        .unwrap();
    let identifiers: i64 = table_count_if_exists(&connection, "identifier_resolutions");
    let pending: i64 = table_count_if_exists(&connection, "pending_resolutions");
    let resolution_version: Option<String> = connection
        .query_row(
            "SELECT value FROM artifact_metadata WHERE key = 'reference_resolution_version'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert!(files > 0, "exported artifact must keep fact files");
    assert!(symbols > 0, "exported artifact must keep fact symbols");
    assert_eq!(identifiers, 0);
    assert_eq!(pending, 0);
    assert_eq!(resolution_version, None);
}

fn assert_store_has_no_bases(store: &Path) {
    let bases = store.join("gen-001/bases");
    if !bases.exists() {
        return;
    }
    let entries = std::fs::read_dir(&bases)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        entries.is_empty(),
        "from-artifact must not create resolution bases, found {entries:?}"
    );
}

fn visible_content_hashes(store: &Path, view: &str) -> Vec<(String, String)> {
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT entry.path, version.content_hash
             FROM views AS view
             JOIN manifest_entries AS entry
               ON entry.view_id = view.view_id AND entry.generation = view.current_generation
             JOIN file_versions AS version ON version.version_id = entry.version_id
             WHERE view.view_id = ?1
             ORDER BY entry.path",
        )
        .unwrap();
    statement
        .query_map([view], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn artifact_content_hashes(path: &Path) -> Vec<(String, String)> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare("SELECT path, content_hash FROM files ORDER BY path")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn strip_artifact_resolution(path: &Path) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch("DELETE FROM artifact_metadata WHERE key LIKE 'reference_resolution_%';")
        .unwrap();
}

fn table_count_if_exists(connection: &Connection, table: &str) -> i64 {
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )
        .unwrap();
    if !exists {
        return 0;
    }
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}
