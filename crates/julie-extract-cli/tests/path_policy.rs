use std::path::Path;
use std::process::{Command, Output};

use julie_extract_artifact::metadata::ArtifactMetadata;
use julie_extract_artifact::model::{
    ArtifactFile, ArtifactSymbol, FileStatus, RevisionInput, WriteMode, WriteOperation,
};
use julie_extract_artifact::writer::ArtifactWriter;
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
    create_artifact_with_file(&db, &root, "src/ignored.rs");

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
    std::fs::create_dir_all(&old_root).unwrap();
    std::fs::create_dir_all(new_root.join("src")).unwrap();
    std::fs::write(new_root.join("src/main.rs"), "fn main() {}\n").unwrap();
    let db = temp.path().join("artifact.sqlite");
    create_artifact_with_file(&db, &old_root, "src/old.rs");

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
    assert_eq!(file_count_for_path(&db, "src/old.rs"), 0);
}

fn create_artifact(path: &Path, root: &Path) {
    let _writer = ArtifactWriter::open_path(
        path,
        ArtifactMetadata {
            artifact_id: "artifact-path-policy-test".to_string(),
            root_path: canonical(root),
            binary_version: "julie-extract 0.1.0".to_string(),
            hash_algorithm: "blake3".to_string(),
            parser_inventory_fingerprint: "sha256:parser".to_string(),
            capability_snapshot_fingerprint: "sha256:cap".to_string(),
            created_at: "2026-05-31T21:00:00Z".to_string(),
            updated_at: "2026-05-31T21:00:00Z".to_string(),
        },
    )
    .unwrap();
}

fn create_artifact_with_file(path: &Path, root: &Path, relative_path: &str) {
    let mut writer = ArtifactWriter::open_path(path, metadata(root)).unwrap();
    writer
        .write_scan(
            revision(WriteOperation::Scan, Some(WriteMode::Incremental), root),
            &[file_with_symbol(relative_path)],
        )
        .unwrap();
}

fn metadata(root: &Path) -> ArtifactMetadata {
    ArtifactMetadata {
        artifact_id: "artifact-path-policy-test".to_string(),
        root_path: canonical(root),
        binary_version: "julie-extract 0.1.0".to_string(),
        hash_algorithm: "blake3".to_string(),
        parser_inventory_fingerprint: "sha256:parser".to_string(),
        capability_snapshot_fingerprint: "sha256:cap".to_string(),
        created_at: "2026-05-31T21:00:00Z".to_string(),
        updated_at: "2026-05-31T21:00:00Z".to_string(),
    }
}

fn revision(operation: WriteOperation, mode: Option<WriteMode>, root: &Path) -> RevisionInput {
    RevisionInput {
        operation,
        mode,
        started_at: "2026-05-31T21:00:00Z".to_string(),
        completed_at: "2026-05-31T21:00:01Z".to_string(),
        binary_version: "julie-extract 0.1.0".to_string(),
        input_root: Some(canonical(root)),
    }
}

fn file_with_symbol(path: &str) -> ArtifactFile {
    ArtifactFile {
        file_id: "file-stale".to_string(),
        path: path.to_string(),
        language: "rust".to_string(),
        content_hash: "blake3:stale".to_string(),
        content_bytes: 16,
        line_count: Some(1),
        indexed_at: "2026-05-31T21:00:00Z".to_string(),
        status: FileStatus::Indexed,
        metadata_json: None,
        symbols: vec![ArtifactSymbol {
            symbol_id: "file-stale-symbol".to_string(),
            name: "stale".to_string(),
            kind: "function".to_string(),
            signature: Some("fn stale()".to_string()),
            start_line: 1,
            end_line: 1,
            ..ArtifactSymbol::default()
        }],
        symbol_annotations: Vec::new(),
        identifiers: Vec::new(),
        relationships: Vec::new(),
        pending_relationships: Vec::new(),
        type_facts: Vec::new(),
        type_argument_usages: Vec::new(),
        type_arguments: Vec::new(),
        literals: Vec::new(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        parse_diagnostics: Vec::new(),
    }
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
    path.as_ref().canonicalize().unwrap().display().to_string()
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}
