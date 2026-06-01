use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

#[test]
fn sqlite_consumer_prints_read_only_artifact_summary() {
    let fixture = PythonConsumerFixture::new();
    fixture.write_artifact(2, 3);

    let output = run_consumer(&fixture.db_path);

    assert!(
        output.status.success(),
        "consumer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = serde_json::from_slice::<Value>(&output.stdout).expect("json summary");
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["extract_contract_version"], 1);
    assert_eq!(json["root_path"], "/tmp/example");
    assert_eq!(json["tables"]["files"], 2);
    assert_eq!(json["tables"]["symbols"], 3);
}

#[test]
fn sqlite_consumer_rejects_missing_metadata_and_zero_files() {
    let missing_metadata = PythonConsumerFixture::new();
    missing_metadata.write_artifact(1, 1);
    missing_metadata.delete_metadata("schema_version");

    let output = run_consumer(&missing_metadata.db_path);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("missing metadata `schema_version`"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let zero_files = PythonConsumerFixture::new();
    zero_files.write_artifact(0, 1);

    let output = run_consumer(&zero_files.db_path);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("table `files` has zero rows"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_consumer(db_path: &Path) -> std::process::Output {
    Command::new("python3")
        .arg(repo_root().join("examples/python/sqlite_consumer.py"))
        .arg(db_path)
        .output()
        .expect("run sqlite consumer")
}

struct PythonConsumerFixture {
    _temp: TempDir,
    db_path: PathBuf,
}

impl PythonConsumerFixture {
    fn new() -> Self {
        let temp = TempDir::new().expect("tempdir");
        let db_path = temp.path().join("artifact.sqlite");
        Self {
            _temp: temp,
            db_path,
        }
    }

    fn write_artifact(&self, files: i64, symbols: i64) {
        let conn = Connection::open(&self.db_path).expect("open sqlite");
        conn.execute_batch(
            "
            CREATE TABLE artifact_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE files (file_id TEXT PRIMARY KEY);
            CREATE TABLE symbols (symbol_id TEXT PRIMARY KEY);
            CREATE TABLE identifiers (identifier_id TEXT PRIMARY KEY);
            CREATE TABLE relationships (relationship_id TEXT PRIMARY KEY);
            CREATE TABLE pending_relationships (pending_relationship_id TEXT PRIMARY KEY);
            ",
        )
        .expect("schema");
        for (key, value) in [
            ("schema_version", "1"),
            ("extract_contract_version", "1"),
            ("sqlite_schema_version", "1"),
            ("root_path", "/tmp/example"),
        ] {
            conn.execute(
                "INSERT INTO artifact_metadata (key, value) VALUES (?1, ?2)",
                (key, value),
            )
            .expect("metadata");
        }
        for index in 0..files {
            conn.execute(
                "INSERT INTO files (file_id) VALUES (?1)",
                [format!("file-{index}")],
            )
            .expect("file");
        }
        for index in 0..symbols {
            conn.execute(
                "INSERT INTO symbols (symbol_id) VALUES (?1)",
                [format!("symbol-{index}")],
            )
            .expect("symbol");
        }
    }

    fn delete_metadata(&self, key: &str) {
        let conn = Connection::open(&self.db_path).expect("open sqlite");
        conn.execute("DELETE FROM artifact_metadata WHERE key = ?1", [key])
            .expect("delete metadata");
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under repo root")
        .to_path_buf()
}
