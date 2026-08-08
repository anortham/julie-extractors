use std::path::PathBuf;
use std::process::Command;

use clap::Parser;
use julie_extract_cli::store::args::{StoreCli, StoreCommand, StoreLevelArg, StoreRootCommand};
use julie_extract_cli::store::report::{
    STORE_EXIT_INCOMPATIBLE, STORE_EXIT_OPERATIONAL_FAILURE, STORE_EXIT_SUCCESS, STORE_EXIT_USAGE,
    STORE_REPORT_SCHEMA_VERSION, StoreCommandOutcome, StoreCoordinatorDisposition,
    StoreFailureClass, StoreLevelCompletion, StoreManifestDisposition, StoreManifestReport,
    StoreOutputFormat, StoreOutputStream, StoreReport, StoreRequestState, StoreRequestedLevel,
    StoreRowCounts,
};
use serde_json::{Value, json};

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
    let StoreCommand::Import(args) = store.command;
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
    let StoreCommand::Import(args) = store.command;
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
    for verb in ["update", "delete", "export", "resolve"] {
        let parsed =
            StoreCli::try_parse_from(["julie-extract", "store", verb, "--store", "/tmp/family"]);
        assert!(parsed.is_err(), "future store verb {verb} must not parse");
    }
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
fn production_store_command_is_rejected_as_usage_without_output_on_stdout() {
    let output = julie_extract(&[
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
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("usage diagnostics must be UTF-8");
    assert!(stderr.contains("error:"));
    assert!(stderr.contains("unrecognized subcommand 'store'"));
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
    let StoreCommand::Import(args) = store.command;
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
fn legacy_help_and_json_parse_contract_do_not_expose_store() {
    let help = julie_extract(&["--help"]);
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).expect("help must be UTF-8");
    assert!(help.contains("scan"));
    assert!(help.contains("languages"));
    assert!(!help.contains("store"));

    let languages = julie_extract(&["languages", "--json"]);
    assert_eq!(languages.status.code(), Some(0));
    assert!(languages.stderr.is_empty());
    let report: Value = serde_json::from_slice(&languages.stdout).expect("legacy JSON report");
    assert_eq!(report["report_schema_version"], 3);
    assert_eq!(report["operation"], "languages");
}
