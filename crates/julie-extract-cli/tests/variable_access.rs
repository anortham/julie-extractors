use std::process::Command;

use rusqlite::Connection;
use tempfile::TempDir;

#[test]
fn references_artifacts_preserve_syntactic_variable_access_facts() {
    let workspace = TempDir::new().unwrap();
    let root = workspace.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("app.js"),
        "function update(input) { target = input; total += input; box.field = input; consume(input); }",
    )
    .unwrap();
    let db = workspace.path().join("artifact.sqlite");
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(["scan", "--root"])
        .arg(&root)
        .arg("--db")
        .arg(&db)
        .args(["--level", "references", "--jobs", "1", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let connection = Connection::open(&db).unwrap();
    let mut statement = connection
        .prepare("SELECT name, kind, json_extract(metadata_json, '$.access') FROM identifiers ORDER BY start_byte")
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for (name, kind, access) in [
        ("target", "variable_ref", "write"),
        ("total", "variable_ref", "read_write"),
        ("box", "variable_ref", "read"),
        ("field", "member_access", "write"),
        ("input", "variable_ref", "read"),
    ] {
        assert!(
            rows.iter()
                .any(|row| { row.0 == name && row.1 == kind && row.2.as_deref() == Some(access) }),
            "missing {name}/{kind}/{access}: {rows:?}"
        );
    }
    assert!(rows.iter().any(|row| row.0 == "consume" && row.1 == "call"));
    assert!(
        rows.iter()
            .filter(|row| row.1 == "call" || row.1 == "type_usage")
            .all(|row| row.2.is_none())
    );
}
