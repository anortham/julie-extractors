use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use julie_extract_artifact::metadata::{RebindMetadata, apply_rebind};
use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

const PROVENANCE_KEYS: &[&str] = &[
    "rebound_from_root",
    "rebound_from_artifact_id",
    "rebound_at",
];
const RETARGETED_KEYS: &[&str] = &["root_path", "artifact_id", "updated_at"];

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

fn exit_code(output: &Output) -> Option<i32> {
    output.status.code()
}

struct Fixture {
    _temp: TempDir,
    temp_path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let temp_path = temp.path().to_path_buf();
        Self {
            _temp: temp,
            temp_path,
        }
    }

    fn tree(&self, name: &str) -> PathBuf {
        let root = self.temp_path.join(name);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/a.rs"),
            "/// Alpha docs\npub fn alpha() { let message = \"hello\"; }\npub fn helper() { alpha(); }\n",
        )
        .unwrap();
        std::fs::write(root.join("src/b.rs"), "pub fn beta() {}\n").unwrap();
        root
    }

    fn db(&self, name: &str) -> PathBuf {
        self.temp_path.join(name)
    }
}

fn scan(root: &Path, db: &Path) {
    let output = julie_extract(&["scan", "--root", str(root), "--db", str(db), "--json"]);
    assert_eq!(
        exit_code(&output),
        Some(0),
        "fixture scan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn rebind(db: &Path, root: &Path) -> Output {
    julie_extract(&["rebind", "--db", str(db), "--root", str(root), "--json"])
}

fn str(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn canonical(path: &Path) -> String {
    path.canonicalize().unwrap().display().to_string()
}

fn metadata(db: &Path) -> BTreeMap<String, String> {
    let connection = Connection::open(db).unwrap();
    let mut statement = connection
        .prepare("SELECT key, value FROM artifact_metadata ORDER BY key")
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap();
    rows.map(Result::unwrap).collect()
}

fn set_metadata(db: &Path, key: &str, value: &str) {
    let connection = Connection::open(db).unwrap();
    connection
        .execute(
            "INSERT INTO artifact_metadata (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )
        .unwrap();
}

fn table_count(db: &Path, table: &str) -> i64 {
    let connection = Connection::open(db).unwrap();
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn sampled_table_counts(db: &Path) -> BTreeMap<&'static str, i64> {
    ["files", "symbols", "identifiers", "extraction_revisions"]
        .into_iter()
        .map(|table| (table, table_count(db, table)))
        .collect()
}

fn drop_extraction_history(db: &Path) {
    let connection = Connection::open(db).unwrap();
    connection
        .execute_batch(
            "DELETE FROM identifier_resolutions; \
             DELETE FROM pending_resolutions; \
             DELETE FROM files; \
             DELETE FROM revision_file_changes; \
             DELETE FROM extraction_revisions;",
        )
        .unwrap();
}

/// The fixed-width `YYYY-MM-DDTHH:MM:SS` head of an RFC3339 UTC stamp, which
/// compares lexicographically. The optional fractional tail does not, because
/// the formatter drops trailing zeroes.
fn to_the_second(value: &str) -> &str {
    assert!(
        value.len() >= 20 && value.ends_with('Z') && value.as_bytes()[10] == b'T',
        "metadata timestamp {value} is not an RFC3339 UTC stamp"
    );
    &value[..19]
}

fn is_rebound_artifact_id(value: &str) -> bool {
    match value.strip_prefix("artifact-") {
        Some(hex) => {
            hex.len() == 32
                && hex
                    .chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        }
        None => false,
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn rebind_retargets_the_root_and_mints_a_new_identity() {
    let fixture = Fixture::new();
    let old_root = fixture.tree("checkout-a");
    let new_root = fixture.tree("checkout-b");
    let db = fixture.db("artifact.sqlite");
    scan(&old_root, &db);
    let before = metadata(&db);
    let before_counts = sampled_table_counts(&db);

    let output = rebind(&db, &new_root);

    assert_eq!(
        exit_code(&output),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["operation"], "rebind");
    assert_eq!(report["mode"], "metadata");
    assert_eq!(report["rebind"]["changed"], Value::Bool(true));
    assert_eq!(report["rebind"]["previous_root"], before["root_path"]);
    assert_eq!(report["rebind"]["new_root"], canonical(&new_root));
    assert_eq!(
        report["rebind"]["previous_artifact_id"],
        before["artifact_id"]
    );
    assert_ne!(
        report["rebind"]["new_artifact_id"],
        report["rebind"]["previous_artifact_id"]
    );

    let after = metadata(&db);
    assert_eq!(after["root_path"], canonical(&new_root));
    assert_eq!(
        after["artifact_id"],
        report["rebind"]["new_artifact_id"].as_str().unwrap()
    );
    assert!(
        is_rebound_artifact_id(&after["artifact_id"]),
        "rebound artifact id must be artifact-<32 lowercase hex>, got {}",
        after["artifact_id"]
    );
    assert_eq!(after["created_at"], before["created_at"]);
    assert_ne!(after["updated_at"], before["updated_at"]);
    assert!(to_the_second(&after["updated_at"]) >= to_the_second(&before["updated_at"]));
    assert_eq!(after["rebound_from_root"], before["root_path"]);
    assert_eq!(after["rebound_from_artifact_id"], before["artifact_id"]);
    assert_eq!(after["rebound_at"], after["updated_at"]);

    let untouched = |rows: &BTreeMap<String, String>| {
        rows.iter()
            .filter(|(key, _)| {
                !RETARGETED_KEYS.contains(&key.as_str()) && !PROVENANCE_KEYS.contains(&key.as_str())
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(untouched(&after), untouched(&before));
    assert_eq!(sampled_table_counts(&db), before_counts);
}

#[test]
fn rebind_to_the_recorded_root_changes_nothing() {
    let fixture = Fixture::new();
    let root = fixture.tree("checkout-a");
    let db = fixture.db("artifact.sqlite");
    scan(&root, &db);
    let before = metadata(&db);

    let output = rebind(&db, &root);

    assert_eq!(exit_code(&output), Some(0));
    let report = json_report(&output);
    assert_eq!(report["operation"], "rebind");
    assert_eq!(report["mode"], "metadata");
    assert_eq!(report["rebind"]["changed"], Value::Bool(false));
    assert_eq!(report["rebind"]["previous_root"], canonical(&root));
    assert_eq!(report["rebind"]["new_root"], canonical(&root));
    assert_eq!(
        report["rebind"]["previous_artifact_id"],
        report["rebind"]["new_artifact_id"]
    );
    assert_eq!(metadata(&db), before);
}

#[test]
fn rebind_refuses_an_artifact_built_by_different_capabilities() {
    let fixture = Fixture::new();
    let old_root = fixture.tree("checkout-a");
    let new_root = fixture.tree("checkout-b");
    let db = fixture.db("artifact.sqlite");
    scan(&old_root, &db);
    set_metadata(&db, "parser_inventory_fingerprint", "sha256:tampered");
    let before = metadata(&db);

    let output = rebind(&db, &new_root);

    assert_eq!(exit_code(&output), Some(3));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "fingerprint_mismatch");
    assert_eq!(metadata(&db), before);
}

#[test]
fn rebind_refuses_an_artifact_with_no_committed_revision() {
    let fixture = Fixture::new();
    let old_root = fixture.tree("checkout-a");
    let new_root = fixture.tree("checkout-b");
    let db = fixture.db("artifact.sqlite");
    scan(&old_root, &db);
    drop_extraction_history(&db);
    let before = metadata(&db);

    let output = rebind(&db, &new_root);

    assert_eq!(exit_code(&output), Some(3));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "no_committed_revision");
    assert_eq!(metadata(&db), before);
}

#[test]
fn rebind_gates_capabilities_before_extraction_history() {
    let fixture = Fixture::new();
    let old_root = fixture.tree("checkout-a");
    let new_root = fixture.tree("checkout-b");
    let db = fixture.db("artifact.sqlite");
    scan(&old_root, &db);
    set_metadata(&db, "capability_snapshot_fingerprint", "sha256:tampered");
    drop_extraction_history(&db);

    let output = rebind(&db, &new_root);

    assert_eq!(exit_code(&output), Some(3));
    assert_eq!(
        json_report(&output)["errors"][0]["code"],
        "fingerprint_mismatch"
    );
}

#[test]
fn rebind_runs_the_same_schema_gates_as_update() {
    let fixture = Fixture::new();
    let old_root = fixture.tree("checkout-a");
    let new_root = fixture.tree("checkout-b");
    let db = fixture.db("artifact.sqlite");
    scan(&old_root, &db);
    let current_schema_version = metadata(&db)["sqlite_schema_version"].clone();
    let older = current_schema_version.parse::<i64>().unwrap() - 1;
    set_metadata(&db, "sqlite_schema_version", &older.to_string());
    set_metadata(&db, "schema_version", &older.to_string());
    let before = metadata(&db);

    let strict = julie_extract(&[
        "rebind",
        "--db",
        str(&db),
        "--root",
        str(&new_root),
        "--strict-schema",
        "--json",
    ]);

    assert_eq!(exit_code(&strict), Some(3));
    let report = json_report(&strict);
    assert_eq!(report["operation"], "rebind");
    assert_eq!(report["errors"][0]["code"], "schema_migration_required");
    assert_eq!(metadata(&db), before);

    let newer = current_schema_version.parse::<i64>().unwrap() + 1;
    set_metadata(&db, "sqlite_schema_version", &newer.to_string());
    set_metadata(&db, "schema_version", &newer.to_string());
    let before = metadata(&db);

    let incompatible = rebind(&db, &new_root);

    assert_eq!(exit_code(&incompatible), Some(3));
    assert_eq!(
        json_report(&incompatible)["errors"][0]["code"],
        "schema_incompatible"
    );
    assert_eq!(metadata(&db), before);
}

#[test]
fn rebind_reports_a_missing_artifact_without_creating_one() {
    let fixture = Fixture::new();
    let new_root = fixture.tree("checkout-b");
    let db = fixture.db("absent.sqlite");

    let output = rebind(&db, &new_root);

    assert_eq!(exit_code(&output), Some(1));
    assert_eq!(json_report(&output)["errors"][0]["code"], "db_open_failed");
    assert!(!db.exists(), "a refused rebind must not create an artifact");
}

#[test]
fn rebind_without_a_root_is_a_usage_error() {
    let fixture = Fixture::new();
    let db = fixture.db("artifact.sqlite");

    let output = julie_extract(&["rebind", "--db", str(&db)]);

    assert_eq!(exit_code(&output), Some(2));
}

#[test]
fn a_scan_after_rebind_passes_the_root_gate_on_a_byte_identical_tree() {
    let fixture = Fixture::new();
    let old_root = fixture.tree("checkout-a");
    let new_root = fixture.temp_path.join("checkout-b");
    copy_tree(&old_root, &new_root);
    let db = fixture.db("artifact.sqlite");
    scan(&old_root, &db);
    assert_eq!(exit_code(&rebind(&db, &new_root)), Some(0));

    let output = julie_extract(&["scan", "--root", str(&new_root), "--db", str(&db), "--json"]);

    assert_eq!(
        exit_code(&output),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_report(&output);
    assert_eq!(report["status"], "no_change");
    assert_eq!(report["counts"]["files_changed"], 0);
}

#[test]
fn rebind_metadata_writes_are_confined_to_their_transaction() {
    let fixture = Fixture::new();
    let root = fixture.tree("checkout-a");
    let db = fixture.db("artifact.sqlite");
    scan(&root, &db);
    let before = metadata(&db);

    let connection = Connection::open(&db).unwrap();
    let transaction = connection.unchecked_transaction().unwrap();
    apply_rebind(
        &transaction,
        &RebindMetadata {
            previous_root: "/old/root",
            previous_artifact_id: "artifact-previous",
            new_root: "/new/root",
            new_artifact_id: "artifact-00000000000000000000000000000000",
            rebound_at: "2026-08-05T00:00:00Z",
        },
    )
    .unwrap();
    let staged: String = transaction
        .query_row(
            "SELECT value FROM artifact_metadata WHERE key = 'root_path'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        staged, "/new/root",
        "the helper must stage every contracted write inside the transaction"
    );
    transaction.rollback().unwrap();
    drop(connection);

    assert_eq!(
        metadata(&db),
        before,
        "an interrupted rebind must leave every metadata row byte-identical"
    );
}
