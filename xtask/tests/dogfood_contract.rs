use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::Connection;
use tempfile::TempDir;
use xtask::dogfood::{CommandDurations, DogfoodOutputPaths, plan_repo_from_args, validate_outputs};

#[test]
fn repo_args_build_default_output_paths_and_binary() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("repo");
    let out_dir = temp.path().join("dogfood");
    std::fs::create_dir(&root).expect("repo root");

    let plan = plan_repo_from_args([
        "repo",
        "--root",
        path_str(&root),
        "--out-dir",
        path_str(&out_dir),
    ])
    .expect("dogfood plan");

    assert_eq!(plan.root, root);
    assert_eq!(plan.out_dir, out_dir);
    assert!(plan.build_default_binary);
    assert_eq!(plan.paths.db_path, plan.out_dir.join("artifact.sqlite"));
    assert_eq!(plan.paths.jsonl_path, plan.out_dir.join("artifact.jsonl"));
    assert_eq!(
        plan.paths.scan_report_path,
        plan.out_dir.join("scan-report.json")
    );
    assert_eq!(
        plan.paths.info_report_path,
        plan.out_dir.join("info-report.json")
    );
    assert_eq!(
        plan.paths.rescan_report_path,
        plan.out_dir.join("rescan-report.json")
    );
    assert_eq!(
        plan.paths.export_report_path,
        plan.out_dir.join("export-report.json")
    );
    assert_eq!(plan.paths.metrics_path, plan.out_dir.join("metrics.json"));
    assert!(
        plan.binary.ends_with(debug_binary_name()),
        "unexpected default binary path: {}",
        plan.binary.display()
    );
}

#[test]
fn repo_args_accept_explicit_binary_and_reject_missing_values() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("repo");
    let out_dir = temp.path().join("dogfood");
    let binary = temp.path().join("bin/julie-extract");
    std::fs::create_dir_all(binary.parent().unwrap()).expect("binary parent");
    std::fs::write(&binary, "").expect("binary");

    let plan = plan_repo_from_args([
        "repo",
        "--root",
        path_str(&root),
        "--out-dir",
        path_str(&out_dir),
        "--binary",
        path_str(&binary),
    ])
    .expect("dogfood plan");

    assert_eq!(plan.binary, binary);
    assert!(!plan.build_default_binary);

    let error = plan_repo_from_args(["repo", "--root", path_str(&root)])
        .expect_err("missing out-dir must fail");
    assert!(
        error.to_string().contains("missing --out-dir"),
        "unexpected error: {error}"
    );

    let error = plan_repo_from_args(["unknown"]).expect_err("unknown subcommand must fail");
    assert!(
        error.to_string().contains("expected `repo`"),
        "unexpected error: {error}"
    );
}

#[test]
fn validate_outputs_accepts_ok_reports_sqlite_metadata_and_valid_jsonl() {
    let fixture = DogfoodFixture::new();
    fixture.write_ok_reports();
    fixture.write_sqlite_artifact(2, 3);
    fixture.write_jsonl_records(4);

    let metrics = validate_outputs(
        &fixture.paths,
        &fixture.root_path,
        CommandDurations {
            scan: Duration::from_millis(200),
            rescan: Duration::from_millis(80),
            info: Duration::from_millis(10),
            export: Duration::from_millis(20),
        },
    )
    .expect("valid dogfood outputs");

    assert_eq!(metrics.sqlite_schema_version, 2);
    assert_eq!(metrics.extract_contract_version, 2);
    assert_eq!(metrics.files, 2);
    assert_eq!(metrics.symbols, 3);
    assert_eq!(metrics.jsonl_records, 4);
    assert_eq!(metrics.row_totals["files"], 2);
    assert_eq!(metrics.row_totals["symbols"], 3);
    assert_eq!(metrics.jsonl_records_by_kind["artifact"], 1);
    assert_eq!(metrics.jsonl_records_by_kind["file"], 2);
    assert_eq!(metrics.jsonl_records_by_kind["symbol"], 1);
    assert!(metrics.sqlite_bytes > 0);
    assert!(metrics.jsonl_bytes > 0);
    assert_eq!(metrics.rescan_duration_ms, 80);
    assert_eq!(metrics.rescan_files_unchanged, 2);
    assert_eq!(metrics.rescan_files_changed, 0);
    assert!(metrics.rows_per_second.is_some());
}

#[test]
fn validate_outputs_rejects_non_ok_reports() {
    let fixture = DogfoodFixture::new();
    fixture.write_report(&fixture.paths.scan_report_path, "failed", "scan");
    fixture.write_report(&fixture.paths.info_report_path, "ok", "info");
    fixture.write_report(&fixture.paths.export_report_path, "ok", "export");
    fixture.write_sqlite_artifact(1, 1);
    fixture.write_jsonl_records(1);

    let error = validate_outputs(
        &fixture.paths,
        &fixture.root_path,
        CommandDurations::default(),
    )
    .expect_err("failed scan report must fail validation");

    assert!(
        error
            .to_string()
            .contains("scan report status was `failed`"),
        "unexpected error: {error}"
    );
}

#[test]
fn validate_outputs_rejects_zero_symbols() {
    let fixture = DogfoodFixture::new();
    fixture.write_ok_reports();
    fixture.write_sqlite_artifact(1, 0);
    fixture.write_jsonl_records(1);

    let error = validate_outputs(
        &fixture.paths,
        &fixture.root_path,
        CommandDurations::default(),
    )
    .expect_err("zero symbols must fail validation");

    assert!(
        error.to_string().contains("artifact contains zero symbols"),
        "unexpected error: {error}"
    );
}

#[test]
fn validate_outputs_rejects_other_required_hard_gate_failures() {
    assert_invalid_evidence(
        |fixture| fixture.delete_metadata("hash_algorithm"),
        "artifact metadata missing `hash_algorithm`",
    );
    assert_invalid_evidence(
        |fixture| fixture.set_metadata("schema_version", "999"),
        "artifact schema version was schema=999",
    );
    assert_invalid_evidence(
        |fixture| fixture.clear_table("files"),
        "artifact contains zero files",
    );
    assert_invalid_evidence(
        |fixture| std::fs::write(&fixture.paths.jsonl_path, "{\"bad\":true}\n").expect("jsonl"),
        "missing integer jsonl_schema_version",
    );
    assert_invalid_evidence(
        |fixture| std::fs::remove_file(&fixture.paths.jsonl_path).expect("remove jsonl"),
        "failed to read",
    );
    assert_invalid_evidence(
        |fixture| fixture.write_rescan_report("ok", 0, 2, 0, 0),
        "rescan report status was `ok`; expected `no_change`",
    );
    assert_invalid_evidence(
        |fixture| fixture.write_rescan_report("no_change", 0, 0, 0, 0),
        "rescan report must include unchanged files and zero changed/deleted/failed files",
    );
    assert_invalid_evidence(
        |fixture| fixture.write_rescan_report("no_change", 1, 1, 0, 0),
        "rescan report must include unchanged files and zero changed/deleted/failed files",
    );
    assert_invalid_evidence(
        |fixture| fixture.write_rescan_report_with_revision("no_change", Some("rev-2"), 0, 2, 0, 0),
        "rescan report must not create a revision",
    );
    assert_invalid_evidence(
        |fixture| fixture.write_rescan_report_with_rows_written("no_change", 0, 2, 0, 0, 1, 0),
        "rescan report must write zero rows",
    );
}

fn assert_invalid_evidence(setup: impl FnOnce(&DogfoodFixture), expected_error: &str) {
    let fixture = DogfoodFixture::new();
    fixture.write_ok_reports();
    fixture.write_sqlite_artifact(1, 1);
    fixture.write_jsonl_records(4);
    setup(&fixture);

    let error = validate_outputs(
        &fixture.paths,
        &fixture.root_path,
        CommandDurations::default(),
    )
    .expect_err("dogfood validation must fail");

    assert!(
        error.to_string().contains(expected_error),
        "expected `{expected_error}` in error: {error}"
    );
}

struct DogfoodFixture {
    _temp: TempDir,
    root_path: PathBuf,
    paths: DogfoodOutputPaths,
}

impl DogfoodFixture {
    fn new() -> Self {
        let temp = TempDir::new().expect("tempdir");
        let root_path = temp.path().join("repo");
        let out_dir = temp.path().join("dogfood");
        std::fs::create_dir_all(&root_path).expect("root");
        std::fs::create_dir_all(&out_dir).expect("out dir");
        Self {
            paths: DogfoodOutputPaths::new(&out_dir),
            root_path,
            _temp: temp,
        }
    }

    fn write_ok_reports(&self) {
        self.write_report(&self.paths.scan_report_path, "ok", "scan");
        self.write_rescan_report("no_change", 0, 2, 0, 0);
        self.write_report(&self.paths.info_report_path, "ok", "info");
        self.write_report(&self.paths.export_report_path, "ok", "export");
    }

    fn write_rescan_report(
        &self,
        status: &str,
        files_changed: i64,
        files_unchanged: i64,
        files_deleted: i64,
        files_failed: i64,
    ) {
        self.write_rescan_report_with_rows_written(
            status,
            files_changed,
            files_unchanged,
            files_deleted,
            files_failed,
            0,
            0,
        );
    }

    fn write_rescan_report_with_revision(
        &self,
        status: &str,
        created_revision_id: Option<&str>,
        files_changed: i64,
        files_unchanged: i64,
        files_deleted: i64,
        files_failed: i64,
    ) {
        self.write_rescan_report_json(
            status,
            created_revision_id,
            files_changed,
            files_unchanged,
            files_deleted,
            files_failed,
            0,
            0,
        );
    }

    fn write_rescan_report_with_rows_written(
        &self,
        status: &str,
        files_changed: i64,
        files_unchanged: i64,
        files_deleted: i64,
        files_failed: i64,
        rows_files: i64,
        rows_symbols: i64,
    ) {
        self.write_rescan_report_json(
            status,
            None,
            files_changed,
            files_unchanged,
            files_deleted,
            files_failed,
            rows_files,
            rows_symbols,
        );
    }

    fn write_rescan_report_json(
        &self,
        status: &str,
        created_revision_id: Option<&str>,
        files_changed: i64,
        files_unchanged: i64,
        files_deleted: i64,
        files_failed: i64,
        rows_files: i64,
        rows_symbols: i64,
    ) {
        let created_revision_id = created_revision_id
            .map(|id| format!(r#""{id}""#))
            .unwrap_or_else(|| "null".to_string());
        std::fs::write(
            &self.paths.rescan_report_path,
            format!(
                r#"{{"report_schema_version":2,"status":"{status}","operation":"scan","mode":"incremental","revision":{{"created_revision_id":{created_revision_id}}},"counts":{{"files_scanned":2,"files_changed":{files_changed},"files_unchanged":{files_unchanged},"files_deleted":{files_deleted},"files_failed":{files_failed},"rows_written":{{"files":{rows_files},"symbols":{rows_symbols}}},"totals":{{"files":2,"symbols":3}}}},"errors":[]}}"#
            ),
        )
        .expect("write rescan report");
    }

    fn write_report(&self, path: &Path, status: &str, operation: &str) {
        let mode = match operation {
            "scan" => "incremental",
            "info" => "read_only",
            "export" => "jsonl",
            other => panic!("unsupported report operation {other}"),
        };
        std::fs::write(
            path,
            format!(
                r#"{{"report_schema_version":2,"status":"{status}","operation":"{operation}","mode":"{mode}","artifact":{{"jsonl_schema_version":2}},"counts":{{"files_scanned":2,"rows_written":{{"files":2,"symbols":3}},"totals":{{"files":2,"symbols":3}}}},"errors":[]}}"#
            ),
        )
        .expect("write report");
    }

    fn write_sqlite_artifact(&self, files: i64, symbols: i64) {
        let conn = Connection::open(&self.paths.db_path).expect("open sqlite");
        conn.execute_batch(
            "
            CREATE TABLE artifact_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE files (file_id TEXT PRIMARY KEY);
            CREATE TABLE symbols (symbol_id TEXT PRIMARY KEY);
            CREATE TABLE parser_inventory (id TEXT);
            CREATE TABLE language_capabilities (id TEXT);
            CREATE TABLE language_capability_fixtures (id TEXT);
            CREATE TABLE language_capability_gaps (id TEXT);
            CREATE TABLE extraction_revisions (id TEXT);
            CREATE TABLE revision_file_changes (id TEXT);
            CREATE TABLE symbol_annotations (id TEXT);
            CREATE TABLE identifiers (id TEXT);
            CREATE TABLE relationships (id TEXT);
            CREATE TABLE pending_relationships (id TEXT);
            CREATE TABLE type_facts (id TEXT);
            CREATE TABLE type_argument_usages (id TEXT);
            CREATE TABLE type_arguments (id TEXT);
            CREATE TABLE literals (id TEXT);
            CREATE TABLE source_regions (id TEXT);
            CREATE TABLE parse_diagnostics (id TEXT);
            ",
        )
        .expect("schema");
        for (key, value) in [
            ("artifact_id", "artifact".to_string()),
            ("schema_version", "2".to_string()),
            ("extract_contract_version", "2".to_string()),
            ("sqlite_schema_version", "2".to_string()),
            ("binary_version", "julie-extract 0.1.0".to_string()),
            ("hash_algorithm", "blake3".to_string()),
            ("parser_inventory_fingerprint", "sha256:parser".to_string()),
            (
                "capability_snapshot_fingerprint",
                "sha256:capability".to_string(),
            ),
            ("created_at", "2026-06-01T00:00:00Z".to_string()),
            ("updated_at", "2026-06-01T00:00:00Z".to_string()),
            ("root_path", self.root_path.display().to_string()),
        ] {
            conn.execute(
                "INSERT INTO artifact_metadata (key, value) VALUES (?1, ?2)",
                (key, value),
            )
            .expect("metadata");
        }
        for index in 0..files {
            conn.execute(
                "INSERT INTO files (file_id) VALUES (?1)",
                [format!("file-{index}")],
            )
            .expect("file");
        }
        for index in 0..symbols {
            conn.execute(
                "INSERT INTO symbols (symbol_id) VALUES (?1)",
                [format!("symbol-{index}")],
            )
            .expect("symbol");
        }
    }

    fn delete_metadata(&self, key: &str) {
        let conn = Connection::open(&self.paths.db_path).expect("open sqlite");
        conn.execute("DELETE FROM artifact_metadata WHERE key = ?1", [key])
            .expect("delete metadata");
    }

    fn set_metadata(&self, key: &str, value: &str) {
        let conn = Connection::open(&self.paths.db_path).expect("open sqlite");
        conn.execute(
            "UPDATE artifact_metadata SET value = ?1 WHERE key = ?2",
            (value, key),
        )
        .expect("update metadata");
    }

    fn clear_table(&self, table: &str) {
        let conn = Connection::open(&self.paths.db_path).expect("open sqlite");
        conn.execute(&format!("DELETE FROM {table}"), [])
            .expect("clear table");
    }

    fn write_jsonl_records(&self, records: usize) {
        let mut jsonl = String::new();
        let kinds = ["artifact", "file", "file", "symbol", "source_region"];
        for index in 0..records {
            let kind = kinds.get(index).copied().unwrap_or("identifier");
            jsonl.push_str(&format!(
                r#"{{"jsonl_schema_version":2,"extract_contract_version":2,"kind":"{kind}","op":"snapshot","artifact_id":"artifact","record_id":"{index}","record":{{}}}}"#
            ));
            jsonl.push('\n');
        }
        std::fs::write(&self.paths.jsonl_path, jsonl).expect("jsonl");
    }
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf-8 path")
}

fn debug_binary_name() -> &'static str {
    if cfg!(windows) {
        "target/debug/julie-extract.exe"
    } else {
        "target/debug/julie-extract"
    }
}
