#![recursion_limit = "256"]

#[cfg(feature = "test-store-contract")]
use std::process::Stdio;
use std::process::{Command, Output};
#[cfg(feature = "test-store-contract")]
use std::time::Duration;

use julie_extract_artifact::store::StoreLayout;
use serde_json::{Value, json};

const FAMILY_ID: &str = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";

fn julie_extract(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .expect("julie-extract should start")
}

#[test]
fn maintenance_namespace_exposes_the_approved_nested_commands() {
    let output = julie_extract(&["store", "maintain", "--help"]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for command in ["inspect", "gc", "repair", "promote"] {
        assert!(stdout.contains(command), "missing {command} in {stdout}");
    }
}

#[test]
fn inspect_is_read_only_and_emits_the_separate_versioned_report() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();

    let output = julie_extract(&[
        "store",
        "maintain",
        "inspect",
        "--store",
        store.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout.iter().filter(|&&byte| byte == b'\n').count(),
        1
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["report_schema_version"], 1);
    assert_eq!(report["action"], "inspect");
    assert_eq!(report["mode"], "plan");
    assert_eq!(report["family_id"], FAMILY_ID);
    assert_eq!(report["source_generation"], "gen-001");
    assert!(report["destination_generation"].is_null());
    assert_eq!(report["disposition"], "planned");
    assert_eq!(report["failure_class"], "none");
    assert!(
        report["plan_fingerprint"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(report["counts"].is_object());
    assert!(report["retention"].is_object());
    assert!(report["capacity"]["free_bytes"].is_number());
    assert!(report.get("request").is_none());
    assert!(report.get("view_id").is_none());
}

#[test]
fn gc_plans_without_apply_and_mutates_only_with_apply() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let store_db = store.join("gen-001/store.db");
    let coord_db = store.join("coord.db");
    let store_before = std::fs::read(&store_db).unwrap();
    let coord_before = std::fs::read(&coord_db).unwrap();

    let planned = julie_extract(&[
        "store",
        "maintain",
        "gc",
        "--store",
        store.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(planned.status.code(), Some(0));
    let planned_report: Value = serde_json::from_slice(&planned.stdout).unwrap();
    assert_eq!(planned_report["mode"], "plan");
    assert_eq!(planned_report["disposition"], "planned");
    assert_eq!(std::fs::read(&store_db).unwrap(), store_before);
    assert_eq!(std::fs::read(&coord_db).unwrap(), coord_before);

    let applied = julie_extract(&[
        "store",
        "maintain",
        "gc",
        "--store",
        store.to_str().unwrap(),
        "--apply",
        "--json",
    ]);
    assert_eq!(
        applied.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&applied.stdout),
        String::from_utf8_lossy(&applied.stderr)
    );
    assert!(applied.stderr.is_empty());
    let applied_report: Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(applied_report["mode"], "apply");
    assert!(matches!(
        applied_report["disposition"].as_str(),
        Some("applied" | "no_change")
    ));
    let coordinator = rusqlite::Connection::open(coord_db).unwrap();
    let active: i64 = coordinator
        .query_row(
            "SELECT (SELECT COUNT(*) FROM maintenance_intent) +
                    (SELECT COUNT(*) FROM writer_lease)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(active, 0);
}

#[test]
fn promote_builds_and_publishes_a_new_generation_only_with_apply() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();

    let planned = julie_extract(&[
        "store",
        "maintain",
        "promote",
        "--store",
        store.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(planned.status.code(), Some(0));
    assert_eq!(
        std::fs::read_to_string(store.join("CURRENT")).unwrap(),
        "gen-001\n"
    );

    let applied = julie_extract(&[
        "store",
        "maintain",
        "promote",
        "--store",
        store.to_str().unwrap(),
        "--apply",
        "--json",
    ]);
    assert_eq!(
        applied.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&applied.stdout),
        String::from_utf8_lossy(&applied.stderr)
    );
    let report: Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(report["mode"], "apply");
    assert_eq!(report["disposition"], "applied");
    assert_eq!(report["source_generation"], "gen-001");
    assert_eq!(report["destination_generation"], "gen-002");
    assert_eq!(
        std::fs::read_to_string(store.join("CURRENT")).unwrap(),
        "gen-002\n"
    );
}

#[test]
fn repair_checkpoints_a_valid_generation_without_replacing_current() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();

    let output = julie_extract(&[
        "store",
        "maintain",
        "repair",
        "--store",
        store.to_str().unwrap(),
        "--apply",
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
    assert_eq!(report["action"], "repair");
    assert_eq!(report["mode"], "apply");
    assert_eq!(report["disposition"], "checkpointed");
    assert_eq!(report["source_generation"], "gen-001");
    assert_eq!(
        std::fs::read_to_string(store.join("CURRENT")).unwrap(),
        "gen-001\n"
    );
}

#[test]
fn cursor_advance_and_release_are_explicit_monotonic_mutations() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let coord_db = store.join("coord.db");

    let planned = julie_extract(&[
        "store",
        "maintain",
        "cursor",
        "advance",
        "--store",
        store.to_str().unwrap(),
        "--consumer",
        "miller-search",
        "--sequence",
        "0",
        "--json",
    ]);
    assert_eq!(planned.status.code(), Some(0));
    let coordinator = rusqlite::Connection::open(&coord_db).unwrap();
    assert_eq!(
        coordinator
            .query_row("SELECT COUNT(*) FROM consumer_cursors", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );

    let released_again = julie_extract(&[
        "store",
        "maintain",
        "cursor",
        "release",
        "--store",
        store.to_str().unwrap(),
        "--consumer",
        "miller-search",
        "--apply",
        "--json",
    ]);
    assert_eq!(released_again.status.code(), Some(0));
    let released_again_report: Value = serde_json::from_slice(&released_again.stdout).unwrap();
    assert_eq!(released_again_report["mode"], "apply");
    assert_eq!(released_again_report["disposition"], "no_change");

    let advanced = julie_extract(&[
        "store",
        "maintain",
        "cursor",
        "advance",
        "--store",
        store.to_str().unwrap(),
        "--consumer",
        "miller-search",
        "--sequence",
        "0",
        "--apply",
        "--json",
    ]);
    assert_eq!(
        advanced.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&advanced.stdout),
        String::from_utf8_lossy(&advanced.stderr)
    );
    let advanced_report: Value = serde_json::from_slice(&advanced.stdout).unwrap();
    assert_eq!(advanced_report["action"], "cursor_advance");
    assert_eq!(advanced_report["disposition"], "advanced");
    assert_eq!(advanced_report["consumer_id"], "miller-search");
    assert_eq!(advanced_report["consumer_sequence"], 0);
    assert!(!String::from_utf8_lossy(&advanced.stdout).contains(store.to_str().unwrap()));

    let released = julie_extract(&[
        "store",
        "maintain",
        "cursor",
        "release",
        "--store",
        store.to_str().unwrap(),
        "--consumer",
        "miller-search",
        "--apply",
        "--json",
    ]);
    assert_eq!(released.status.code(), Some(0));
    let released_report: Value = serde_json::from_slice(&released.stdout).unwrap();
    assert_eq!(released_report["action"], "cursor_release");
    assert_eq!(released_report["disposition"], "released");
    assert_eq!(
        coordinator
            .query_row("SELECT COUNT(*) FROM consumer_cursors", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn live_writer_reports_busy_as_json_stdout_without_mutation() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let coordinator = rusqlite::Connection::open(store.join("coord.db")).unwrap();
    coordinator
        .execute(
            "INSERT INTO writer_lease
             (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,fencing_token)
             VALUES ('store-writer','other','2.30.0',4242,1,?1,1)",
            [i64::MAX / 2],
        )
        .unwrap();

    let output = julie_extract(&[
        "store",
        "maintain",
        "gc",
        "--store",
        store.to_str().unwrap(),
        "--apply",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "busy");
    assert_eq!(report["error"]["class"], "busy");
    assert_eq!(report["error"]["code"], "maintenance_busy");
    assert_eq!(
        coordinator
            .query_row("SELECT COUNT(*) FROM maintenance_intent", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn incompatible_reader_floor_uses_exit_three_and_stable_failure_class() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    connection
        .execute(
            "UPDATE store_meta SET value='999.0.0' WHERE key='min_reader_version'",
            [],
        )
        .unwrap();

    let output = julie_extract(&[
        "store",
        "maintain",
        "inspect",
        "--store",
        store.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "incompatible_store");
    assert_eq!(report["error"]["class"], "incompatible_store");
    assert_eq!(report["error"]["code"], "store_reader_too_old");
}

#[test]
fn supplied_family_mismatch_is_an_incompatible_store_without_mutation() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let store_db = store.join("gen-001/store.db");
    let before = std::fs::read(&store_db).unwrap();

    let output = julie_extract(&[
        "store",
        "maintain",
        "inspect",
        "--store",
        store.to_str().unwrap(),
        "--family",
        "1d2a4ed0-c29d-4f69-b8f1-098453e764cd",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "incompatible_store");
    assert_eq!(report["error"]["code"], "family_mismatch");
    assert_eq!(std::fs::read(&store_db).unwrap(), before);
}

#[test]
fn missing_current_with_named_generation_reports_recovery_required() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    std::fs::remove_file(store.join("CURRENT")).unwrap();

    let output = julie_extract(&[
        "store",
        "maintain",
        "inspect",
        "--store",
        store.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["family_id"], FAMILY_ID);
    assert_eq!(report["source_generation"], "gen-001");
    assert_eq!(report["failure_class"], "recovery_required");
    assert_eq!(report["error"]["code"], "store_recovery_required");
    assert!(!store.join("CURRENT").exists());
    assert!(store.join("gen-001").is_dir());
}

#[test]
fn repair_without_a_selectable_generation_reports_repair_unavailable() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    std::fs::remove_file(store.join("CURRENT")).unwrap();

    let output = julie_extract(&[
        "store",
        "maintain",
        "repair",
        "--store",
        store.to_str().unwrap(),
        "--apply",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["action"], "repair");
    assert_eq!(report["mode"], "apply");
    assert_eq!(report["family_id"], FAMILY_ID);
    assert_eq!(report["source_generation"], "gen-001");
    assert_eq!(report["failure_class"], "repair_unavailable");
    assert_eq!(report["error"]["code"], "repair_unavailable");
    assert!(!store.join("CURRENT").exists());
}

#[test]
fn unknown_resolution_root_reports_integrity_failure_without_mutation() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let store_db = store.join("gen-001/store.db");
    let connection = rusqlite::Connection::open(&store_db).unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_bases
             (base_id,manifest_hash,resolver_output_epoch,state,relative_path,
              identifier_count,pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
             VALUES ('missing-base','sha256:manifest',1,'building','bases/missing.db',
                     0,0,NULL,NULL,'maintenance-test','2026-08-09T00:00:00Z','2026-08-09T00:00:00Z')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_base_versions(base_id,version_id) VALUES ('missing-base',999)",
            [],
        )
        .unwrap();
    drop(connection);
    let store_before = std::fs::read(&store_db).unwrap();

    let output = julie_extract(&[
        "store",
        "maintain",
        "inspect",
        "--store",
        store.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "integrity_failed");
    assert_eq!(report["error"]["code"], "unknown_maintenance_root");
    assert_eq!(std::fs::read(&store_db).unwrap(), store_before);
}

#[test]
fn human_failures_use_stderr_and_name_the_stable_class() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let coordinator = rusqlite::Connection::open(store.join("coord.db")).unwrap();
    coordinator
        .execute(
            "INSERT INTO writer_lease
             (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,fencing_token)
             VALUES ('store-writer','other','2.30.0',4242,1,?1,1)",
            [i64::MAX / 2],
        )
        .unwrap();

    let output = julie_extract(&[
        "store",
        "maintain",
        "gc",
        "--store",
        store.to_str().unwrap(),
        "--apply",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "failed action=gc mode=apply family={FAMILY_ID} source=gen-001 destination=none disposition=failed failure=busy code=maintenance_busy\n"
        )
    );
}

#[cfg(feature = "test-store-contract")]
#[test]
fn apply_refuses_a_plan_when_the_coordinator_root_changes() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let ready = fixture.path().join("plan-ready");
    let proceed = fixture.path().join("plan-proceed");
    let child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "maintain",
            "gc",
            "--store",
            store.to_str().unwrap(),
            "--apply",
            "--json",
        ])
        .env("JULIE_EXTRACT_STORE_TEST_MAINTENANCE_PLAN_READY", &ready)
        .env(
            "JULIE_EXTRACT_STORE_TEST_MAINTENANCE_PLAN_CONTINUE",
            &proceed,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    for _ in 0..500 {
        if ready.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists());
    let coordinator = rusqlite::Connection::open(store.join("coord.db")).unwrap();
    coordinator
        .execute(
            "INSERT INTO requests
             (request_id,idempotency_key,kind,payload_json,state,requester_id,created_at,updated_at)
             VALUES ('raced-request','raced-key','import','{}','queued','test',1,1)",
            [],
        )
        .unwrap();
    std::fs::write(&proceed, b"continue").unwrap();

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "stale_plan");
    assert_eq!(report["error"]["code"], "maintenance_plan_stale");
    assert!(
        report["plan_fingerprint"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert_eq!(
        coordinator
            .query_row("SELECT COUNT(*) FROM maintenance_intent", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn maintenance_report_json_and_human_snapshots_are_stable() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();

    let json_output = julie_extract(&[
        "store",
        "maintain",
        "inspect",
        "--store",
        store.to_str().unwrap(),
        "--json",
    ]);
    let mut report: Value = serde_json::from_slice(&json_output.stdout).unwrap();
    report["plan_fingerprint"] = json!("<fingerprint>");
    report["fingerprints"]["store_root"] = json!("<fingerprint>");
    report["fingerprints"]["coordinator_root"] = json!("<fingerprint>");
    for field in [
        "measured_bytes",
        "free_bytes",
        "store_page_bytes",
        "store_freelist_bytes",
        "store_wal_bytes",
        "base_bytes",
        "scratch_bytes",
        "staged_generation_bytes",
        "demotion_wal_headroom_bytes",
        "gc_required_bytes",
        "promotion_required_bytes",
    ] {
        report["capacity"][field] = json!(0);
    }
    assert_eq!(
        report,
        json!({
            "report_schema_version": 1,
            "action": "inspect",
            "mode": "plan",
            "run_id": null,
            "family_id": FAMILY_ID,
            "source_generation": "gen-001",
            "destination_generation": null,
            "selected_generation": null,
            "disposition": "planned",
            "plan_fingerprint": "<fingerprint>",
            "fingerprints": {
                "store_root": "<fingerprint>",
                "coordinator_root": "<fingerprint>"
            },
            "counts": {
                "versions": 0,
                "purge_eligible_versions": 0,
                "eligible_manifests": 0,
                "pressure_only_manifests": 0,
                "demotion_versions": 0,
                "protected_bases": 0,
                "eligible_bases": 0,
                "protected_deltas": 0,
                "eligible_deltas": 0,
                "protected_pins": 0,
                "expired_pins": 0,
                "protected_requests": 0,
                "protected_scratch": 0,
                "protected_cursors": 0,
                "protected_generations": 1,
                "protected_failed_paths": 0,
                "demoted_l3": 0,
                "demoted_l2": 0,
                "purged_versions": 0,
                "removed_manifests": 0,
                "removed_deltas": 0,
                "removed_bases": 0,
                "removed_base_files": 0,
                "removed_pins": 0,
                "removed_scratch_files": 0,
                "archived_requests": 0,
                "pruned_log_rows": 0,
                "copied_file_versions": 0,
                "copied_rows": 0,
                "copied_base_files": 0,
                "removed_generations": 0
            },
            "retention": {
                "protected_current_bytes": 0,
                "retained_logical_bytes": 0,
                "eligible_bytes": 0,
                "target_bytes": 0,
                "ceiling_bytes": 0,
                "pressure": false,
                "physical_current_bytes": 593920,
                "physical_bytes_before_gc": 593920,
                "physical_bytes_after_gc": 0,
                "physical_baseline_bytes": 593920,
                "physical_target_bytes": 712704,
                "physical_ceiling_bytes": 742400,
                "physical_target_breached": false,
                "physical_ceiling_breached": false,
                "physical_breach_limit": 3,
                "physical_breach_streak": 0,
                "compaction_required": false
            },
            "capacity": {
                "measured_bytes": 0,
                "free_bytes": 0,
                "store_page_bytes": 0,
                "store_freelist_bytes": 0,
                "store_wal_bytes": 0,
                "base_bytes": 0,
                "scratch_bytes": 0,
                "staged_generation_bytes": 0,
                "demotion_wal_headroom_bytes": 0,
                "gc_required_bytes": 0,
                "promotion_required_bytes": 0,
                "gc_fits": true,
                "promotion_fits": true
            },
            "integrity_checks": [
                "store_roots_validated",
                "coordinator_roots_validated"
            ],
            "escalation": null,
            "recovery_actions": [],
            "last_version_cursor": null,
            "consumer_id": null,
            "consumer_sequence": null,
            "failure_class": "none",
            "error": null
        })
    );

    let human = julie_extract(&[
        "store",
        "maintain",
        "inspect",
        "--store",
        store.to_str().unwrap(),
    ]);
    assert_eq!(human.status.code(), Some(0));
    assert!(human.stderr.is_empty());
    assert_eq!(
        String::from_utf8(human.stdout).unwrap(),
        format!(
            "ok action=inspect mode=plan family={FAMILY_ID} source=gen-001 destination=none disposition=planned failure=none\n"
        )
    );
}

#[cfg(unix)]
#[test]
fn promotion_capacity_refusal_happens_before_maintenance_mutation() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let sparse = store.join("gen-001/capacity-probe");
    let file = std::fs::File::create(&sparse).unwrap();
    file.set_len(1_000_000_000_000).unwrap();

    let output = julie_extract(&[
        "store",
        "maintain",
        "promote",
        "--store",
        store.to_str().unwrap(),
        "--apply",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "capacity_insufficient");
    assert_eq!(report["error"]["code"], "capacity_insufficient");
    assert_eq!(
        std::fs::read_to_string(store.join("CURRENT")).unwrap(),
        "gen-001\n"
    );
    let coordinator = rusqlite::Connection::open(store.join("coord.db")).unwrap();
    assert_eq!(
        coordinator
            .query_row(
                "SELECT (SELECT COUNT(*) FROM maintenance_intent) +
                        (SELECT COUNT(*) FROM writer_lease)",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}
