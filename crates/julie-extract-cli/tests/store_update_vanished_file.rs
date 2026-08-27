use std::path::Path;
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::Value;

const FAMILY_ID: &str = "4d1a7c58-6e2f-4b3a-8c9d-1f0e2a3b4c5d";

fn run_store(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .unwrap()
}

fn seed_store(root: &Path, store: &Path, request: &str) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 1 }\n").unwrap();
    std::fs::write(
        root.join("util.rs"),
        "pub fn twice(x: u32) -> u32 { x * 2 }\n",
    )
    .unwrap();
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
        request,
        "--idempotency-key",
        request,
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

fn update(root: &Path, store: &Path, file: &str, request: &str) -> Output {
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
        file,
        "--level",
        "l1",
        "--request-id",
        request,
        "--idempotency-key",
        request,
        "--json",
    ])
}

fn report_of(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "expected a JSON report, got {error}: stdout: {} stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn current_manifest_paths(store: &Path) -> Vec<String> {
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT me.path FROM views v
             JOIN manifest_entries me
               ON me.view_id = v.view_id AND me.generation = v.current_generation
             WHERE v.view_id = 'view-main'
             ORDER BY me.path",
        )
        .unwrap();
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn serving_generation(store: &Path) -> i64 {
    Connection::open(store.join("gen-001/store.db"))
        .unwrap()
        .query_row(
            "SELECT current_generation FROM views WHERE view_id = 'view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn update_of_a_vanished_indexed_file_commits_as_a_delete() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    seed_store(&root, &store, "request-vanished-seed");
    std::fs::remove_file(root.join("util.rs")).unwrap();

    let updated = update(&root, &store, "util.rs", "request-vanished-update");

    assert_eq!(
        updated.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&updated.stdout),
        String::from_utf8_lossy(&updated.stderr)
    );
    let report = report_of(&updated);
    assert_eq!(report["operation"], "update");
    assert_eq!(report["state"], "committed");
    assert_eq!(report["failure_class"], "none");
    assert_eq!(report["error"], Value::Null);
    assert_eq!(report["manifest"]["disposition"], "created");
    assert_eq!(current_manifest_paths(&store), vec!["lib.rs".to_string()]);
    assert_eq!(serving_generation(&store), 2);
}

#[test]
fn update_of_a_vanished_never_indexed_file_commits_without_a_new_generation() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    seed_store(&root, &store, "request-ghost-seed");

    let updated = update(&root, &store, "ghost.rs", "request-ghost-update");

    assert_eq!(
        updated.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&updated.stdout),
        String::from_utf8_lossy(&updated.stderr)
    );
    let report = report_of(&updated);
    assert_eq!(report["operation"], "update");
    assert_eq!(report["state"], "committed");
    assert_eq!(report["manifest"]["disposition"], "reused");
    assert_eq!(
        current_manifest_paths(&store),
        vec!["lib.rs".to_string(), "util.rs".to_string()]
    );
    assert_eq!(serving_generation(&store), 1);
}

#[test]
fn replaying_a_vanished_file_update_returns_the_same_committed_outcome() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    seed_store(&root, &store, "request-replay-seed");
    std::fs::remove_file(root.join("util.rs")).unwrap();
    let first = update(&root, &store, "util.rs", "request-replay-update");
    assert_eq!(first.status.code(), Some(0));

    let replay = update(&root, &store, "util.rs", "request-replay-update");

    assert_eq!(
        replay.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&replay.stdout),
        String::from_utf8_lossy(&replay.stderr)
    );
    let report = report_of(&replay);
    assert_eq!(report["operation"], "update");
    assert_eq!(report["state"], "committed");
    assert_eq!(current_manifest_paths(&store), vec!["lib.rs".to_string()]);
    assert_eq!(serving_generation(&store), 2);
}

#[test]
fn update_of_an_unreadable_existing_file_still_fails_the_request() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    seed_store(&root, &store, "request-unreadable-seed");
    std::fs::create_dir(root.join("blocked.rs")).unwrap();

    let updated = update(&root, &store, "blocked.rs", "request-unreadable-update");

    assert_eq!(
        updated.status.code(),
        Some(1),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&updated.stdout),
        String::from_utf8_lossy(&updated.stderr)
    );
    let report = report_of(&updated);
    assert_eq!(report["operation"], "update");
    assert_eq!(report["state"], "failed");
    assert_eq!(
        current_manifest_paths(&store),
        vec!["lib.rs".to_string(), "util.rs".to_string()]
    );
    assert_eq!(serving_generation(&store), 1);
}
