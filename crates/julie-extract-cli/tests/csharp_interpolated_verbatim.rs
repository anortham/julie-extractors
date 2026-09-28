use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;

const FIXTURE: &str =
    include_str!("../../../fixtures/extraction/csharp/interpolated_verbatim/source.cs");

#[test]
fn cli_artifact_keeps_interpolated_verbatim_methods_and_references() {
    let temp = TempDir::new().expect("temporary directory");
    let root = temp.path().join("repo");
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    std::fs::write(root.join("src/interpolated.cs"), FIXTURE).expect("write C# fixture");
    std::fs::write(
        root.join("src/malformed.cs"),
        r#"
public sealed class Malformed
{
    public string Broken(string value) => @$"prefix ""{value}"";
}
"#,
    )
    .expect("write malformed C# control");
    let db = temp.path().join("artifact.sqlite");

    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "scan",
            "--root",
            root.to_str().expect("root path is UTF-8"),
            "--db",
            db.to_str().expect("database path is UTF-8"),
            "--json",
        ])
        .output()
        .expect("run julie-extract scan");
    assert_eq!(
        output.status.code(),
        Some(0),
        "scan should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON scan report");
    assert_eq!(report["status"], "ok", "scan report: {report}");
    assert_eq!(report["counts"]["files_failed"], 0, "scan report: {report}");

    let connection = Connection::open(&db).expect("open SQLite artifact");
    let diagnostics: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM parse_diagnostics WHERE path = 'src/interpolated.cs'",
            [],
            |row| row.get(0),
        )
        .expect("query C# parser diagnostics");
    assert_eq!(diagnostics, 0, "valid C# must have no parser diagnostics");
    let malformed_diagnostics: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM parse_diagnostics WHERE path = 'src/malformed.cs'",
            [],
            |row| row.get(0),
        )
        .expect("query malformed-control diagnostics");
    assert!(
        malformed_diagnostics > 0,
        "malformed C# must remain diagnostic rather than being accepted"
    );

    let methods = {
        let mut statement = connection
            .prepare(
                "SELECT name FROM symbols
                 WHERE path = 'src/interpolated.cs' AND kind = 'method'
                 ORDER BY start_line",
            )
            .expect("prepare method query");
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query method symbols")
            .collect::<Result<Vec<_>, _>>()
            .expect("read method symbols")
    };
    assert_eq!(
        methods,
        ["DollarAt", "AtDollar", "SubsequentMethod"],
        "both interpolated-verbatim forms and the method after them must be extracted"
    );

    let interpolation_references = {
        let mut statement = connection
            .prepare(
                "SELECT method.name, identifier.name, identifier.kind
                 FROM identifiers AS identifier
                 JOIN symbols AS method
                   ON method.symbol_id = identifier.containing_symbol_id
                 WHERE identifier.path = 'src/interpolated.cs'
                   AND identifier.name = 'value'
                   AND identifier.kind = 'variable_ref'
                 ORDER BY identifier.start_line",
            )
            .expect("prepare interpolation reference query");
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .expect("query interpolation reference facts")
            .collect::<Result<Vec<_>, _>>()
            .expect("read interpolation reference facts")
    };
    assert_eq!(
        interpolation_references,
        [
            (
                "DollarAt".to_owned(),
                "value".to_owned(),
                "variable_ref".to_owned()
            ),
            (
                "AtDollar".to_owned(),
                "value".to_owned(),
                "variable_ref".to_owned()
            ),
        ],
        "each interpolation expression must be published as an artifact reference fact"
    );
}
