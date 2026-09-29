use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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

fn assert_exit(output: &Output, expected: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn fixture() -> (TempDir, PathBuf, PathBuf) {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("source");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn original() {}\n").unwrap();
    let db = temp.path().join("artifact.sqlite");
    assert_exit(
        &run(&[
            "scan",
            "--root",
            path_str(&root),
            "--db",
            path_str(&db),
            "--json",
        ]),
        0,
    );
    (temp, root, db)
}

fn metadata(db: &Path) -> BTreeMap<String, String> {
    Connection::open(db)
        .unwrap()
        .prepare("SELECT key, value FROM artifact_metadata ORDER BY key")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn stale(db: &Path, key: &str) {
    Connection::open(db)
        .unwrap()
        .execute(
            "UPDATE artifact_metadata SET value = 'previous-producer' WHERE key = ?1",
            params![key],
        )
        .unwrap();
}

fn snapshot(db: &Path) -> BTreeMap<String, Vec<String>> {
    let connection = Connection::open(db).unwrap();
    let tables = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let mut query = connection
                .prepare(&format!("SELECT * FROM \"{}\"", table.replace('"', "\"\"")))
                .unwrap();
            let columns = query.column_count();
            let mut rows = query
                .query_map([], |row| {
                    let values = (0..columns)
                        .map(|column| row.get::<_, rusqlite::types::Value>(column))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(format!("{values:?}"))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            rows.sort();
            (table, rows)
        })
        .collect()
}

#[test]
fn every_producer_field_gates_all_incremental_verbs_but_preserves_reads() {
    for key in [
        "binary_version",
        "parser_inventory_fingerprint",
        "capability_snapshot_fingerprint",
    ] {
        let (temp, root, db) = fixture();
        let moved = temp.path().join("moved");
        std::fs::create_dir(&moved).unwrap();
        let file = root.join("lib.rs");
        std::fs::write(&file, "pub fn changed() {}\n").unwrap();
        stale(&db, key);
        let before = snapshot(&db);
        for verb in ["scan", "update", "delete", "rebind"] {
            let target_root = if verb == "rebind" { &moved } else { &root };
            let mut args = vec![
                verb,
                "--root",
                path_str(target_root),
                "--db",
                path_str(&db),
                "--json",
            ];
            if matches!(verb, "update" | "delete") {
                args.extend(["--file", path_str(&file)]);
            }
            let report = assert_exit(&run(&args), 3);
            assert_eq!(
                report["errors"][0]["code"], "fingerprint_mismatch",
                "{key}: {verb}"
            );
            assert_eq!(
                report["errors"][0]["details"]["action"],
                "julie-extract scan --force"
            );
            assert_eq!(snapshot(&db), before, "{key}: {verb}");
        }
        assert_exit(&run(&["info", "--db", path_str(&db), "--json"]), 0);
        assert_eq!(snapshot(&db), before);
    }
}

#[test]
fn force_reextracts_unchanged_files_and_removes_deleted_files_across_generations() {
    let (_temp, root, db) = fixture();
    let removed = root.join("removed.rs");
    std::fs::write(&removed, "pub fn removed() {}\n").unwrap();
    assert_exit(
        &run(&[
            "scan",
            "--root",
            path_str(&root),
            "--db",
            path_str(&db),
            "--json",
        ]),
        0,
    );
    let current = metadata(&db);
    Connection::open(&db)
        .unwrap()
        .execute(
            "UPDATE symbols SET name = 'stale_symbol' WHERE name = 'original'",
            [],
        )
        .unwrap();
    std::fs::remove_file(removed).unwrap();
    stale(&db, "capability_snapshot_fingerprint");

    assert_exit(
        &run(&[
            "scan",
            "--root",
            path_str(&root),
            "--db",
            path_str(&db),
            "--force",
            "--json",
        ]),
        0,
    );

    let after = metadata(&db);
    for key in [
        "artifact_id",
        "binary_version",
        "parser_inventory_fingerprint",
        "capability_snapshot_fingerprint",
    ] {
        assert_eq!(after[key], current[key], "{key}");
    }
    let connection = Connection::open(&db).unwrap();
    let names = connection
        .prepare("SELECT name FROM symbols ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(names, ["original"]);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM files", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(connection);
    assert_exit(
        &run(&[
            "scan",
            "--root",
            path_str(&root),
            "--db",
            path_str(&db),
            "--json",
        ]),
        0,
    );
}

#[test]
fn empty_force_scan_adopts_the_current_generation() {
    let (_temp, root, db) = fixture();
    let current = metadata(&db);
    std::fs::remove_file(root.join("lib.rs")).unwrap();
    for _ in 0..2 {
        stale(&db, "binary_version");
        assert_exit(
            &run(&[
                "scan",
                "--root",
                path_str(&root),
                "--db",
                path_str(&db),
                "--force",
                "--json",
            ]),
            0,
        );
        let after = metadata(&db);
        assert_eq!(after["binary_version"], current["binary_version"]);
        assert_eq!(after["artifact_id"], current["artifact_id"]);
        let connection = Connection::open(&db).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM files", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn failed_force_metadata_write_rolls_back_every_fact_and_producer_value() {
    let (_temp, root, db) = fixture();
    stale(&db, "binary_version");
    Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TRIGGER reject_producer_change BEFORE UPDATE ON artifact_metadata WHEN NEW.key = 'binary_version' BEGIN SELECT RAISE(ABORT, 'producer write rejected'); END;")
        .unwrap();
    let before = snapshot(&db);
    std::fs::remove_file(root.join("lib.rs")).unwrap();
    std::fs::write(root.join("added.rs"), "pub fn added() {}\n").unwrap();

    let output = run(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]);

    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("producer write rejected"),
        "{report}"
    );
    assert_eq!(snapshot(&db), before);
}

#[test]
fn different_root_force_metadata_write_failure_preserves_existing_artifact() {
    let (temp, _old_root, db) = fixture();
    let new_root = temp.path().join("replacement-source");
    std::fs::create_dir(&new_root).unwrap();
    std::fs::write(new_root.join("replacement.rs"), "pub fn replacement() {}\n").unwrap();
    Connection::open(&db)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_force_metadata_insert BEFORE INSERT ON artifact_metadata BEGIN SELECT RAISE(ABORT, 'force metadata insert rejected'); END;
             CREATE TRIGGER reject_force_metadata_update BEFORE UPDATE ON artifact_metadata BEGIN SELECT RAISE(ABORT, 'force metadata update rejected'); END;",
        )
        .unwrap();
    let before = snapshot(&db);

    let output = run(&[
        "scan",
        "--root",
        path_str(&new_root),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]);

    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("force metadata"),
        "{report}"
    );
    assert_eq!(snapshot(&db), before);
}

#[test]
fn different_root_force_with_source_error_after_generation_change_preserves_artifact() {
    let (temp, _old_root, db) = fixture();
    stale(&db, "binary_version");
    let new_root = temp.path().join("replacement-source");
    std::fs::create_dir(&new_root).unwrap();
    std::fs::write(new_root.join("broken.rs"), [0xff]).unwrap();
    let before = snapshot(&db);

    let output = run(&[
        "scan",
        "--root",
        path_str(&new_root),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(3));
    assert_eq!(snapshot(&db), before);
}

#[cfg(unix)]
#[test]
fn parent_exit_during_different_root_force_preserves_existing_artifact() {
    let (temp, _old_root, db) = fixture();
    let new_root = temp.path().join("replacement-source");
    std::fs::create_dir(&new_root).unwrap();
    let before = snapshot(&db);

    let output = run(&[
        "scan",
        "--root",
        path_str(&new_root),
        "--db",
        path_str(&db),
        "--force",
        "--parent-pid",
        "4294967295",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["errors"][0]["code"], "parent_exited");
    assert_eq!(snapshot(&db), before);
}
