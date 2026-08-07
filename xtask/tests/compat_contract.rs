use std::path::PathBuf;

use rusqlite::Connection;
use xtask::compat::{
    ArtifactDiff, ArtifactDump, CompatError, CompatOutcome, CompatPlan, DEFAULT_MAX_DIFF_ROWS,
    LedgerEntry, LineDifference, TableDifference, TableDump, decide,
    declared_change_for_current_build, default_fixture, diff_dumps, dump_connection,
    find_ledger_entry, plan_from_args,
};

fn dump(tables: &[(&str, &str)]) -> ArtifactDump {
    ArtifactDump {
        tables: tables
            .iter()
            .map(|(table, text)| TableDump {
                table: (*table).to_string(),
                text: (*text).to_string(),
            })
            .collect(),
    }
}

fn rows_differ(table: &str) -> ArtifactDiff {
    ArtifactDiff {
        differences: vec![TableDifference::RowsDiffer {
            table: table.to_string(),
            previous_lines: 1,
            current_lines: 1,
            first_differences: Vec::new(),
        }],
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn database(schema: &str) -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory database");
    conn.execute_batch(schema).expect("schema should apply");
    conn
}

#[test]
fn identical_dumps_pass_regardless_of_declaration() {
    assert_eq!(decide(&ArtifactDiff::default(), false), CompatOutcome::Pass);
    assert_eq!(decide(&ArtifactDiff::default(), true), CompatOutcome::Pass);
}

#[test]
fn declared_difference_is_a_notice() {
    assert_eq!(decide(&rows_differ("symbols"), true), CompatOutcome::Notice);
}

#[test]
fn undeclared_difference_fails() {
    assert_eq!(decide(&rows_differ("symbols"), false), CompatOutcome::Fail);
}

#[test]
fn pass_and_notice_exit_zero_and_fail_exits_one() {
    assert_eq!(CompatOutcome::Pass.exit_code(), 0);
    assert_eq!(CompatOutcome::Notice.exit_code(), 0);
    assert_eq!(CompatOutcome::Fail.exit_code(), 1);
}

#[test]
fn matching_dumps_produce_no_differences() {
    let previous = dump(&[("files", "a\nb\n"), ("symbols", "c\n")]);
    let current = dump(&[("files", "a\nb\n"), ("symbols", "c\n")]);
    assert!(diff_dumps(&previous, &current, 5).is_identical());
}

#[test]
fn differing_rows_are_reported_with_line_numbers() {
    let previous = dump(&[("symbols", "header\nrow-one\nrow-two\n")]);
    let current = dump(&[("symbols", "header\nrow-one\nrow-changed\n")]);

    let diff = diff_dumps(&previous, &current, 5);

    assert_eq!(
        diff.differences,
        vec![TableDifference::RowsDiffer {
            table: "symbols".to_string(),
            previous_lines: 3,
            current_lines: 3,
            first_differences: vec![LineDifference {
                line: 3,
                previous: Some("row-two".to_string()),
                current: Some("row-changed".to_string()),
            }],
        }]
    );
}

#[test]
fn reported_differences_are_capped() {
    let previous = dump(&[("symbols", "a\nb\nc\nd\n")]);
    let current = dump(&[("symbols", "w\nx\ny\nz\n")]);

    let diff = diff_dumps(&previous, &current, 2);

    let TableDifference::RowsDiffer {
        first_differences, ..
    } = &diff.differences[0]
    else {
        panic!("expected a row difference");
    };
    assert_eq!(first_differences.len(), 2);
}

#[test]
fn a_row_present_only_in_one_dump_is_reported() {
    let previous = dump(&[("symbols", "a\n")]);
    let current = dump(&[("symbols", "a\nb\n")]);

    let diff = diff_dumps(&previous, &current, 5);

    assert_eq!(
        diff.differences,
        vec![TableDifference::RowsDiffer {
            table: "symbols".to_string(),
            previous_lines: 1,
            current_lines: 2,
            first_differences: vec![LineDifference {
                line: 2,
                previous: None,
                current: Some("b".to_string()),
            }],
        }]
    );
}

#[test]
fn a_table_dropped_by_the_current_build_is_a_difference() {
    let previous = dump(&[("legacy", "a\n"), ("symbols", "b\n")]);
    let current = dump(&[("symbols", "b\n")]);

    let diff = diff_dumps(&previous, &current, 5);

    assert_eq!(
        diff.differences,
        vec![TableDifference::OnlyInPrevious {
            table: "legacy".to_string()
        }]
    );
}

#[test]
fn a_table_added_by_the_current_build_is_a_difference() {
    let previous = dump(&[("symbols", "b\n")]);
    let current = dump(&[("store_epochs", "a\n"), ("symbols", "b\n")]);

    let diff = diff_dumps(&previous, &current, 5);

    assert_eq!(
        diff.differences,
        vec![TableDifference::OnlyInCurrent {
            table: "store_epochs".to_string()
        }]
    );
}

#[test]
fn a_declared_version_is_found_by_its_heading() {
    let markdown = "# Ledger\n\n## 2.30.0\n\nclassification: compatible\n\nprose\n";
    assert_eq!(
        find_ledger_entry(markdown, "2.30.0"),
        Some(LedgerEntry {
            version: "2.30.0".to_string(),
            classification: Some("compatible".to_string()),
        })
    );
}

#[test]
fn a_heading_with_trailing_prose_still_declares_the_version() {
    let markdown = "## 2.30.0 — metadata_json key ordering\n\nclassification: incompatible\n";
    assert_eq!(
        find_ledger_entry(markdown, "2.30.0"),
        Some(LedgerEntry {
            version: "2.30.0".to_string(),
            classification: Some("incompatible".to_string()),
        })
    );
}

#[test]
fn a_missing_classification_line_still_declares_the_version() {
    let markdown = "## 2.30.0\n\nprose only\n";
    assert_eq!(
        find_ledger_entry(markdown, "2.30.0"),
        Some(LedgerEntry {
            version: "2.30.0".to_string(),
            classification: None,
        })
    );
}

#[test]
fn a_later_sections_classification_does_not_leak_backwards() {
    let markdown = "## 2.30.0\n\nprose only\n\n## 2.31.0\n\nclassification: incompatible\n";
    assert_eq!(
        find_ledger_entry(markdown, "2.30.0"),
        Some(LedgerEntry {
            version: "2.30.0".to_string(),
            classification: None,
        })
    );
}

#[test]
fn an_undeclared_version_is_not_found() {
    let markdown = "## 2.29.0\n\nclassification: compatible\n";
    assert_eq!(find_ledger_entry(markdown, "2.30.0"), None);
}

#[test]
fn a_heading_inside_a_code_fence_does_not_declare_a_version() {
    let markdown = "# Ledger\n\n```md\n## 2.30.0\n\nclassification: compatible\n```\n";
    assert_eq!(find_ledger_entry(markdown, "2.30.0"), None);
}

#[test]
fn a_v_prefixed_heading_declares_the_same_version() {
    let markdown = "## v2.30.0\n\nclassification: compatible\n";
    assert!(find_ledger_entry(markdown, "2.30.0").is_some());
}

#[test]
fn the_shipped_ledger_declares_nothing_for_the_current_version() {
    assert_eq!(
        declared_change_for_current_build().expect("ledger should be readable"),
        None
    );
}

#[test]
fn a_plan_without_a_previous_binary_is_a_usage_error() {
    let error = plan_from_args(&args(&[])).expect_err("expected a usage error");
    assert_eq!(error.exit_code(), 2);
    assert!(matches!(error, CompatError::Usage(_)));
}

#[test]
fn an_unknown_flag_is_a_usage_error() {
    let error = plan_from_args(&args(&["--previous-binary", "prev", "--declared-ok"]))
        .expect_err("expected a usage error");
    assert!(matches!(error, CompatError::Usage(_)));
}

#[test]
fn a_flag_without_a_value_is_a_usage_error() {
    let error = plan_from_args(&args(&["--previous-binary"])).expect_err("expected a usage error");
    assert!(matches!(error, CompatError::Usage(_)));
}

#[test]
fn a_plan_defaults_the_fixture_and_the_diff_cap() {
    let plan = plan_from_args(&args(&["--previous-binary", "prev"])).expect("plan should parse");
    assert_eq!(plan.previous_binary, PathBuf::from("prev"));
    assert_eq!(plan.current_binary, None);
    assert_eq!(plan.fixture, default_fixture());
    assert_eq!(plan.out_dir, None);
    assert_eq!(plan.max_diff_rows, DEFAULT_MAX_DIFF_ROWS);
}

#[test]
fn a_plan_reads_every_flag() {
    let plan = plan_from_args(&args(&[
        "--previous-binary",
        "prev",
        "--current-binary",
        "curr",
        "--fixture",
        "fix",
        "--out-dir",
        "out",
        "--max-diff-rows",
        "3",
    ]))
    .expect("plan should parse");
    assert_eq!(
        plan,
        CompatPlan {
            previous_binary: PathBuf::from("prev"),
            current_binary: Some(PathBuf::from("curr")),
            fixture: PathBuf::from("fix"),
            out_dir: Some(PathBuf::from("out")),
            max_diff_rows: 3,
        }
    );
}

#[test]
fn the_default_fixture_exists() {
    assert!(default_fixture().is_dir());
}

#[test]
fn a_dump_drops_excluded_tables_and_volatile_file_columns() {
    let conn = database(
        "CREATE TABLE artifact_metadata (key TEXT PRIMARY KEY, value TEXT);
         CREATE TABLE identifier_resolutions (id TEXT PRIMARY KEY);
         CREATE TABLE files (
           file_id TEXT PRIMARY KEY,
           path TEXT NOT NULL,
           indexed_at TEXT NOT NULL,
           last_revision_id INTEGER NOT NULL
         );
         INSERT INTO artifact_metadata VALUES ('created_at', 'now');
         INSERT INTO files VALUES ('f1', 'a.rs', '2026-01-01', 7);",
    );

    let dumped = dump_connection(&conn).expect("dump should succeed");

    assert_eq!(
        dumped.table_names().collect::<Vec<_>>(),
        vec!["files"],
        "excluded tables must not reach the comparison"
    );
    assert_eq!(
        dumped.find("files"),
        Some("#columns\tfile_id\tpath\ntext:f1\ttext:a.rs\n")
    );
}

#[test]
fn a_dump_orders_rows_by_primary_key_not_insertion_order() {
    let conn = database(
        "CREATE TABLE symbols (symbol_id TEXT PRIMARY KEY, name TEXT);
         INSERT INTO symbols VALUES ('s2', 'beta');
         INSERT INTO symbols VALUES ('s1', 'alpha');",
    );

    let dumped = dump_connection(&conn).expect("dump should succeed");

    assert_eq!(
        dumped.find("symbols"),
        Some("#columns\tsymbol_id\tname\ntext:s1\ttext:alpha\ntext:s2\ttext:beta\n")
    );
}

#[test]
fn a_dump_orders_a_keyless_table_by_every_column() {
    let conn = database(
        "CREATE TABLE notes (kind TEXT, detail TEXT);
         INSERT INTO notes VALUES ('b', 'two');
         INSERT INTO notes VALUES ('a', 'one');",
    );

    let dumped = dump_connection(&conn).expect("dump should succeed");

    assert_eq!(
        dumped.find("notes"),
        Some("#columns\tkind\tdetail\ntext:a\ttext:one\ntext:b\ttext:two\n")
    );
}

#[test]
fn a_dump_distinguishes_null_from_the_text_null() {
    let conn = database(
        "CREATE TABLE notes (id TEXT PRIMARY KEY, detail TEXT);
         INSERT INTO notes VALUES ('a', NULL);
         INSERT INTO notes VALUES ('b', 'null:');",
    );

    let dumped = dump_connection(&conn).expect("dump should succeed");

    assert_eq!(
        dumped.find("notes"),
        Some("#columns\tid\tdetail\ntext:a\tnull:\ntext:b\ttext:null:\n")
    );
}

#[test]
fn a_dump_escapes_separators_inside_text_values() {
    let conn = database(
        "CREATE TABLE notes (id TEXT PRIMARY KEY, detail TEXT);
         INSERT INTO notes VALUES ('a', 'one' || char(9) || 'two' || char(10) || 'three');",
    );

    let dumped = dump_connection(&conn).expect("dump should succeed");

    assert_eq!(
        dumped.find("notes"),
        Some("#columns\tid\tdetail\ntext:a\ttext:one\\ttwo\\nthree\n")
    );
}
