use std::path::Path;
use std::process::{Command, Output};

use rusqlite::{Connection, params};
use serde_json::Value;
use tempfile::TempDir;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .unwrap()
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn metadata(db: &Path) -> Vec<(String, String)> {
    let connection = Connection::open(db).unwrap();
    let mut statement = connection
        .prepare("SELECT key, value FROM artifact_metadata ORDER BY key")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn file_rows(db: &Path) -> Vec<(String, String)> {
    let connection = Connection::open(db).unwrap();
    let mut statement = connection
        .prepare("SELECT path, content_hash FROM files ORDER BY path")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn symbol_rows(db: &Path) -> Vec<(String, String, String)> {
    let connection = Connection::open(db).unwrap();
    let mut statement = connection
        .prepare("SELECT path, name, COALESCE(body_hash, '') FROM symbols ORDER BY path, name")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn scan_refuses_incremental_write_when_producer_generation_differs() {
    let fixture = TempDir::new().unwrap();
    let root = fixture.path().join("src");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn value() -> i32 { 1 }\n").unwrap();
    let db = fixture.path().join("artifact.sqlite");
    let scan = run(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&db),
        "--json",
    ]);
    assert_eq!(scan.status.code(), Some(0));

    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE artifact_metadata SET value = ?1 WHERE key = 'capability_snapshot_fingerprint'",
            params!["sha256:older-semantic-contract"],
        )
        .unwrap();
    drop(connection);
    let before_metadata = metadata(&db);
    let before_files = file_rows(&db);

    let output = run(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(3));
    assert_eq!(report(&output)["errors"][0]["code"], "fingerprint_mismatch");
    assert_eq!(metadata(&db), before_metadata);
    assert_eq!(file_rows(&db), before_files);
}

#[test]
fn force_scan_refuses_to_change_generation_when_existing_source_is_invalid_utf8() {
    let fixture = TempDir::new().unwrap();
    let root = fixture.path().join("src");
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("lib.rs");
    std::fs::write(&source, "pub fn previous() -> i32 { 1 }\n").unwrap();
    let db = fixture.path().join("artifact.sqlite");
    let scan = run(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&db),
        "--json",
    ]);
    assert_eq!(scan.status.code(), Some(0));

    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE artifact_metadata SET value = ?1 WHERE key = 'capability_snapshot_fingerprint'",
            params!["sha256:older-semantic-contract"],
        )
        .unwrap();
    drop(connection);
    let before_metadata = metadata(&db);
    let before_files = file_rows(&db);
    let before_symbols = symbol_rows(&db);
    std::fs::write(source, [0xff, 0xfe, 0x00]).unwrap();

    let output = run(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(3));
    assert_eq!(report(&output)["errors"][0]["code"], "fingerprint_mismatch");
    assert_eq!(metadata(&db), before_metadata);
    assert_eq!(file_rows(&db), before_files);
    assert_eq!(symbol_rows(&db), before_symbols);
}

#[test]
fn force_scan_refuses_to_change_generation_when_new_source_is_invalid_utf8() {
    let fixture = TempDir::new().unwrap();
    let root = fixture.path().join("src");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn previous() -> i32 { 1 }\n").unwrap();
    let db = fixture.path().join("artifact.sqlite");
    let scan = run(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&db),
        "--json",
    ]);
    assert_eq!(scan.status.code(), Some(0));

    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE artifact_metadata SET value = ?1 WHERE key = 'capability_snapshot_fingerprint'",
            params!["sha256:older-semantic-contract"],
        )
        .unwrap();
    drop(connection);
    let before_metadata = metadata(&db);
    let before_files = file_rows(&db);
    let before_symbols = symbol_rows(&db);
    std::fs::write(root.join("new.rs"), [0xff, 0xfe, 0x00]).unwrap();

    let output = run(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(3));
    assert_eq!(report(&output)["errors"][0]["code"], "fingerprint_mismatch");
    assert_eq!(metadata(&db), before_metadata);
    assert_eq!(file_rows(&db), before_files);
    assert_eq!(symbol_rows(&db), before_symbols);
}
