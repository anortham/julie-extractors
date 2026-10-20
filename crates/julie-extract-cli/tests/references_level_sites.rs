use std::process::Command;

use rusqlite::Connection;
use tempfile::TempDir;

const SOURCE: &str = r#"
def helper(value):
    return value

def run(items):
    total = 0
    for item in items:
        total = total + helper(item)
    return missing(total)
"#;

fn scan_at(level: &str) -> (TempDir, Connection) {
    let workspace = TempDir::new().expect("create fixture directory");
    let root = workspace.path().join("repo");
    std::fs::create_dir_all(&root).expect("create source root");
    std::fs::write(root.join("app.py"), SOURCE).expect("write fixture");
    let db = workspace.path().join("artifact.sqlite");
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "scan",
            "--root",
            root.to_str().unwrap(),
            "--db",
            db.to_str().unwrap(),
            "--level",
            level,
            "--jobs",
            "1",
            "--json",
        ])
        .output()
        .expect("run julie-extract scan");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let connection = Connection::open(&db).expect("open artifact");
    (workspace, connection)
}

fn count(connection: &Connection, sql: &str) -> i64 {
    connection.query_row(sql, [], |row| row.get(0)).unwrap()
}

#[test]
fn the_references_level_writes_a_site_only_for_an_identifier_a_relationship_shares() {
    let (_full_dir, full) = scan_at("full");
    let (_references_dir, references) = scan_at("references");
    let identifiers = "SELECT COUNT(*) FROM identifiers";
    assert_eq!(count(&full, identifiers), count(&references, identifiers));
    assert_eq!(
        count(
            &full,
            "SELECT COUNT(*) FROM identifiers WHERE reference_site_id IS NULL"
        ),
        0
    );

    let unshared = count(
        &references,
        "SELECT COUNT(*) FROM identifiers WHERE reference_site_id IS NULL",
    );
    assert!(unshared > 0, "the variable reads have no shared site");
    assert_eq!(
        count(
            &references,
            "SELECT COUNT(*) FROM identifiers i
             WHERE i.reference_site_id IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM relationships r
                               WHERE r.reference_site_id = i.reference_site_id)
               AND NOT EXISTS (SELECT 1 FROM pending_relationships p
                               WHERE p.reference_site_id = i.reference_site_id)"
        ),
        0
    );
    assert!(
        count(&references, "SELECT COUNT(*) FROM reference_sites")
            < count(&full, "SELECT COUNT(*) FROM reference_sites")
    );
    let violations = references
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |_| Ok(()))
        .unwrap()
        .count();
    assert_eq!(violations, 0, "every site an identifier names exists");
}
