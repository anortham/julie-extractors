use std::path::Path;
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::{Value, json};
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
fn binary_declares_only_contract_commands() {
    let output = julie_extract(&["--help"]);

    assert!(output.status.success(), "help should succeed");
    let help = String::from_utf8(output.stdout).unwrap();
    for command in ["scan", "update", "delete", "info", "export", "languages"] {
        assert!(
            help.contains(command),
            "top-level help must declare {command}"
        );
    }
    assert!(
        !help.contains("analyze"),
        "old Julie analyze command must not exist"
    );

    let lower_help = help.to_ascii_lowercase();
    for forbidden in [
        "server",
        "daemon",
        "mcp",
        "search",
        "embedding",
        "watcher",
        "dashboard",
        "editing",
    ] {
        assert!(
            !lower_help.contains(forbidden),
            "CLI help leaked forbidden behavior term {forbidden}"
        );
    }
}

#[test]
fn contract_subcommands_parse_their_documented_flags() {
    let output = julie_extract(&["scan", "--help"]);
    assert_help_contains(
        &output,
        &[
            "--root",
            "--db",
            "--force",
            "--ignore-file",
            "--strict-schema",
            "--json",
        ],
    );

    let output = julie_extract(&["update", "--help"]);
    assert_help_contains(
        &output,
        &[
            "--root",
            "--db",
            "--file",
            "--ignore-file",
            "--strict-schema",
            "--json",
        ],
    );

    let output = julie_extract(&["delete", "--help"]);
    assert_help_contains(
        &output,
        &["--root", "--db", "--file", "--strict-schema", "--json"],
    );

    let output = julie_extract(&["info", "--help"]);
    assert_help_contains(&output, &["--db", "--strict-schema", "--json"]);

    let output = julie_extract(&["export", "--help"]);
    assert_help_contains(
        &output,
        &["--db", "--format", "--out", "--strict-schema", "--json"],
    );

    let output = julie_extract(&["languages", "--help"]);
    assert_help_contains(&output, &["--json"]);
}

#[test]
fn languages_json_report_matches_report_contract() {
    let output = julie_extract(&["languages", "--json"]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_common_report_shape(&report, "ok", "languages", "capability_snapshot");
    assert!(report["artifact"].is_null());
    assert!(report["revision"].is_null());
    assert_eq!(report["tool"]["binary_name"], "julie-extract");
    assert_eq!(
        report["input"],
        json!({
            "db_path": null,
            "root_path": null,
            "file_path": null,
            "root_relative_path": null,
            "format": null,
            "output_path": null
        })
    );
    assert!(report["languages"]["total"].as_i64().unwrap() > 0);
}

#[test]
fn exit_codes_and_json_errors_match_contract() {
    let temp = TempDir::new().unwrap();
    let missing_db = temp.path().join("missing.sqlite");
    let incompatible_db = temp.path().join("incompatible.sqlite");
    create_incompatible_artifact(&incompatible_db);

    let ok = julie_extract(&["languages", "--json"]);
    assert_eq!(ok.status.code(), Some(0));

    let failed = julie_extract(&[
        "export",
        "--db",
        missing_db.to_str().unwrap(),
        "--format",
        "xml",
        "--out",
        "-",
        "--json",
    ]);
    assert_eq!(failed.status.code(), Some(1));
    let report = json_report(&failed);
    assert_common_report_shape(&report, "failed", "export", "jsonl");
    assert_eq!(report["errors"][0]["code"], "unsupported_format");

    let usage = julie_extract(&["analyze", "--json"]);
    assert_eq!(usage.status.code(), Some(2));

    let incompatible = julie_extract(&[
        "info",
        "--db",
        incompatible_db.to_str().unwrap(),
        "--strict-schema",
        "--json",
    ]);
    assert_eq!(incompatible.status.code(), Some(3));
    let report = json_report(&incompatible);
    assert_common_report_shape(&report, "failed", "info", "read_only");
    assert_eq!(report["errors"][0]["code"], "schema_incompatible");
}

#[test]
fn cli_crate_does_not_link_forbidden_julie_behaviors() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("CLI manifest should be readable");
    let source = [
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/main.rs"))
            .unwrap_or_default(),
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/args.rs"))
            .unwrap_or_default(),
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/commands.rs"))
            .unwrap_or_default(),
    ]
    .join("\n");
    let searchable = format!("{manifest}\n{source}").to_ascii_lowercase();

    for forbidden in [
        "julie-server",
        "mcp",
        "daemon",
        "search",
        "embedding",
        "watcher",
        "dashboard",
        "editing",
        "workspace_id",
    ] {
        assert!(
            !searchable.contains(forbidden),
            "CLI crate must not link or expose forbidden Julie behavior {forbidden}"
        );
    }
}

fn assert_help_contains(output: &Output, expected_flags: &[&str]) {
    assert!(
        output.status.success(),
        "help command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8(output.stdout.clone()).unwrap();
    for flag in expected_flags {
        assert!(help.contains(flag), "help is missing {flag}:\n{help}");
    }
    assert!(
        !help.contains("analyze"),
        "old Julie analyze command leaked into help"
    );
}

fn assert_common_report_shape(report: &Value, status: &str, operation: &str, mode: &str) {
    assert_eq!(report["report_schema_version"], 1);
    assert_eq!(report["status"], status);
    assert_eq!(report["operation"], operation);
    assert_eq!(report["mode"], mode);
    assert_eq!(report["tool"]["binary_name"], "julie-extract");
    assert!(report["tool"]["binary_version"].as_str().is_some());
    assert!(report["counts"]["rows_written"].is_object());
    assert!(report["counts"]["totals"].is_object());
    assert!(report["errors"].is_array());
    assert!(report["warnings"].is_array());
}

fn create_incompatible_artifact(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute(
        "CREATE TABLE artifact_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    )
    .unwrap();
    for (key, value) in [
        ("artifact_id", "artifact-incompatible"),
        ("root_path", "/repo"),
        ("schema_version", "999"),
        ("extract_contract_version", "1"),
        ("sqlite_schema_version", "999"),
        ("binary_version", "julie-extract 0.1.0"),
        ("hash_algorithm", "blake3"),
        ("parser_inventory_fingerprint", "sha256:parser"),
        ("capability_snapshot_fingerprint", "sha256:capability"),
        ("created_at", "2026-05-31T21:00:00Z"),
        ("updated_at", "2026-05-31T21:00:00Z"),
    ] {
        conn.execute(
            "INSERT INTO artifact_metadata (key, value) VALUES (?1, ?2)",
            [key, value],
        )
        .unwrap();
    }
}
