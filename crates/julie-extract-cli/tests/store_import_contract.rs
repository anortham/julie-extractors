use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use julie_extract_artifact::store::{
    CoordinatorRequest, LeaseDisposition, LeaseHolder, RequestKind, RequestState, StoreCoordinator,
    StoreLayout,
};

const FAMILY_ID: &str = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";

#[test]
fn one_drain_uses_each_queued_imports_own_root_plan_and_level() {
    let fixture = tempfile::tempdir().unwrap();
    let root_a = fixture.path().join("root-a");
    let root_b = fixture.path().join("root-b");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root_a).unwrap();
    std::fs::create_dir(&root_b).unwrap();
    std::fs::write(root_a.join("a.rs"), "pub fn a() {}\n").unwrap();
    let source_b = b"pub fn b(input: u32) -> u32 { input }\n";
    std::fs::write(root_b.join("b.rs"), source_b).unwrap();
    let root_b = root_b.canonicalize().unwrap();

    let layout = StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            "request-b",
            "idem-b",
            RequestKind::Import,
            serde_json::json!({
                "schema_version": 1,
                "family_id": FAMILY_ID,
                "root": root_b,
                "view_id": "view-b",
                "requested_level": "full",
                "files": [{
                    "root_relative_path": "b.rs",
                    "content_hash": format!("blake3:{}", blake3::hash(source_b).to_hex()),
                    "content_bytes": source_b.len(),
                }],
                "controls": { "jobs": 0 },
            })
            .to_string(),
            "requester-b",
            i64::MAX,
            1,
        ))
        .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root_a.to_str().unwrap(),
            "--view",
            "view-a",
            "--level",
            "l1",
            "--request-id",
            "request-a",
            "--idempotency-key",
            "idem-a",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let connection = rusqlite::Connection::open(layout.store_db()).unwrap();
    let view_b: (String, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT me.path, fv.complete_l2, fv.complete_l3
             FROM views v
             JOIN manifest_entries me ON me.view_id = v.view_id AND me.generation = v.current_generation
             JOIN file_versions fv ON fv.version_id = me.version_id
             WHERE v.view_id = 'view-b'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(view_b.0, "b.rs");
    assert!(view_b.1.is_some());
    assert!(view_b.2.is_some());
}

#[test]
fn idempotency_replay_observes_the_original_request_and_rejects_level_changes() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn value() -> u32 { 1 }\n").unwrap();
    let run = |request_id: &str, level: &str| {
        Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args([
                "store",
                "import",
                "--store",
                store.to_str().unwrap(),
                "--family",
                FAMILY_ID,
                "--root",
                root.to_str().unwrap(),
                "--view",
                "view-main",
                "--level",
                level,
                "--request-id",
                request_id,
                "--idempotency-key",
                "idem-shared",
                "--json",
            ])
            .output()
            .unwrap()
    };

    assert_eq!(run("request-original", "l1").status.code(), Some(0));
    let replay = run("request-retry", "l1");
    assert_eq!(replay.status.code(), Some(0));
    let replay: serde_json::Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["request"]["id"], "request-original");
    assert_eq!(replay["requested_level"], "l1");

    let conflict = run("request-conflict", "full");
    assert_ne!(conflict.status.code(), Some(0));
    let conflict: serde_json::Value = serde_json::from_slice(&conflict.stdout).unwrap();
    assert_eq!(conflict["failure_class"], "idempotency_conflict");
}

#[test]
fn idempotency_replay_with_a_different_family_is_a_conflict_without_mutation() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn value() {}\n").unwrap();
    let run = |request_id: &str, family: &str| {
        Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args([
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
                "--request-id",
                request_id,
                "--idempotency-key",
                "idem-family",
                "--json",
            ])
            .output()
            .unwrap()
    };

    assert_eq!(run("request-original", FAMILY_ID).status.code(), Some(0));
    let layout = StoreLayout::open(&store).unwrap();
    let store_before: (i64, i64) = rusqlite::Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM manifests), (SELECT COUNT(*) FROM store_log)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let coord_before: i64 = rusqlite::Connection::open(layout.coordinator_db())
        .unwrap()
        .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
        .unwrap();

    let conflict = run("request-conflict", "00000000-0000-0000-0000-000000000001");
    assert_eq!(conflict.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&conflict.stdout).unwrap();
    assert_eq!(report["failure_class"], "idempotency_conflict");
    let store_after: (i64, i64) = rusqlite::Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM manifests), (SELECT COUNT(*) FROM store_log)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let coord_after: i64 = rusqlite::Connection::open(layout.coordinator_db())
        .unwrap()
        .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
        .unwrap();
    assert_eq!(store_after, store_before);
    assert_eq!(coord_after, coord_before);
}

#[test]
fn idempotency_replay_observes_terminal_request_after_root_deletion_without_touching_progress() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let progress = fixture.path().join("scan.progress");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn value() -> u32 { 1 }\n").unwrap();
    let run = |request_id: &str| {
        Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args([
                "store",
                "import",
                "--store",
                store.to_str().unwrap(),
                "--family",
                FAMILY_ID,
                "--root",
                root.to_str().unwrap(),
                "--view",
                "view-main",
                "--level",
                "l1",
                "--progress-file",
                progress.to_str().unwrap(),
                "--request-id",
                request_id,
                "--idempotency-key",
                "idem-deleted-root",
                "--json",
            ])
            .output()
            .unwrap()
    };

    assert_eq!(run("request-original").status.code(), Some(0));
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::write(&progress, "progress sentinel\n").unwrap();

    let replay = run("request-retry");
    assert_eq!(
        replay.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let replay: serde_json::Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["request"]["id"], "request-original");
    assert_eq!(
        std::fs::read_to_string(&progress).unwrap(),
        "progress sentinel\n"
    );
    assert!(!root.exists());
}

#[test]
fn replay_report_uses_the_original_requests_manifest_and_row_counts_after_a_newer_flip() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn original() {}\n").unwrap();
    let run = |request_id: &str, idempotency_key: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args([
                "store",
                "import",
                "--store",
                store.to_str().unwrap(),
                "--family",
                FAMILY_ID,
                "--root",
                root.to_str().unwrap(),
                "--view",
                "view-main",
                "--level",
                "l1",
                "--request-id",
                request_id,
                "--idempotency-key",
                idempotency_key,
                "--json",
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };

    let original = run("request-a", "idem-a");
    std::fs::write(
        root.join("lib.rs"),
        "pub fn replacement() {}\npub fn additional() {}\n",
    )
    .unwrap();
    let newer = run("request-b", "idem-b");
    assert_ne!(
        newer["manifest"]["generation"],
        original["manifest"]["generation"]
    );

    let replay = run("request-a-retry", "idem-a");
    assert_eq!(replay["request"]["id"], "request-a");
    assert_eq!(replay["manifest"], original["manifest"]);
    assert_eq!(replay["row_counts"], original["row_counts"]);
    assert_eq!(replay["completion"], original["completion"]);
}

#[test]
fn replay_preserves_original_l1_counts_when_a_later_request_deepens_the_same_version() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("lib.rs"),
        "pub fn answer(value: u32) -> &'static str { let _ = value; \"answer\" }\n",
    )
    .unwrap();
    let run = |request_id: &str, idempotency_key: &str, level: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args([
                "store",
                "import",
                "--store",
                store.to_str().unwrap(),
                "--family",
                FAMILY_ID,
                "--root",
                root.to_str().unwrap(),
                "--view",
                "view-main",
                "--level",
                level,
                "--request-id",
                request_id,
                "--idempotency-key",
                idempotency_key,
                "--json",
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };

    let original = run("request-l1", "idem-l1", "l1");
    assert_eq!(original["completion"]["l2"], false);
    assert_eq!(original["completion"]["l3"], false);
    assert_eq!(original["row_counts"]["l2"], 0);
    assert_eq!(original["row_counts"]["l3"], 0);

    let deepened = run("request-full", "idem-full", "full");
    assert_eq!(deepened["completion"]["l2"], true);
    assert_eq!(deepened["completion"]["l3"], true);

    let replay = run("request-l1-retry", "idem-l1", "l1");
    assert_eq!(replay["request"]["id"], "request-l1");
    assert_eq!(replay["manifest"], original["manifest"]);
    assert_eq!(replay["completion"], original["completion"]);
    assert_eq!(replay["row_counts"], original["row_counts"]);
}

#[test]
fn preflight_failure_never_enqueues_a_request() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    let missing = fixture.path().join("missing-root");
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            missing.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "request-preflight",
            "--idempotency-key",
            "idem-preflight",
            "--json",
        ])
        .output()
        .unwrap();
    assert_ne!(output.status.code(), Some(0));
    assert!(!store.join("gen-001/coordinator.db").exists());
}

#[test]
fn crafted_payload_cannot_redirect_progress_to_the_live_store_or_escape_root() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("safe.rs"), "pub fn safe() {}\n").unwrap();
    let outside = fixture.path().join("outside.rs");
    std::fs::write(&outside, "outside sentinel\n").unwrap();
    let root = root.canonicalize().unwrap();
    let layout = StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let redirected_progress = root.join("redirect.progress");
    std::fs::hard_link(layout.store_db(), &redirected_progress).unwrap();
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            "request-crafted",
            "idem-crafted",
            RequestKind::Import,
            serde_json::json!({
                "schema_version": 1,
                "family_id": FAMILY_ID,
                "root": root,
                "view_id": "view-crafted",
                "requested_level": "l1",
                "files": [{
                    "root_relative_path": "../outside.rs",
                    "content_hash": format!("blake3:{}", "0".repeat(64)),
                    "content_bytes": 1,
                }],
                "controls": {
                    "jobs": 1,
                    "progress_file": redirected_progress,
                },
            })
            .to_string(),
            "crafted-requester",
            i64::MAX,
            1,
        ))
        .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-safe",
            "--level",
            "l1",
            "--request-id",
            "request-safe",
            "--idempotency-key",
            "idem-safe",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let crafted = StoreCoordinator::open(&layout)
        .unwrap()
        .request("request-crafted")
        .unwrap();
    assert_eq!(crafted.state, RequestState::Failed);
    let crafted_error: serde_json::Value =
        serde_json::from_str(crafted.error_json.as_deref().unwrap()).unwrap();
    assert_eq!(
        crafted_error["message"],
        "invalid_import_request_payload:invalid_file_path"
    );
    let integrity: String = rusqlite::Connection::open(layout.store_db())
        .unwrap()
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let catalog_rows: i64 = rusqlite::Connection::open(layout.store_db())
        .unwrap()
        .query_row("SELECT COUNT(*) FROM store_meta", [], |row| row.get(0))
        .unwrap();
    assert!(catalog_rows > 0);
    assert_eq!(
        std::fs::read_to_string(outside).unwrap(),
        "outside sentinel\n"
    );
}

#[test]
fn durable_payload_uses_absolute_scan_paths_and_excludes_process_runtime_authority() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn safe() {}\n").unwrap();
    std::fs::write(fixture.path().join("custom.ignore"), "ignored.rs\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .current_dir(fixture.path())
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--ignore-file",
            "custom.ignore",
            "--spool-dir",
            "spool",
            "--progress-file",
            "progress.jsonl",
            "--parent-pid",
            &std::process::id().to_string(),
            "--request-id",
            "request-controls",
            "--idempotency-key",
            "idem-controls",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));

    let layout = StoreLayout::open(&store).unwrap();
    let request = StoreCoordinator::open(&layout)
        .unwrap()
        .request("request-controls")
        .unwrap();
    let payload: serde_json::Value = serde_json::from_str(&request.payload_json).unwrap();
    assert!(payload.get("store_db").is_none());
    assert!(payload.get("parent_pid").is_none());
    let controls = payload["controls"].as_object().unwrap();
    assert!(controls.get("store_db").is_none());
    assert!(controls.get("parent_pid").is_none());
    for key in ["spool_dir", "progress_file"] {
        assert!(std::path::Path::new(controls[key].as_str().unwrap()).is_absolute());
    }
    assert!(
        controls["ignore_files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|path| std::path::Path::new(path.as_str().unwrap()).is_absolute())
    );
}

#[test]
fn committed_report_contains_truthful_level_row_counts() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("lib.rs"),
        "/// documented\npub fn value(input: u32) -> u32 { let answer = input + 42; answer }\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-counts",
            "--idempotency-key",
            "idem-counts",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["row_counts"]["file_versions"], 1);
    assert!(report["row_counts"]["l1"].as_u64().unwrap() > 0);
    assert!(report["row_counts"]["l2"].as_u64().unwrap() > 0);
    assert!(report["row_counts"]["l3"].as_u64().unwrap() > 0);
}

#[test]
fn empty_full_import_reports_all_requested_levels_complete() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-empty",
            "--level",
            "full",
            "--request-id",
            "request-empty",
            "--idempotency-key",
            "idem-empty",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["completion"]["l1"], true);
    assert_eq!(report["completion"]["l2"], true);
    assert_eq!(report["completion"]["l3"], true);
    assert_eq!(report["row_counts"]["file_versions"], 0);
}

#[test]
fn nonholder_times_out_without_removing_the_durable_queued_request() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn value() {}\n").unwrap();
    let layout = StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let mut blocker = StoreCoordinator::open(&layout).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert!(matches!(
        blocker
            .try_acquire_or_takeover(
                LeaseHolder::new("live-holder", env!("CARGO_PKG_VERSION"), std::process::id()),
                now,
            )
            .unwrap(),
        LeaseDisposition::Acquired { .. }
    ));

    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "request-timeout",
            "--idempotency-key",
            "idem-timeout",
            "--request-timeout-seconds",
            "1",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "request_timeout");
    assert!(matches!(
        report["state"].as_str(),
        Some("queued" | "claimed")
    ));
    let durable = StoreCoordinator::open(&layout)
        .unwrap()
        .request("request-timeout")
        .unwrap();
    assert!(matches!(
        durable.state,
        RequestState::Queued | RequestState::Claimed
    ));
}

#[test]
fn import_drains_as_soon_as_a_blocking_lease_is_released() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn value() {}\n").unwrap();
    let layout = StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let mut blocker = StoreCoordinator::open(&layout).unwrap();
    let blocker_holder =
        LeaseHolder::new("live-holder", env!("CARGO_PKG_VERSION"), std::process::id());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let LeaseDisposition::Acquired { fencing_token } = blocker
        .try_acquire_or_takeover(blocker_holder.clone(), now)
        .unwrap()
    else {
        panic!("blocking lease was not acquired");
    };

    let started = Instant::now();
    let child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "request-blocked",
            "--idempotency-key",
            "idem-blocked",
            "--request-timeout-seconds",
            "30",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    std::thread::sleep(Duration::from_millis(500));
    assert!(
        blocker
            .release_lease(&blocker_holder, fencing_token)
            .unwrap()
    );

    let output = child.wait_with_output().unwrap();
    let elapsed = started.elapsed();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["state"], "committed");
    // The import must re-attempt the drain, not wait out its whole request
    // budget while the lease sits free.
    assert!(
        elapsed < Duration::from_secs(15),
        "the import took {elapsed:?} to finish a lease that was released after 500ms"
    );
}

#[test]
fn successor_process_completes_a_queued_request_after_the_submitters_parent_exits() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn value() {}\n").unwrap();
    let layout = StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let mut blocker = StoreCoordinator::open(&layout).unwrap();
    let blocker_holder =
        LeaseHolder::new("live-holder", env!("CARGO_PKG_VERSION"), std::process::id());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let LeaseDisposition::Acquired { fencing_token } = blocker
        .try_acquire_or_takeover(blocker_holder.clone(), now)
        .unwrap()
    else {
        panic!("blocking lease was not acquired");
    };

    let submitter = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "\"{}\" store import --store \"{}\" --family {} --root \"{}\" --view view-main --request-id request-orphaned --idempotency-key idem-orphaned --request-timeout-seconds 1 --parent-pid $$ --json",
            env!("CARGO_BIN_EXE_julie-extract"),
            store.display(),
            FAMILY_ID,
            root.display(),
        ))
        .output()
        .unwrap();
    assert_eq!(submitter.status.code(), Some(1));
    assert!(
        blocker
            .release_lease(&blocker_holder, fencing_token)
            .unwrap()
    );

    let successor = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--request-id",
            "request-successor",
            "--idempotency-key",
            "idem-orphaned",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        successor.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&successor.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&successor.stdout).unwrap();
    assert_eq!(report["request"]["id"], "request-orphaned");
    assert_eq!(report["state"], "committed");
}

#[test]
fn multi_quantum_import_keeps_one_append_only_progress_stream_per_process_and_request() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let progress = fixture.path().join("scan.progress");
    std::fs::create_dir(&root).unwrap();
    for index in 0..3 {
        std::fs::write(
            root.join(format!("file-{index}.rs")),
            format!("pub fn value_{index}() -> u32 {{ {index} }}\n"),
        )
        .unwrap();
    }

    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .env("MILLER_STORE_CHUNK_VERSIONS", "0")
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--progress-file",
            progress.to_str().unwrap(),
            "--request-id",
            "request-progress",
            "--idempotency-key",
            "idem-progress",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let records = std::fs::read_to_string(progress)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(records.len() >= 3);
    assert_eq!(records.first().unwrap()["files_extracted"], 0);
    assert_eq!(records.last().unwrap()["files_extracted"], 6);
    assert_eq!(records.last().unwrap()["phase"], "complete");
    assert!(records.windows(2).all(|pair| {
        pair[0]["files_extracted"].as_u64().unwrap() <= pair[1]["files_extracted"].as_u64().unwrap()
    }));
}

#[test]
fn public_store_import_reaches_the_production_executor() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "request-public-parse",
            "--idempotency-key",
            "idem-public-parse",
            "--json",
        ])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let (path, complete_l1, complete_l2, complete_l3): (
        String,
        Option<i64>,
        Option<i64>,
        Option<i64>,
    ) = connection
        .query_row(
            "SELECT path, complete_l1, complete_l2, complete_l3 FROM file_versions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(path, "lib.rs");
    assert!(complete_l1.is_some());
    assert_eq!(complete_l2, None);
    assert_eq!(complete_l3, None);
    let manifest_version: i64 = connection
        .query_row(
            "SELECT version_id FROM manifest_entries WHERE view_id = 'view-main' AND path = 'lib.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(manifest_version > 0);
    let events = connection
        .prepare("SELECT event_kind FROM store_log ORDER BY sequence")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        events,
        [
            "version_level_completed",
            "manifest_flipped",
            "store_import_completed",
        ]
    );
}

#[test]
fn full_import_publishes_l1_before_committing_l2_and_l3() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("lib.rs"),
        "pub fn answer(input: u32) -> u32 { input + 42 }\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "--request-id",
            "request-full",
            "--idempotency-key",
            "idem-full",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let stamps: (Option<i64>, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT complete_l1, complete_l2, complete_l3 FROM file_versions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert!(stamps.0 < stamps.1 && stamps.1 < stamps.2);
    let events = connection
        .prepare("SELECT event_kind, level FROM store_log ORDER BY sequence")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let l1 = events
        .iter()
        .position(|event| event == &("version_level_completed".to_string(), Some(1)))
        .unwrap();
    let manifest = events
        .iter()
        .position(|event| event.0 == "manifest_flipped")
        .unwrap();
    let l2 = events
        .iter()
        .position(|event| event == &("version_level_completed".to_string(), Some(2)))
        .unwrap();
    let l3 = events
        .iter()
        .position(|event| event == &("version_level_completed".to_string(), Some(3)))
        .unwrap();
    let terminal = events
        .iter()
        .position(|event| event.0 == "store_import_completed")
        .unwrap();
    assert!(l1 < manifest && manifest < l2 && l2 < l3 && l3 < terminal);
}

#[test]
fn unchanged_completed_full_import_reuses_without_extraction_or_new_level_effects() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let progress = fixture.path().join("retry.progress");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();

    for (request_id, idempotency_key, progress_file) in [
        ("request-reuse-first", "idem-reuse-first", None),
        (
            "request-reuse-second",
            "idem-reuse-second",
            Some(progress.as_path()),
        ),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
        command.args([
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
            "--request-id",
            request_id,
            "--idempotency-key",
            idempotency_key,
            "--json",
        ]);
        if let Some(progress_file) = progress_file {
            command.args(["--progress-file", progress_file.to_str().unwrap()]);
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let repeated = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "--request-id",
            "request-reuse-second",
            "--idempotency-key",
            "idem-reuse-second",
            "--progress-file",
            progress.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(repeated.status.code(), Some(0));

    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let version_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_versions", [], |row| row.get(0))
        .unwrap();
    let level_effect_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM store_log WHERE event_kind = 'version_level_completed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version_count, 1);
    assert_eq!(level_effect_count, 3);
    let repeated_terminal: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM store_log
             WHERE request_id = 'request-reuse-second' AND event_kind = 'store_import_completed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(repeated_terminal, 1);

    let progress = std::fs::read_to_string(progress).unwrap();
    let final_record: serde_json::Value =
        serde_json::from_str(progress.lines().last().unwrap()).unwrap();
    assert_eq!(final_record["files_extracted"], 0);
}

#[test]
fn full_deepening_refuses_a_persisted_l1_natural_key_mismatch() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();

    let first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "request-l1-seed",
            "--idempotency-key",
            "idem-l1-seed",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(0));

    let database = store.join("gen-001/store.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    let changed = connection
        .execute(
            "UPDATE complexity_metrics SET decision_count = decision_count + 1",
            [],
        )
        .unwrap();
    assert!(changed > 0);
    drop(connection);

    let full = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "--request-id",
            "request-full-mismatch",
            "--idempotency-key",
            "idem-full-mismatch",
            "--json",
        ])
        .output()
        .unwrap();
    assert_ne!(full.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&full.stdout).contains("l1_projection_mismatch"));

    let connection = rusqlite::Connection::open(database).unwrap();
    let stamps: (Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT complete_l2, complete_l3 FROM file_versions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stamps, (None, None));
    let deeper_rows: i64 = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM identifiers) +
                (SELECT COUNT(*) FROM reference_sites WHERE level = 2) +
                (SELECT COUNT(*) FROM type_argument_usages) +
                (SELECT COUNT(*) FROM type_arguments) +
                (SELECT COUNT(*) FROM literals) +
                (SELECT COUNT(*) FROM source_regions) +
                (SELECT COUNT(*) FROM structural_facts)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(deeper_rows, 0);
}

#[test]
fn retry_after_l1_manifest_progress_resumes_deepening_without_republishing() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    for index in 0..101 {
        std::fs::write(
            root.join(format!("file_{index:03}.rs")),
            format!("pub fn answer_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }
    std::fs::write(root.join("broken.rs"), [0xff, 0xfe, 0x00]).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "--request-id",
            "request-crash-resume",
            "--idempotency-key",
            "idem-crash-resume",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let database = store.join("gen-001/store.db");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(
            Instant::now() < deadline,
            "manifest progress was not observed"
        );
        if database.exists()
            && rusqlite::Connection::open(&database)
                .and_then(|connection| {
                    connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM store_log WHERE event_kind = 'manifest_flipped')",
                        [],
                        |row| row.get::<_, bool>(0),
                    )
                })
                .unwrap_or(false)
        {
            let wal = database.with_extension("db-wal");
            if wal.exists() {
                assert!(std::fs::metadata(wal).unwrap().len() <= 128 * 1024 * 1024);
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    std::fs::write(
        root.join("aaa_inserted_after_crash.rs"),
        "pub fn inserted() {}\n",
    )
    .unwrap();

    let retry = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "--request-id",
            "request-crash-resume",
            "--idempotency-key",
            "idem-crash-resume",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        retry.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );

    let connection = rusqlite::Connection::open(database).unwrap();
    let completed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM file_versions
             WHERE complete_l1 IS NOT NULL AND complete_l2 IS NOT NULL AND complete_l3 IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let l1_effects: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM store_log
             WHERE event_kind = 'version_level_completed' AND level = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let manifest_effects: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM store_log WHERE event_kind = 'manifest_flipped'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let terminal_effects: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM store_log WHERE event_kind = 'store_import_completed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let chunk_span: (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*), COALESCE(MAX(chunk_index), -1) + 1
             FROM request_chunks WHERE request_id = 'request-crash-resume'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let l1_chunks: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM request_chunks
             WHERE request_id = 'request-crash-resume' AND level = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(completed, 101);
    assert_eq!(l1_effects, 101);
    assert_eq!(manifest_effects, 1);
    assert_eq!(terminal_effects, 1);
    assert_eq!(chunk_span.0, chunk_span.1);
    assert_eq!(l1_chunks, 2);
    let failed_after_restart: (String, Option<i64>) = connection
        .query_row(
            "SELECT status, version_id FROM manifest_entries WHERE path = 'broken.rs'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(failed_after_restart, ("failed".to_string(), None));
    let inserted_manifest_rows: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM manifest_entries WHERE path = 'aaa_inserted_after_crash.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(inserted_manifest_rows, 0);
}

#[test]
fn zero_chunk_override_runs_one_version_per_quantum_with_global_indices() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    for index in 0..3 {
        std::fs::write(
            root.join(format!("file_{index}.rs")),
            format!("pub fn answer_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .env("MILLER_STORE_CHUNK_VERSIONS", "0")
        .args([
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
            "--request-id",
            "request-one-version",
            "--idempotency-key",
            "idem-one-version",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));

    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let chunks = connection
        .prepare(
            "SELECT chunk_index, level FROM request_chunks
             WHERE request_id = 'request-one-version' ORDER BY chunk_index",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(chunks, [(0, 1), (1, 1), (2, 1), (3, 3), (4, 3)]);
}

#[test]
fn default_chunk_limit_processes_101_l1_versions_in_two_quanta() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    for index in 0..101 {
        std::fs::write(
            root.join(format!("file_{index:03}.rs")),
            format!("pub fn answer_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "request-101",
            "--idempotency-key",
            "idem-101",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let progress_quanta: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM request_chunks WHERE request_id = 'request-101' AND level = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let terminal: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM store_log
             WHERE request_id = 'request-101' AND event_kind = 'store_import_completed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let versions: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM file_versions WHERE complete_l1 IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(progress_quanta, 1);
    assert_eq!(terminal, 1);
    assert_eq!(versions, 101);
}

#[test]
fn default_full_import_freezes_deep_chunks_at_eight_versions() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    for index in 0..17 {
        std::fs::write(
            root.join(format!("file_{index:03}.rs")),
            format!("pub fn answer_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-default-deep-chunks",
            "--idempotency-key",
            "idem-default-deep-chunks",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let chunks = connection
        .prepare(
            "SELECT level, COUNT(*) FROM request_chunks
             WHERE request_id = 'request-default-deep-chunks' GROUP BY level ORDER BY level",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(chunks, [(1, 1), (3, 2)]);
    let deep_progress: Vec<i64> = connection
        .prepare(
            "SELECT json_extract(payload_json, '$.completed_files')
             FROM store_log
             WHERE request_id = 'request-default-deep-chunks'
               AND event_kind = 'store_import_l3_chunk'
             ORDER BY sequence",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(deep_progress, [8, 16]);
}

#[test]
fn queued_request_keeps_frozen_chunk_schedule_when_environment_changes() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    std::fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let store = fixture.path().join("store");
    let mut files = Vec::new();
    for index in 0..3 {
        let path = root.join(format!("file_{index}.rs"));
        let contents = format!("pub fn answer_{index}() -> usize {{ {index} }}\n");
        std::fs::write(&path, &contents).unwrap();
        files.push(serde_json::json!({
            "root_relative_path": format!("file_{index}.rs"),
            "content_hash": format!("blake3:{}", blake3::hash(contents.as_bytes()).to_hex()),
            "content_bytes": contents.len(),
        }));
    }
    let layout = StoreLayout::create(&store, FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    StoreCoordinator::open(&layout)
        .unwrap()
        .enqueue(CoordinatorRequest::new(
            "request-frozen-queue",
            "idem-frozen-queue",
            RequestKind::Import,
            serde_json::json!({
                "schema_version": 1,
                "family_id": FAMILY_ID,
                "root": root,
                "view_id": "view-main",
                "requested_level": "l1",
                "files": files,
                "controls": {
                    "jobs": 0,
                    "l1_chunk_versions": 2,
                    "deep_chunk_versions": 2,
                },
            })
            .to_string(),
            "queued-requester",
            i64::MAX,
            1,
        ))
        .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .env("MILLER_STORE_CHUNK_VERSIONS", "1")
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-frozen-observer",
            "--idempotency-key",
            "idem-frozen-queue",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let progress: Vec<i64> = connection
        .prepare(
            "SELECT json_extract(payload_json, '$.completed_files')
             FROM store_log
             WHERE request_id = 'request-frozen-queue'
               AND event_kind = 'store_import_l1_chunk'
             ORDER BY sequence",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(progress, [2]);
}

#[test]
fn crash_resume_keeps_frozen_chunk_schedule_when_environment_changes() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    for index in 0..5 {
        std::fs::write(
            root.join(format!("file_{index}.rs")),
            format!("pub fn answer_{index}() -> usize {{ {index} }}\n"),
        )
        .unwrap();
    }
    let root = root.canonicalize().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .env("MILLER_STORE_CHUNK_VERSIONS", "2")
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-crash-frozen",
            "--idempotency-key",
            "idem-crash-frozen",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let database = store.join("gen-001/store.db");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(
            Instant::now() < deadline,
            "manifest progress was not observed"
        );
        if database.exists()
            && rusqlite::Connection::open(&database)
                .and_then(|connection| {
                    connection.query_row(
                        "SELECT EXISTS(SELECT 1 FROM store_log
                         WHERE request_id = 'request-crash-frozen'
                           AND event_kind = 'manifest_flipped')",
                        [],
                        |row| row.get::<_, bool>(0),
                    )
                })
                .unwrap_or(false)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let _ = child.kill();
    let _ = child.wait();

    let retry = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .env("MILLER_STORE_CHUNK_VERSIONS", "1")
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-crash-frozen-observer",
            "--idempotency-key",
            "idem-crash-frozen",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        retry.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&retry.stdout),
        String::from_utf8_lossy(&retry.stderr)
    );
    let connection = rusqlite::Connection::open(database).unwrap();
    let progress: Vec<i64> = connection
        .prepare(
            "SELECT json_extract(payload_json, '$.completed_files')
             FROM store_log
             WHERE request_id = 'request-crash-frozen'
               AND event_kind = 'store_import_l1_chunk'
             ORDER BY sequence",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(progress, [2, 4]);
    let deep_progress: Vec<i64> = connection
        .prepare(
            "SELECT json_extract(payload_json, '$.completed_files')
             FROM store_log
             WHERE request_id = 'request-crash-frozen'
               AND event_kind = 'store_import_l3_chunk'
             ORDER BY sequence",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(deep_progress, [2, 4]);
}

#[test]
fn first_failed_path_has_no_version_and_prior_good_failure_is_preserved() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("new.rs"), [0xff, 0xfe, 0x00]).unwrap();

    let first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "request-new-failed",
            "--idempotency-key",
            "idem-new-failed",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(0));
    let database = store.join("gen-001/store.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    let first_entry: (String, Option<i64>) = connection
        .query_row(
            "SELECT status, version_id FROM manifest_entries WHERE path = 'new.rs'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(first_entry, ("failed".to_string(), None));
    let version_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_versions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version_count, 0);
    drop(connection);

    std::fs::write(root.join("new.rs"), "pub fn good() -> u32 { 1 }\n").unwrap();
    let good = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "request-good",
            "--idempotency-key",
            "idem-good",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(good.status.code(), Some(0));
    let connection = rusqlite::Connection::open(&database).unwrap();
    let prior: (i64, i64) = connection
        .query_row(
            "SELECT version_id, complete_l1 FROM file_versions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    drop(connection);

    std::fs::write(root.join("new.rs"), [0xff, 0xfe, 0x00]).unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "request-preserved",
            "--idempotency-key",
            "idem-preserved",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(0));
    let connection = rusqlite::Connection::open(database).unwrap();
    let preserved: (String, Option<i64>) = connection
        .query_row(
            "SELECT status, version_id FROM manifest_entries
             WHERE view_id = 'view-main' ORDER BY generation DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let retained_stamp: i64 = connection
        .query_row(
            "SELECT complete_l1 FROM file_versions WHERE version_id = ?1",
            [prior.0],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(preserved, ("failed_preserved".to_string(), Some(prior.0)));
    assert_eq!(retained_stamp, prior.1);
}

#[test]
#[cfg(feature = "test-store-contract")]
fn source_change_between_waves_keeps_published_l1_and_requires_a_new_request() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let ready = fixture.path().join("l1.ready");
    let resume = fixture.path().join("l1.resume");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 1 }\n").unwrap();

    let child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .env("JULIE_EXTRACT_STORE_TEST_L1_READY_FILE", &ready)
        .env("JULIE_EXTRACT_STORE_TEST_L1_RESUME_FILE", &resume)
        .args([
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
            "--request-id",
            "request-changing",
            "--idempotency-key",
            "idem-changing",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "L1 hook was not reached");
        std::thread::sleep(Duration::from_millis(2));
    }
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 2 }\n").unwrap();
    std::fs::write(&resume, b"resume").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "changed_between_waves");
    assert_eq!(report["completion"]["l1"], true);
    assert_eq!(report["completion"]["l2"], false);
    assert_eq!(report["completion"]["l3"], false);
    assert!(report["manifest"]["generation"].as_u64().is_some());
    assert!(report["manifest"]["hash"].as_str().is_some());
    assert_ne!(report["manifest"]["disposition"], "not_published");
    assert_eq!(report["row_counts"]["file_versions"], 1);
    assert!(report["row_counts"]["l1"].as_u64().unwrap() > 0);

    let database = store.join("gen-001/store.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    let published: (i64, String, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT fv.version_id, fv.content_hash, fv.complete_l2, fv.complete_l3
             FROM views v JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             JOIN file_versions fv ON fv.version_id = me.version_id
             WHERE v.view_id = 'view-main' AND me.path = 'lib.rs'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!((published.2, published.3), (None, None));
    drop(connection);

    let retry = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "--request-id",
            "request-new-hash",
            "--idempotency-key",
            "idem-new-hash",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(retry.status.code(), Some(0));
    let connection = rusqlite::Connection::open(database).unwrap();
    let current_version: i64 = connection
        .query_row(
            "SELECT me.version_id FROM views v JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             WHERE v.view_id = 'view-main' AND me.path = 'lib.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_ne!(current_version, published.0);
}

#[test]
fn extraction_epoch_change_creates_a_new_version_for_unchanged_content() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
    let run = |request: &str, idempotency: &str| {
        Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args([
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
                request,
                "--idempotency-key",
                idempotency,
                "--json",
            ])
            .output()
            .unwrap()
    };
    assert_eq!(
        run("request-epoch-a", "idem-epoch-a").status.code(),
        Some(0)
    );
    let database = store.join("gen-001/store.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .execute(
            "UPDATE file_versions SET extraction_epoch = extraction_epoch + 1",
            [],
        )
        .unwrap();
    drop(connection);
    assert_eq!(
        run("request-epoch-b", "idem-epoch-b").status.code(),
        Some(0)
    );
    let connection = rusqlite::Connection::open(database).unwrap();
    let versions: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_versions", [], |row| row.get(0))
        .unwrap();
    let epochs: i64 = connection
        .query_row(
            "SELECT COUNT(DISTINCT extraction_epoch) FROM file_versions",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(versions, 2);
    assert_eq!(epochs, 2);
}

#[test]
fn existing_view_refuses_a_different_root_without_republishing() {
    let fixture = tempfile::tempdir().unwrap();
    let root_a = fixture.path().join("root-a");
    let root_b = fixture.path().join("root-b");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root_a).unwrap();
    std::fs::create_dir(&root_b).unwrap();
    std::fs::write(root_a.join("lib.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(root_b.join("lib.rs"), "pub fn b() {}\n").unwrap();
    let first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root_a.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-root-a",
            "--idempotency-key",
            "idem-root-a",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(first.status.code(), Some(0));
    let second = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root_b.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-root-b",
            "--idempotency-key",
            "idem-root-b",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(second.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(report["failure_class"], "view_root_mismatch");
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let manifests: i64 = connection
        .query_row("SELECT COUNT(*) FROM manifests", [], |row| row.get(0))
        .unwrap();
    assert_eq!(manifests, 1);
}

#[test]
fn import_honors_ignore_spool_progress_jobs_and_parent_supervision_controls() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let spool = root.join("spool");
    let progress = root.join("scan.progress");
    let ignore = fixture.path().join("extra.ignore");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&spool).unwrap();
    std::fs::write(root.join("kept.rs"), "pub fn kept() {}\n").unwrap();
    std::fs::write(root.join("ignored.rs"), "pub fn ignored() {}\n").unwrap();
    std::fs::write(&ignore, "ignored.rs\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "request-controls",
            "--idempotency-key",
            "idem-controls",
            "--ignore-file",
            ignore.to_str().unwrap(),
            "--jobs",
            "1",
            "--spool-dir",
            spool.to_str().unwrap(),
            "--progress-file",
            progress.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let paths = connection
        .prepare("SELECT path FROM manifest_entries ORDER BY path")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(paths, ["kept.rs"]);
    let progress = std::fs::read_to_string(progress).unwrap();
    let final_progress: serde_json::Value =
        serde_json::from_str(progress.lines().last().unwrap()).unwrap();
    assert_eq!(final_progress["phase"], "complete");
    assert_eq!(final_progress["files_extracted"], 1);
    assert!(std::fs::read_dir(spool).unwrap().next().is_none());

    let supervised_store = fixture.path().join("supervised-store");
    let supervised = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            supervised_store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-supervised",
            "--idempotency-key",
            "idem-supervised",
            "--parent-pid",
            &u32::MAX.to_string(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(supervised.status.code(), Some(1));
    let connection = rusqlite::Connection::open(supervised_store.join("gen-001/store.db")).unwrap();
    let versions: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_versions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(versions, 0);
}

#[test]
fn full_import_persists_two_distinct_language_parsers() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("lib.rs"),
        "pub fn answer(input: u32) -> &'static str { let _ = input; \"rust\" }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("module.py"),
        "def answer(value):\n    return \"python\" if value else \"none\"\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "--request-id",
            "request-languages",
            "--idempotency-key",
            "idem-languages",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let rows = connection
        .prepare(
            "SELECT fv.language,
                    (SELECT COUNT(*) FROM symbols s WHERE s.version_id = fv.version_id),
                    (SELECT COUNT(*) FROM identifiers i WHERE i.version_id = fv.version_id),
                    (SELECT COUNT(*) FROM reference_sites r WHERE r.version_id = fv.version_id),
                    (SELECT COUNT(*) FROM source_regions sr WHERE sr.version_id = fv.version_id)
             FROM file_versions fv ORDER BY fv.language",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.1 > 0 && row.2 > 0 && row.3 > 0 && row.4 > 0),
        "rows: {rows:?}"
    );
}

#[test]
#[cfg(feature = "test-store-contract")]
fn resumed_full_import_reports_its_l1_generation_after_an_intervening_flip() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let ready = fixture.path().join("full-import-resume.ready");
    let resume = fixture.path().join("full-import-resume.resume");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.rs"), "pub fn a() -> u32 { 1 }\n").unwrap();
    std::fs::write(root.join("b.rs"), "pub fn b() -> u32 { 1 }\n").unwrap();
    let seed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "l1",
            "--request-id",
            "request-full-import-resume-seed",
            "--idempotency-key",
            "idem-full-import-resume-seed",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(seed.status.code(), Some(0));
    std::fs::write(root.join("a.rs"), "pub fn a() -> u32 { 2 }\n").unwrap();
    let mut first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-full-import-resume-a",
            "--idempotency-key",
            "idem-full-import-resume-a",
            "--json",
        ])
        .env("JULIE_EXTRACT_STORE_TEST_FULL_RESUME_READY_FILE", &ready)
        .env("JULIE_EXTRACT_STORE_TEST_FULL_RESUME_FILE", &resume)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(
            first.try_wait().unwrap().is_none(),
            "full import exited before pausing after durable L1 progress"
        );
        assert!(
            Instant::now() < deadline,
            "full import did not pause after durable L1 progress"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    first.kill().unwrap();
    let killed = first.wait_with_output().unwrap();
    assert!(!killed.status.success());
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let generation_two_hash: String = connection
        .query_row(
            "SELECT manifest_hash FROM manifests
             WHERE view_id = 'view-main' AND generation = 2",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);
    let deleted = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "delete",
            "--store",
            store.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--file",
            "b.rs",
            "--request-id",
            "request-full-import-resume-b",
            "--idempotency-key",
            "idem-full-import-resume-b",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        deleted.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&deleted.stdout),
        String::from_utf8_lossy(&deleted.stderr)
    );
    let connection = rusqlite::Connection::open(store.join("gen-001/store.db")).unwrap();
    let current_generation: i64 = connection
        .query_row(
            "SELECT current_generation FROM views WHERE view_id = 'view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(current_generation, 3);
    drop(connection);
    let resumed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--level",
            "full",
            "--request-id",
            "request-full-import-resume-observer",
            "--idempotency-key",
            "idem-full-import-resume-a",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        resumed.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&resumed.stdout),
        String::from_utf8_lossy(&resumed.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&resumed.stdout).unwrap();
    assert_eq!(report["request"]["id"], "request-full-import-resume-a");
    assert_eq!(report["manifest"]["generation"], 2);
    assert_eq!(report["manifest"]["hash"], generation_two_hash);
    let coordinator = rusqlite::Connection::open(store.join("coord.db")).unwrap();
    let result_json: String = coordinator
        .query_row(
            "SELECT result_json FROM requests
             WHERE request_id = 'request-full-import-resume-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let result: serde_json::Value = serde_json::from_str(&result_json).unwrap();
    assert_eq!(result["manifest_generation"], 2);
    assert_eq!(result["manifest_hash"], report["manifest"]["hash"]);
}
