use std::path::Path;
use std::process::{Command, Output};

use julie_extract_cli::limits::{MAX_SOURCE_FILE_BYTES, slow_file_skip_message};
use rusqlite::Connection;
use serde_json::Value;

const FAMILY_ID: &str = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";

fn run_store(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .unwrap()
}

fn seed_store(root: &Path, store: &Path, request: &str) {
    std::fs::create_dir_all(root).unwrap();
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

fn queued_request_count(store: &Path, request: &str) -> i64 {
    Connection::open(store.join("coord.db"))
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM requests WHERE request_id = ?1",
            [request],
            |row| row.get(0),
        )
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

fn report_of(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "expected a JSON report, got {error}: stdout: {} stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn update_refuses_a_file_over_the_extraction_limit_without_queueing_a_request() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    seed_store(&root, &store, "request-oversized-seed");
    std::fs::write(
        root.join("parser.rs"),
        vec![b'a'; MAX_SOURCE_FILE_BYTES + 1],
    )
    .unwrap();

    let updated = update(&root, &store, "parser.rs", "request-oversized-update");

    assert_eq!(
        updated.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&updated.stdout),
        String::from_utf8_lossy(&updated.stderr)
    );
    let report = report_of(&updated);
    assert_eq!(report["operation"], "update");
    assert_eq!(report["state"], "unsupported");
    assert_eq!(report["coordinator"], "not_started");
    assert_eq!(report["failure_class"], "none");
    assert_eq!(report["error"], Value::Null);
    assert_eq!(report["unsupported"]["reason"], "oversized");
    assert_eq!(report["unsupported"]["root_relative_path"], "parser.rs");
    assert_eq!(
        report["unsupported"]["message"],
        Value::String(slow_file_skip_message())
    );
    assert_eq!(queued_request_count(&store, "request-oversized-update"), 0);
    assert_eq!(serving_generation(&store), 1);
}

#[test]
fn update_refuses_a_hard_excluded_minified_file_without_queueing_a_request() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    seed_store(&root, &store, "request-minified-seed");
    std::fs::write(root.join("vendor.min.js"), "var a=1;\n").unwrap();

    let updated = update(&root, &store, "vendor.min.js", "request-minified-update");

    assert_eq!(
        updated.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&updated.stdout),
        String::from_utf8_lossy(&updated.stderr)
    );
    let report = report_of(&updated);
    assert_eq!(report["state"], "unsupported");
    assert_eq!(report["unsupported"]["reason"], "hard_excluded");
    assert_eq!(report["unsupported"]["root_relative_path"], "vendor.min.js");
    assert_eq!(queued_request_count(&store, "request-minified-update"), 0);
    assert_eq!(serving_generation(&store), 1);
}

#[test]
fn update_of_a_supported_file_still_commits_end_to_end() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("root");
    let store = fixture.path().join("store");
    seed_store(&root, &store, "request-supported-seed");
    std::fs::write(root.join("lib.rs"), "pub fn answer() -> u32 { 2 }\n").unwrap();

    let updated = update(&root, &store, "lib.rs", "request-supported-update");

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
    assert_eq!(report.get("unsupported"), None);
    assert_eq!(queued_request_count(&store, "request-supported-update"), 1);
    assert_eq!(serving_generation(&store), 2);
}
