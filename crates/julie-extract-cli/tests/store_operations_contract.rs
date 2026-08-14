#[cfg(feature = "test-store-contract")]
use std::process::Stdio;
use std::process::{Command, Output};
#[cfg(feature = "test-store-contract")]
use std::time::{Duration, Instant};

#[cfg(unix)]
use julie_extract_artifact::store::{
    CoordinatorRequest, RequestKind, RequestState, StoreCoordinator, StoreLayout,
};
use rusqlite::Connection;
use serde_json::Value;

const FAMILY_ID: &str = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
const OTHER_FAMILY_ID: &str = "105a746d-2f1a-4eaa-a487-94b0a6c5ca39";

fn run_store(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn update_uses_the_existing_store_family_when_omitted() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 1 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-family-update-seed",
            "--idempotency-key",
            "idem-family-update-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 2 }\n").unwrap();
    let updated = run_store(&[
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
        "l1",
        "--request-id",
        "request-family-update",
        "--idempotency-key",
        "idem-family-update",
        "--json",
    ]);
    assert_eq!(
        updated.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&updated.stdout),
        String::from_utf8_lossy(&updated.stderr)
    );
    let report: Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(report["family_id"], FAMILY_ID);
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let generation: i64 = connection
        .query_row(
            "SELECT current_generation FROM views WHERE view_id = 'view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(generation, 2);
}

#[cfg(unix)]
#[test]
fn update_rejects_a_symlink_escape_before_enqueue() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let outside = fixture.path().join("outside.rs");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("inside.rs"), "pub fn inside() {}\n").unwrap();
    std::fs::write(&outside, "pub fn outside() {}\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-symlink-seed",
            "--idempotency-key",
            "idem-symlink-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    std::os::unix::fs::symlink(&outside, root.join("escape.rs")).unwrap();

    let output = run_store(&[
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
        "escape.rs",
        "--level",
        "l1",
        "--request-id",
        "request-symlink-update",
        "--idempotency-key",
        "idem-symlink-update",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "invalid_path");

    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (generation, entries): (i64, i64) = connection
        .query_row(
            "SELECT current_generation,
                    (SELECT COUNT(*) FROM manifest_entries WHERE view_id = 'view-main')
             FROM views WHERE view_id = 'view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((generation, entries), (1, 1));
    let requests: i64 = Connection::open(store.join("coord.db"))
        .unwrap()
        .query_row("SELECT COUNT(*) FROM requests", [], |row| row.get(0))
        .unwrap();
    assert_eq!(requests, 1);
}

#[cfg(unix)]
#[test]
fn executor_rejects_a_symlink_escape_from_a_durable_update_payload() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let outside = fixture.path().join("outside.rs");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("inside.rs"), "pub fn inside() {}\n").unwrap();
    std::fs::write(&outside, "pub fn outside() {}\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-symlink-executor-seed",
            "--idempotency-key",
            "idem-symlink-executor-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    std::os::unix::fs::symlink(&outside, root.join("escape.rs")).unwrap();

    let layout = StoreLayout::open(&store).unwrap();
    let root = root.canonicalize().unwrap();
    let outside_hash = blake3::hash(&std::fs::read(&outside).unwrap()).to_hex();
    StoreCoordinator::open(&layout)
        .unwrap()
        .enqueue(CoordinatorRequest::new(
            "request-symlink-executor",
            "idem-symlink-executor",
            RequestKind::Update,
            serde_json::json!({
                "schema_version": 1,
                "family_id": FAMILY_ID,
                "root": root,
                "view_id": "view-main",
                "requested_level": "l1",
                "file": {
                    "root_relative_path": "escape.rs",
                    "content_hash": format!("blake3:{outside_hash}"),
                    "content_bytes": 22,
                },
                "controls": {
                    "jobs": 0,
                    "l1_chunk_versions": 100,
                    "deep_chunk_versions": 8,
                },
            })
            .to_string(),
            "crafted-requester",
            i64::MAX,
            1,
        ))
        .unwrap();

    let output = run_store(&[
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
        "escape.rs",
        "--level",
        "l1",
        "--request-id",
        "request-symlink-executor-observer",
        "--idempotency-key",
        "idem-symlink-executor",
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "invalid_path");

    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (generation, entries): (i64, i64) = connection
        .query_row(
            "SELECT current_generation,
                    (SELECT COUNT(*) FROM manifest_entries WHERE view_id = 'view-main')
             FROM views WHERE view_id = 'view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((generation, entries), (1, 1));
    let request = StoreCoordinator::open(&layout)
        .unwrap()
        .request("request-symlink-executor")
        .unwrap();
    assert_eq!(request.state, RequestState::Failed);
    let error: Value = serde_json::from_str(request.error_json.as_deref().unwrap()).unwrap();
    assert_eq!(error["message"], "invalid_file_path:outside_root");
}

#[test]
fn delete_uses_the_existing_store_family_when_omitted() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() {}\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-family-delete-seed",
            "--idempotency-key",
            "idem-family-delete-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let deleted = run_store(&[
        "store",
        "delete",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "lib.rs",
        "--request-id",
        "request-family-delete",
        "--idempotency-key",
        "idem-family-delete",
        "--json",
    ]);
    assert_eq!(
        deleted.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&deleted.stdout),
        String::from_utf8_lossy(&deleted.stderr)
    );
    let report: Value = serde_json::from_slice(&deleted.stdout).unwrap();
    assert_eq!(report["family_id"], FAMILY_ID);
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let current_entries: i64 = connection
        .query_row(
            "SELECT COUNT(*)
             FROM views v JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             WHERE v.view_id = 'view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(current_entries, 0);
}

#[test]
fn update_rejects_a_supplied_family_that_does_not_match_the_store() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() {}\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-family-update-mismatch-seed",
            "--idempotency-key",
            "idem-family-update-mismatch-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let updated = run_store(&[
        "store",
        "update",
        "--store",
        store.to_str().unwrap(),
        "--family",
        OTHER_FAMILY_ID,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "lib.rs",
        "--level",
        "l1",
        "--request-id",
        "request-family-update-mismatch",
        "--idempotency-key",
        "idem-family-update-mismatch",
        "--json",
    ]);
    assert_eq!(updated.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(report["failure_class"], "family_mismatch");
    let connection = Connection::open(store.join("coord.db")).unwrap();
    let requests: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM requests
             WHERE request_id = 'request-family-update-mismatch'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(requests, 0);
}

#[test]
fn delete_rejects_a_supplied_family_that_does_not_match_the_store() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() {}\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-family-delete-mismatch-seed",
            "--idempotency-key",
            "idem-family-delete-mismatch-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let deleted = run_store(&[
        "store",
        "delete",
        "--store",
        store.to_str().unwrap(),
        "--family",
        OTHER_FAMILY_ID,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "lib.rs",
        "--request-id",
        "request-family-delete-mismatch",
        "--idempotency-key",
        "idem-family-delete-mismatch",
        "--json",
    ]);
    assert_eq!(deleted.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&deleted.stdout).unwrap();
    assert_eq!(report["failure_class"], "family_mismatch");
    let connection = Connection::open(store.join("coord.db")).unwrap();
    let requests: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM requests
             WHERE request_id = 'request-family-delete-mismatch'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(requests, 0);
}

#[test]
fn update_content_change_appends_a_version_and_publishes_the_new_entry() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 1 }\n").unwrap();

    let imported = run_store(&[
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
        "request-update-seed",
        "--idempotency-key",
        "idem-update-seed",
        "--json",
    ]);
    assert_eq!(imported.status.code(), Some(0));

    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 2 }\n").unwrap();
    let updated = run_store(&[
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
        "lib.rs",
        "--level",
        "l1",
        "--request-id",
        "request-update-content",
        "--idempotency-key",
        "idem-update-content",
        "--json",
    ]);
    assert_eq!(
        updated.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&updated.stdout),
        String::from_utf8_lossy(&updated.stderr)
    );
    let report: Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(report["operation"], "update");
    assert_eq!(report["state"], "committed");
    assert_eq!(
        report["resolution"],
        serde_json::json!({"state": "unbound", "exact_at_matches": false})
    );

    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (generation, version_count, path): (i64, i64, String) = connection
        .query_row(
            "SELECT v.current_generation,
                    (SELECT COUNT(*) FROM file_versions),
                    me.path
             FROM views v
             JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             WHERE v.view_id = 'view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(generation, 2);
    assert_eq!(version_count, 2);
    assert_eq!(path, "lib.rs");
}

#[test]
fn delete_existing_path_publishes_without_removing_versions() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.rs"), "pub fn a() {}\n").unwrap();
    std::fs::write(root.join("b.rs"), "pub fn b() {}\n").unwrap();
    let imported = run_store(&[
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
        "request-delete-seed",
        "--idempotency-key",
        "idem-delete-seed",
        "--json",
    ]);
    assert_eq!(imported.status.code(), Some(0));

    let deleted = run_store(&[
        "store",
        "delete",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "a.rs",
        "--request-id",
        "request-delete-existing",
        "--idempotency-key",
        "idem-delete-existing",
        "--json",
    ]);
    assert_eq!(
        deleted.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&deleted.stdout),
        String::from_utf8_lossy(&deleted.stderr)
    );
    let report: Value = serde_json::from_slice(&deleted.stdout).unwrap();
    assert_eq!(report["operation"], "delete");
    assert_eq!(
        report["resolution"],
        serde_json::json!({"state": "unbound", "exact_at_matches": false})
    );

    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (generation, manifest_entries, version_count): (i64, i64, i64) = connection
        .query_row(
            "SELECT v.current_generation,
                    (SELECT COUNT(*) FROM manifest_entries me
                     WHERE me.view_id = v.view_id AND me.generation = v.current_generation),
                    (SELECT COUNT(*) FROM file_versions)
             FROM views v WHERE v.view_id = 'view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(generation, 2);
    assert_eq!(manifest_entries, 1);
    assert_eq!(version_count, 2);
}

#[test]
fn same_hash_update_is_a_semantic_noop() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
    let imported = run_store(&[
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
        "request-noop-seed",
        "--idempotency-key",
        "idem-noop-seed",
        "--json",
    ]);
    assert_eq!(imported.status.code(), Some(0));

    let updated = run_store(&[
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
        "lib.rs",
        "--level",
        "l1",
        "--request-id",
        "request-noop-update",
        "--idempotency-key",
        "idem-noop-update",
        "--json",
    ]);
    assert_eq!(updated.status.code(), Some(0));

    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (generation, versions, effects): (i64, i64, i64) = connection
        .query_row(
            "SELECT v.current_generation,
                    (SELECT COUNT(*) FROM file_versions),
                    (SELECT COUNT(*) FROM store_log WHERE event_kind = 'manifest_flipped')
             FROM views v WHERE v.view_id = 'view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(generation, 1);
    assert_eq!(versions, 1);
    assert_eq!(effects, 1);
}

#[test]
fn update_requires_an_existing_view_without_creating_one() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() {}\n").unwrap();
    let imported = run_store(&[
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
        "request-view-seed",
        "--idempotency-key",
        "idem-view-seed",
        "--json",
    ]);
    assert_eq!(imported.status.code(), Some(0));

    let updated = run_store(&[
        "store",
        "update",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-missing",
        "--file",
        "lib.rs",
        "--level",
        "l1",
        "--request-id",
        "request-view-missing",
        "--idempotency-key",
        "idem-view-missing",
        "--json",
    ]);
    assert_eq!(updated.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(report["family_id"], FAMILY_ID);
    assert_eq!(report["failure_class"], "view_not_found");
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let view_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM views", [], |row| row.get(0))
        .unwrap();
    assert_eq!(view_count, 1);
}

#[test]
fn delete_missing_view_reports_the_existing_family_when_omitted() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() {}\n").unwrap();
    let imported = run_store(&[
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
        "request-delete-view-seed",
        "--idempotency-key",
        "idem-delete-view-seed",
        "--json",
    ]);
    assert_eq!(imported.status.code(), Some(0));

    let deleted = run_store(&[
        "store",
        "delete",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-missing",
        "--file",
        "lib.rs",
        "--request-id",
        "request-delete-view-missing",
        "--idempotency-key",
        "idem-delete-view-missing",
        "--json",
    ]);
    assert_eq!(deleted.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&deleted.stdout).unwrap();
    assert_eq!(report["family_id"], FAMILY_ID);
    assert_eq!(report["failure_class"], "view_not_found");
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let view_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM views", [], |row| row.get(0))
        .unwrap();
    assert_eq!(view_count, 1);
}

#[test]
fn symbols_update_then_full_reuses_l1_and_deepens_the_same_version() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 1 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-level-seed",
            "--idempotency-key",
            "idem-level-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 2 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "lib.rs",
            "--level",
            "l1",
            "--request-id",
            "request-level-l1",
            "--idempotency-key",
            "idem-level-l1",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let full = run_store(&[
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
        "lib.rs",
        "--level",
        "full",
        "--request-id",
        "request-level-full",
        "--idempotency-key",
        "idem-level-full",
        "--json",
    ]);
    assert_eq!(
        full.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&full.stdout),
        String::from_utf8_lossy(&full.stderr)
    );
    let report: Value = serde_json::from_slice(&full.stdout).unwrap();
    assert_eq!(
        report["completion"],
        serde_json::json!({"l1": true, "l2": true, "l3": true})
    );
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (generation, versions, l1, l2, l3): (i64, i64, Option<i64>, Option<i64>, Option<i64>) =
        connection
            .query_row(
                "SELECT v.current_generation,
                        (SELECT COUNT(*) FROM file_versions),
                        fv.complete_l1, fv.complete_l2, fv.complete_l3
                 FROM views v
                 JOIN manifest_entries me
                   ON me.view_id = v.view_id AND me.generation = v.current_generation
                 JOIN file_versions fv ON fv.version_id = me.version_id
                 WHERE v.view_id = 'view-main'",
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
    assert_eq!(generation, 2);
    assert_eq!(versions, 2);
    assert!(l1 < l2 && l2 < l3);
}

#[test]
fn failed_update_preserves_the_prior_version_and_invalidates_resolution() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-failed-seed",
            "--idempotency-key",
            "idem-failed-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let database = store.join("gen-001/store.db");
    let connection = Connection::open(&database).unwrap();
    let prior_version: i64 = connection
        .query_row("SELECT version_id FROM file_versions", [], |row| row.get(0))
        .unwrap();
    drop(connection);
    std::fs::write(root.join("lib.rs"), [0xff, 0xfe, 0x00]).unwrap();
    let updated = run_store(&[
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
        "lib.rs",
        "--level",
        "l1",
        "--request-id",
        "request-failed-update",
        "--idempotency-key",
        "idem-failed-update",
        "--json",
    ]);
    assert_eq!(
        updated.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&updated.stdout),
        String::from_utf8_lossy(&updated.stderr)
    );
    let report: Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(
        report["resolution"],
        serde_json::json!({"state": "unbound", "exact_at_matches": false})
    );
    let connection = Connection::open(database).unwrap();
    let (status, version_id, version_count): (String, Option<i64>, i64) = connection
        .query_row(
            "SELECT me.status, me.version_id,
                    (SELECT COUNT(*) FROM file_versions)
             FROM views v
             JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             WHERE v.view_id = 'view-main' AND me.path = 'lib.rs'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let (resolution, base, delta, exact_at): (String, Option<i64>, Option<i64>, Option<String>) =
        connection
            .query_row(
                "SELECT resolution_state, resolution_base_id,
                    resolution_delta_generation, resolution_exact_at
             FROM views WHERE view_id = 'view-main'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
    assert_eq!(status, "failed_preserved");
    assert_eq!(version_id, Some(prior_version));
    assert_eq!(version_count, 1);
    assert_eq!(resolution, "unbound");
    assert_eq!((base, delta, exact_at), (None, None, None));
}

#[test]
fn failed_update_without_a_prior_version_publishes_null_version() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("seed.rs"), "pub fn seed() {}\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-failed-new-seed",
            "--idempotency-key",
            "idem-failed-new-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    std::fs::write(root.join("new.rs"), [0xff, 0xfe, 0x00]).unwrap();
    let updated = run_store(&[
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
        "new.rs",
        "--level",
        "l1",
        "--request-id",
        "request-failed-new-update",
        "--idempotency-key",
        "idem-failed-new-update",
        "--json",
    ]);
    assert_eq!(
        updated.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&updated.stdout),
        String::from_utf8_lossy(&updated.stderr)
    );
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (status, version_id, version_count): (String, Option<i64>, i64) = connection
        .query_row(
            "SELECT me.status, me.version_id, (SELECT COUNT(*) FROM file_versions)
             FROM views v
             JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             WHERE v.view_id = 'view-main' AND me.path = 'new.rs'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, "failed");
    assert_eq!(version_id, None);
    assert_eq!(version_count, 1);
}

#[test]
fn delete_missing_path_is_a_semantic_noop() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() {}\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-delete-missing-seed",
            "--idempotency-key",
            "idem-delete-missing-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let deleted = run_store(&[
        "store",
        "delete",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "missing.rs",
        "--request-id",
        "request-delete-missing",
        "--idempotency-key",
        "idem-delete-missing",
        "--json",
    ]);
    assert_eq!(deleted.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&deleted.stdout).unwrap();
    assert_eq!(report["manifest"]["disposition"], "reused");
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (generation, versions, effects): (i64, i64, i64) = connection
        .query_row(
            "SELECT current_generation,
                    (SELECT COUNT(*) FROM file_versions),
                    (SELECT COUNT(*) FROM store_log WHERE event_kind = 'manifest_flipped')
             FROM views WHERE view_id = 'view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!((generation, versions, effects), (1, 1, 1));
}

#[test]
fn delete_then_readd_reuses_the_retained_version() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() {}\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-readd-seed",
            "--idempotency-key",
            "idem-readd-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    assert_eq!(
        run_store(&[
            "store",
            "delete",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--file",
            "lib.rs",
            "--request-id",
            "request-readd-delete",
            "--idempotency-key",
            "idem-readd-delete",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    assert_eq!(
        run_store(&[
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
            "lib.rs",
            "--level",
            "l1",
            "--request-id",
            "request-readd-update",
            "--idempotency-key",
            "idem-readd-update",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (generation, versions, entries): (i64, i64, i64) = connection
        .query_row(
            "SELECT current_generation,
                    (SELECT COUNT(*) FROM file_versions),
                    (SELECT COUNT(*) FROM manifest_entries me
                     WHERE me.view_id = views.view_id AND me.generation = views.current_generation)
             FROM views WHERE view_id = 'view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!((generation, versions, entries), (1, 1, 1));
}

#[test]
fn path_rename_is_delete_then_update_without_removing_the_old_version() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("old.rs"), "pub fn answer() {}\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-rename-seed",
            "--idempotency-key",
            "idem-rename-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    std::fs::rename(root.join("old.rs"), root.join("new.rs")).unwrap();
    assert_eq!(
        run_store(&[
            "store",
            "delete",
            "--store",
            store.to_str().unwrap(),
            "--family",
            FAMILY_ID,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--file",
            "old.rs",
            "--request-id",
            "request-rename-delete",
            "--idempotency-key",
            "idem-rename-delete",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    assert_eq!(
        run_store(&[
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
            "new.rs",
            "--level",
            "l1",
            "--request-id",
            "request-rename-update",
            "--idempotency-key",
            "idem-rename-update",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (path, versions): (String, i64) = connection
        .query_row(
            "SELECT me.path, (SELECT COUNT(*) FROM file_versions)
             FROM views v JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             WHERE v.view_id = 'view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(path, "new.rs");
    assert_eq!(versions, 2);
}

#[test]
fn duplicate_update_request_has_one_manifest_and_terminal_effect() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 1 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-duplicate-seed",
            "--idempotency-key",
            "idem-duplicate-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 2 }\n").unwrap();
    let args = [
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
        "lib.rs",
        "--level",
        "l1",
        "--request-id",
        "request-duplicate-update",
        "--idempotency-key",
        "idem-duplicate-update",
        "--json",
    ];
    assert_eq!(run_store(&args).status.code(), Some(0));
    assert_eq!(run_store(&args).status.code(), Some(0));
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (terminal, update_manifest_effects): (i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM store_log
                 WHERE request_id = 'request-duplicate-update'
                   AND event_kind = 'store_update_completed'),
                (SELECT COUNT(*) FROM store_log
                 WHERE request_id = 'request-duplicate-update'
                   AND event_kind = 'manifest_flipped')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((terminal, update_manifest_effects), (1, 1));
}

#[test]
fn full_update_publishes_l1_before_l2_and_l3() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 1 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-full-wave-seed",
            "--idempotency-key",
            "idem-full-wave-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 2 }\n").unwrap();
    let updated = run_store(&[
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
        "lib.rs",
        "--level",
        "full",
        "--request-id",
        "request-full-wave",
        "--idempotency-key",
        "idem-full-wave",
        "--json",
    ]);
    assert_eq!(
        updated.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&updated.stdout),
        String::from_utf8_lossy(&updated.stderr)
    );
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let events = connection
        .prepare(
            "SELECT event_kind, level FROM store_log
             WHERE request_id = 'request-full-wave' ORDER BY sequence",
        )
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
        .position(|event| event.0 == "store_update_completed")
        .unwrap();
    assert!(l1 < manifest && manifest < l2 && l2 < l3 && l3 < terminal);
}

#[test]
fn full_update_rejects_l1_projection_mismatch_without_rewriting_l1() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-mismatch-seed",
            "--idempotency-key",
            "idem-mismatch-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let database = store.join("gen-001/store.db");
    let connection = Connection::open(&database).unwrap();
    let l1_rows_before: i64 = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM symbols)
                  + (SELECT COUNT(*) FROM complexity_metrics)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute(
            "UPDATE complexity_metrics SET decision_count = decision_count + 1",
            [],
        )
        .unwrap();
    drop(connection);
    let updated = run_store(&[
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
        "lib.rs",
        "--level",
        "full",
        "--request-id",
        "request-mismatch-full",
        "--idempotency-key",
        "idem-mismatch-full",
        "--json",
    ]);
    assert_eq!(updated.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(report["operation"], "update");
    assert_eq!(report["failure_class"], "l1_projection_mismatch");
    let connection = Connection::open(database).unwrap();
    let (l1_rows_after, l2, l3, update_l1_effects): (i64, Option<i64>, Option<i64>, i64) =
        connection
            .query_row(
                "SELECT
                (SELECT COUNT(*) FROM symbols) + (SELECT COUNT(*) FROM complexity_metrics),
                complete_l2, complete_l3,
                (SELECT COUNT(*) FROM store_log
                 WHERE request_id = 'request-mismatch-full'
                   AND event_kind = 'version_level_completed' AND level = 1)
             FROM file_versions",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
    assert_eq!(l1_rows_after, l1_rows_before);
    assert_eq!((l2, l3), (None, None));
    assert_eq!(update_l1_effects, 0);
}

#[test]
#[cfg(feature = "test-store-contract")]
fn source_change_between_full_update_waves_keeps_the_published_l1_entry() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let ready = fixture.path().join("l1.ready");
    let resume = fixture.path().join("l1.resume");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 1 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-wave-change-seed",
            "--idempotency-key",
            "idem-wave-change-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let published_bytes = b"pub fn answer() -> u32 { 2 }\n";
    std::fs::write(root.join("lib.rs"), published_bytes).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "lib.rs",
            "--level",
            "full",
            "--request-id",
            "request-wave-change",
            "--idempotency-key",
            "idem-wave-change",
            "--json",
        ])
        .env("JULIE_EXTRACT_STORE_TEST_L1_READY_FILE", &ready)
        .env("JULIE_EXTRACT_STORE_TEST_L1_RESUME_FILE", &resume)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "L1 update wave was not observed");
        std::thread::sleep(Duration::from_millis(2));
    }
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 3 }\n").unwrap();
    std::fs::write(&resume, b"resume").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "changed_between_waves");
    assert_eq!(report["manifest"]["generation"], 2);
    assert_eq!(
        report["completion"],
        serde_json::json!({"l1": true, "l2": false, "l3": false})
    );
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (hash, l2, l3): (String, Option<i64>, Option<i64>) = connection
        .query_row(
            "SELECT me.observed_content_hash, fv.complete_l2, fv.complete_l3
             FROM views v
             JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             JOIN file_versions fv ON fv.version_id = me.version_id
             WHERE v.view_id = 'view-main' AND me.path = 'lib.rs'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        hash,
        format!("blake3:{}", blake3::hash(published_bytes).to_hex())
    );
    assert_eq!((l2, l3), (None, None));
}

#[test]
#[cfg(feature = "test-store-contract")]
fn concurrent_disjoint_updates_converge_without_losing_either_delta() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let ready = fixture.path().join("concurrent.ready");
    let resume = fixture.path().join("concurrent.resume");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.rs"), "pub fn a() -> u32 { 1 }\n").unwrap();
    std::fs::write(root.join("b.rs"), "pub fn b() -> u32 { 1 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-concurrent-seed",
            "--idempotency-key",
            "idem-concurrent-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let a_bytes = b"pub fn a() -> u32 { 2 }\n";
    let b_bytes = b"pub fn b() -> u32 { 2 }\n";
    std::fs::write(root.join("a.rs"), a_bytes).unwrap();
    std::fs::write(root.join("b.rs"), b_bytes).unwrap();
    let first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "a.rs",
            "--level",
            "l1",
            "--request-id",
            "request-concurrent-a",
            "--idempotency-key",
            "idem-concurrent-a",
            "--json",
        ])
        .env("JULIE_EXTRACT_STORE_TEST_L1_READY_FILE", &ready)
        .env("JULIE_EXTRACT_STORE_TEST_L1_RESUME_FILE", &resume)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready.exists() {
        assert!(
            Instant::now() < deadline,
            "first update did not hold the lease"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let second = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "b.rs",
            "--level",
            "l1",
            "--request-id",
            "request-concurrent-b",
            "--idempotency-key",
            "idem-concurrent-b",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&resume, b"resume").unwrap();
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    assert_eq!(
        first.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(
        second.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let hashes = connection
        .prepare(
            "SELECT me.path, me.observed_content_hash
             FROM views v JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             WHERE v.view_id = 'view-main' ORDER BY me.path",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        hashes,
        vec![
            (
                "a.rs".to_string(),
                format!("blake3:{}", blake3::hash(a_bytes).to_hex())
            ),
            (
                "b.rs".to_string(),
                format!("blake3:{}", blake3::hash(b_bytes).to_hex())
            ),
        ]
    );
}

#[test]
#[cfg(feature = "test-store-contract")]
fn concurrent_same_file_waiter_recomputes_from_the_new_generation() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let ready = fixture.path().join("same-file.ready");
    let resume = fixture.path().join("same-file.resume");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 1 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-same-file-seed",
            "--idempotency-key",
            "idem-same-file-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 2 }\n").unwrap();
    let first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "lib.rs",
            "--level",
            "l1",
            "--request-id",
            "request-same-file-first",
            "--idempotency-key",
            "idem-same-file-first",
            "--json",
        ])
        .env("JULIE_EXTRACT_STORE_TEST_L1_READY_FILE", &ready)
        .env("JULIE_EXTRACT_STORE_TEST_L1_RESUME_FILE", &resume)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready.exists() {
        assert!(
            Instant::now() < deadline,
            "first same-file update did not hold the lease"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let winner_bytes = b"pub fn answer() -> u32 { 3 }\n";
    std::fs::write(root.join("lib.rs"), winner_bytes).unwrap();
    let second = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
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
            "lib.rs",
            "--level",
            "l1",
            "--request-id",
            "request-same-file-second",
            "--idempotency-key",
            "idem-same-file-second",
            "--json",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&resume, b"resume").unwrap();
    let first = first.wait_with_output().unwrap();
    let second = second.wait_with_output().unwrap();
    assert_eq!(first.status.code(), Some(0));
    assert_eq!(
        second.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (generation, hash, versions): (i64, String, i64) = connection
        .query_row(
            "SELECT v.current_generation, me.observed_content_hash,
                    (SELECT COUNT(*) FROM file_versions)
             FROM views v JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             WHERE v.view_id = 'view-main' AND me.path = 'lib.rs'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(generation, 3);
    assert_eq!(
        hash,
        format!("blake3:{}", blake3::hash(winner_bytes).to_hex())
    );
    assert_eq!(versions, 3);
}

#[test]
fn delete_accepts_repeated_files_and_removes_exactly_the_named_paths() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    for name in ["a.rs", "b.rs", "c.rs"] {
        std::fs::write(root.join(name), format!("pub fn {}() {{}}\n", &name[..1])).unwrap();
    }
    assert_eq!(
        run_store(&[
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
            "request-multi-delete-seed",
            "--idempotency-key",
            "idem-multi-delete-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let deleted = run_store(&[
        "store",
        "delete",
        "--store",
        store.to_str().unwrap(),
        "--family",
        FAMILY_ID,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "a.rs",
        "--file",
        "b.rs",
        "--request-id",
        "request-multi-delete",
        "--idempotency-key",
        "idem-multi-delete",
        "--json",
    ]);
    assert_eq!(deleted.status.code(), Some(0));
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (path, versions): (String, i64) = connection
        .query_row(
            "SELECT me.path, (SELECT COUNT(*) FROM file_versions)
             FROM views v JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             WHERE v.view_id = 'view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(path, "c.rs");
    assert_eq!(versions, 3);
}

#[test]
fn terminal_update_replay_uses_the_canonical_request_after_the_root_disappears() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let moved_root = fixture.path().join("moved-root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 1 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-replay-root-seed",
            "--idempotency-key",
            "idem-replay-root-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 2 }\n").unwrap();
    let first = run_store(&[
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
        "l1",
        "--request-id",
        "request-replay-root-update",
        "--idempotency-key",
        "idem-replay-root-update",
        "--json",
    ]);
    assert_eq!(first.status.code(), Some(0));
    let first_report: Value = serde_json::from_slice(&first.stdout).unwrap();
    std::fs::rename(&root, moved_root).unwrap();
    let replay = run_store(&[
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
        "l1",
        "--request-id",
        "request-replay-root-observer",
        "--idempotency-key",
        "idem-replay-root-update",
        "--json",
    ]);
    assert_eq!(
        replay.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&replay.stdout),
        String::from_utf8_lossy(&replay.stderr)
    );
    let replay_report: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay_report, first_report);
    let store_connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (terminal, manifest_effects): (i64, i64) = store_connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM store_log
                WHERE request_id = 'request-replay-root-update'
                  AND event_kind = 'store_update_completed'),
               (SELECT COUNT(*) FROM store_log
                WHERE request_id = 'request-replay-root-update'
                  AND event_kind = 'manifest_flipped')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((terminal, manifest_effects), (1, 1));
    let coordinator = Connection::open(store.join("coord.db")).unwrap();
    let requests: i64 = coordinator
        .query_row(
            "SELECT COUNT(*) FROM requests
             WHERE idempotency_key = 'idem-replay-root-update'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(requests, 1);
}

#[test]
fn terminal_delete_replay_uses_the_canonical_request_after_the_root_disappears() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let moved_root = fixture.path().join("moved-root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() {}\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-delete-replay-root-seed",
            "--idempotency-key",
            "idem-delete-replay-root-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let first = run_store(&[
        "store",
        "delete",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "lib.rs",
        "--request-id",
        "request-delete-replay-root",
        "--idempotency-key",
        "idem-delete-replay-root",
        "--json",
    ]);
    assert_eq!(first.status.code(), Some(0));
    let first_report: Value = serde_json::from_slice(&first.stdout).unwrap();
    std::fs::rename(&root, moved_root).unwrap();
    let replay = run_store(&[
        "store",
        "delete",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "lib.rs",
        "--request-id",
        "request-delete-replay-root-observer",
        "--idempotency-key",
        "idem-delete-replay-root",
        "--json",
    ]);
    assert_eq!(
        replay.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&replay.stdout),
        String::from_utf8_lossy(&replay.stderr)
    );
    let replay_report: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay_report, first_report);
    let store_connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (terminal, manifest_effects): (i64, i64) = store_connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM store_log
                WHERE request_id = 'request-delete-replay-root'
                  AND event_kind = 'store_delete_completed'),
               (SELECT COUNT(*) FROM store_log
                WHERE request_id = 'request-delete-replay-root'
                  AND event_kind = 'manifest_flipped')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((terminal, manifest_effects), (1, 1));
    let coordinator = Connection::open(store.join("coord.db")).unwrap();
    let requests: i64 = coordinator
        .query_row(
            "SELECT COUNT(*) FROM requests
             WHERE idempotency_key = 'idem-delete-replay-root'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(requests, 1);
}

#[test]
fn delete_reports_idempotency_conflict_before_parsing_an_update_payload() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 1 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-cross-kind-seed",
            "--idempotency-key",
            "idem-cross-kind-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 2 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "l1",
            "--request-id",
            "request-cross-kind-update",
            "--idempotency-key",
            "idem-cross-kind",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let deleted = run_store(&[
        "store",
        "delete",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "lib.rs",
        "--request-id",
        "request-cross-kind-delete",
        "--idempotency-key",
        "idem-cross-kind",
        "--json",
    ]);
    assert_eq!(deleted.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&deleted.stdout).unwrap();
    assert_eq!(report["failure_class"], "idempotency_conflict");
    assert_eq!(report["error"]["message"], "idempotency_conflict");
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let generation: i64 = connection
        .query_row(
            "SELECT current_generation FROM views WHERE view_id = 'view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(generation, 2);
}

#[test]
fn update_reports_idempotency_conflict_before_parsing_a_delete_payload() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() {}\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-reverse-cross-kind-seed",
            "--idempotency-key",
            "idem-reverse-cross-kind-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    assert_eq!(
        run_store(&[
            "store",
            "delete",
            "--store",
            store.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--file",
            "lib.rs",
            "--request-id",
            "request-reverse-cross-kind-delete",
            "--idempotency-key",
            "idem-reverse-cross-kind",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    let updated = run_store(&[
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
        "l1",
        "--request-id",
        "request-reverse-cross-kind-update",
        "--idempotency-key",
        "idem-reverse-cross-kind",
        "--json",
    ]);
    assert_eq!(updated.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(report["failure_class"], "idempotency_conflict");
    assert_eq!(report["error"]["message"], "idempotency_conflict");
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let generation: i64 = connection
        .query_row(
            "SELECT current_generation FROM views WHERE view_id = 'view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(generation, 2);
}

#[test]
#[cfg(feature = "test-store-contract")]
fn resumed_full_update_reports_its_l1_generation_after_an_intervening_flip() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    let ready = fixture.path().join("full-resume.ready");
    let resume = fixture.path().join("full-resume.resume");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.rs"), "pub fn a() -> u32 { 1 }\n").unwrap();
    std::fs::write(root.join("b.rs"), "pub fn b() -> u32 { 1 }\n").unwrap();
    assert_eq!(
        run_store(&[
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
            "request-full-resume-seed",
            "--idempotency-key",
            "idem-full-resume-seed",
            "--json",
        ])
        .status
        .code(),
        Some(0)
    );
    std::fs::write(root.join("a.rs"), "pub fn a() -> u32 { 2 }\n").unwrap();
    let mut first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "update",
            "--store",
            store.to_str().unwrap(),
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--file",
            "a.rs",
            "--level",
            "full",
            "--request-id",
            "request-full-resume-a",
            "--idempotency-key",
            "idem-full-resume-a",
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
            "full update exited before pausing after durable L1 progress"
        );
        assert!(
            Instant::now() < deadline,
            "full update did not pause after durable L1 progress"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    first.kill().unwrap();
    let killed = first.wait_with_output().unwrap();
    assert!(!killed.status.success());
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let generation_two_hash: String = connection
        .query_row(
            "SELECT manifest_hash FROM manifests
             WHERE view_id = 'view-main' AND generation = 2",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);
    std::fs::write(root.join("b.rs"), "pub fn b() -> u32 { 2 }\n").unwrap();
    let intervening = run_store(&[
        "store",
        "update",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "b.rs",
        "--level",
        "l1",
        "--request-id",
        "request-full-resume-b",
        "--idempotency-key",
        "idem-full-resume-b",
        "--json",
    ]);
    assert_eq!(
        intervening.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&intervening.stdout),
        String::from_utf8_lossy(&intervening.stderr)
    );
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (current_generation, generation_three_hash): (i64, String) = connection
        .query_row(
            "SELECT current_generation,
                    (SELECT manifest_hash FROM manifests
                     WHERE view_id = views.view_id AND generation = 3)
             FROM views WHERE view_id = 'view-main'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(current_generation, 3);
    assert_ne!(generation_two_hash, generation_three_hash);
    drop(connection);
    let resumed = run_store(&[
        "store",
        "update",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "a.rs",
        "--level",
        "full",
        "--request-id",
        "request-full-resume-observer",
        "--idempotency-key",
        "idem-full-resume-a",
        "--json",
    ]);
    assert_eq!(
        resumed.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&resumed.stdout),
        String::from_utf8_lossy(&resumed.stderr)
    );
    let report: Value = serde_json::from_slice(&resumed.stdout).unwrap();
    assert_eq!(report["request"]["id"], "request-full-resume-a");
    assert_eq!(report["manifest"]["generation"], 2);
    assert_eq!(report["manifest"]["hash"], generation_two_hash);
    let coordinator = Connection::open(store.join("coord.db")).unwrap();
    let result_json: String = coordinator
        .query_row(
            "SELECT result_json FROM requests
             WHERE request_id = 'request-full-resume-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let result: Value = serde_json::from_str(&result_json).unwrap();
    assert_eq!(result["manifest_generation"], 2);
    assert_eq!(result["manifest_hash"], report["manifest"]["hash"]);
}
