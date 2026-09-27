use std::path::Path;
use std::process::Command;

use rusqlite::Connection;
use tempfile::TempDir;

fn scan(root: &Path, db: &Path, level: &str, force: bool) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
    command
        .arg("scan")
        .arg("--root")
        .arg(root)
        .arg("--db")
        .arg(db)
        .args(["--level", level, "--jobs", "1", "--json"]);
    if force {
        command.arg("--force");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn occurrence_ids(db: &Path) -> Vec<(String, String)> {
    let connection = Connection::open(db).unwrap();
    connection
        .prepare(
            "SELECT pending_relationship_id, reference_site_id FROM pending_relationships
             UNION ALL SELECT relationship_id, reference_site_id FROM relationships
             ORDER BY 1, 2",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn same_line_reference_occurrences_survive_mapping_at_each_level() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    for (path, source) in [
        (
            "same.rs",
            "fn local() {} fn caller() { local(); local(); external(); external(); }\n",
        ),
        (
            "same.ts",
            "function local() {} function caller() { local(); local(); external(); external(); }\n",
        ),
        (
            "same.py",
            "def local(): pass\ndef caller(): local(); local(); external(); external()\n",
        ),
        (
            "same.go",
            "package p\nfunc local() {}\nfunc caller() { local(); local(); external(); external() }\n",
        ),
    ] {
        std::fs::write(root.join(path), source).unwrap();
    }

    for level in ["symbols", "facts", "full"] {
        let db = temporary.path().join(format!("{level}.sqlite"));
        scan(&root, &db, level, false);
        let connection = Connection::open(&db).unwrap();
        for path in ["same.rs", "same.ts", "same.py", "same.go"] {
            let pending: i64 = connection
                .query_row(
                    "SELECT count(*) FROM pending_relationships p
                     JOIN files f ON f.file_id = p.file_id
                     WHERE f.path = ?1 AND p.target_terminal_name = 'external'",
                    [path],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(pending, 2, "{level}: unresolved calls in {path}");

            let resolved: i64 = connection
                .query_row(
                    "SELECT count(*) FROM relationships r
                     JOIN files f ON f.file_id = r.file_id
                     JOIN symbols s ON s.symbol_id = r.to_symbol_id
                     WHERE f.path = ?1 AND r.kind = 'calls' AND s.name = 'local'",
                    [path],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(resolved, 2, "{level}: resolved calls in {path}");
            let sites: i64 = connection
                .query_row(
                    "SELECT count(DISTINCT r.reference_site_id) FROM relationships r
                     JOIN files f ON f.file_id = r.file_id
                     JOIN symbols s ON s.symbol_id = r.to_symbol_id
                     WHERE f.path = ?1 AND r.kind = 'calls' AND s.name = 'local'",
                    [path],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(sites, 2, "{level}: resolved reference sites in {path}");
        }

        let inexact_with_positions: i64 = connection
            .query_row(
                "SELECT count(*) FROM reference_sites WHERE is_exact = 0
                 AND (start_byte IS NOT NULL OR end_byte IS NOT NULL OR start_column IS NOT NULL)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(inexact_with_positions, 0);

        if level == "full" {
            let joined: i64 = connection
                .query_row(
                    "SELECT count(*) FROM pending_relationships p
                     JOIN identifiers i ON i.reference_site_id = p.reference_site_id
                     JOIN files f ON f.file_id = p.file_id
                     WHERE f.path = 'same.py' AND p.target_terminal_name = 'external'
                     AND i.kind = 'call'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(joined, 2);
        }
        drop(connection);
        let before = occurrence_ids(&db);
        scan(&root, &db, level, true);
        assert_eq!(
            occurrence_ids(&db),
            before,
            "{level}: stable occurrence IDs"
        );
    }
}

#[test]
fn same_line_sql_table_references_keep_distinct_occurrences() {
    let temporary = TempDir::new().unwrap();
    let root = temporary.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("view.sql"),
        "CREATE VIEW v AS SELECT * FROM remote a JOIN remote b ON a.id=b.id;",
    )
    .unwrap();
    let db = temporary.path().join("artifact.sqlite");
    scan(&root, &db, "facts", false);
    let connection = Connection::open(&db).unwrap();
    let (rows, sites): (i64, i64) = connection
        .query_row(
            "SELECT count(*), count(DISTINCT reference_site_id)
             FROM pending_relationships WHERE target_terminal_name = 'remote'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((rows, sites), (2, 2));
}
