use std::collections::BTreeMap;
use std::path::Path;

use tempfile::TempDir;
use xtask::dogfood::DogfoodMetrics;
use xtask::performance::{
    BaselineRun, MetricSummary, plan_baseline_from_args, plan_writer_current_schema_from_args,
    run_writer_current_schema, summarize_baseline,
};

#[test]
fn baseline_args_plan_repeated_run_directories_and_summary_path() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("repo");
    let out_dir = temp.path().join("baseline");
    let binary = temp.path().join("bin/julie-extract");

    let plan = plan_baseline_from_args([
        "baseline",
        "--root",
        path_str(&root),
        "--out-dir",
        path_str(&out_dir),
        "--binary",
        path_str(&binary),
        "--runs",
        "3",
    ])
    .expect("baseline plan");

    assert_eq!(plan.root, root);
    assert_eq!(plan.out_dir, out_dir);
    assert_eq!(plan.binary, binary);
    assert_eq!(plan.runs, 3);
    assert_eq!(
        plan.summary_path,
        plan.out_dir.join("baseline-summary.json")
    );
    assert_eq!(
        plan.run_output_dirs(),
        vec![
            plan.out_dir.join("run-001"),
            plan.out_dir.join("run-002"),
            plan.out_dir.join("run-003")
        ]
    );
}

#[test]
fn baseline_args_require_at_least_three_runs() {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path().join("repo");
    let out_dir = temp.path().join("baseline");
    let binary = temp.path().join("bin/julie-extract");

    for runs in ["1", "2"] {
        let error = plan_baseline_from_args([
            "baseline",
            "--root",
            path_str(&root),
            "--out-dir",
            path_str(&out_dir),
            "--binary",
            path_str(&binary),
            "--runs",
            runs,
        ])
        .expect_err("too few runs must fail");

        assert!(
            error.to_string().contains("at least 3"),
            "unexpected error for runs={runs}: {error}"
        );
    }
}

#[test]
fn writer_current_schema_args_plan_default_dimensions_and_paths() {
    let temp = TempDir::new().expect("tempdir");
    let out_dir = temp.path().join("writer-current-schema");

    let plan = plan_writer_current_schema_from_args([
        "writer-current-schema",
        "--out-dir",
        path_str(&out_dir),
    ])
    .expect("writer current-schema plan");

    assert_eq!(plan.out_dir, out_dir);
    assert_eq!(plan.db_path, plan.out_dir.join("artifact.sqlite"));
    assert_eq!(
        plan.summary_path,
        plan.out_dir.join("writer-current-schema-summary.json")
    );
    assert_eq!(plan.files, 10_000);
    assert_eq!(plan.symbols_per_file, 8);
    assert_eq!(plan.identifiers_per_file, 24);
    assert_eq!(plan.source_regions_per_file, 12);
}

#[test]
fn writer_current_schema_args_accept_explicit_dimensions() {
    let temp = TempDir::new().expect("tempdir");
    let out_dir = temp.path().join("writer-current-schema");

    let plan = plan_writer_current_schema_from_args([
        "writer-current-schema",
        "--out-dir",
        path_str(&out_dir),
        "--files",
        "3",
        "--symbols-per-file",
        "4",
        "--identifiers-per-file",
        "5",
        "--source-regions-per-file",
        "6",
    ])
    .expect("writer current-schema plan");

    assert_eq!(plan.files, 3);
    assert_eq!(plan.symbols_per_file, 4);
    assert_eq!(plan.identifiers_per_file, 5);
    assert_eq!(plan.source_regions_per_file, 6);
}

#[test]
fn writer_current_schema_args_reject_invalid_dimensions() {
    let temp = TempDir::new().expect("tempdir");
    let out_dir = temp.path().join("writer-current-schema");

    for (flag, value, expected) in [
        ("--files", "0", "--files must be greater than zero"),
        (
            "--symbols-per-file",
            "0",
            "--symbols-per-file must be greater than zero",
        ),
        (
            "--identifiers-per-file",
            "abc",
            "--identifiers-per-file must be an integer",
        ),
        (
            "--source-regions-per-file",
            "0",
            "--source-regions-per-file must be greater than zero",
        ),
    ] {
        let error = plan_writer_current_schema_from_args([
            "writer-current-schema",
            "--out-dir",
            path_str(&out_dir),
            flag,
            value,
        ])
        .expect_err("invalid dimension must fail");

        assert!(
            error.to_string().contains(expected),
            "unexpected error for {flag}={value}: {error}"
        );
    }
}

#[test]
fn writer_current_schema_args_reject_unknown_arguments() {
    let temp = TempDir::new().expect("tempdir");
    let out_dir = temp.path().join("writer-current-schema");

    let error = plan_writer_current_schema_from_args([
        "writer-current-schema",
        "--out-dir",
        path_str(&out_dir),
        "--unexpected",
        "value",
    ])
    .expect_err("unknown argument must fail");

    assert!(
        error
            .to_string()
            .contains("unknown performance writer-current-schema argument `--unexpected`"),
        "unexpected error: {error}"
    );
}

#[test]
fn writer_current_schema_execution_writes_every_current_child_domain() {
    let temp = TempDir::new().expect("tempdir");
    let out_dir = temp.path().join("writer-current-schema");
    let plan = plan_writer_current_schema_from_args([
        "writer-current-schema",
        "--out-dir",
        path_str(&out_dir),
        "--files",
        "2",
        "--symbols-per-file",
        "3",
        "--identifiers-per-file",
        "4",
        "--source-regions-per-file",
        "3",
    ])
    .expect("writer current-schema plan");

    let summary = run_writer_current_schema(plan).expect("writer current-schema run");

    assert!(
        summary.db_path.exists(),
        "SQLite artifact should be written"
    );
    assert!(
        summary.summary_path.exists(),
        "summary JSON should be written"
    );
    assert_eq!(summary.transactions_committed, 1);
    assert_eq!(summary.files_changed, 2);
    assert_eq!(summary.rows_written.files, 2);
    assert_eq!(summary.rows_written.symbols, 6);
    assert_eq!(summary.rows_written.symbol_annotations, 2);
    assert_eq!(summary.rows_written.identifiers, 8);
    assert_eq!(summary.rows_written.relationships, 4);
    assert_eq!(summary.rows_written.pending_relationships, 2);
    assert_eq!(summary.rows_written.type_facts, 6);
    assert_eq!(summary.rows_written.type_argument_usages, 8);
    assert_eq!(summary.rows_written.type_arguments, 8);
    assert_eq!(summary.rows_written.literals, 2);
    assert_eq!(summary.rows_written.source_regions, 6);
    assert_eq!(summary.rows_written.parse_diagnostics, 2);
    assert_eq!(summary.rows_written.revision_file_changes, 2);
    assert!(summary.sqlite_bytes > 0);
    assert!(
        summary.rows_per_second.is_none_or(f64::is_finite),
        "rows_per_second must be absent or finite"
    );
}

#[test]
fn writer_current_schema_summary_serializes_stable_fields() {
    let temp = TempDir::new().expect("tempdir");
    let out_dir = temp.path().join("writer-current-schema");
    let plan = plan_writer_current_schema_from_args([
        "writer-current-schema",
        "--out-dir",
        path_str(&out_dir),
        "--files",
        "1",
        "--symbols-per-file",
        "2",
        "--identifiers-per-file",
        "3",
        "--source-regions-per-file",
        "3",
    ])
    .expect("writer current-schema plan");

    let summary = run_writer_current_schema(plan).expect("writer current-schema run");
    let value = serde_json::to_value(&summary).expect("summary json");

    assert_eq!(value["input"]["files"], 1);
    assert_eq!(value["input"]["symbols_per_file"], 2);
    assert_eq!(value["input"]["identifiers_per_file"], 3);
    assert_eq!(value["input"]["source_regions_per_file"], 3);
    assert_eq!(value["rows_written"]["files"], 1);
    assert_eq!(value["rows_written"]["source_regions"], 3);
    assert!(value["elapsed_write_ms"].is_number());
    assert!(value["sqlite_bytes"].is_number());
    assert!(
        value["db_path"]
            .as_str()
            .expect("db_path string")
            .ends_with("artifact.sqlite")
    );
    assert!(
        value["summary_path"]
            .as_str()
            .expect("summary_path string")
            .ends_with("writer-current-schema-summary.json")
    );
}

#[test]
fn baseline_summary_computes_min_median_and_max() {
    let temp = TempDir::new().expect("tempdir");
    let plan = plan(temp.path(), 3);
    let summary = summarize_baseline(
        &plan,
        vec![
            BaselineRun::new(1, plan.out_dir.join("run-001"), metrics(30, 9, 3, 90)),
            BaselineRun::new(2, plan.out_dir.join("run-002"), metrics(10, 3, 1, 30)),
            BaselineRun::new(3, plan.out_dir.join("run-003"), metrics(20, 6, 2, 60)),
        ],
    )
    .expect("summary");

    assert_eq!(
        summary.aggregates.scan_duration_ms,
        MetricSummary::new(10.0, 20.0, 30.0)
    );
    assert_eq!(
        summary.aggregates.rescan_duration_ms,
        MetricSummary::new(3.0, 6.0, 9.0)
    );
    assert_eq!(
        summary.aggregates.export_duration_ms,
        MetricSummary::new(30.0, 60.0, 90.0)
    );
    assert_eq!(
        summary.aggregates.sqlite_bytes,
        MetricSummary::new(1000.0, 2000.0, 3000.0)
    );
    assert_eq!(
        summary.aggregates.jsonl_records,
        MetricSummary::new(200.0, 200.0, 200.0)
    );
    assert_eq!(
        summary.aggregates.rows_per_second,
        Some(MetricSummary::new(11.0, 22.0, 33.0))
    );
}

#[test]
fn baseline_summary_serializes_per_run_metrics_and_aggregates() {
    let temp = TempDir::new().expect("tempdir");
    let plan = plan(temp.path(), 3);
    let summary = summarize_baseline(
        &plan,
        vec![
            BaselineRun::new(1, plan.out_dir.join("run-001"), metrics(30, 9, 3, 90)),
            BaselineRun::new(2, plan.out_dir.join("run-002"), metrics(10, 3, 1, 30)),
            BaselineRun::new(3, plan.out_dir.join("run-003"), metrics(20, 6, 2, 60)),
        ],
    )
    .expect("summary");

    let value = serde_json::to_value(&summary).expect("summary json");

    assert_eq!(value["runs"], 3);
    assert_eq!(value["samples"][0]["run_index"], 1);
    assert_eq!(value["samples"][0]["metrics"]["files"], 20);
    assert_eq!(value["samples"][0]["metrics"]["row_totals"]["files"], 20);
    assert_eq!(value["aggregates"]["scan_duration_ms"]["median"], 20.0);
    assert_eq!(value["aggregates"]["jsonl_bytes"]["max"], 6000.0);
    assert_eq!(value["aggregates"]["rows_per_second"]["median"], 22.0);
}

#[test]
fn baseline_summary_rejects_inconsistent_row_counts() {
    let temp = TempDir::new().expect("tempdir");
    let plan = plan(temp.path(), 3);
    let mut changed_metrics = metrics(20, 6, 2, 60);
    changed_metrics.row_totals.insert("files".to_string(), 999);

    let error = summarize_baseline(
        &plan,
        vec![
            BaselineRun::new(1, plan.out_dir.join("run-001"), metrics(30, 9, 3, 90)),
            BaselineRun::new(2, plan.out_dir.join("run-002"), changed_metrics),
            BaselineRun::new(3, plan.out_dir.join("run-003"), metrics(10, 3, 1, 30)),
        ],
    )
    .expect_err("inconsistent row totals must fail");

    assert!(
        error.to_string().contains("row totals changed"),
        "unexpected error: {error}"
    );
}

fn plan(root: &Path, runs: usize) -> xtask::performance::BaselinePlan {
    plan_baseline_from_args([
        "baseline",
        "--root",
        path_str(root),
        "--out-dir",
        path_str(&root.join("baseline")),
        "--binary",
        path_str(&root.join("bin/julie-extract")),
        "--runs",
        &runs.to_string(),
    ])
    .expect("baseline plan")
}

fn metrics(scan: u128, rescan: u128, info: u128, export: u128) -> DogfoodMetrics {
    let files = 20;
    let symbols = 200;
    let jsonl_records = 200;
    let sqlite_bytes = scan as u64 * 100;
    let jsonl_bytes = scan as u64 * 200;
    let mut row_totals = BTreeMap::new();
    row_totals.insert("files".to_string(), files);
    row_totals.insert("symbols".to_string(), symbols);
    let mut jsonl_records_by_kind = BTreeMap::new();
    jsonl_records_by_kind.insert("file".to_string(), files as usize);
    jsonl_records_by_kind.insert("symbol".to_string(), symbols as usize);

    DogfoodMetrics {
        sqlite_schema_version: 1,
        extract_contract_version: 1,
        jsonl_schema_version: 1,
        root_path: "/repo".to_string(),
        files,
        symbols,
        row_totals,
        jsonl_records_by_kind,
        jsonl_records,
        sqlite_bytes,
        jsonl_bytes,
        scan_duration_ms: scan,
        rescan_duration_ms: rescan,
        rescan_files_unchanged: files,
        rescan_files_changed: 0,
        rescan_files_deleted: 0,
        rescan_files_failed: 0,
        info_duration_ms: info,
        export_duration_ms: export,
        rows_per_second: Some(scan as f64 * 1.1),
    }
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf-8 path")
}
