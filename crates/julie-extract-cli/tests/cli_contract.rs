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
fn commands_module_does_not_own_capability_snapshot_mapping() {
    let commands_source = include_str!("../src/commands.rs");
    for forbidden_definition in [
        "fn artifact_capability_snapshot(",
        "fn cargo_lock_packages(",
        "fn parser_inventory_fingerprint(",
        "fn capability_snapshot_fingerprint(",
        "fn artifact_flags(",
        "fn kind_coverage_json(",
    ] {
        assert!(
            !commands_source.contains(forbidden_definition),
            "commands.rs still owns capability snapshot helper {forbidden_definition}"
        );
    }
}

#[test]
fn commands_module_does_not_own_report_error_mapping() {
    let commands_source = include_str!("../src/commands.rs");
    for forbidden_definition in [
        "struct CommandOutcome",
        "enum ReportStream",
        "fn base_report(",
        "fn command_error(",
        "fn diagnostic(",
        "fn path_error_outcome(",
        "fn extract_error_outcome(",
        "fn write_error_outcome(",
        "fn spool_error_outcome(",
        "fn write_outcome(",
        "fn display_path(",
    ] {
        assert!(
            !commands_source.contains(forbidden_definition),
            "commands.rs still owns report/error helper {forbidden_definition}"
        );
    }
}

#[test]
fn commands_module_does_not_own_artifact_access_mapping() {
    let commands_source = include_str!("../src/commands.rs");
    for forbidden_definition in [
        "struct OpenArtifact",
        "struct OpenInfoArtifact",
        "struct ExistingArtifact",
        "fn artifact_report_from_connection(",
        "fn open_artifact(",
        "fn open_artifact_for_info(",
        "fn open_artifact_for_root(",
        "fn existing_artifact_for_root(",
        "fn load_existing_content_hashes(",
        "fn check_versions(",
        "fn artifact_report(",
        "fn artifact_metadata_from_rows(",
        "fn metadata_string(",
        "fn metadata_i64(",
        "fn table_totals(",
        "fn latest_revision_id(",
        "fn jsonl_counts(",
    ] {
        assert!(
            !commands_source.contains(forbidden_definition),
            "commands.rs still owns artifact access helper {forbidden_definition}"
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
            "--jobs",
            "--spool-dir",
            "--progress-file",
            "--parent-pid",
            "--level",
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
fn fleet_safety_flags_are_scan_only() {
    for command in ["update", "delete", "info", "export", "languages"] {
        let output = julie_extract(&[command, "--help"]);
        assert!(output.status.success(), "{command} help should succeed");
        let help = String::from_utf8(output.stdout).unwrap();
        for flag in ["--spool-dir", "--progress-file", "--parent-pid"] {
            assert!(
                !help.contains(flag),
                "{flag} must stay scan-only but appears in {command} help:\n{help}"
            );
        }
    }
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
fn languages_json_report_publishes_structural_fact_pattern_registry() {
    let output = julie_extract(&["languages", "--json"]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);

    let patterns = &report["structural_fact_patterns"];
    assert!(
        patterns.is_array(),
        "languages --json must publish a top-level structural_fact_patterns array, got: {patterns}"
    );
    assert!(
        !patterns.as_array().unwrap().is_empty(),
        "structural_fact_patterns must be a non-empty array"
    );
    assert_eq!(
        *patterns,
        julie_extractors::structural_fact_patterns_json(),
        "structural_fact_patterns must equal the extractor registry serializer output byte-for-byte"
    );
}

#[test]
fn languages_json_report_publishes_discovery_limits() {
    let output = julie_extract(&["languages", "--json"]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);

    let discovery_limits = &report["languages"]["discovery_limits"];
    assert_eq!(
        discovery_limits["max_source_file_bytes"].as_u64(),
        Some(julie_extract_cli::limits::MAX_SOURCE_FILE_BYTES as u64),
        "max_source_file_bytes must equal the real limit constant, got: {discovery_limits}"
    );
    assert_eq!(
        discovery_limits["hard_exclude_directories"],
        json!(julie_extract_cli::limits::HARD_EXCLUDE_DIRS),
        "hard_exclude_directories must equal the real discovery constant, got: {discovery_limits}"
    );
    assert_eq!(
        discovery_limits["hard_exclude_suffixes"],
        json!(julie_extract_cli::limits::HARD_EXCLUDE_SUFFIXES),
        "hard_exclude_suffixes must equal the real discovery constant, got: {discovery_limits}"
    );
}

#[test]
fn exit_codes_and_json_errors_match_contract() {
    let temp = TempDir::new().unwrap();
    let missing_db = temp.path().join("missing.sqlite");
    let incompatible_db = temp.path().join("incompatible.sqlite");
    let old_v2_db = temp.path().join("old-v2.sqlite");
    create_incompatible_artifact(&incompatible_db);
    create_artifact_metadata(&old_v2_db, "artifact-old-v2", "2", "2", "2");

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

    let old_v2 = julie_extract(&["info", "--db", old_v2_db.to_str().unwrap(), "--json"]);
    assert_eq!(old_v2.status.code(), Some(3));
    let report = json_report(&old_v2);
    assert_common_report_shape(&report, "failed", "info", "read_only");
    assert_eq!(report["errors"][0]["code"], "contract_incompatible");
    assert_eq!(
        report["errors"][0]["details"]["artifact_extract_contract_version"],
        2
    );
    assert_eq!(
        report["errors"][0]["details"]["supported_extract_contract_version"],
        4
    );
}

#[test]
fn strict_schema_rejects_older_v5_artifact_with_migration_required() {
    let temp = TempDir::new().unwrap();
    let old_v5_db = temp.path().join("old-v5.sqlite");
    create_artifact_metadata(&old_v5_db, "artifact-old-v5", "5", "4", "5");

    let old_v5 = julie_extract(&[
        "info",
        "--db",
        old_v5_db.to_str().unwrap(),
        "--strict-schema",
        "--json",
    ]);
    assert_eq!(old_v5.status.code(), Some(3));
    let report = json_report(&old_v5);
    assert_common_report_shape(&report, "failed", "info", "read_only");
    assert_eq!(report["errors"][0]["code"], "schema_migration_required");
    assert_eq!(
        report["errors"][0]["details"]["required_sqlite_schema_version"],
        7
    );
    assert_eq!(
        report["errors"][0]["details"]["artifact_sqlite_schema_version"],
        5
    );

    let non_strict = julie_extract(&["info", "--db", old_v5_db.to_str().unwrap(), "--json"]);
    assert_ne!(
        json_report(&non_strict)["errors"][0]["code"],
        "schema_migration_required",
        "without --strict-schema a v5 artifact must not be rejected for migration"
    );
}

#[test]
fn write_verbs_refuse_older_v5_artifact_without_strict_schema() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn seeded() {}\n").unwrap();
    let file = root.join("src/lib.rs");
    let db = temp.path().join("artifact.sqlite");
    let seed = julie_extract(&["scan", "--root", str(&root), "--db", str(&db), "--json"]);
    assert_eq!(
        seed.status.code(),
        Some(0),
        "fixture scan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&seed.stdout),
        String::from_utf8_lossy(&seed.stderr)
    );
    downgrade_artifact_schema_version(&db, "5");
    std::fs::write(root.join("src/added.rs"), "pub fn added() {}\n").unwrap();
    let untouched = std::fs::read(&db).unwrap();

    let rebind_root = temp.path().join("moved");
    std::fs::create_dir_all(&rebind_root).unwrap();
    for (verb, args) in [
        (
            "scan",
            vec!["scan", "--root", str(&root), "--db", str(&db), "--json"],
        ),
        (
            "scan --force",
            vec![
                "scan",
                "--root",
                str(&root),
                "--db",
                str(&db),
                "--force",
                "--json",
            ],
        ),
        (
            "update",
            vec![
                "update",
                "--root",
                str(&root),
                "--db",
                str(&db),
                "--file",
                str(&file),
                "--json",
            ],
        ),
        (
            "delete",
            vec![
                "delete",
                "--root",
                str(&root),
                "--db",
                str(&db),
                "--file",
                str(&file),
                "--json",
            ],
        ),
        (
            "rebind",
            vec![
                "rebind",
                "--root",
                str(&rebind_root),
                "--db",
                str(&db),
                "--json",
            ],
        ),
    ] {
        let output = julie_extract(&args);
        assert_eq!(
            output.status.code(),
            Some(3),
            "{verb} must refuse a v5 artifact without --strict-schema\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let report = json_report(&output);
        assert_eq!(
            report["errors"][0]["code"], "schema_migration_required",
            "{verb} refused a v5 artifact for the wrong reason: {report}"
        );
        assert_eq!(
            report["errors"][0]["details"]["required_sqlite_schema_version"], 7,
            "{verb} must name the required schema version"
        );
        assert_eq!(
            report["errors"][0]["details"]["artifact_sqlite_schema_version"], 5,
            "{verb} must name the artifact schema version"
        );
        assert_eq!(
            std::fs::read(&db).unwrap(),
            untouched,
            "{verb} must leave the refused artifact byte-identical"
        );
    }

    let export = julie_extract(&[
        "export",
        "--db",
        str(&db),
        "--format",
        "jsonl",
        "--out",
        str(&temp.path().join("export.jsonl")),
        "--json",
    ]);
    assert_eq!(
        export.status.code(),
        Some(0),
        "a read verb must still serve a v5 artifact without --strict-schema\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&export.stdout),
        String::from_utf8_lossy(&export.stderr)
    );
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
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/discovery.rs"))
            .unwrap_or_default(),
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/extraction.rs"))
            .unwrap_or_default(),
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/paths.rs"))
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
    assert_eq!(report["report_schema_version"], 3);
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

fn str(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn downgrade_artifact_schema_version(db: &Path, schema_version: &str) {
    let connection = Connection::open(db).unwrap();
    for key in ["schema_version", "sqlite_schema_version"] {
        let updated = connection
            .execute(
                "UPDATE artifact_metadata SET value = ?1 WHERE key = ?2",
                [schema_version, key],
            )
            .unwrap();
        assert_eq!(updated, 1, "artifact metadata is missing {key}");
    }
}

fn create_incompatible_artifact(path: &Path) {
    create_artifact_metadata(path, "artifact-incompatible", "999", "1", "999");
}

fn create_artifact_metadata(
    path: &Path,
    artifact_id: &str,
    schema_version: &str,
    extract_contract_version: &str,
    sqlite_schema_version: &str,
) {
    let conn = Connection::open(path).unwrap();
    conn.execute(
        "CREATE TABLE artifact_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    )
    .unwrap();
    for (key, value) in [
        ("artifact_id", artifact_id),
        ("root_path", "/repo"),
        ("schema_version", schema_version),
        ("extract_contract_version", extract_contract_version),
        ("sqlite_schema_version", sqlite_schema_version),
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
