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
fn scan_creates_sqlite_artifact_with_expected_rows() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["operation"], "scan");
    assert_eq!(report["mode"], "incremental");
    assert_eq!(report["counts"]["files_scanned"], 2);
    assert_eq!(report["counts"]["files_changed"], 2);
    assert_eq!(report["counts"]["rows_written"]["files"], 2);
    assert!(
        report["counts"]["rows_written"]["symbols"]
            .as_i64()
            .unwrap()
            >= 2
    );
    let parser_inventory = table_count(&db, "parser_inventory");
    let language_capabilities = table_count(&db, "language_capabilities");
    let language_capability_fixtures = table_count(&db, "language_capability_fixtures");
    let language_capability_gaps = table_count(&db, "language_capability_gaps");
    assert!(
        parser_inventory > 0,
        "scan artifacts must persist parser inventory rows"
    );
    assert_eq!(
        parser_inventory, language_capabilities,
        "parser inventory and language capability rows must cover the same language snapshot"
    );
    assert!(
        language_capability_fixtures >= language_capabilities,
        "capability fixture evidence should be persisted with the language snapshot"
    );
    assert!(
        language_capability_gaps > 0,
        "known capability gaps should be persisted instead of hidden"
    );
    assert_eq!(
        report["counts"]["rows_written"]["parser_inventory"],
        parser_inventory
    );
    assert_eq!(
        report["counts"]["rows_written"]["language_capabilities"],
        language_capabilities
    );
    assert_eq!(
        report["counts"]["rows_written"]["language_capability_fixtures"],
        language_capability_fixtures
    );
    assert_eq!(
        report["counts"]["rows_written"]["language_capability_gaps"],
        language_capability_gaps
    );
    assert_eq!(
        report["counts"]["totals"]["parser_inventory"],
        parser_inventory
    );
    assert_eq!(
        report["counts"]["totals"]["language_capabilities"],
        language_capabilities
    );
    assert_eq!(
        report["counts"]["totals"]["language_capability_fixtures"],
        language_capability_fixtures
    );
    assert_eq!(
        report["counts"]["totals"]["language_capability_gaps"],
        language_capability_gaps
    );
    assert_eq!(report["revision"]["created_revision_id"], 1);

    assert_eq!(table_count(&db, "files"), 2);
    assert!(table_count(&db, "symbols") >= 2);
    assert_eq!(symbols_for_path(&db, "src/a.rs"), vec!["alpha", "helper"]);
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);
}

#[test]
fn scan_with_no_changes_returns_no_change_without_new_revision() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));
    let before = artifact_fingerprint(&db);

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "no_change");
    assert_eq!(report["revision"]["created_revision_id"], Value::Null);
    assert_eq!(table_count(&db, "extraction_revisions"), 1);
    assert_eq!(artifact_fingerprint(&db), before);
}

#[test]
fn scan_deletes_rows_for_source_files_missing_from_the_snapshot() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));
    std::fs::remove_file(fixture.root.join("src/a.rs")).unwrap();

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["operation"], "scan");
    assert_eq!(report["counts"]["files_deleted"], 1);
    assert_eq!(report["counts"]["files_unchanged"], 1);
    assert_eq!(symbols_for_path(&db, "src/a.rs"), Vec::<String>::new());
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);
}

#[test]
fn scan_deduplicates_duplicate_extractor_identifiers_before_writing() {
    let fixture = FixtureRoot::with_file(
        "src/lib.rs",
        r#"fn f(args: &[String]) {
    let x = args.iter().map(|arg| arg.as_str()).collect::<Vec<_>>();
}
"#,
    );
    let db = fixture.path("artifact.sqlite");

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_success(output);
    assert!(table_count(&db, "identifiers") > 0);
    assert_eq!(
        table_count(&db, "identifiers"),
        distinct_count(&db, "identifiers", "identifier_id"),
        "artifact identifiers must have unique IDs after CLI normalization"
    );
}

#[test]
fn scan_skips_relationships_with_missing_symbol_endpoints_before_writing() {
    let fixture = FixtureRoot::with_file(
        "src/page.razor",
        r#"@page "/example"
@using OtherProject.Models

<h3>Example Page</h3>
<p>@LocalHelper()</p>

@code {
    private int LocalHelper() { return 42; }

    private void Entry() {
        var item = new ItemFromOther();
        _ = LocalHelper();
    }
}
"#,
    );
    let db = fixture.path("artifact.sqlite");

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_success(output);
    assert!(table_count(&db, "symbols") > 0);
}

#[test]
fn force_scan_rebuilds_and_reports_force_mode() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["operation"], "scan");
    assert_eq!(report["mode"], "force");
    assert_eq!(report["counts"]["files_changed"], 2);
    assert_eq!(table_count(&db, "files"), 2);
    assert_eq!(table_count(&db, "extraction_revisions"), 2);
}

#[test]
fn update_changes_one_file_and_preserves_other_files() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));
    std::fs::write(
        fixture.root.join("src/a.rs"),
        "pub fn alpha_v2() {}\npub fn helper() { alpha_v2(); }\n",
    )
    .unwrap();

    let output = julie_extract(&[
        "update",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--file",
        "src/a.rs",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["operation"], "update");
    assert_eq!(report["mode"], "single_file");
    assert_eq!(report["counts"]["files_changed"], 1);
    assert_eq!(
        report["input"]["file_path"],
        canonical_string(&fixture.root.join("src/a.rs"))
    );
    assert_eq!(report["input"]["root_relative_path"], "src/a.rs");
    assert_eq!(
        symbols_for_path(&db, "src/a.rs"),
        vec!["alpha_v2", "helper"]
    );
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);
}

#[test]
fn delete_removes_one_file_and_missing_rows_return_not_found() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));

    let deleted = julie_extract(&[
        "delete",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--file",
        "src/a.rs",
        "--json",
    ]);
    assert_eq!(deleted.status.code(), Some(0));
    let report = json_report(&deleted);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["mode"], "single_file");
    assert_eq!(
        report["input"]["file_path"],
        canonical_string(&fixture.root.join("src/a.rs"))
    );
    assert_eq!(report["input"]["root_relative_path"], "src/a.rs");
    assert_eq!(report["counts"]["files_deleted"], 1);
    assert_eq!(symbols_for_path(&db, "src/a.rs"), Vec::<String>::new());
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);

    let missing = julie_extract(&[
        "delete",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--file",
        "src/a.rs",
        "--json",
    ]);
    assert_eq!(missing.status.code(), Some(0));
    let report = json_report(&missing);
    assert_eq!(report["status"], "not_found");
}

#[test]
fn info_is_read_only_for_artifact_metadata_and_revisions() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));
    let before = artifact_fingerprint(&db);

    let output = julie_extract(&["info", "--db", path_str(&db), "--json"]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["operation"], "info");
    assert_eq!(report["counts"]["totals"]["files"], 2);
    assert_eq!(report["counts"]["totals"]["symbols"], 3);
    assert_eq!(artifact_fingerprint(&db), before);
}

#[test]
fn export_jsonl_emits_valid_jsonl_records_from_scanned_artifact() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    let out = fixture.path("artifact.jsonl");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));

    let output = julie_extract(&[
        "export",
        "--db",
        path_str(&db),
        "--format",
        "jsonl",
        "--out",
        path_str(&out),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["operation"], "export");
    assert_eq!(report["mode"], "jsonl");
    assert_eq!(report["artifact"]["jsonl_schema_version"], 1);
    assert_eq!(report["counts"]["rows_written"]["files"], 2);
    assert_eq!(report["counts"]["rows_written"]["symbols"], 3);
    let records = std::fs::read_to_string(&out).unwrap();
    let parsed = records
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(parsed[0]["kind"], "artifact");
    assert_eq!(parsed[0]["op"], "snapshot");
    assert_eq!(parsed[0]["jsonl_schema_version"], 1);
    assert!(
        parsed
            .iter()
            .any(|record| record["kind"] == "parser_inventory")
    );
    assert!(
        parsed
            .iter()
            .any(|record| record["kind"] == "language_capability")
    );
    assert!(
        parsed
            .iter()
            .any(|record| record["kind"] == "language_capability_fixture")
    );
    assert!(
        parsed
            .iter()
            .any(|record| record["kind"] == "language_capability_gap")
    );
    assert!(parsed.iter().any(|record| record["kind"] == "file"));
    assert!(parsed.iter().any(|record| record["kind"] == "symbol"));
}

#[test]
fn languages_json_emits_capability_snapshot_data() {
    let output = julie_extract(&["languages", "--json"]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["operation"], "languages");
    assert!(report["languages"]["total"].as_i64().unwrap() > 0);
    assert!(
        report["languages"]["languages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|language| language["language"] == "rust")
    );
}

struct FixtureRoot {
    _temp: TempDir,
    root: std::path::PathBuf,
}

impl FixtureRoot {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/a.rs"),
            "pub fn alpha() {}\npub fn helper() { alpha(); }\n",
        )
        .unwrap();
        std::fs::write(root.join("src/b.rs"), "pub fn beta() {}\n").unwrap();
        Self { _temp: temp, root }
    }

    fn with_file(relative: &str, contents: &str) -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("repo");
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
        Self { _temp: temp, root }
    }

    fn root_str(&self) -> &str {
        path_str(&self.root)
    }

    fn path(&self, relative: &str) -> std::path::PathBuf {
        self.root.join(relative)
    }
}

fn assert_success(output: Output) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn table_count(db: &Path, table: &str) -> i64 {
    let conn = Connection::open(db).unwrap();
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

fn distinct_count(db: &Path, table: &str, column: &str) -> i64 {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        &format!("SELECT COUNT(DISTINCT {column}) FROM {table}"),
        [],
        |row| row.get(0),
    )
    .unwrap()
}

fn symbols_for_path(db: &Path, path: &str) -> Vec<String> {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM symbols WHERE path = ?1 ORDER BY name")
        .unwrap();
    stmt.query_map([path], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn artifact_fingerprint(db: &Path) -> Vec<(String, String)> {
    let conn = Connection::open(db).unwrap();
    let mut rows = Vec::new();
    let mut stmt = conn
        .prepare("SELECT key, value FROM artifact_metadata ORDER BY key")
        .unwrap();
    rows.extend(
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
    );
    for table in [
        "artifact_metadata",
        "parser_inventory",
        "language_capabilities",
        "language_capability_fixtures",
        "language_capability_gaps",
        "extraction_revisions",
        "revision_file_changes",
        "files",
        "symbols",
        "symbol_annotations",
        "identifiers",
        "relationships",
        "pending_relationships",
        "type_facts",
        "type_argument_usages",
        "type_arguments",
        "literals",
        "parse_diagnostics",
    ] {
        rows.push((format!("table:{table}"), table_count(db, table).to_string()));
    }
    rows
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn canonical_string(path: &Path) -> String {
    path.canonicalize().unwrap().display().to_string()
}
