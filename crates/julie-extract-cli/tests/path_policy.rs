use std::path::Path;
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

fn julie_extract(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run julie-extract {args:?}: {err}"))
}

fn json_report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "stdout was not a JSON report: {err}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn update_canonicalizes_root_db_file_and_ignore_file_before_reporting() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    let source_dir = root.join("src");
    let artifact_dir = temp.path().join("artifacts");
    std::fs::create_dir_all(&source_dir).unwrap();
    std::fs::create_dir_all(&artifact_dir).unwrap();
    std::fs::write(source_dir.join("ignored.rs"), "fn ignored() {}\n").unwrap();
    std::fs::write(root.join(".extractignore"), "src/ignored.rs\n").unwrap();
    let db = artifact_dir.join("artifact.sqlite");
    create_artifact(&db, &root);

    let root_arg = root.join(".");
    let db_arg = artifact_dir
        .join("..")
        .join("artifacts")
        .join("artifact.sqlite");
    let ignore_arg = root.join(".").join(".extractignore");
    let output = julie_extract(&[
        "update",
        "--root",
        path_str(&root_arg),
        "--db",
        path_str(&db_arg),
        "--file",
        "src/./ignored.rs",
        "--ignore-file",
        path_str(&ignore_arg),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "unsupported");
    assert_eq!(report["operation"], "update");
    assert_eq!(report["input"]["root_path"], canonical(&root));
    assert_eq!(report["input"]["db_path"], canonical(&db));
    assert_eq!(
        report["input"]["file_path"],
        canonical(source_dir.join("ignored.rs"))
    );
    assert_eq!(report["input"]["root_relative_path"], "src/ignored.rs");
    assert_eq!(report["warnings"][0]["code"], "unsupported_file");
    assert_eq!(
        report["warnings"][0]["root_relative_path"],
        "src/ignored.rs"
    );
    assert_eq!(file_count_for_path(&db, "src/ignored.rs"), 0);
}

#[test]
fn update_file_outside_root_returns_typed_error() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    let outside = temp.path().join("outside.rs");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&outside, "fn outside() {}\n").unwrap();
    let db = temp.path().join("artifact.sqlite");
    create_artifact(&db, &root);

    let output = julie_extract(&[
        "update",
        "--root",
        path_str(&root),
        "--db",
        path_str(&db),
        "--file",
        path_str(&outside),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "file_outside_root");
    assert_eq!(report["errors"][0]["path"], canonical(&outside));
}

#[test]
fn update_file_must_exist() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir_all(root.join("src")).unwrap();
    let db = temp.path().join("artifact.sqlite");
    create_artifact(&db, &root);

    let output = julie_extract(&[
        "update",
        "--root",
        path_str(&root),
        "--db",
        path_str(&db),
        "--file",
        "src/missing.rs",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "file_not_found");
    assert_eq!(report["errors"][0]["root_relative_path"], "src/missing.rs");
}

#[test]
fn delete_file_does_not_require_source_file_to_exist() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir_all(root.join("src")).unwrap();
    let db = temp.path().join("artifact.sqlite");
    create_artifact(&db, &root);

    let output = julie_extract(&[
        "delete",
        "--root",
        path_str(&root),
        "--db",
        path_str(&db),
        "--file",
        "src/missing.rs",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "not_found");
    assert_eq!(report["input"]["root_relative_path"], "src/missing.rs");
    assert!(report["errors"].as_array().unwrap().is_empty());
}

#[test]
fn root_mismatch_returns_exit_3_unless_scan_force_rebuilds_metadata() {
    let temp = TempDir::new().unwrap();
    let old_root = temp.path().join("old-root");
    let new_root = temp.path().join("new-root");
    std::fs::create_dir_all(old_root.join("src")).unwrap();
    std::fs::write(old_root.join("src/old.rs"), "fn stale() {}\n").unwrap();
    std::fs::create_dir_all(new_root.join("src")).unwrap();
    std::fs::write(new_root.join("src/main.rs"), "fn main() {}\n").unwrap();
    let db = temp.path().join("artifact.sqlite");
    create_artifact(&db, &old_root);
    let artifact_alias = temp.path().join("artifact-alias.sqlite");
    std::fs::hard_link(&db, &artifact_alias).unwrap();

    let mismatch = julie_extract(&[
        "update",
        "--root",
        path_str(&new_root),
        "--db",
        path_str(&db),
        "--file",
        "src/main.rs",
        "--json",
    ]);
    assert_eq!(mismatch.status.code(), Some(3));
    let report = json_report(&mismatch);
    assert_eq!(report["errors"][0]["code"], "root_mismatch");

    std::fs::remove_file(new_root.join("src/main.rs")).unwrap();
    let forced = julie_extract(&[
        "scan",
        "--root",
        path_str(&new_root),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]);
    assert_eq!(forced.status.code(), Some(0));
    let report = json_report(&forced);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["mode"], "force");
    assert_eq!(report["artifact"]["root_path"], canonical(&new_root));
    assert_eq!(artifact_root(&db), canonical(&new_root));
    assert_eq!(artifact_root(&artifact_alias), canonical(&new_root));
    assert_eq!(file_count_for_path(&db, "src/old.rs"), 0);
}

fn create_artifact(path: &Path, root: &Path) {
    let output = julie_extract(&[
        "scan",
        "--root",
        path_str(root),
        "--db",
        path_str(path),
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(0), "{}", json_report(&output));
}

#[test]
fn scan_creates_a_missing_spool_dir_and_leaves_it_empty() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
    let spool_dir = temp.path().join("scratch").join("spools");

    let output = julie_extract(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&temp.path().join("artifact.sqlite")),
        "--spool-dir",
        path_str(&spool_dir),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(spool_dir.is_dir(), "a missing spool dir should be created");
    assert_eq!(
        std::fs::read_dir(&spool_dir).unwrap().count(),
        0,
        "a completed scan should leave no spool behind"
    );
}

#[test]
fn scan_spool_dir_pointing_at_a_regular_file_is_a_typed_error() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    let not_a_dir = temp.path().join("not-a-dir");
    std::fs::write(&not_a_dir, b"regular file").unwrap();

    let output = julie_extract(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&temp.path().join("artifact.sqlite")),
        "--spool-dir",
        path_str(&not_a_dir),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "invalid_path");
    assert_eq!(report["errors"][0]["path"], path_str(&not_a_dir));
}

#[test]
fn scan_progress_file_under_a_missing_directory_is_a_typed_error() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    let progress = temp.path().join("absent").join("scan.progress");

    let output = julie_extract(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&temp.path().join("artifact.sqlite")),
        "--progress-file",
        path_str(&progress),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "invalid_path");
    assert!(
        !temp.path().join("artifact.sqlite").exists(),
        "an unusable progress path must fail before any scan work runs"
    );
}

#[test]
fn scan_progress_file_pointing_at_an_unwritable_path_is_a_typed_error() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    let progress = temp.path().join("occupied.progress");
    std::fs::create_dir_all(&progress).unwrap();

    let output = julie_extract(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&temp.path().join("artifact.sqlite")),
        "--progress-file",
        path_str(&progress),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "invalid_path");
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn artifact_root(path: &Path) -> String {
    let conn = Connection::open(path).unwrap();
    conn.query_row(
        "SELECT value FROM artifact_metadata WHERE key = 'root_path'",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

fn file_count_for_path(path: &Path, relative_path: &str) -> i64 {
    let conn = Connection::open(path).unwrap();
    conn.query_row(
        "SELECT COUNT(*) FROM files WHERE path = ?1",
        [relative_path],
        |row| row.get(0),
    )
    .unwrap()
}

fn canonical(path: impl AsRef<Path>) -> String {
    julie_extract_cli::strip_verbatim_prefix(path.as_ref().canonicalize().unwrap())
        .display()
        .to_string()
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}
