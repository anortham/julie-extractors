use std::fs;
use std::process::{Command, Output};

use julie_extract_artifact::reports::ReportCode;
use serde_json::Value;
use tempfile::TempDir;

fn julie_extract(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run julie-extract {args:?}: {err}"))
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("julie-extract output must be UTF-8")
}

#[test]
fn failed_scan_without_json_prints_status_then_error_diagnostics_on_stderr() {
    let temp = TempDir::new().unwrap();
    let missing_root = temp.path().join("missing-root");
    let db = temp.path().join("artifact.sqlite");

    let output = julie_extract(&[
        "scan",
        "--root",
        missing_root.to_str().unwrap(),
        "--db",
        db.to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stdout.is_empty(),
        "failures must not write to stdout, got: {}",
        text(output.stdout)
    );

    let stderr = text(output.stderr);
    let mut lines = stderr.lines();
    assert_eq!(lines.next(), Some("failed"));

    let diagnostic = lines.next().expect("error diagnostic line is missing");
    assert!(
        diagnostic.starts_with("invalid_path: "),
        "diagnostic line must lead with the report code, got: {diagnostic}"
    );
    assert!(
        diagnostic.contains("source root could not be canonicalized"),
        "diagnostic line must carry the message, got: {diagnostic}"
    );
    assert!(
        diagnostic.contains(missing_root.to_str().unwrap()),
        "diagnostic line must carry the offending path, got: {diagnostic}"
    );

    assert_eq!(
        lines.next(),
        Some("files: scanned=0 changed=0 unchanged=0 failed=0")
    );
    assert_eq!(lines.next(), None);
}

#[test]
fn successful_scan_without_json_prints_status_then_file_counts_on_stdout() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("lib.rs"), "pub fn hello() {}\n").unwrap();
    let db = temp.path().join("artifact.sqlite");

    let output = julie_extract(&[
        "scan",
        "--root",
        root.to_str().unwrap(),
        "--db",
        db.to_str().unwrap(),
    ]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = text(output.stdout);
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("ok"));
    assert_eq!(
        lines.next(),
        Some("files: scanned=1 changed=1 unchanged=0 failed=0")
    );
    assert_eq!(lines.next(), None);
}

#[test]
fn json_reports_stay_single_line_json_on_success_and_failure() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("root");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("lib.rs"), "pub fn hello() {}\n").unwrap();
    let db = temp.path().join("artifact.sqlite");

    let ok = julie_extract(&[
        "scan",
        "--root",
        root.to_str().unwrap(),
        "--db",
        db.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(ok.status.code(), Some(0));
    assert!(ok.stderr.is_empty(), "json runs must not narrate on stderr");
    let stdout = text(ok.stdout);
    assert_eq!(
        stdout.lines().count(),
        1,
        "json output must stay one line, got: {stdout}"
    );
    assert!(stdout.ends_with('\n'));
    let report: Value = serde_json::from_str(stdout.trim_end()).expect("stdout must be one report");
    assert_eq!(report["status"], "ok");

    let missing_root = temp.path().join("missing-root");
    let failed = julie_extract(&[
        "scan",
        "--root",
        missing_root.to_str().unwrap(),
        "--db",
        db.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(failed.status.code(), Some(1));
    assert!(
        failed.stderr.is_empty(),
        "json runs must not narrate on stderr"
    );
    let stdout = text(failed.stdout);
    assert_eq!(
        stdout.lines().count(),
        1,
        "json output must stay one line, got: {stdout}"
    );
    let report: Value = serde_json::from_str(stdout.trim_end()).expect("stdout must be one report");
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "invalid_path");
}

#[test]
fn report_code_as_str_matches_its_serialized_json_spelling() {
    for code in ReportCode::ALL {
        assert_eq!(
            serde_json::to_value(code).unwrap(),
            Value::String(code.as_str().to_string()),
            "human spelling must match the JSON spelling for {code:?}"
        );
    }

    for code in ReportCode::ERROR_CODES {
        assert!(
            ReportCode::ALL.contains(&code),
            "ReportCode::ALL must cover every error code, missing {code:?}"
        );
    }
}
