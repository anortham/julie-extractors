#![cfg(feature = "test-store-resolution-contract")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use julie_extract_artifact::store::{
    GenerationFence, LeaseDisposition, LeaseHolder, ResolutionBindingStore, ResolutionPinOwnerKind,
    StoreConnectionFactory, StoreCoordinator, StoreLayout,
};
use julie_extract_cli::store::args::{StoreCli, StoreCommand, StoreRootCommand};
use julie_extract_cli::store::report::{
    StoreCommandOutcome, StoreFailureClass, StoreOperation, StoreReport, StoreRequestState,
    StoreRequestedLevel, StoreResolutionState,
};
use rusqlite::{Connection, OpenFlags};
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

fn julie_extract_with_resolution_finalization_hook(
    store: &Path,
    request_id: &str,
    idempotency_key: &str,
    delta: Option<&str>,
    delay_ms: Option<u64>,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
    command
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            request_id,
            "--idempotency-key",
            idempotency_key,
            "--json",
        ])
        .env(
            "JULIE_EXTRACT_STORE_RESOLUTION_FAIL_BEFORE_EXACT_FINALIZE",
            "1",
        );
    if let Some(delay_ms) = delay_ms {
        command.env(
            "JULIE_EXTRACT_STORE_RESOLUTION_DELAY_SCOPED_FINALIZE_MS",
            delay_ms.to_string(),
        );
    } else {
        command.env_remove("JULIE_EXTRACT_STORE_RESOLUTION_DELAY_SCOPED_FINALIZE_MS");
    }
    if let Some(delta) = delta {
        command.env("JULIE_STORE_RESOLUTION_DELTA", delta);
    } else {
        command.env_remove("JULIE_STORE_RESOLUTION_DELTA");
    }
    command.output().expect("julie-extract should start")
}

#[track_caller]
fn assert_ran(output: std::process::Output) {
    assert!(
        output.status.success(),
        "command failed with {}
stdout={}
stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_child_stream<S: std::io::Read>(stream: Option<S>) -> String {
    let Some(mut stream) = stream else {
        return "<not captured>".to_string();
    };
    let mut text = String::new();
    match std::io::Read::read_to_string(&mut stream, &mut text) {
        Ok(_) => text,
        Err(error) => format!("<unreadable: {error}>"),
    }
}

fn open_store_read_only(path: &Path) -> Connection {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .unwrap()
}

/// Waits for a spawned resolve to reach its fault-injection pause point.
///
/// The deadline is a liveness backstop, not a performance budget. The child runs
/// a whole store resolve before it writes the marker, and a four-core hosted
/// runner needs much more time than a developer machine. A child that exits
/// without pausing fails at once, so the long deadline costs nothing in the
/// common failure.
fn wait_for_pause(child: &mut std::process::Child, path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(180);
    while !path.exists() {
        if let Some(status) = child.try_wait().expect("child status should be readable") {
            if path.exists() {
                return;
            }
            panic!(
                "child exited with {status} before it wrote {}\nstdout={}\nstderr={}",
                path.display(),
                read_child_stream(child.stdout.take()),
                read_child_stream(child.stderr.take())
            );
        }
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

fn create_ready_store_with_below_crossover_change(temp: &TempDir) -> (PathBuf, PathBuf) {
    let root = temp.path().join("scoped-source");
    let store = temp.path().join("scoped-family");
    let source = root.join("src");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("changed.rs"),
        "pub fn changed_target() -> i32 { 1 }\npub fn changed_use() -> i32 { changed_target() }\n",
    )
    .unwrap();
    for index in 0..8 {
        fs::write(
            source.join(format!("stable-{index}.rs")),
            format!(
                "pub fn stable_target_{index}() -> i32 {{ {index} }}\npub fn stable_use_{index}() -> i32 {{ stable_target_{index}() }}\n"
            ),
        )
        .unwrap();
    }
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
    assert_ran(import);
    let seed = julie_extract_with_resolution_delta(
        &[
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-seeded-ready-base",
            "--idempotency-key",
            "resolve-seeded-ready-base-key",
            "--json",
        ],
        "off",
    );
    assert_ran(seed);
    fs::write(
        source.join("changed.rs"),
        "pub fn changed_target() -> i32 { 2 }\npub fn changed_use() -> i32 { changed_target() }\n",
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
        "src/changed.rs",
        "--level",
        "full",
        "--request-id",
        "update-below-crossover",
        "--idempotency-key",
        "update-below-crossover-key",
        "--json",
    ]);
    assert_ran(update);
    (root, store)
}

fn install_canonical_gap_payload_bytes(store_db: &Path, view_id: &str, bytes: usize) {
    let prefix = r#"{"files":[1],"rows":[{"kind":"added","local_id":""#;
    let suffix = r#"","table":"identifier","version_id":1}]}"#;
    let connection = Connection::open(store_db).unwrap();
    let other_bytes: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(length(CAST(exact_gap_json AS BLOB))),0)
             FROM resolution_deltas
             WHERE view_id=?1
               AND delta_generation<>(SELECT resolution_delta_generation FROM views
                                      WHERE view_id=?1)",
            [view_id],
            |row| row.get(0),
        )
        .unwrap();
    let current_bytes = bytes
        .checked_sub(usize::try_from(other_bytes).unwrap())
        .unwrap();
    let padding = current_bytes
        .checked_sub(prefix.len() + suffix.len())
        .unwrap();
    connection
        .execute(
            "UPDATE resolution_deltas
             SET exact_gap_rows=1,exact_gap_files=1,
                 exact_gap_json=?1 || substr(replace(hex(zeroblob((?2 + 1) / 2)),'0','x'),1,?2) || ?3
             WHERE view_id=?4
               AND delta_generation=(SELECT resolution_delta_generation FROM views
                                     WHERE view_id=?4)",
            rusqlite::params![
                prefix,
                i64::try_from(padding).unwrap(),
                suffix,
                view_id
            ],
        )
        .unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT SUM(length(CAST(exact_gap_json AS BLOB))),MIN(json_valid(exact_gap_json))
                 FROM resolution_deltas
                 WHERE view_id=?1",
                [view_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap(),
        (i64::try_from(bytes).unwrap(), 1)
    );
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
fn scoped_resolution_streams_delta_without_full_exact_materialization() {
    let temp = TempDir::new();
    let (_root, store) = create_ready_store_with_below_crossover_change(&temp);

    let forced = julie_extract_with_resolution_finalization_hook(
        &store,
        "resolve-hook-forced-full",
        "resolve-hook-forced-full-key",
        Some("off"),
        None,
    );
    assert_eq!(forced.status.code(), Some(1));
    let forced_report: Value = serde_json::from_slice(&forced.stdout).unwrap();
    assert_eq!(forced_report["failure_class"], "resolution_failed");
    assert_eq!(
        forced_report["error"]["message"],
        "resolution_failed: test hook before exact finalization"
    );

    let scoped = julie_extract_with_resolution_finalization_hook(
        &store,
        "resolve-hook-scoped-default",
        "resolve-hook-scoped-default-key",
        None,
        Some(100),
    );
    assert!(
        scoped.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&scoped.stdout),
        String::from_utf8_lossy(&scoped.stderr)
    );
    let report: Value = serde_json::from_slice(&scoped.stdout).unwrap();
    assert_eq!(report["resolution"]["resolution_mode"], "scoped");
    assert!(report["resolution"]["fallback_reason"].is_null());
    assert!(report["resolution"]["phase_timings_ms"]["resolution"].is_u64());
    assert!(
        report["resolution"]["phase_timings_ms"]["diff"]
            .as_u64()
            .unwrap()
            >= 50
    );
}

#[test]
fn resolve_explicit_off_forces_full_and_emits_additive_telemetry() {
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
            "resolve-explicit-full",
            "--idempotency-key",
            "resolve-explicit-full-key",
            "--json",
        ],
        "off",
    );

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
fn resolve_unset_delta_promotes_crossover_and_reports_full_telemetry() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert_ran(resolve_output(
        &store,
        "resolve-scope-base",
        "resolve-scope-base-key",
    ));
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
    let resolve = resolve_output(&store, "resolve-scoped", "resolve-scoped-key");

    assert!(
        resolve.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );
    let report: Value = serde_json::from_slice(&resolve.stdout).unwrap();
    assert_eq!(report["resolution"]["resolution_mode"], "full");
    assert_eq!(
        report["resolution"]["fallback_reason"],
        "resolution_scope_crossover"
    );
    assert!(report["resolution"]["scope_file_count"].as_u64().unwrap() >= 1);
    assert_eq!(report["resolution"]["scope_name_count"], 0);
    assert!(report["resolution"]["scope_row_count"].as_u64().unwrap() >= 1);
    assert!(report["resolution"]["phase_timings_ms"]["scope"].is_u64());
}

#[test]
fn forced_full_resolve_ignores_unreadable_incremental_state() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    let seed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
        .env("JULIE_STORE_RESOLUTION_DELTA", "off")
        .env(
            "JULIE_EXTRACT_STORE_TEST_CRASH_AT",
            "resolution_prior_state_read",
        )
        .output()
        .unwrap();
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
    assert_ran(julie_extract(&[
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
    ]));
    Connection::open(store.join("gen-001/store.db"))
        .unwrap()
        .execute(
            "UPDATE resolution_scope_state SET current_manifest_hash='malformed'
                 WHERE view_id='view-main'",
            [],
        )
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
        ])
        .env("JULIE_STORE_RESOLUTION_DELTA", "off")
        .env(
            "JULIE_EXTRACT_STORE_TEST_CRASH_AT",
            "resolution_prior_state_read",
        )
        .output()
        .unwrap();
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

#[test]
fn retry_after_exact_publish_crash_replays_the_actual_scoped_telemetry() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    for index in 0..4 {
        let file = format!("stable-{index}.rs");
        fs::write(
            root.join(&file),
            format!(
                "pub fn stable_{index}() -> i32 {{ helper_{index}() }}\nfn helper_{index}() -> i32 {{ {index} }}\n"
            ),
        )
        .unwrap();
        assert_ran(julie_extract(&[
            "store",
            "update",
            "--store",
            store.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--file",
            &file,
            "--level",
            "full",
            "--json",
        ]));
    }
    assert_ran(resolve_output(
        &store,
        "resolve-telemetry-seed",
        "resolve-telemetry-seed-key",
    ));
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 2 }\n",
    )
    .unwrap();
    assert_ran(julie_extract(&[
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
    ]));

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
fn artifact_rebase_validation_precedes_private_accumulated_work_trigger() {
    let resolve = include_str!("../src/store/resolve.rs");
    let validation = resolve
        .find("exact_rebase_required_with_proof")
        .expect("resolve must validate artifact rebase requirements");
    let fold = resolve
        .find("artifact_rebase_required || decision.rebase_after_exact")
        .expect("resolve must fold the private trigger after validation");
    assert!(validation < fold);
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
    assert_ran(resolve_output(
        &store,
        "resolve-predecessor",
        "resolve-predecessor-key",
    ));
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
    for index in 0..4 {
        fs::write(
            root.join(format!("stable-{index}.rs")),
            format!(
                "pub fn stable_{index}() -> i32 {{ helper_{index}() }}\nfn helper_{index}() -> i32 {{ {index} }}\n"
            ),
        )
        .unwrap();
    }
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
fn replacement_rows_over_one_quarter_rebase_to_the_current_manifest_base() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert_ran(resolve_output(
        &store,
        "resolve-rebase-seed",
        "resolve-rebase-seed-key",
    ));
    let store_db = store.join("gen-001/store.db");
    let old_base: String = Connection::open(&store_db)
        .unwrap()
        .query_row(
            "SELECT resolution_base_id FROM views WHERE view_id='view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { replacement() }\nfn replacement() -> i32 { 9 }\n",
    )
    .unwrap();
    assert_ran(julie_extract(&[
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
    ]));

    let resolved = resolve_output(
        &store,
        "resolve-rebase-threshold",
        "resolve-rebase-threshold-key",
    );

    assert!(
        resolved.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&resolved.stdout),
        String::from_utf8_lossy(&resolved.stderr)
    );
    let connection = Connection::open(store_db).unwrap();
    let current: (String, i64, i64, i64, i64) = connection
        .query_row(
            "SELECT view.resolution_base_id,delta.identifier_replacements,
                    delta.pending_replacements,delta.pending_tombstones,delta.exact_gap_rows
             FROM views AS view
             JOIN resolution_deltas AS delta
               ON delta.view_id=view.view_id
              AND delta.delta_generation=view.resolution_delta_generation
             WHERE view.view_id='view-main'",
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
    assert_ne!(current.0, old_base);
    assert_eq!((current.1, current.2, current.3, current.4), (0, 0, 0, 0));
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM store_log
                 WHERE request_id='resolve-rebase-threshold'
                   AND event_kind='resolution_exact_rebased'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM resolution_deltas WHERE view_id='view-main'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1,
        "successful rebase must collect superseded deltas",
    );
}

#[test]
fn rebase_publication_failure_before_view_cas_keeps_ready_base_pinned() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert_ran(resolve_output(
        &store,
        "resolve-pre-cas-seed",
        "resolve-pre-cas-seed-key",
    ));
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { replacement() }\nfn replacement() -> i32 { 9 }\n",
    )
    .unwrap();
    assert_ran(julie_extract(&[
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
    ]));

    let request_id = "resolve-pre-cas-failure";
    let failed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            request_id,
            "--idempotency-key",
            "resolve-pre-cas-failure-key",
            "--json",
        ])
        .env(
            "JULIE_EXTRACT_STORE_TEST_FAIL_AT",
            "resolution_rebase_before_view_cas",
        )
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(report["failure_class"], "resolution_failed");

    let store_db = store.join("gen-001/store.db");
    let db = Connection::open(&store_db).unwrap();
    let (resolution_state, bound_base, manifest_hash): (String, String, String) = db
        .query_row(
            "SELECT view.resolution_state,view.resolution_base_id,manifest.manifest_hash
             FROM views AS view
             JOIN manifests AS manifest
               ON manifest.view_id=view.view_id AND manifest.generation=view.current_generation
             WHERE view.view_id='view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(resolution_state, "converging");
    let ready_base: String = db
        .query_row(
            "SELECT base.base_id
             FROM resolution_bases AS base
             WHERE base.state='ready' AND base.manifest_hash=?1 AND base.base_id<>?2
             ORDER BY base.updated_at DESC",
            rusqlite::params![manifest_hash, bound_base],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM resolution_pins
             WHERE pin_id=?1 AND owner_kind='resolve' AND owner_id=?2
               AND base_id=?3 AND delta_generation IS NULL",
            rusqlite::params![
                format!("resolve-rebase-{request_id}"),
                request_id,
                ready_base,
            ],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
        "a ready but unbound base must remain protected after pre-CAS failure",
    );
}

#[test]
fn conflicting_rebase_pin_survives_open_failure() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert_ran(resolve_output(
        &store,
        "resolve-conflict-seed",
        "resolve-conflict-seed-key",
    ));
    let store_db = store.join("gen-001/store.db");
    let (old_base, old_generation): (String, i64) = Connection::open(&store_db)
        .unwrap()
        .query_row(
            "SELECT resolution_base_id,current_generation
             FROM views WHERE view_id='view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let conflicting_pin_id = "resolve-rebase-resolve-conflict";
    Connection::open(&store_db)
        .unwrap()
        .execute(
            "INSERT INTO resolution_pins
             (pin_id,owner_kind,owner_id,view_id,manifest_generation,base_id,
              delta_generation,expires_at,created_at)
             VALUES (?1,'resolve',?2,'view-main',?3,?4,NULL,?5,?6)",
            rusqlite::params![
                conflicting_pin_id,
                "resolve-conflict",
                old_generation,
                old_base,
                "2099-01-01T00:00:00Z",
                "2026-08-14T00:00:00Z",
            ],
        )
        .unwrap();

    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { replacement() }\nfn replacement() -> i32 { 9 }\n",
    )
    .unwrap();
    assert_ran(julie_extract(&[
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
    ]));

    let failed = resolve_output(&store, "resolve-conflict", "resolve-conflict-key");
    assert_eq!(failed.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(report["failure_class"], "resolution_failed");
    assert!(
        report["error"]["message"]
            .as_str()
            .unwrap()
            .contains("temporary base pin identity conflicts with an existing pin")
    );

    let db = Connection::open(&store_db).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM resolution_pins WHERE pin_id=?1",
            [conflicting_pin_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
        "the pre-existing conflicting pin must survive the failed open",
    );
    let preserved: (String, String, String, i64, String, Option<i64>) = db
        .query_row(
            "SELECT pin_id,owner_kind,owner_id,manifest_generation,base_id,delta_generation
             FROM resolution_pins WHERE pin_id=?1",
            [conflicting_pin_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        preserved,
        (
            conflicting_pin_id.to_string(),
            "resolve".to_string(),
            "resolve-conflict".to_string(),
            old_generation,
            old_base,
            None,
        )
    );
}

#[test]
fn stale_rebase_pin_cleanup_cannot_delete_a_successor_reuse() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert_ran(resolve_output(
        &store,
        "resolve-fenced-seed",
        "resolve-fenced-seed-key",
    ));
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { replacement() }\nfn replacement() -> i32 { 9 }\n",
    )
    .unwrap();
    assert_ran(julie_extract(&[
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
    ]));

    let pause = temp.path().join("fenced-rebase.pause");
    let mut child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-fenced-stale",
            "--idempotency-key",
            "resolve-fenced-stale-key",
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
    wait_for_pause(&mut child, &pause);

    let store_db = store.join("gen-001/store.db");
    let pin_id = "resolve-rebase-resolve-fenced-stale";
    let db = Connection::open(&store_db).unwrap();
    let current: (i64, String, String, i64) = db
        .query_row(
            "SELECT view.current_generation,manifest.manifest_hash,
                    view.resolution_base_id,base.resolver_output_epoch
             FROM views AS view
             JOIN manifests AS manifest
               ON manifest.view_id=view.view_id AND manifest.generation=view.current_generation
             JOIN resolution_bases AS base ON base.base_id=view.resolution_base_id
             WHERE view.view_id='view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM resolution_pins
             WHERE pin_id=?1 AND owner_kind='resolve' AND owner_id='resolve-fenced-stale'
               AND delta_generation IS NULL",
            [pin_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
        "the paused resolver must hold the temporary rebase pin",
    );
    drop(db);

    let coordinator_db = store.join("coord.db");
    assert_eq!(
        Connection::open(&coordinator_db)
            .unwrap()
            .execute(
                "UPDATE writer_lease
                 SET expires_at=0,heartbeat_at=0,fencing_token=fencing_token+1",
                [],
            )
            .unwrap(),
        1
    );
    std::thread::sleep(Duration::from_millis(300));

    let layout = StoreLayout::open(&store).unwrap();
    let family_id = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
    let successor_holder = LeaseHolder::new(
        "successor-fenced-reuse",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
    );
    let mut successor_coordinator = StoreCoordinator::open(&layout).unwrap();
    let LeaseDisposition::Acquired {
        fencing_token: successor_token,
    } = successor_coordinator
        .try_acquire_or_takeover_now(successor_holder.clone())
        .unwrap()
    else {
        panic!("successor must take over the expired writer lease");
    };
    let successor_fence = GenerationFence::writer(
        &layout,
        successor_holder.holder_id.clone(),
        successor_holder.holder_pid,
        successor_token,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
    );
    let successor_factory =
        StoreConnectionFactory::new(layout.clone(), family_id, env!("CARGO_PKG_VERSION"))
            .with_generation_fence(successor_fence);
    let reused = ResolutionBindingStore::new(successor_factory.clone())
        .open_pin_for_base(
            pin_id,
            ResolutionPinOwnerKind::Resolve,
            "resolve-fenced-stale",
            "view-main",
            current.0,
            &current.1,
            &current.2,
            current.3,
            "2099-01-01T00:00:00Z",
            "2026-08-14T00:00:00Z",
        )
        .unwrap();
    assert_eq!(reused.base_id, current.2);
    assert!(
        successor_coordinator
            .release_lease(&successor_holder, successor_token)
            .unwrap()
    );

    fs::write(pause.with_extension("resume"), b"resume").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let db = Connection::open(&store_db).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM resolution_pins WHERE pin_id=?1",
            [pin_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
        "a stale fenced guard must not delete the successor's reused pin",
    );
    drop(db);

    let cleanup_holder = LeaseHolder::new(
        "successor-fenced-cleanup",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
    );
    let mut cleanup_coordinator = StoreCoordinator::open(&layout).unwrap();
    let LeaseDisposition::Acquired {
        fencing_token: cleanup_token,
    } = cleanup_coordinator
        .try_acquire_or_takeover_now(cleanup_holder.clone())
        .unwrap()
    else {
        panic!("same-fence cleanup must reacquire the writer lease");
    };
    let cleanup_fence = GenerationFence::writer(
        &layout,
        cleanup_holder.holder_id.clone(),
        cleanup_holder.holder_pid,
        cleanup_token,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64,
    );
    let cleanup_factory = StoreConnectionFactory::new(layout, family_id, env!("CARGO_PKG_VERSION"))
        .with_generation_fence(cleanup_fence);
    assert!(
        ResolutionBindingStore::new(cleanup_factory)
            .release_pin(
                pin_id,
                ResolutionPinOwnerKind::Resolve,
                "resolve-fenced-stale"
            )
            .unwrap()
    );
    assert!(
        cleanup_coordinator
            .release_lease(&cleanup_holder, cleanup_token)
            .unwrap()
    );
    assert_eq!(
        Connection::open(&store_db)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM resolution_pins WHERE pin_id=?1",
                [pin_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0,
    );
}

#[test]
fn rebase_crash_after_ready_keeps_the_new_base_pinned_until_retry() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert_ran(resolve_output(
        &store,
        "resolve-ready-seed",
        "resolve-ready-seed-key",
    ));
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { replacement() }\nfn replacement() -> i32 { 9 }\n",
    )
    .unwrap();
    assert_ran(julie_extract(&[
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
    ]));

    let crashed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-ready-crash",
            "--idempotency-key",
            "resolve-ready-crash-key",
            "--json",
        ])
        .env(
            "JULIE_EXTRACT_STORE_TEST_CRASH_AT",
            "resolution_base_after_ready_commit",
        )
        .output()
        .unwrap();
    assert!(!crashed.status.success());

    let db_path = store.join("gen-001/store.db");
    let rebased_base: String = open_store_read_only(&db_path)
        .query_row(
            "SELECT base.base_id
             FROM resolution_bases AS base
             JOIN views AS view ON view.view_id='view-main'
             JOIN manifests AS manifest
               ON manifest.view_id=view.view_id AND manifest.generation=view.current_generation
             WHERE base.state='ready' AND base.manifest_hash=manifest.manifest_hash
               AND base.base_id<>view.resolution_base_id",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let db = open_store_read_only(&db_path);
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM resolution_pins
             WHERE owner_kind='resolve' AND delta_generation IS NULL AND base_id=?1",
            [&rebased_base],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
        "a ready but unbound rebase base must remain protected after a crash",
    );
    drop(db);
    let db = Connection::open(&db_path).unwrap();
    db.execute(
        "UPDATE resolution_pins SET expires_at='1970-01-02T00:00:00Z'
         WHERE owner_kind='resolve' AND delta_generation IS NULL AND base_id=?1",
        [&rebased_base],
    )
    .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT resolution_state FROM views WHERE view_id='view-main'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "converging"
    );
    drop(db);

    let retry = resolve_output(&store, "resolve-ready-retry", "resolve-ready-crash-key");
    assert!(
        retry.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );
    let db = open_store_read_only(&db_path);
    assert_eq!(
        db.query_row(
            "SELECT resolution_state FROM views WHERE view_id='view-main'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap(),
        "exact"
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM resolution_deltas WHERE view_id='view-main'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
        "retry must converge to one current delta",
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM resolution_pins
             WHERE owner_kind='resolve' AND delta_generation IS NULL AND base_id=?1",
            [&rebased_base],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0,
        "retry must release the temporary rebase pin",
    );
}

#[test]
fn rebase_crash_after_view_cas_retries_cleanup_before_reporting_success() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert_ran(resolve_output(
        &store,
        "resolve-cas-seed",
        "resolve-cas-seed-key",
    ));
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { replacement() }\nfn replacement() -> i32 { 9 }\n",
    )
    .unwrap();
    assert_ran(julie_extract(&[
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
    ]));

    let crashed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-cas-crash",
            "--idempotency-key",
            "resolve-cas-crash-key",
            "--json",
        ])
        .env(
            "JULIE_EXTRACT_STORE_TEST_CRASH_AT",
            "resolution_rebase_after_store_commit",
        )
        .output()
        .unwrap();
    assert!(!crashed.status.success());

    let db_path = store.join("gen-001/store.db");
    let db = open_store_read_only(&db_path);
    let state: (String, i64, i64) = db
        .query_row(
            "SELECT resolution_state,resolution_delta_generation,
                    (SELECT COUNT(*) FROM resolution_deltas WHERE view_id='view-main')
             FROM views WHERE view_id='view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(state.0, "exact");
    assert!(state.1 > 0);
    assert!(state.2 > 1, "crash must leave cleanup owed for retry");
    let rebased_base: String = db
        .query_row(
            "SELECT resolution_base_id FROM views WHERE view_id='view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(db);

    let retry = resolve_output(&store, "resolve-cas-retry", "resolve-cas-crash-key");
    assert!(
        retry.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );
    let db = Connection::open(&db_path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM resolution_deltas WHERE view_id='view-main'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
        "retry must finish superseded-delta cleanup",
    );
    let current_delta: i64 = db
        .query_row(
            "SELECT resolution_delta_generation FROM views WHERE view_id='view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM resolution_deltas
             WHERE view_id='view-main' AND delta_generation=?1",
            [current_delta],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1,
        "the surviving delta must be the exact current view delta",
    );
    assert_eq!(
        db.query_row(
            "SELECT COUNT(*) FROM resolution_pins
             WHERE owner_kind='resolve' AND delta_generation IS NULL AND base_id=?1",
            [&rebased_base],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        0,
    );
}

#[test]
fn cumulative_gap_threshold_keeps_equality_and_rebases_the_first_byte_over() {
    const LIMIT: usize = 64 * 1024 * 1024;

    for extra_byte in [0, 1] {
        let temp = TempDir::new();
        let (root, store) = create_full_store(&temp);
        assert_ran(resolve_output(
            &store,
            &format!("resolve-gap-seed-{extra_byte}"),
            &format!("resolve-gap-seed-key-{extra_byte}"),
        ));
        let store_db = store.join("gen-001/store.db");
        let old_base: String = Connection::open(&store_db)
            .unwrap()
            .query_row(
                "SELECT resolution_base_id FROM views WHERE view_id='view-main'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let current_payload_bytes = serde_json::json!({"files": [], "rows": []})
            .to_string()
            .len();
        install_canonical_gap_payload_bytes(
            &store_db,
            "view-main",
            (LIMIT - current_payload_bytes) / 2 + extra_byte,
        );
        fs::write(root.join("extra.rs"), "// structural-only addition\n").unwrap();
        assert_ran(julie_extract(&[
            "store",
            "update",
            "--store",
            store.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--file",
            "extra.rs",
            "--level",
            "full",
            "--json",
        ]));

        let resolved = resolve_output(
            &store,
            &format!("resolve-gap-threshold-{extra_byte}"),
            &format!("resolve-gap-threshold-key-{extra_byte}"),
        );

        assert!(
            resolved.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&resolved.stdout),
            String::from_utf8_lossy(&resolved.stderr)
        );
        let connection = Connection::open(store_db).unwrap();
        let current: (String, i64, i64, i64) = connection
            .query_row(
                "SELECT view.resolution_base_id,delta.identifier_replacements,
                        delta.pending_replacements,delta.pending_tombstones
                 FROM views AS view
                 JOIN resolution_deltas AS delta
                   ON delta.view_id=view.view_id
                  AND delta.delta_generation=view.resolution_delta_generation
                 WHERE view.view_id='view-main'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        if extra_byte == 0 {
            assert_eq!(current.0, old_base);
        } else {
            assert_ne!(current.0, old_base);
        }
        assert_eq!((current.1, current.2, current.3), (0, 0, 0));
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM store_log
                     WHERE request_id=?1
                       AND event_kind='resolution_exact_rebased'",
                    [format!("resolve-gap-threshold-{extra_byte}")],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            extra_byte as i64,
        );
    }
}

#[test]
fn changes_below_both_rebase_thresholds_keep_the_cumulative_delta_path() {
    let temp = TempDir::new();
    let (root, store) = create_full_store(&temp);
    assert_ran(resolve_output(
        &store,
        "resolve-below-seed",
        "resolve-below-seed-key",
    ));
    let store_db = store.join("gen-001/store.db");
    let connection = Connection::open(&store_db).unwrap();
    let old_base: String = connection
        .query_row(
            "SELECT resolution_base_id FROM views WHERE view_id='view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);
    fs::write(root.join("small.rs"), "// structural-only addition\n").unwrap();
    assert_ran(julie_extract(&[
        "store",
        "update",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "small.rs",
        "--level",
        "full",
        "--json",
    ]));

    let resolved = resolve_output(
        &store,
        "resolve-below-threshold",
        "resolve-below-threshold-key",
    );

    assert!(resolved.status.success());
    let connection = Connection::open(store_db).unwrap();
    let current: (String, i64) = connection
        .query_row(
            "SELECT view.resolution_base_id,
                    delta.identifier_replacements + delta.pending_replacements
                      + delta.pending_tombstones
             FROM views AS view
             JOIN resolution_deltas AS delta
               ON delta.view_id=view.view_id
              AND delta.delta_generation=view.resolution_delta_generation
             WHERE view.view_id='view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(current.0, old_base);
    assert_eq!(current.1, 0);
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM store_log
                 WHERE request_id='resolve-below-threshold'
                   AND event_kind='resolution_exact_published'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
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
    assert_ran(julie_extract(&[
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
    ]));
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
    assert_ran(julie_extract(&[
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
    ]));

    let pause = temp.path().join("resolve.pause");
    let mut child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
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
    wait_for_pause(&mut child, &pause);
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
    let mut first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
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
    wait_for_pause(&mut first, &pause);

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
fn failed_resolve_waiter_returns_durable_failure_before_request_timeout() {
    let temp = TempDir::new();
    let (_, store) = create_full_store(&temp);
    let pause = temp.path().join("first-resolve.pause");
    let mut first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
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
    wait_for_pause(&mut first, &pause);

    let mut waiter = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-failed-waiter",
            "--idempotency-key",
            "resolve-failed-waiter-key",
            "--request-timeout-seconds",
            "2",
            "--json",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let coord = Connection::open(store.join("coord.db")).unwrap();
    let observe_deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if let Some(status) = waiter.try_wait().unwrap() {
            panic!(
                "waiter exited before its queued request was observed: {status}\nstdout={}\nstderr={}",
                read_child_stream(waiter.stdout.take()),
                read_child_stream(waiter.stderr.take())
            );
        }
        let state = coord
            .query_row(
                "SELECT state FROM requests WHERE request_id='resolve-failed-waiter'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok();
        if state.as_deref() == Some("queued") {
            break;
        }
        assert!(
            std::time::Instant::now() < observe_deadline,
            "timed out waiting for the resolve waiter request"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    coord
        .execute(
            "UPDATE requests SET state='failed',claim_owner=NULL,
             claim_heartbeat_at=NULL,result_json=NULL,error_json=?1,
             updated_at=updated_at+1
             WHERE request_id=?2 AND state='queued'",
            rusqlite::params![
                r#"{"message":"resolution_failed: injected durable failure"}"#,
                "resolve-failed-waiter"
            ],
        )
        .unwrap();

    let transitioned = std::time::Instant::now();
    let waiter_output = waiter.wait_with_output().unwrap();
    let waiter_elapsed = transitioned.elapsed();
    fs::write(pause.with_extension("resume"), b"resume").unwrap();
    let first_output = first.wait_with_output().unwrap();
    assert!(
        first_output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&first_output.stdout),
        String::from_utf8_lossy(&first_output.stderr)
    );
    assert_eq!(waiter_output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&waiter_output.stdout).unwrap();
    assert_eq!(report["failure_class"], "resolution_failed");
    assert!(
        waiter_elapsed < std::time::Duration::from_secs(1),
        "waiter did not return promptly: elapsed={:?}",
        waiter_elapsed
    );
    assert_eq!(
        report["error"]["message"],
        "resolution_failed: injected durable failure"
    );
    assert_eq!(report["state"], "failed");

    let waiter_state: (String, Option<String>) = coord
        .query_row(
            "SELECT state,claim_owner FROM requests WHERE request_id='resolve-failed-waiter'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(waiter_state, ("failed".to_string(), None));
}

#[test]
fn committed_resolve_waiter_reports_durable_success_before_request_timeout() {
    let temp = TempDir::new();
    let (_, store) = create_full_store(&temp);
    let pause = temp.path().join("first-resolve.pause");
    let mut first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-committed-waiter",
            "--idempotency-key",
            "resolve-committed-waiter-key",
            "--json",
        ])
        .env("JULIE_EXTRACT_STORE_RESOLUTION_PAUSE_FILE", &pause)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_pause(&mut first, &pause);

    let waiter = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "resolve-committed-waiter",
            "--idempotency-key",
            "resolve-committed-waiter-key",
            "--request-timeout-seconds",
            "2",
            "--json",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let transitioned = std::time::Instant::now();
    fs::write(pause.with_extension("resume"), b"resume").unwrap();
    let waiter_output = waiter.wait_with_output().unwrap();
    let waiter_elapsed = transitioned.elapsed();
    let first_output = first.wait_with_output().unwrap();
    assert!(
        first_output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&first_output.stdout),
        String::from_utf8_lossy(&first_output.stderr)
    );
    assert!(
        waiter_output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&waiter_output.stdout),
        String::from_utf8_lossy(&waiter_output.stderr)
    );
    assert!(
        waiter_elapsed < std::time::Duration::from_secs(1),
        "waiter did not return promptly: elapsed={:?}",
        waiter_elapsed
    );
    let report: Value = serde_json::from_slice(&waiter_output.stdout).unwrap();
    assert_eq!(report["state"], "committed");
    assert_eq!(report["resolution"]["state"], "exact");

    let coord = Connection::open(store.join("coord.db")).unwrap();
    let waiter_state: (String, Option<String>) = coord
        .query_row(
            "SELECT state,claim_owner FROM requests WHERE request_id='resolve-committed-waiter'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(waiter_state, ("committed".to_string(), None));
}

#[test]
fn resolve_claim_loss_stops_before_base_or_exact_publication() {
    let temp = TempDir::new();
    let root = temp.path().join("source");
    let store = temp.path().join("family");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 1 }\n").unwrap();
    let family = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
    assert_ran(julie_extract(&[
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
    ]));
    let pause = temp.path().join("lost.pause");
    let mut child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
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
    wait_for_pause(&mut child, &pause);
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
    assert_ran(resolve_output(&store, "resolve-seed", "resolve-seed-key"));
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 2 }\n",
    )
    .unwrap();
    assert_ran(julie_extract(&[
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
    ]));

    let pause = temp.path().join("pin-release.pause");
    let mut child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
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
    wait_for_pause(&mut child, &pause);

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
    assert_ran(resolve_output(
        &store,
        "resolve-clean-pin",
        "resolve-clean-pin-key",
    ));
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
    assert_ran(resolve_output(&store, "resolve-seed", "resolve-seed-key"));
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 2 }\n",
    )
    .unwrap();
    assert_ran(julie_extract(&[
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
    ]));

    let pause = temp.path().join("after-exact.pause");
    let mut child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
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
    wait_for_pause(&mut child, &pause);

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
        assert_ran(resolve_output(&store, "resolve-seed", "resolve-seed-key"));
        fs::write(
            root.join("lib.rs"),
            "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 2 }\n",
        )
        .unwrap();
        assert_ran(julie_extract(&[
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
        ]));
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
        let scope_rows = open_store_read_only(&store_db)
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
        let db = open_store_read_only(&store_db);
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
        let coord = open_store_read_only(&store.join("coord.db"));
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

#[test]
fn rebase_crash_boundaries_retry_with_one_ready_base_and_one_empty_delta() {
    for boundary in [
        "resolution_base_after_row_insert",
        "resolution_base_after_root_insert",
        "resolution_rebase_after_scratch_promote",
        "resolution_base_after_final_publish",
        "resolution_base_before_ready_commit",
        "resolution_base_after_ready_commit",
        "resolution_rebase_before_store_commit",
        "resolution_rebase_after_store_commit",
    ] {
        let temp = TempDir::new();
        let (root, store) = create_full_store(&temp);
        assert_ran(resolve_output(
            &store,
            "resolve-rebase-seed",
            "resolve-rebase-seed-key",
        ));
        let store_db = store.join("gen-001/store.db");
        let old_base: String = open_store_read_only(&store_db)
            .query_row(
                "SELECT resolution_base_id FROM views WHERE view_id='view-main'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        Connection::open(&store_db)
            .unwrap()
            .execute(
                "UPDATE resolution_bases SET identifier_count=1,pending_count=0 WHERE base_id=?1",
                [&old_base],
            )
            .unwrap();
        fs::write(
            root.join("lib.rs"),
            "pub fn answer() -> i32 { retry_target() }\nfn retry_target() -> i32 { 12 }\n",
        )
        .unwrap();
        assert_ran(julie_extract(&[
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
        ]));
        let crashed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args([
                "store",
                "resolve",
                "--store",
                store.to_str().unwrap(),
                "--view",
                "view-main",
                "--request-id",
                "resolve-rebase-crash",
                "--idempotency-key",
                "resolve-rebase-crash-key",
                "--json",
            ])
            .env("JULIE_EXTRACT_STORE_TEST_CRASH_AT", boundary)
            .output()
            .unwrap();
        assert!(!crashed.status.success(), "boundary={boundary}");
        let before_retry: (String, Option<String>, Option<i64>) = open_store_read_only(&store_db)
            .query_row(
                "SELECT resolution_state,resolution_base_id,resolution_delta_generation
                 FROM views WHERE view_id='view-main'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        match before_retry.0.as_str() {
            "converging" => assert_eq!(before_retry.1.as_deref(), Some(old_base.as_str())),
            "exact" => {
                assert_ne!(before_retry.1.as_deref(), Some(old_base.as_str()));
                let bound_base: String = open_store_read_only(&store_db)
                    .query_row(
                        "SELECT base_id FROM resolution_deltas
                         WHERE view_id='view-main' AND delta_generation=?1",
                        [before_retry.2.unwrap()],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(Some(bound_base), before_retry.1);
            }
            // `assert!(!crashed.status.success())` above passes for ANY failure, including one
            // that never reached the injected boundary. When that happens the view is still
            // `unbound` and this arm fires, so it must print the crashed child's own report —
            // otherwise the real cause is discarded and the state is all that is left to guess
            // from.
            state => panic!(
                "boundary={boundary} left unexpected state {state}; the crashed resolve may have \
                 failed before reaching the boundary. crashed stdout={} crashed stderr={}",
                String::from_utf8_lossy(&crashed.stdout),
                String::from_utf8_lossy(&crashed.stderr)
            ),
        }

        let retry = resolve_output(&store, "resolve-rebase-retry", "resolve-rebase-crash-key");

        assert!(
            retry.status.success(),
            "boundary={boundary} stdout={} stderr={}",
            String::from_utf8_lossy(&retry.stdout),
            String::from_utf8_lossy(&retry.stderr)
        );
        let report: Value = serde_json::from_slice(&retry.stdout).unwrap();
        assert_eq!(report["request"]["id"], "resolve-rebase-crash");
        assert_eq!(report["resolution"]["state"], "exact");
        let connection = open_store_read_only(&store_db);
        let final_state: (String, String, i64, String) = connection
            .query_row(
                "SELECT view.resolution_state,view.resolution_base_id,
                        view.resolution_delta_generation,manifest.manifest_hash
                 FROM views AS view
                 JOIN manifests AS manifest
                   ON manifest.view_id=view.view_id
                  AND manifest.generation=view.current_generation
                 WHERE view.view_id='view-main'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(final_state.0, "exact");
        assert_ne!(final_state.1, old_base);
        let delta: (String, i64, i64, i64, i64) = connection
            .query_row(
                "SELECT base_id,identifier_replacements,pending_replacements,
                        pending_tombstones,exact_gap_rows
                 FROM resolution_deltas
                 WHERE view_id='view-main' AND delta_generation=?1",
                [final_state.2],
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
        assert_eq!(delta, (final_state.1.clone(), 0, 0, 0, 0));
        let terminal: Value = serde_json::from_str(
            &connection
                .query_row(
                    "SELECT payload_json FROM store_log
                     WHERE request_id='resolve-rebase-crash' AND terminal=1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(terminal["identifier_replacements"], 0);
        assert_eq!(terminal["pending_replacements"], 0);
        assert_eq!(terminal["pending_tombstones"], 0);
        assert_eq!(terminal["exact_gap_rows"], 0);
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM resolution_bases
                     WHERE manifest_hash=?1 AND resolver_output_epoch=6",
                    [&final_state.3],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1,
            "boundary={boundary}"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM store_log
                     WHERE request_id='resolve-rebase-crash'
                       AND event_kind='resolution_exact_rebased'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1,
            "boundary={boundary}"
        );
    }
}
