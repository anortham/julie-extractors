#![cfg(feature = "test-store-resolution-contract")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use julie_extract_cli::store::args::{StoreCli, StoreCommand, StoreRootCommand};
use julie_extract_cli::store::report::{
    StoreCommandOutcome, StoreFailureClass, StoreOperation, StoreReport, StoreRequestState,
    StoreRequestedLevel, StoreResolutionState,
};
use rusqlite::Connection;
use serde_json::Value;

struct TempDir(PathBuf);

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "julie-store-resolution-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn julie_extract(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .env_remove("JULIE_STORE_RESOLUTION_DELTA")
        .args(args)
        .output()
        .expect("julie-extract should start")
}

fn julie_extract_with_resolution_delta(args: &[&str], value: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .env("JULIE_STORE_RESOLUTION_DELTA", value)
        .args(args)
        .output()
        .expect("julie-extract should start")
}

fn wait_for_path(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn create_full_store(temp: &TempDir) -> (PathBuf, PathBuf) {
    let root = temp.path().join("source");
    let store = temp.path().join("family");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 1 }\n",
    )
    .unwrap();
    let import = julie_extract(&[
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
        "full",
        "--json",
    ]);
    assert!(
        import.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&import.stdout),
        String::from_utf8_lossy(&import.stderr)
    );
    (root, store)
}

fn resolve_output(store: &Path, request_id: &str, key: &str) -> std::process::Output {
    julie_extract(&[
        "store",
        "resolve",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--request-id",
        request_id,
        "--idempotency-key",
        key,
        "--json",
    ])
}

#[test]
fn resolve_defaults_to_forced_full_and_emits_additive_telemetry() {
    let temp = TempDir::new();
    let (_, store) = create_full_store(&temp);

    let output = resolve_output(&store, "resolve-default-full", "resolve-default-full-key");

    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["resolution"]["resolution_mode"], "full");
    assert_eq!(
        report["resolution"]["fallback_reason"],
        "incremental_resolution_disabled"
    );
    assert!(report["resolution"]["scope_file_count"].is_u64());
    assert!(report["resolution"]["scope_name_count"].is_u64());
    assert!(report["resolution"]["scope_row_count"].is_u64());
    assert!(report["resolution"]["phase_timings_ms"].is_object());
}

#[test]
fn resolve_rejects_invalid_delta_escape_hatch_values() {
    let temp = TempDir::new();
    let (_, store) = create_full_store(&temp);

    let output = julie_extract_with_resolution_delta(
        &[
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-invalid-delta",
            "--idempotency-key",
            "resolve-invalid-delta-key",
            "--json",
        ],
        "sometimes",
    );

    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "resolution_failed");
    assert_eq!(
        report["error"]["message"],
        "resolution_failed: JULIE_STORE_RESOLUTION_DELTA must be 'on' or 'off', found 'sometimes'"
    );
}

#[test]
fn resolve_uses_scoped_mode_only_when_escape_hatch_is_on() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert!(
        resolve_output(&store, "resolve-scope-base", "resolve-scope-base-key")
            .status
            .success()
    );
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 2 }\n",
    )
    .unwrap();
    let update = julie_extract(&[
        "store",
        "update",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "lib.rs",
        "--level",
        "full",
        "--request-id",
        "update-for-scope",
        "--idempotency-key",
        "update-for-scope-key",
        "--json",
    ]);
    assert!(
        update.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&update.stdout),
        String::from_utf8_lossy(&update.stderr)
    );

    let resolve = julie_extract_with_resolution_delta(
        &[
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-scoped",
            "--idempotency-key",
            "resolve-scoped-key",
            "--json",
        ],
        "on",
    );

    assert!(
        resolve.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );
    let report: Value = serde_json::from_slice(&resolve.stdout).unwrap();
    assert_eq!(report["resolution"]["resolution_mode"], "scoped");
    assert!(report["resolution"]["fallback_reason"].is_null());
    assert!(report["resolution"]["scope_file_count"].as_u64().unwrap() >= 1);
    assert!(report["resolution"]["scope_name_count"].as_u64().unwrap() >= 1);
    assert!(report["resolution"]["scope_row_count"].as_u64().unwrap() >= 1);
    assert!(report["resolution"]["phase_timings_ms"]["resolution"].is_u64());
}

#[test]
fn explicit_off_and_unset_choose_the_same_forced_full_contract() {
    let unset_temp = TempDir::new();
    let off_temp = TempDir::new();
    let (_, unset_store) = create_full_store(&unset_temp);
    let (_, off_store) = create_full_store(&off_temp);

    let unset = resolve_output(&unset_store, "resolve-unset", "resolve-unset-key");
    let off = julie_extract_with_resolution_delta(
        &[
            "store",
            "resolve",
            "--store",
            off_store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-off",
            "--idempotency-key",
            "resolve-off-key",
            "--json",
        ],
        "off",
    );

    assert!(unset.status.success());
    assert!(off.status.success());
    let unset: Value = serde_json::from_slice(&unset.stdout).unwrap();
    let off: Value = serde_json::from_slice(&off.stdout).unwrap();
    for field in [
        "resolution_mode",
        "scope_file_count",
        "scope_name_count",
        "scope_row_count",
        "fallback_reason",
    ] {
        assert_eq!(unset["resolution"][field], off["resolution"][field]);
    }
}

#[test]
fn forced_full_resolve_ignores_unreadable_incremental_state() {
    for delta in [None, Some("off")] {
        let temp = TempDir::new();
        let (root, store) = create_full_store(&temp);
        let mut seed = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
        seed.args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-forced-seed",
            "--idempotency-key",
            "resolve-forced-seed-key",
            "--json",
        ])
        .env(
            "JULIE_EXTRACT_STORE_TEST_CRASH_AT",
            "resolution_prior_state_read",
        );
        match delta {
            Some(value) => {
                seed.env("JULIE_STORE_RESOLUTION_DELTA", value);
            }
            None => {
                seed.env_remove("JULIE_STORE_RESOLUTION_DELTA");
            }
        }
        let seed = seed.output().unwrap();
        assert!(
            seed.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&seed.stdout),
            String::from_utf8_lossy(&seed.stderr)
        );
        fs::write(
            root.join("lib.rs"),
            "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 2 }\n",
        )
        .unwrap();
        assert!(
            julie_extract(&[
                "store",
                "update",
                "--store",
                store.to_str().unwrap(),
                "--root",
                root.to_str().unwrap(),
                "--view",
                "view-main",
                "--file",
                "lib.rs",
                "--level",
                "full",
                "--json",
            ])
            .status
            .success()
        );
        Connection::open(store.join("gen-001/store.db"))
            .unwrap()
            .execute(
                "UPDATE resolution_scope_state SET current_manifest_hash='malformed'
                 WHERE view_id='view-main'",
                [],
            )
            .unwrap();
        let args = [
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-forced-corrupt-scope",
            "--idempotency-key",
            "resolve-forced-corrupt-scope-key",
            "--json",
        ];
        let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
        command.args(args).env(
            "JULIE_EXTRACT_STORE_TEST_CRASH_AT",
            "resolution_prior_state_read",
        );
        match delta {
            Some(value) => {
                command.env("JULIE_STORE_RESOLUTION_DELTA", value);
            }
            None => {
                command.env_remove("JULIE_STORE_RESOLUTION_DELTA");
            }
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["resolution"]["resolution_mode"], "full");
        assert_eq!(
            report["resolution"]["fallback_reason"],
            "incremental_resolution_disabled"
        );
    }
}

#[test]
fn retry_after_exact_publish_crash_replays_the_actual_scoped_telemetry() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert!(
        resolve_output(
            &store,
            "resolve-telemetry-seed",
            "resolve-telemetry-seed-key"
        )
        .status
        .success()
    );
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 2 }\n",
    )
    .unwrap();
    assert!(
        julie_extract(&[
            "store",
            "update",
            "--store",
            store.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--file",
            "lib.rs",
            "--level",
            "full",
            "--json",
        ])
        .status
        .success()
    );

    let crashed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-telemetry-crash",
            "--idempotency-key",
            "resolve-telemetry-crash-key",
            "--json",
        ])
        .env("JULIE_STORE_RESOLUTION_DELTA", "on")
        .env(
            "JULIE_EXTRACT_STORE_TEST_CRASH_AT",
            "resolution_exact_after_store_commit",
        )
        .output()
        .unwrap();
    assert!(!crashed.status.success());
    let durable_counts: (i64, i64, i64, i64, i64) =
        Connection::open(store.join("gen-001/store.db"))
            .unwrap()
            .query_row(
                "SELECT identifier_replacements,pending_replacements,pending_tombstones,
                        exact_gap_rows,exact_gap_files
                 FROM resolution_deltas
                 WHERE request_id='resolve-telemetry-crash'
                 ORDER BY delta_generation DESC LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
    assert!(durable_counts.0 + durable_counts.1 + durable_counts.2 > 0);
    assert!(durable_counts.3 > 0);

    let retry = julie_extract_with_resolution_delta(
        &[
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-telemetry-retry",
            "--idempotency-key",
            "resolve-telemetry-crash-key",
            "--json",
        ],
        "on",
    );
    assert!(
        retry.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );
    let report: Value = serde_json::from_slice(&retry.stdout).unwrap();
    assert_eq!(report["request"]["id"], "resolve-telemetry-crash");
    assert_eq!(report["resolution"]["resolution_mode"], "scoped");
    assert!(report["resolution"]["fallback_reason"].is_null());
    assert!(report["resolution"]["scope_file_count"].as_u64().unwrap() >= 1);
    assert!(report["resolution"]["scope_name_count"].as_u64().unwrap() >= 1);
    assert!(report["resolution"]["scope_row_count"].as_u64().unwrap() >= 1);
    assert!(report["resolution"]["phase_timings_ms"]["scope"].is_u64());
    let terminal: Value = serde_json::from_str(
        &Connection::open(store.join("gen-001/store.db"))
            .unwrap()
            .query_row(
                "SELECT payload_json FROM store_log
                 WHERE request_id='resolve-telemetry-crash' AND terminal=1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(terminal["identifier_replacements"], durable_counts.0);
    assert_eq!(terminal["pending_replacements"], durable_counts.1);
    assert_eq!(terminal["pending_tombstones"], durable_counts.2);
    assert_eq!(terminal["exact_gap_rows"], durable_counts.3);
    assert_eq!(terminal["exact_gap_files"], durable_counts.4);
    assert_eq!(terminal["gap_lower_bound"], durable_counts.3);
}

#[test]
fn resolve_has_one_store_scope_planner_seam_and_reports_its_actual_decision() {
    let resolve = include_str!("../src/store/resolve.rs");
    let session = include_str!("../src/store/resolution_session.rs");

    assert!(!resolve.contains("build_store_delta_scope"));
    assert_eq!(session.matches("build_store_delta_scope(").count(), 1);
    assert!(resolve.contains("!payload.resolution_delta_enabled"));
    assert!(resolve.contains(".decision_telemetry()"));
}

#[test]
fn resolve_parser_and_report_vocabulary_are_stable() {
    let parsed = StoreCli::try_parse_from([
        "julie-extract",
        "store",
        "resolve",
        "--store",
        "/tmp/family",
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--view",
        "view-main",
        "--request-id",
        "resolve-1",
        "--idempotency-key",
        "resolve-key-1",
        "--request-timeout-seconds",
        "45",
        "--json",
    ])
    .expect("public resolve syntax should parse");

    let StoreRootCommand::Store(store) = parsed.command;
    let StoreCommand::Resolve(args) = store.command else {
        panic!("expected resolve command");
    };
    assert_eq!(args.store, PathBuf::from("/tmp/family"));
    assert_eq!(
        args.family.as_deref(),
        Some("9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11")
    );
    assert_eq!(args.view, "view-main");
    assert_eq!(args.request.request_id.as_deref(), Some("resolve-1"));
    assert_eq!(
        args.request.idempotency_key.as_deref(),
        Some("resolve-key-1")
    );
    assert_eq!(args.request.request_timeout_seconds, 45);
    assert!(args.json);

    let mut report = StoreReport::new(
        "resolve-1",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "view-main",
        StoreRequestState::Claimed,
    );
    report.operation = StoreOperation::Resolve;
    report.requested_level = StoreRequestedLevel::NotApplicable;
    report.resolution.state = StoreResolutionState::Converging;
    report.resolution.base_id = Some("base-1".to_string());
    report.resolution.delta_generation = Some(2);
    report.resolution.gap_lower_bound = Some(7);
    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["operation"], "resolve");
    assert_eq!(value["requested_level"], "not_applicable");
    assert_eq!(value["resolution"]["state"], "converging");
    assert_eq!(value["resolution"]["base_id"], "base-1");
    assert_eq!(value["resolution"]["delta_generation"], 2);
    assert_eq!(value["resolution"]["gap_lower_bound"], 7);
    let human = StoreCommandOutcome::queued(report).render_human();
    assert!(human.contains(
        "resolution_detail: base=base-1 delta_generation=2 exact_at_generation=none gap_lower_bound=7 exact_gap_rows=none exact_gap_files=none\n"
    ));

    assert_eq!(
        serde_json::to_value(StoreResolutionState::Exact).unwrap(),
        "exact"
    );
    assert_eq!(
        serde_json::to_value(StoreFailureClass::ResolutionInputIncomplete).unwrap(),
        "resolution_input_incomplete"
    );
    assert_eq!(
        serde_json::to_value(StoreFailureClass::ResolutionFailed).unwrap(),
        "resolution_failed"
    );
    assert_eq!(
        serde_json::to_value(StoreFailureClass::ResolutionNotExact).unwrap(),
        "resolution_not_exact"
    );
}

#[test]
fn resolve_rejects_extraction_and_future_command_controls() {
    for forbidden in [
        ["--root", "/tmp/source"],
        ["--level", "full"],
        ["--jobs", "2"],
        ["--file", "src/lib.rs"],
    ] {
        let mut args = vec![
            "julie-extract",
            "store",
            "resolve",
            "--store",
            "/tmp/family",
            "--view",
            "view-main",
        ];
        args.extend(forbidden);
        assert!(StoreCli::try_parse_from(args).is_err());
    }
}

#[test]
fn public_resolve_builds_an_exact_binding_without_extracting_again() {
    let temp = TempDir::new();
    let root = temp.path().join("source");
    let store = temp.path().join("family");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    let family = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
    let import = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        family,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "full",
        "--request-id",
        "import-1",
        "--idempotency-key",
        "import-key-1",
        "--json",
    ]);
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );

    let store_db = store.join("gen-001/store.db");
    let versions_before: i64 = Connection::open(&store_db)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM file_versions", [], |row| row.get(0))
        .unwrap();
    let resolve = julie_extract(&[
        "store",
        "resolve",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--request-id",
        "resolve-1",
        "--idempotency-key",
        "resolve-key-1",
        "--json",
    ]);
    assert!(
        resolve.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );
    let report: Value = serde_json::from_slice(&resolve.stdout).unwrap();
    assert_eq!(report["operation"], "resolve");
    assert_eq!(report["requested_level"], "not_applicable");
    assert_eq!(report["state"], "committed");
    assert_eq!(report["resolution"]["state"], "exact");
    assert_eq!(report["resolution"]["exact_at_matches"], true);
    assert!(report["resolution"]["base_id"].as_str().is_some());
    assert!(report["resolution"]["delta_generation"].as_u64().is_some());

    let store_connection = Connection::open(&store_db).unwrap();
    assert_eq!(
        store_connection
            .query_row("SELECT COUNT(*) FROM file_versions", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        versions_before
    );
    assert_eq!(
        store_connection
            .query_row(
                "SELECT resolution_state FROM views WHERE view_id='view-main'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "exact"
    );
    let coord = Connection::open(store.join("coord.db")).unwrap();
    assert_eq!(
        coord
            .query_row(
                "SELECT state FROM requests WHERE request_id='resolve-1' AND kind='resolve'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "committed"
    );
    assert_eq!(
        coord
            .query_row("SELECT COUNT(*) FROM writer_lease", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
    let delta_generation = store_connection
        .query_row(
            "SELECT MAX(delta_generation) FROM resolution_deltas WHERE view_id='view-main'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    let delta_high_water = coord
        .query_row(
            "SELECT high_water FROM family_allocator_marks
             WHERE allocator_kind='resolution_delta_generation' AND scope_id='view-main'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert!(delta_high_water >= delta_generation);
}

#[test]
fn from_artifact_exact_publication_clears_scope_atomically_after_retry() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert!(
        resolve_output(&store, "resolve-predecessor", "resolve-predecessor-key")
            .status
            .success()
    );
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 2 }\n",
    )
    .unwrap();
    let artifact = temp.path().join("updated.db");
    let scan = julie_extract(&[
        "scan",
        "--root",
        root.to_str().unwrap(),
        "--db",
        artifact.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        scan.status.success(),
        "{}",
        String::from_utf8_lossy(&scan.stdout)
    );
    let args = [
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
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--request-id",
        "from-artifact-scope",
        "--idempotency-key",
        "from-artifact-scope-key",
        "--json",
    ];
    let crashed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .env(
            "JULIE_EXTRACT_STORE_TEST_CRASH_AT",
            "from_artifact_exact_after_cas_before_commit",
        )
        .output()
        .unwrap();
    assert!(!crashed.status.success());

    let store_db = store.join("gen-001/store.db");
    assert_eq!(
        Connection::open(&store_db)
            .unwrap()
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM resolution_scope_state),
                   (SELECT COUNT(*) FROM resolution_scope_batches),
                   (SELECT COUNT(*) FROM resolution_scope_journal)",
                [],
                |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?
                )),
            )
            .unwrap(),
        (1, 1, 1)
    );

    let retried = julie_extract(&args);
    assert!(
        retried.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&retried.stdout),
        String::from_utf8_lossy(&retried.stderr)
    );
    assert_eq!(
        Connection::open(&store_db)
            .unwrap()
            .query_row(
                "SELECT resolution_state='exact',
                        (SELECT COUNT(*) FROM resolution_scope_state),
                        (SELECT COUNT(*) FROM resolution_scope_batches),
                        (SELECT COUNT(*) FROM resolution_scope_journal)
                 FROM views WHERE view_id='view-main'",
                [],
                |row| {
                    Ok((
                        row.get::<_, bool>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .unwrap(),
        (true, 0, 0, 0)
    );
}

#[test]
fn incomplete_l2_resolve_fails_durably_without_base_delta_or_exactness() {
    let temp = TempDir::new();
    let root = temp.path().join("source");
    let store = temp.path().join("family");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    let family = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
    let import = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        family,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "l1",
        "--json",
    ]);
    assert!(import.status.success());

    let resolve = julie_extract(&[
        "store",
        "resolve",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--request-id",
        "resolve-incomplete",
        "--idempotency-key",
        "resolve-incomplete-key",
        "--json",
    ]);
    assert_eq!(resolve.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&resolve.stdout).unwrap();
    assert_eq!(report["failure_class"], "resolution_input_incomplete");
    assert_eq!(report["state"], "failed");
    assert_eq!(report["resolution"]["state"], "unbound");

    let db = Connection::open(store.join("gen-001/store.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM resolution_bases", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM resolution_deltas", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT resolution_state FROM views WHERE view_id='view-main'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "unbound"
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM store_log WHERE request_id='resolve-incomplete'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
    let coord = Connection::open(store.join("coord.db")).unwrap();
    assert_eq!(
        coord
            .query_row(
                "SELECT state FROM requests WHERE request_id='resolve-incomplete'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "failed"
    );
}

#[test]
fn changed_manifest_resolves_against_the_ready_base_and_publishes_a_cumulative_delta() {
    let temp = TempDir::new();
    let root = temp.path().join("source");
    let store = temp.path().join("family");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 1 }\n",
    )
    .unwrap();
    let family = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
    let import = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        family,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "full",
        "--json",
    ]);
    assert!(import.status.success());
    let first = julie_extract(&[
        "store",
        "resolve",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--request-id",
        "resolve-1",
        "--idempotency-key",
        "resolve-key-1",
        "--json",
    ]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );

    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 2 }\n",
    )
    .unwrap();
    let update = julie_extract(&[
        "store",
        "update",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "lib.rs",
        "--level",
        "full",
        "--json",
    ]);
    assert!(
        update.status.success(),
        "{}",
        String::from_utf8_lossy(&update.stdout)
    );
    let second = julie_extract(&[
        "store",
        "resolve",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--request-id",
        "resolve-2",
        "--idempotency-key",
        "resolve-key-2",
        "--json",
    ]);
    assert!(
        second.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    let report: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(report["resolution"]["state"], "exact");
    assert_eq!(report["manifest"]["generation"], 2);
    assert!(report["resolution"]["delta_generation"].as_u64().unwrap() > 1);

    let db = Connection::open(store.join("gen-001/store.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM resolution_bases", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM resolution_deltas", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        3
    );
}

#[test]
fn resolve_idempotency_replay_observes_the_original_terminal_without_reexecution() {
    let temp = TempDir::new();
    let root = temp.path().join("source");
    let store = temp.path().join("family");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
    let family = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
    assert!(
        julie_extract(&[
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            family,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--json",
        ])
        .status
        .success()
    );
    let first = julie_extract(&[
        "store",
        "resolve",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--request-id",
        "resolve-original",
        "--idempotency-key",
        "resolve-stable-key",
        "--json",
    ]);
    assert!(first.status.success());
    let replay = julie_extract(&[
        "store",
        "resolve",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--request-id",
        "resolve-retry",
        "--idempotency-key",
        "resolve-stable-key",
        "--json",
    ]);
    assert!(replay.status.success());
    let original: Value = serde_json::from_slice(&first.stdout).unwrap();
    let observed: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(observed, original);
    assert_eq!(observed["request"]["id"], "resolve-original");

    let coord = Connection::open(store.join("coord.db")).unwrap();
    assert_eq!(
        coord
            .query_row(
                "SELECT COUNT(*) FROM requests WHERE kind='resolve'",
                [],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
        1
    );
    let db = Connection::open(store.join("gen-001/store.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM store_log WHERE request_id='resolve-original' AND terminal=1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
}

#[test]
fn claimed_resolve_holds_no_writer_lease_and_a_short_update_completes() {
    let temp = TempDir::new();
    let root = temp.path().join("source");
    let store = temp.path().join("family");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 1 }\n").unwrap();
    let family = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
    assert!(
        julie_extract(&[
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            family,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--json",
        ])
        .status
        .success()
    );

    let pause = temp.path().join("resolve.pause");
    let child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-paused",
            "--idempotency-key",
            "resolve-paused-key",
            "--json",
        ])
        .env("JULIE_EXTRACT_STORE_RESOLUTION_PAUSE_FILE", &pause)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_path(&pause);
    let coord = Connection::open(store.join("coord.db")).unwrap();
    assert_eq!(
        coord
            .query_row("SELECT COUNT(*) FROM writer_lease", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        coord
            .query_row(
                "SELECT state FROM requests WHERE request_id='resolve-paused'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "claimed"
    );

    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 2 }\n").unwrap();
    let update = julie_extract(&[
        "store",
        "update",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "lib.rs",
        "--level",
        "full",
        "--json",
    ]);
    assert!(
        update.status.success(),
        "{}",
        String::from_utf8_lossy(&update.stdout)
    );
    fs::write(root.join("extra.rs"), "pub fn extra() -> i32 { 3 }\n").unwrap();
    let import = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        family,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "full",
        "--json",
    ]);
    assert!(
        import.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&import.stdout),
        String::from_utf8_lossy(&import.stderr)
    );
    let delete = julie_extract(&[
        "store",
        "delete",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "extra.rs",
        "--json",
    ]);
    assert!(
        delete.status.success(),
        "{}",
        String::from_utf8_lossy(&delete.stdout)
    );
    fs::write(pause.with_extension("resume"), b"resume").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let store_db = Connection::open(store.join("gen-001/store.db")).unwrap();
    assert_eq!(
        store_db
            .query_row(
                "SELECT resolution_state FROM views WHERE view_id='view-main'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "exact"
    );
    assert_eq!(
        store_db
            .query_row(
                "SELECT COUNT(*)
                 FROM manifest_entries AS entry JOIN views AS view
                   ON view.view_id=entry.view_id AND view.current_generation=entry.generation
                 WHERE entry.view_id='view-main' AND entry.path='extra.rs'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn same_view_resolves_serialize_and_the_waiter_reuses_its_durable_request() {
    let temp = TempDir::new();
    let (_, store) = create_full_store(&temp);
    let pause = temp.path().join("first-resolve.pause");
    let first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-first",
            "--idempotency-key",
            "resolve-first-key",
            "--json",
        ])
        .env("JULIE_EXTRACT_STORE_RESOLUTION_PAUSE_FILE", &pause)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_path(&pause);

    let waiter = julie_extract(&[
        "store",
        "resolve",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--request-id",
        "resolve-waiter",
        "--idempotency-key",
        "resolve-waiter-key",
        "--request-timeout-seconds",
        "1",
        "--json",
    ]);
    assert_eq!(waiter.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&waiter.stdout).unwrap();
    assert_eq!(report["failure_class"], "request_timeout");

    fs::write(pause.with_extension("resume"), b"resume").unwrap();
    let first = first.wait_with_output().unwrap();
    assert!(first.status.success());
    let retry = resolve_output(&store, "ignored-retry-id", "resolve-waiter-key");
    assert!(
        retry.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );
    let retry_report: Value = serde_json::from_slice(&retry.stdout).unwrap();
    assert_eq!(retry_report["request"]["id"], "resolve-waiter");
    assert_eq!(retry_report["resolution"]["state"], "exact");
    let store_db = Connection::open(store.join("gen-001/store.db")).unwrap();
    let coord = Connection::open(store.join("coord.db")).unwrap();
    assert_eq!(
        store_db
            .query_row(
                "SELECT COUNT(*) FROM store_log
                 WHERE request_id IN ('resolve-first','resolve-waiter') AND terminal=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        2
    );
    assert_eq!(
        coord
            .query_row(
                "SELECT COUNT(*) FROM requests
                 WHERE request_id IN ('resolve-first','resolve-waiter') AND state='committed'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        2
    );
    assert_eq!(
        coord
            .query_row("SELECT COUNT(*) FROM writer_lease", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn resolve_claim_loss_stops_before_base_or_exact_publication() {
    let temp = TempDir::new();
    let root = temp.path().join("source");
    let store = temp.path().join("family");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 1 }\n").unwrap();
    let family = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
    assert!(
        julie_extract(&[
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            family,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--json",
        ])
        .status
        .success()
    );
    let pause = temp.path().join("lost.pause");
    let child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-lost",
            "--idempotency-key",
            "resolve-lost-key",
            "--json",
        ])
        .env("JULIE_EXTRACT_STORE_RESOLUTION_PAUSE_FILE", &pause)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_path(&pause);
    let coord = Connection::open(store.join("coord.db")).unwrap();
    coord
        .execute(
            "UPDATE requests SET claim_owner='successor',claim_heartbeat_at=9999999999999
             WHERE request_id='resolve-lost'",
            [],
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    fs::write(pause.with_extension("resume"), b"resume").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "resolution_failed");
    let db = Connection::open(store.join("gen-001/store.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM resolution_bases", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM resolution_deltas", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap(),
        0
    );
}

#[test]
fn failed_resolve_releases_live_pin_when_writer_ownership_can_be_reacquired() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert!(
        resolve_output(&store, "resolve-seed", "resolve-seed-key")
            .status
            .success()
    );
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 2 }\n",
    )
    .unwrap();
    assert!(
        julie_extract(&[
            "store",
            "update",
            "--store",
            store.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--file",
            "lib.rs",
            "--level",
            "full",
            "--json",
        ])
        .status
        .success()
    );

    let pause = temp.path().join("pin-release.pause");
    let child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-pin-release",
            "--idempotency-key",
            "resolve-pin-release-key",
            "--json",
        ])
        .env(
            "JULIE_EXTRACT_STORE_RESOLUTION_PAUSE_AFTER_EXACT_FILE",
            &pause,
        )
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_path(&pause);

    // Expire the held writer lease so the post-exact terminal append fails closed.
    // Claim ownership remains; Drop can reacquire a writer lease and release the pin.
    Connection::open(store.join("coord.db"))
        .unwrap()
        .execute("UPDATE writer_lease SET expires_at=0, heartbeat_at=0", [])
        .unwrap();

    fs::write(pause.with_extension("resume"), b"resume").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        !output.status.success(),
        "resolve must fail after lease loss; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let db = Connection::open(store.join("gen-001/store.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM resolution_pins
             WHERE pin_id LIKE 'resolve-resolve-pin-release-%'
               AND owner_kind='resolve'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0,
        "failed resolve must release its live pin when writer ownership can be reacquired"
    );
}

#[test]
fn successful_resolve_leaves_no_live_resolve_pin() {
    let temp = TempDir::new();
    let (_, store) = create_full_store(&temp);
    assert!(
        resolve_output(&store, "resolve-clean-pin", "resolve-clean-pin-key")
            .status
            .success()
    );
    let db = Connection::open(store.join("gen-001/store.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM resolution_pins WHERE owner_kind='resolve'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0
    );
}

#[test]
fn resolve_terminal_append_fails_closed_under_foreign_live_maintenance_intent() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert!(
        resolve_output(&store, "resolve-seed", "resolve-seed-key")
            .status
            .success()
    );
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 2 }\n",
    )
    .unwrap();
    assert!(
        julie_extract(&[
            "store",
            "update",
            "--store",
            store.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--file",
            "lib.rs",
            "--level",
            "full",
            "--json",
        ])
        .status
        .success()
    );

    let pause = temp.path().join("after-exact.pause");
    let child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-fenced-terminal",
            "--idempotency-key",
            "resolve-fenced-terminal-key",
            "--json",
        ])
        .env(
            "JULIE_EXTRACT_STORE_RESOLUTION_PAUSE_AFTER_EXACT_FILE",
            &pause,
        )
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_path(&pause);

    // Foreign live maintenance intent is durable before the terminal append.
    // Unfenced Connection::open(store.db) would still write the terminal row;
    // fenced open_writer must refuse and leave no terminal fact.
    Connection::open(store.join("coord.db"))
        .unwrap()
        .execute(
            "INSERT INTO maintenance_intent
             (resource, run_id, action, source_generation_name, owner_id, owner_pid,
              fencing_token, heartbeat_at, expires_at, started_at, plan_fingerprint,
              source_min_writer_version)
             VALUES ('store-maintenance', 'run-foreign', 'promote', 'gen-001', 'maint-owner', 7,
                     99, 1, 9223372036854775807, 1, 'plan-foreign', '2.30.0')",
            [],
        )
        .unwrap();

    fs::write(pause.with_extension("resume"), b"resume").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        !output.status.success(),
        "resolve must fail closed under foreign live maintenance intent; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let db = Connection::open(store.join("gen-001/store.db")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM store_log
             WHERE request_id='resolve-fenced-terminal' AND terminal=1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0,
        "terminal append must not land without a generation fence"
    );
}

#[test]
fn hard_kill_boundaries_resume_one_resolve_without_duplicate_terminal_state() {
    {
        let temp = TempDir::new();
        let (_, store) = create_full_store(&temp);
        let crashed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args([
                "store",
                "resolve",
                "--store",
                store.to_str().unwrap(),
                "--view",
                "view-main",
                "--request-id",
                "resolve-base-crash",
                "--idempotency-key",
                "resolve-base-crash-key",
                "--json",
            ])
            .env(
                "JULIE_EXTRACT_STORE_TEST_CRASH_AT",
                "resolution_base_after_final_publish",
            )
            .output()
            .unwrap();
        assert!(!crashed.status.success());
        let retry = resolve_output(&store, "resolve-base-retry", "resolve-base-crash-key");
        assert!(
            retry.status.success(),
            "{}",
            String::from_utf8_lossy(&retry.stdout)
        );
    }

    for boundary in [
        "resolution_exact_after_scratch_create",
        "resolution_before_exact_publish",
        "resolution_exact_before_store_commit",
        "resolution_exact_after_store_commit",
        "resolution_terminal_after_store_commit",
        "resolution_coord_after_commit",
    ] {
        let temp = TempDir::new();
        let (root, store) = create_full_store(&temp);
        assert!(
            resolve_output(&store, "resolve-seed", "resolve-seed-key")
                .status
                .success()
        );
        fs::write(
            root.join("lib.rs"),
            "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 2 }\n",
        )
        .unwrap();
        assert!(
            julie_extract(&[
                "store",
                "update",
                "--store",
                store.to_str().unwrap(),
                "--root",
                root.to_str().unwrap(),
                "--view",
                "view-main",
                "--file",
                "lib.rs",
                "--level",
                "full",
                "--json",
            ])
            .status
            .success()
        );
        let crashed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args([
                "store",
                "resolve",
                "--store",
                store.to_str().unwrap(),
                "--view",
                "view-main",
                "--request-id",
                "resolve-crash",
                "--idempotency-key",
                "resolve-crash-key",
                "--json",
            ])
            .env("JULIE_EXTRACT_STORE_TEST_CRASH_AT", boundary)
            .output()
            .unwrap();
        assert!(
            !crashed.status.success(),
            "boundary {boundary} returned normally"
        );
        let store_db = store.join("gen-001/store.db");
        let scope_rows = Connection::open(&store_db)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM resolution_scope_state", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        assert_eq!(
            scope_rows,
            i64::from(matches!(
                boundary,
                "resolution_exact_after_scratch_create"
                    | "resolution_before_exact_publish"
                    | "resolution_exact_before_store_commit"
            )),
            "boundary={boundary}"
        );
        let retry = resolve_output(&store, "resolve-retry", "resolve-crash-key");
        assert!(
            retry.status.success(),
            "boundary={boundary} stdout={} stderr={}",
            String::from_utf8_lossy(&retry.stdout),
            String::from_utf8_lossy(&retry.stderr)
        );
        let report: Value = serde_json::from_slice(&retry.stdout).unwrap();
        assert_eq!(report["request"]["id"], "resolve-crash");
        assert_eq!(report["resolution"]["state"], "exact");
        let db = Connection::open(store_db).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM store_log
                 WHERE request_id='resolve-crash' AND terminal=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM resolution_scope_state", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
            0,
            "boundary={boundary}"
        );
        let coord = Connection::open(store.join("coord.db")).unwrap();
        assert_eq!(
            coord
                .query_row(
                    "SELECT state FROM requests WHERE request_id='resolve-crash'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "committed"
        );
        assert_eq!(
            coord
                .query_row("SELECT COUNT(*) FROM writer_lease", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
        assert!(
            fs::read_dir(store.join("scratch"))
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("resolve-crash")),
            "boundary {boundary} left request-owned scratch"
        );
    }
}
