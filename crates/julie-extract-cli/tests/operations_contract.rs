use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Output};

use julie_extract_cli::limits::MAX_SOURCE_FILE_BYTES;
use julie_extract_cli::resolution::RESOLUTION_VERSION;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{Value, json};
use tempfile::TempDir;

const CAPABILITIES_JSON: &str = include_str!("../../../fixtures/extraction/capabilities.json");
const FILE_ATTRIBUTED_ROW_DOMAINS: &[&str] = &[
    "files",
    "symbols",
    "symbol_annotations",
    "reference_sites",
    "identifiers",
    "relationships",
    "pending_relationships",
    "type_facts",
    "type_argument_usages",
    "type_arguments",
    "literals",
    "source_regions",
    "structural_facts",
    "complexity_metrics",
    "parse_diagnostics",
];
const NON_FILE_ATTRIBUTED_ROW_DOMAINS: &[&str] = &[
    "artifact_metadata",
    "parser_inventory",
    "language_capabilities",
    "language_capability_fixtures",
    "language_capability_gaps",
    "extraction_revisions",
    "revision_file_changes",
];

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

fn json_report_from_stderr(output: &Output) -> Value {
    serde_json::from_slice(&output.stderr).unwrap_or_else(|err| {
        panic!(
            "stderr was not a JSON report: {err}\nstdout:\n{}\nstderr:\n{}",
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

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
    let reference_sites = table_count(&db, "reference_sites");
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
    assert!(
        reference_sites > 0,
        "scan must persist canonical reference sites"
    );
    let connection = Connection::open(&db).unwrap();
    let open_reference_resolution_gaps = connection
        .query_row(
            "SELECT COUNT(*)
             FROM language_capability_gaps
             WHERE capability LIKE 'reference_resolution.%'
               AND status = 'open'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    let unknown_gap_statuses = connection
        .query_row(
            "SELECT COUNT(*)
             FROM language_capability_gaps
             WHERE status NOT IN ('open', 'exception')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(open_reference_resolution_gaps, 105);
    assert_eq!(unknown_gap_statuses, 0);
    let rust_kind_coverage = language_kind_coverage(&db, "rust");
    assert_eq!(
        rust_kind_coverage["structural_facts"]["supported"],
        json!([
            "actix.attribute_route.v1",
            "actix.mount.v1",
            "actix.scope_route.v1",
            "axum.nest.v1",
            "axum.route.v1",
            "http.client_request.v1",
            "rust.unsafe_block.v1"
        ]),
        "language_capabilities.kind_coverage_json must persist structural fact pattern claims"
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
        report["counts"]["rows_written"]["reference_sites"],
        reference_sites
    );
    let revision_counts = latest_revision_counts(&db);
    assert_eq!(revision_counts["parser_inventory"], parser_inventory);
    assert_eq!(
        revision_counts["language_capabilities"],
        language_capabilities
    );
    assert_eq!(
        revision_counts["language_capability_fixtures"],
        language_capability_fixtures
    );
    assert_eq!(
        revision_counts["language_capability_gaps"],
        language_capability_gaps
    );
    assert_eq!(revision_counts["reference_sites"], reference_sites);
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
    assert_eq!(
        report["counts"]["totals"]["reference_sites"],
        reference_sites
    );
    assert_eq!(
        report["counts"]["totals"]["artifact_metadata"],
        table_count(&db, "artifact_metadata")
    );
    assert_eq!(report["counts"]["file_rows_truncated"], false);
    assert_eq!(report["counts"]["file_rows"].as_array().unwrap().len(), 2);
    assert_eq!(report["revision"]["created_revision_id"], 1);

    assert_eq!(table_count(&db, "files"), 2);
    assert!(table_count(&db, "symbols") >= 2);
    let source_region_count = table_count(&db, "source_regions");
    assert!(
        source_region_count >= 3,
        "scan should persist comment, doc-comment, and string-literal source regions"
    );
    assert_eq!(
        report["counts"]["rows_written"]["source_regions"],
        source_region_count
    );
    assert_eq!(
        report["counts"]["totals"]["source_regions"],
        source_region_count
    );
    let source_region_kinds = source_region_kinds_for_path(&db, "src/a.rs");
    assert!(source_region_kinds.contains(&"comment".to_string()));
    assert!(source_region_kinds.contains(&"doc_comment".to_string()));
    assert!(source_region_kinds.contains(&"string_literal".to_string()));
    let structural_facts = structural_facts_for_path(&db, "src/a.rs");
    assert!(
        structural_facts
            .iter()
            .any(|(pattern_id, capture_name, node_kind)| {
                pattern_id == "rust.unsafe_block.v1"
                    && capture_name == "unsafe_block"
                    && node_kind == "unsafe_block"
            }),
        "scan should persist a Rust unsafe-block structural fact, got {structural_facts:?}"
    );
    assert_eq!(
        report["counts"]["rows_written"]["structural_facts"],
        structural_facts.len() as i64
    );
    assert_eq!(
        report["counts"]["totals"]["structural_facts"],
        structural_facts.len() as i64
    );
    let complexity_scopes = complexity_metric_scopes_for_path(&db, "src/a.rs");
    assert!(
        complexity_scopes.contains(&"file".to_string()),
        "scan should persist a file complexity metric, got {complexity_scopes:?}"
    );
    assert!(
        complexity_scopes.contains(&"symbol".to_string()),
        "scan should persist a symbol complexity metric, got {complexity_scopes:?}"
    );
    let complexity_metric_count = table_count(&db, "complexity_metrics");
    assert_eq!(
        report["counts"]["rows_written"]["complexity_metrics"],
        complexity_metric_count
    );
    assert_eq!(
        report["counts"]["totals"]["complexity_metrics"],
        complexity_metric_count
    );
    assert_eq!(symbols_for_path(&db, "src/a.rs"), vec!["alpha", "helper"]);
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);
}

#[test]
fn scan_persists_same_span_verb_specific_route_facts() {
    let fixture = FixtureRoot::with_file(
        "routes/web.php",
        r#"<?php
use Illuminate\Support\Facades\Route;

Route::match(['get', 'post'], '/search', [SearchController::class, 'index']);
"#,
    );
    let rust_path = fixture.path("src/main.rs");
    std::fs::create_dir_all(rust_path.parent().unwrap()).unwrap();
    std::fs::write(
        rust_path,
        r#"use actix_web::{route, HttpResponse, Responder};
use axum::{routing::get, Router};

fn app() -> Router {
    Router::new().route("/items", get(list).post(create))
}

async fn list() {}
async fn create() {}

#[route("/thing", method = "GET", method = "POST")]
async fn thing() -> impl Responder {
    HttpResponse::Ok()
}
"#,
    )
    .unwrap();
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

    assert_persisted_route_verbs(
        &db,
        "routes/web.php",
        "laravel.route.v1",
        "/search",
        ["GET", "POST"],
    );
    assert_persisted_route_verbs(
        &db,
        "src/main.rs",
        "axum.route.v1",
        "/items",
        ["GET", "POST"],
    );
    assert_persisted_route_verbs(
        &db,
        "src/main.rs",
        "actix.attribute_route.v1",
        "/thing",
        ["GET", "POST"],
    );
}

#[test]
fn scan_report_includes_profile_phases_and_language_timings() {
    let fixture = FixtureRoot::with_file("src/lib.rs", "pub fn alpha() {}\n");
    std::fs::write(fixture.path("src/app.js"), "function run() { return 1; }\n").unwrap();
    let db = fixture.path("artifact.sqlite");

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]);

    assert_success(output);
    let report = json_report(&julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]));
    let profile = report["profile"]
        .as_object()
        .expect("scan reports should include a profile object");
    assert!(
        profile["total_duration_ms"].as_u64().is_some(),
        "profile should include total_duration_ms: {profile:#?}"
    );
    for phase in ["discovery", "extraction_spool", "artifact_write"] {
        assert!(
            profile["phases"][phase].as_u64().is_some(),
            "profile phase {phase} should be present: {profile:#?}"
        );
    }

    let languages = profile["languages"]
        .as_object()
        .expect("profile should include language timings");
    for language in ["rust", "javascript"] {
        let entry = languages
            .get(language)
            .and_then(|value| value.as_object())
            .unwrap_or_else(|| panic!("profile should include {language}: {languages:#?}"));
        assert_eq!(entry["files"].as_i64(), Some(1));
        assert_eq!(entry["changed_files"].as_i64(), Some(1));
        assert!(entry["bytes"].as_i64().unwrap_or_default() > 0);
        assert!(
            entry["read_duration_ms"].as_u64().is_some(),
            "language profile should include read timing: {entry:#?}"
        );
        assert!(
            entry["extract_duration_ms"].as_u64().is_some(),
            "language profile should include extract timing: {entry:#?}"
        );
        assert!(
            entry["spool_write_duration_ms"].as_u64().is_some(),
            "language profile should include spool write timing: {entry:#?}"
        );
    }
}

#[test]
fn scan_report_includes_profile_when_db_open_fails_after_extraction() {
    let fixture = FixtureRoot::with_file("src/lib.rs", "pub fn alpha() {}\n");
    let db = fixture.path("artifact.sqlite");
    std::fs::create_dir_all(&db).unwrap();

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "db_open_failed");
    let profile = report["profile"]
        .as_object()
        .expect("scan failure after extraction should include a profile");
    assert!(
        profile["phases"]["extraction_spool"].as_u64().is_some(),
        "profile should include extraction_spool phase: {profile:#?}"
    );
    assert!(
        profile["phases"]["writer_open"].as_u64().is_some(),
        "profile should include writer_open phase: {profile:#?}"
    );
    assert_eq!(profile["languages"]["rust"]["files"].as_i64(), Some(1));
    assert_eq!(
        profile["languages"]["rust"]["changed_files"].as_i64(),
        Some(1)
    );
}

#[test]
fn scan_promotes_test_role_metadata_to_indexed_sqlite_columns() {
    let fixture = FixtureRoot::with_file(
        "src/math.test.js",
        r#"
describe("math", () => {
  beforeEach(() => {});
  it("adds", () => {});
});
"#,
    );
    let db = fixture.path("artifact.sqlite");

    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));

    assert!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM symbols \
             WHERE is_test = 1 AND json_extract(metadata_json, '$.is_test') = 1",
        ) >= 1,
        "test cases must preserve metadata.is_test and expose indexed symbols.is_test"
    );
    assert!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM symbols \
             WHERE test_container = 1 AND json_extract(metadata_json, '$.test_container') = 1",
        ) >= 1,
        "test containers must preserve metadata.test_container and expose symbols.test_container"
    );
    assert!(
        scalar_i64(
            &db,
            "SELECT COUNT(*) FROM symbols \
             WHERE is_test = 1 \
               AND test_lifecycle = 1 \
               AND json_extract(metadata_json, '$.test_lifecycle') = 1",
        ) >= 1,
        "lifecycle hooks must preserve metadata.test_lifecycle and expose symbols.test_lifecycle"
    );
    assert!(
        query_plan(&db, "SELECT symbol_id FROM symbols WHERE is_test = 1")
            .contains("idx_symbols_is_test"),
        "test-symbol lookups must use the first-class test index"
    );
}

#[test]
fn scan_metadata_fingerprints_are_computed_sha256_hashes() {
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

    let parser = metadata_value(&db, "parser_inventory_fingerprint");
    let capabilities = metadata_value(&db, "capability_snapshot_fingerprint");

    assert_sha256_fingerprint(&parser);
    assert_sha256_fingerprint(&capabilities);
    assert_ne!(parser, "sha256:parser-inventory-v1");
    assert_ne!(capabilities, "sha256:capability-snapshot-v1");
    assert_ne!(
        parser, capabilities,
        "independent metadata domains must not share one placeholder fingerprint"
    );
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
fn scan_reextracts_unchanged_files_when_resolution_version_is_stale() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    let expected_files = table_count(&db, "files");

    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE artifact_metadata SET value = '1' WHERE key = 'reference_resolution_version'",
            [],
        )
        .unwrap();
    drop(connection);

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
    assert_eq!(report["counts"]["files_changed"], expected_files);
    assert_eq!(report["counts"]["files_unchanged"], 0);
    assert_ne!(report["revision"]["created_revision_id"], Value::Null);
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["code"] == "resolution_upgraded")
    );
    assert_eq!(
        metadata_value(&db, "reference_resolution_version"),
        RESOLUTION_VERSION.to_string()
    );
}

#[test]
fn scan_upgrades_resolution_metadata_for_an_empty_artifact() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    let db = temp.path().join("artifact.sqlite");
    assert_success(scan(path_str(&root), &db));

    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE artifact_metadata SET value = '1' WHERE key = 'reference_resolution_version'",
            [],
        )
        .unwrap();
    drop(connection);

    let output = julie_extract(&[
        "scan",
        "--root",
        path_str(&root),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["code"] == "resolution_upgraded")
    );
    assert_eq!(
        metadata_value(&db, "reference_resolution_status"),
        "complete"
    );
    assert_eq!(
        metadata_value(&db, "reference_resolution_version"),
        RESOLUTION_VERSION.to_string()
    );
}

#[test]
fn stale_resolution_version_requires_full_scan_before_delete() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));

    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE artifact_metadata SET value = '1' WHERE key = 'reference_resolution_version'",
            [],
        )
        .unwrap();
    drop(connection);

    let output = delete(fixture.root_str(), &db, "src/b.rs");

    assert_eq!(output.status.code(), Some(3));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "schema_migration_required");
    assert_eq!(table_count(&db, "files"), 2);
}

#[test]
fn failed_resolution_status_requires_full_scan_before_update() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));

    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE artifact_metadata SET value = 'failed' WHERE key = 'reference_resolution_status'",
            [],
        )
        .unwrap();
    drop(connection);

    let output = update(fixture.root_str(), &db, "src/b.rs");

    assert_eq!(output.status.code(), Some(3));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "schema_migration_required");
    assert_eq!(report["errors"][0]["recoverable"], true);
    assert_eq!(
        report["errors"][0]["details"]["action"],
        "julie-extract scan"
    );
}

#[test]
fn scan_records_revision_when_only_capability_snapshot_changes() {
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
    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE parser_inventory SET source = 'stale' WHERE language = 'rust'",
        [],
    )
    .unwrap();
    drop(conn);

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
    assert_eq!(report["revision"]["created_revision_id"], 2);
    assert!(
        report["counts"]["rows_written"]["parser_inventory"]
            .as_i64()
            .unwrap()
            >= 1
    );
    assert_eq!(table_count(&db, "extraction_revisions"), 2);
    let revision_counts = latest_revision_counts(&db);
    assert!(
        revision_counts["parser_inventory"].as_i64().unwrap() >= 1,
        "capability-only revisions must record capability row counts: {revision_counts:#?}"
    );
    assert_eq!(revision_counts["files"], 0);
    assert_ne!(parser_inventory_source(&db, "rust"), "stale");
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

#[cfg(unix)]
#[test]
fn scan_preserves_existing_rows_when_discovery_cannot_read_directory() {
    use std::os::unix::fs::PermissionsExt;

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

    let src = fixture.path("src");
    let original_permissions = std::fs::metadata(&src).unwrap().permissions();
    std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o000)).unwrap();
    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);
    std::fs::set_permissions(&src, original_permissions).unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_report(&output);
    assert_eq!(report["status"], "partial");
    assert_eq!(report["counts"]["files_deleted"], 0);
    assert_eq!(report["errors"][0]["code"], "read_failed");
    assert_eq!(report["errors"][0]["root_relative_path"], "src");
    assert_eq!(symbols_for_path(&db, "src/a.rs"), vec!["alpha", "helper"]);
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);
}

#[test]
fn scan_allows_intentional_empty_supported_file_to_replace_old_symbols() {
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

    std::fs::write(fixture.root.join("src/a.rs"), "// intentionally empty\n").unwrap();

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["counts"]["files_failed"], 0);
    assert_eq!(report["counts"]["files_changed"], 1);
    assert_eq!(symbols_for_path(&db, "src/a.rs"), Vec::<String>::new());
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);
    assert_eq!(file_status_for_path(&db, "src/a.rs"), "indexed");
    assert_eq!(table_count(&db, "symbols"), 1);
    assert_eq!(table_count(&db, "extraction_revisions"), 2);
}

#[test]
fn scan_commits_valid_files_and_reports_partial_when_one_supported_file_fails() {
    let fixture = FixtureRoot::with_file("src/good.rs", "pub fn good() {}\n");
    let bad = fixture.path("src/bad.rs");
    std::fs::write(&bad, [0xff, 0xfe, 0xfd]).unwrap();
    let db = fixture.path("artifact.sqlite");

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_report(&output);
    assert_eq!(report["status"], "partial");
    assert_eq!(report["counts"]["files_scanned"], 2);
    assert_eq!(report["counts"]["files_failed"], 1);
    assert_eq!(report["counts"]["rows_written"]["files"], 2);
    assert_eq!(report["counts"]["rows_written"]["symbols"], 1);
    assert_eq!(report["counts"]["rows_written"]["parse_diagnostics"], 1);
    assert_eq!(report["errors"][0]["code"], "read_failed");
    assert_eq!(report["errors"][0]["root_relative_path"], "src/bad.rs");
    assert_eq!(report["profile"]["languages"]["rust"]["bytes"], 20);

    assert_eq!(symbols_for_path(&db, "src/good.rs"), vec!["good"]);
    assert_eq!(file_status_for_path(&db, "src/good.rs"), "indexed");
    assert_eq!(file_status_for_path(&db, "src/bad.rs"), "failed_preserved");
    assert_eq!(diagnostics_for_path(&db, "src/bad.rs"), vec!["error"]);
}

#[test]
fn scan_preserves_existing_symbols_when_changed_file_becomes_unreadable() {
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

    std::fs::write(fixture.root.join("src/a.rs"), [0xff, 0xfe, 0xfd]).unwrap();

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_report(&output);
    assert_eq!(report["status"], "partial");
    assert_eq!(report["counts"]["files_failed"], 1);
    assert_eq!(report["counts"]["files_unchanged"], 1);
    assert_eq!(report["counts"]["rows_written"]["files"], 1);
    assert_eq!(report["counts"]["rows_written"]["symbols"], 0);
    assert_eq!(report["counts"]["rows_written"]["parse_diagnostics"], 1);
    assert_eq!(report["errors"][0]["code"], "read_failed");
    assert_eq!(report["errors"][0]["root_relative_path"], "src/a.rs");

    assert_eq!(symbols_for_path(&db, "src/a.rs"), vec!["alpha", "helper"]);
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);
    assert_eq!(file_status_for_path(&db, "src/a.rs"), "failed_preserved");
    assert_eq!(diagnostics_for_path(&db, "src/a.rs"), vec!["error"]);
}

#[test]
fn resolution_upgrade_remains_blocked_when_a_source_file_cannot_be_reextracted() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));

    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE artifact_metadata SET value = '1' WHERE key = 'reference_resolution_version'",
            [],
        )
        .unwrap();
    drop(connection);
    std::fs::write(fixture.root.join("src/a.rs"), [0xff, 0xfe, 0xfd]).unwrap();

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(3));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert!(
        report["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == "read_failed")
    );
    assert!(
        report["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == "schema_migration_required")
    );
    assert_eq!(metadata_value(&db, "reference_resolution_status"), "failed");

    let update_output = update(fixture.root_str(), &db, "src/b.rs");
    assert_eq!(update_output.status.code(), Some(3));
    assert_eq!(
        json_report(&update_output)["errors"][0]["code"],
        "schema_migration_required"
    );
}

#[test]
fn forced_resolution_upgrade_remains_blocked_when_a_source_file_cannot_be_reextracted() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));

    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE artifact_metadata SET value = '1' WHERE key = 'reference_resolution_version'",
            [],
        )
        .unwrap();
    drop(connection);
    std::fs::write(fixture.root.join("src/a.rs"), [0xff, 0xfe, 0xfd]).unwrap();

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(3));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert!(
        report["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == "read_failed")
    );
    assert!(
        report["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == "schema_migration_required")
    );
    assert_eq!(metadata_value(&db, "reference_resolution_status"), "failed");

    let update_output = update(fixture.root_str(), &db, "src/b.rs");
    assert_eq!(update_output.status.code(), Some(3));
    assert_eq!(
        json_report(&update_output)["errors"][0]["code"],
        "schema_migration_required"
    );
}

#[cfg(unix)]
#[test]
fn scan_does_not_follow_symlinked_paths_outside_root() {
    let fixture = FixtureRoot::with_file("src/local.rs", "pub fn local() {}\n");
    let outside = fixture._temp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.rs"), "pub fn secret() {}\n").unwrap();
    std::os::unix::fs::symlink(&outside, fixture.path("vendor_link")).unwrap();
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
    assert_eq!(table_count(&db, "files"), 1);
    assert_eq!(symbols_for_path(&db, "src/local.rs"), vec!["local"]);
    assert_eq!(
        symbols_for_path(&db, "vendor_link/secret.rs"),
        Vec::<String>::new()
    );
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
fn update_ignored_file_records_update_revision_with_unsupported_change() {
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
    std::fs::write(fixture.path(".gitignore"), "src/a.rs\n").unwrap();

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
    assert_eq!(report["status"], "unsupported");
    assert_eq!(report["operation"], "update");
    assert_eq!(report["counts"]["files_deleted"], 1);
    assert_eq!(report["warnings"][0]["code"], "unsupported_file");
    assert_eq!(
        latest_revision_operation_and_change(&db),
        Some(("update".to_string(), "unsupported".to_string()))
    );
    assert_eq!(symbols_for_path(&db, "src/a.rs"), Vec::<String>::new());
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);
}

#[test]
fn update_ignored_missing_row_reports_no_artifact_rows_removed() {
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
    assert_success(julie_extract(&[
        "delete",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--file",
        "src/a.rs",
        "--json",
    ]));
    let revisions_after_delete = table_count(&db, "extraction_revisions");
    std::fs::write(fixture.path(".gitignore"), "src/a.rs\n").unwrap();

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
    assert_eq!(report["status"], "unsupported");
    assert_eq!(report["revision"]["created_revision_id"], Value::Null);
    assert_eq!(report["counts"]["files_deleted"], 0);
    assert_eq!(
        report["warnings"][0]["message"],
        "file is ignored or unsupported and no artifact rows exist"
    );
    assert_eq!(
        table_count(&db, "extraction_revisions"),
        revisions_after_delete
    );
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
    let revisions_after_delete = table_count(&db, "extraction_revisions");

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
    assert_eq!(report["revision"]["created_revision_id"], Value::Null);
    assert_eq!(report["counts"]["files_deleted"], 0);
    assert_eq!(
        table_count(&db, "extraction_revisions"),
        revisions_after_delete
    );
}

#[test]
fn delete_missing_artifact_reports_not_found_without_creating_sqlite_file() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("missing.sqlite");

    let output = julie_extract(&[
        "delete",
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
    assert_eq!(report["status"], "not_found");
    assert_eq!(report["operation"], "delete");
    assert_eq!(report["counts"]["files_deleted"], 0);
    assert!(
        !db.exists(),
        "delete against a missing artifact must not create {}",
        db.display()
    );
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
fn info_json_reports_per_file_extraction_row_attribution() {
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
    let file_rows = report["counts"]["file_rows"]
        .as_array()
        .expect("info reports should include file row attribution");
    assert_eq!(report["counts"]["file_rows_truncated"], false);
    assert_eq!(file_rows.len(), 2);
    assert_eq!(file_rows[0]["path"], "src/a.rs");
    assert_eq!(file_rows[0]["language"], "rust");
    assert_eq!(file_rows[0]["status"], "indexed");
    assert!(
        file_rows[0]["total_rows"].as_i64().unwrap() > file_rows[1]["total_rows"].as_i64().unwrap(),
        "file_rows should be sorted by descending artifact row footprint: {file_rows:#?}"
    );
    assert_eq!(file_rows[1]["path"], "src/b.rs");

    let attributed_total: i64 = file_rows
        .iter()
        .map(|entry| entry["total_rows"].as_i64().unwrap())
        .sum();
    let domain_total: i64 = FILE_ATTRIBUTED_ROW_DOMAINS
        .iter()
        .map(|domain| report["counts"]["totals"][*domain].as_i64().unwrap())
        .sum();
    assert_eq!(attributed_total, domain_total);

    for domain in FILE_ATTRIBUTED_ROW_DOMAINS {
        assert_eq!(
            sum_report_file_rows(file_rows, domain),
            report["counts"]["totals"][*domain].as_i64().unwrap(),
            "per-file {domain} rows should sum to artifact totals"
        );
    }
    for domain in NON_FILE_ATTRIBUTED_ROW_DOMAINS {
        assert_eq!(
            sum_report_file_rows(file_rows, domain),
            0,
            "{domain} rows are artifact/revision-level and should not be attributed to files"
        );
    }
    assert_eq!(artifact_fingerprint(&db), before);
}

#[test]
fn info_reports_missing_noncritical_metadata_as_warning() {
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
    let conn = Connection::open(&db).unwrap();
    conn.execute("DELETE FROM artifact_metadata WHERE key = 'updated_at'", [])
        .unwrap();
    drop(conn);

    let output = julie_extract(&["info", "--db", path_str(&db), "--json"]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["operation"], "info");
    assert_eq!(report["warnings"][0]["code"], "metadata_missing");
    assert_eq!(
        report["warnings"][0]["details"]["missing_key"],
        "updated_at"
    );
    assert_eq!(report["counts"]["totals"]["files"], 2);
    assert_eq!(table_count(&db, "extraction_revisions"), 1);
    assert_eq!(table_count(&db, "files"), 2);
    assert_eq!(table_count(&db, "symbols"), 3);
    // 11 base metadata keys + 3 `reference_resolution_*` keys written by the
    // resolution pass, minus the deleted `updated_at`.
    assert_eq!(table_count(&db, "artifact_metadata"), 13);
    assert_eq!(before.len(), artifact_fingerprint(&db).len() + 1);
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
    assert_eq!(report["artifact"]["jsonl_schema_version"], 4);
    assert_eq!(report["counts"]["rows_written"]["files"], 2);
    assert_eq!(report["counts"]["rows_written"]["symbols"], 3);
    let records = std::fs::read_to_string(&out).unwrap();
    let parsed = records
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(parsed[0]["kind"], "artifact");
    assert_eq!(parsed[0]["op"], "snapshot");
    assert_eq!(parsed[0]["jsonl_schema_version"], 4);
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
    assert!(
        parsed
            .iter()
            .any(|record| record["kind"] == "source_region")
    );
    assert!(
        parsed
            .iter()
            .any(|record| record["kind"] == "structural_fact")
    );
}

#[test]
fn failed_file_jsonl_export_preserves_existing_output_file() {
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
    std::fs::write(&out, "previous export\n").unwrap();
    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE files SET metadata_json = '{' WHERE path = 'src/a.rs'",
        [],
    )
    .unwrap();
    drop(conn);

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

    assert_eq!(output.status.code(), Some(1));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "export_failed");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "previous export\n");
}

#[test]
fn failed_stdout_jsonl_export_writes_report_to_stderr() {
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
    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE files SET metadata_json = '{' WHERE path = 'src/a.rs'",
        [],
    )
    .unwrap();

    let output = julie_extract(&[
        "export",
        "--db",
        path_str(&db),
        "--format",
        "jsonl",
        "--out",
        "-",
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report = json_report_from_stderr(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["operation"], "export");
    assert_eq!(report["errors"][0]["code"], "export_failed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let records = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(
        !records.is_empty(),
        "export should have emitted partial JSONL before the corrupt row"
    );
    assert!(
        records.iter().all(|record| record.get("kind").is_some()),
        "stdout must contain only JSONL records, got:\n{stdout}"
    );
}

#[test]
fn scan_treats_supported_extensions_case_insensitively() {
    let fixture = FixtureRoot::with_file("src/A.TS", "export function alpha() { return 1; }\n");
    let db = fixture.path("artifact.sqlite");

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["counts"]["files_unsupported"], 0);
    assert_eq!(file_language_for_path(&db, "src/A.TS"), "typescript");
    assert!(symbols_for_path(&db, "src/A.TS").contains(&"alpha".to_string()));
}

#[test]
fn scan_reports_slow_file_skipped_warning_for_oversized_source_file() {
    let oversized_contents = "x".repeat(MAX_SOURCE_FILE_BYTES + 1);
    let fixture = FixtureRoot::with_file("src/huge.rs", &oversized_contents);
    let db = fixture.path("artifact.sqlite");

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["counts"]["files_unsupported"], 1);
    assert_eq!(report["warnings"][0]["code"], "slow_file_skipped");
    assert_eq!(report["warnings"][0]["root_relative_path"], "src/huge.rs");
}

#[test]
fn scan_preserves_existing_rows_when_source_file_becomes_oversized() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    let before = symbols_for_path(&db, "src/a.rs");
    assert!(!before.is_empty());

    std::fs::write(
        fixture.path("src/a.rs"),
        "x".repeat(MAX_SOURCE_FILE_BYTES + 1),
    )
    .unwrap();

    let output = scan(fixture.root_str(), &db);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_report(&output);
    assert_eq!(report["warnings"][0]["code"], "slow_file_skipped");
    assert_eq!(report["warnings"][0]["root_relative_path"], "src/a.rs");
    assert_eq!(symbols_for_path(&db, "src/a.rs"), before);
}

#[test]
fn resolution_upgrade_remains_blocked_when_a_source_file_is_oversized() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    let before = symbols_for_path(&db, "src/a.rs");
    assert!(!before.is_empty());

    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE artifact_metadata SET value = '1' WHERE key = 'reference_resolution_version'",
            [],
        )
        .unwrap();
    drop(connection);
    std::fs::write(
        fixture.path("src/a.rs"),
        "x".repeat(MAX_SOURCE_FILE_BYTES + 1),
    )
    .unwrap();

    let output = scan(fixture.root_str(), &db);

    assert_eq!(output.status.code(), Some(3));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["code"] == "slow_file_skipped")
    );
    assert!(
        report["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == "schema_migration_required")
    );
    assert_eq!(symbols_for_path(&db, "src/a.rs"), before);
    assert_eq!(metadata_value(&db, "reference_resolution_status"), "failed");

    let update_output = update(fixture.root_str(), &db, "src/b.rs");
    assert_eq!(update_output.status.code(), Some(3));
    assert_eq!(
        json_report(&update_output)["errors"][0]["code"],
        "schema_migration_required"
    );
}

#[test]
fn update_oversized_supported_file_preserves_rows_and_reports_slow_file_skipped() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    let before = symbols_for_path(&db, "src/a.rs");
    assert!(!before.is_empty());

    std::fs::write(
        fixture.path("src/a.rs"),
        "x".repeat(MAX_SOURCE_FILE_BYTES + 1),
    )
    .unwrap();

    let output = update(fixture.root_str(), &db, "src/a.rs");
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report = json_report(&output);
    assert_eq!(report["operation"], "update");
    assert_eq!(report["status"], "no_change");
    assert_eq!(report["counts"]["files_deleted"], 0);
    assert_eq!(report["warnings"][0]["code"], "slow_file_skipped");
    assert_eq!(report["warnings"][0]["root_relative_path"], "src/a.rs");

    let diagnostic_codes = report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .chain(report["errors"].as_array().unwrap())
        .map(|entry| entry["code"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(!diagnostic_codes.contains(&"unsupported_file"));

    assert_eq!(symbols_for_path(&db, "src/a.rs"), before);
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);
}

#[test]
fn scan_persists_typescript_generic_client_call_url_literals() {
    let fixture = FixtureRoot::with_file(
        "src/messagesService.ts",
        r#"
import { BroadcastMessage, AppSetting, Parameter } from "@/models"
import axios from "./apiConfig"

export async function getActiveMessages() {
    let response = await axios.get<BroadcastMessage[]>("/api/messages/active")
    return response.data
}

export async function getAppSetting(id: string) {
    let response = await axios.get<AppSetting>(`/api/appsettings/${id}`)
    return response.data
}

export async function saveParameter(parameter: Parameter) {
    let response = await axios.put<Parameter>("/api/parameter", parameter)
    return response.data
}
"#,
    );
    let db = fixture.path("artifact.sqlite");

    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));

    let literals = literals_for_path(&db, "src/messagesService.ts");
    let active = literals
        .iter()
        .find(|literal| literal.literal_text == "/api/messages/active")
        .unwrap_or_else(|| panic!("expected active-message URL literal, got {literals:?}"));
    assert_eq!(active.kind, "url");
    assert_eq!(active.carrier.as_deref(), Some("axios.get"));
    assert_eq!(active.arg_position, 0);
    assert_eq!(
        active.containing_symbol_name.as_deref(),
        Some("getActiveMessages")
    );

    let app_setting = literals
        .iter()
        .find(|literal| literal.literal_text == "/api/appsettings/{}")
        .unwrap_or_else(|| panic!("expected app-setting URL literal, got {literals:?}"));
    assert_eq!(app_setting.kind, "url");
    assert_eq!(app_setting.carrier.as_deref(), Some("axios.get"));
    assert_eq!(app_setting.arg_position, 0);

    let parameter = literals
        .iter()
        .find(|literal| literal.literal_text == "/api/parameter")
        .unwrap_or_else(|| panic!("expected parameter URL literal, got {literals:?}"));
    assert_eq!(parameter.kind, "url");
    assert_eq!(parameter.carrier.as_deref(), Some("axios.put"));
    assert_eq!(parameter.arg_position, 0);
}

#[test]
fn scan_records_content_based_language_for_cpp_headers() {
    let fixture = FixtureRoot::with_file(
        "src/widget.h",
        "namespace demo { class Widget { public: void run(); }; }\n",
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

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(file_language_for_path(&db, "src/widget.h"), "cpp");
}

#[test]
fn scan_persists_parser_inventory_versions() {
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

    let missing_versions = scalar_i64(
        &db,
        "SELECT COUNT(*) FROM parser_inventory WHERE parser_version IS NULL OR parser_version = ''",
    );
    assert_eq!(missing_versions, 0);
}

#[test]
fn languages_json_emits_capability_snapshot_data() {
    let output = julie_extract(&["languages", "--json"]);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "ok");
    assert_eq!(report["operation"], "languages");
    let emitted = report["languages"]["languages"].as_array().unwrap();
    let expected_kind_coverage = expected_kind_coverage_by_language();
    let expected_names = expected_capability_languages();
    let emitted_names = emitted
        .iter()
        .map(|language| {
            language["language"]
                .as_str()
                .expect("language rows include language")
                .to_string()
        })
        .collect::<BTreeSet<_>>();

    assert_eq!(report["languages"]["total"], expected_names.len());
    assert_eq!(emitted_names, expected_names);
    for language in emitted {
        assert!(
            language["parser_crate"]
                .as_str()
                .is_some_and(|v| !v.is_empty())
        );
        assert!(
            language["extensions"]
                .as_array()
                .is_some_and(|v| !v.is_empty())
        );
        assert!(language["target_capabilities"].is_object());
        assert!(language["actual_capabilities"].is_object());
        let language_name = language["language"].as_str().unwrap();
        assert_eq!(
            language["kind_coverage"], expected_kind_coverage[language_name],
            "languages --json must expose exact kind_coverage for {language_name}"
        );
        assert!(
            language["kind_coverage"]["structural_facts"]["supported"].is_array(),
            "languages --json must expose structural fact pattern coverage for {language_name}"
        );
        assert!(language["fixtures"].as_i64().unwrap() > 0);
    }
}

#[test]
fn languages_json_test_detection_surface_is_documented() {
    let contract_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/test-evidence-v1.md");
    assert!(
        contract_path.is_file(),
        "the test-evidence-v1 consumer contract must exist"
    );
    let contract = std::fs::read_to_string(contract_path).unwrap();
    assert!(
        contract.contains("`julie-extract languages --json`"),
        "the test-evidence-v1 contract must name the public CLI surface"
    );
    for unit in ["test_case", "test_container", "test_lifecycle"] {
        assert!(
            contract.contains(unit),
            "the test-evidence-v1 contract must define the `{unit}` vocabulary unit"
        );
    }
    assert!(
        contract.contains("Consumer Gates"),
        "the test-evidence-v1 contract must document consumer absence gates"
    );
    assert!(
        contract.contains("language-native applicability"),
        "the test-evidence-v1 contract must forbid treating open gaps as construct proof"
    );
}

fn expected_capability_languages() -> BTreeSet<String> {
    let snapshot: Value = serde_json::from_str(CAPABILITIES_JSON).unwrap();
    snapshot["languages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|language| language["language"].as_str().unwrap().to_string())
        .collect()
}

fn expected_kind_coverage_by_language() -> BTreeMap<String, Value> {
    let snapshot: Value = serde_json::from_str(CAPABILITIES_JSON).unwrap();
    snapshot["languages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|language| {
            (
                language["language"].as_str().unwrap().to_string(),
                language["kind_coverage"].clone(),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Reference-resolution flow tests (Task 5)
// ---------------------------------------------------------------------------

fn scan(root: &str, db: &Path) -> Output {
    julie_extract(&["scan", "--root", root, "--db", path_str(db), "--json"])
}

fn update(root: &str, db: &Path, file: &str) -> Output {
    julie_extract(&[
        "update",
        "--root",
        root,
        "--db",
        path_str(db),
        "--file",
        file,
        "--json",
    ])
}

fn delete(root: &str, db: &Path, file: &str) -> Output {
    julie_extract(&[
        "delete",
        "--root",
        root,
        "--db",
        path_str(db),
        "--file",
        file,
        "--json",
    ])
}

fn symbol_id_for(db: &Path, name: &str) -> Option<String> {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT symbol_id FROM symbols WHERE name = ?1",
        [name],
        |row| row.get(0),
    )
    .optional()
    .unwrap()
}

fn identifier_target(db: &Path, name: &str) -> Option<String> {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT target_symbol_id FROM identifiers WHERE name = ?1",
        [name],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .unwrap()
    .flatten()
}

/// Serialize the two resolution overlay tables as ordered strings for a
/// byte-for-byte determinism comparison.
fn dump_resolution_tables(db: &Path) -> Vec<String> {
    let conn = Connection::open(db).unwrap();
    let mut dump = Vec::new();
    let mut pending = conn
        .prepare(
            "SELECT pending_relationship_id, target_symbol_id, tier, confidence, method, \
                    resolved_at_revision FROM pending_resolutions \
             ORDER BY pending_relationship_id",
        )
        .unwrap();
    let rows = pending
        .query_map([], |row| {
            Ok(format!(
                "pending|{}|{}|{}|{}|{}|{}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, f64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    dump.extend(rows);
    let mut identifiers = conn
        .prepare(
            "SELECT identifier_id, target_symbol_id, tier, confidence, method, outcome, \
                    candidates, resolved_at_revision FROM identifier_resolutions \
             ORDER BY identifier_id",
        )
        .unwrap();
    let rows = identifiers
        .query_map([], |row| {
            Ok(format!(
                "identifier|{}|{:?}|{:?}|{:?}|{}|{:?}|{}",
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<f64>>(3)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    dump.extend(rows);
    dump
}

/// Two-file fixture: a unique free function in `a.rs`, a cross-file caller in
/// `b.rs`. The call is deferred to a `pending_relationships` row (cross-file), so
/// the workspace pass must resolve it (tier 4 unique-global) and propagate the
/// target onto the co-located identifier.
fn cross_file_fixture() -> FixtureRoot {
    let fixture = FixtureRoot::with_file("src/a.rs", "pub fn produce_widget() {}\n");
    std::fs::write(
        fixture.path("src/b.rs"),
        "pub fn consume() { produce_widget(); }\n",
    )
    .unwrap();
    fixture
}

fn identifiers_without_resolution(db: &Path) -> i64 {
    let conn = Connection::open(db).expect("artifact opens");
    conn.query_row(
        "SELECT COUNT(*) FROM identifiers i \
         LEFT JOIN identifier_resolutions r ON r.identifier_id = i.identifier_id \
         WHERE r.identifier_id IS NULL",
        [],
        |row| row.get(0),
    )
    .expect("count query runs")
}

fn identifier_outcome(db: &Path, name: &str) -> Option<String> {
    let conn = Connection::open(db).expect("artifact opens");
    conn.query_row(
        "SELECT r.outcome FROM identifiers i \
         JOIN identifier_resolutions r ON r.identifier_id = i.identifier_id \
         WHERE i.name = ?1 LIMIT 1",
        [name],
        |row| row.get(0),
    )
    .optional()
    .expect("outcome query runs")
}

#[test]
fn touching_only_the_receiver_type_name_rechecks_the_resolution() {
    // A static-type resolution depends on the RECEIVER's name, not the member's.
    // Delta invalidation therefore has to sweep resolved identifiers by receiver
    // the way it already does for pending rows; keying on the member name alone
    // leaves `Color.Red` claiming an exact target after a second `Color` appears.
    let fixture = FixtureRoot::with_file(
        "src/color.cs",
        "namespace App { public enum Color { Red, Blue } }\n",
    );
    std::fs::write(
        fixture.path("src/consumer.cs"),
        "namespace App { public class Consumer { public int Run() { var c = Color.Red; return 0; } } }\n",
    )
    .unwrap();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    assert!(
        identifier_target(&db, "Red").is_some(),
        "the enum member must resolve through its enum type before the collision"
    );

    // A second `Color` makes the receiver ambiguous. It deliberately does NOT
    // declare `Red`, so the only touched names are `Color` and `Blue`. `update`
    // is the delta path; `scan` always forces a full pass and would sweep the row
    // for unrelated reasons.
    std::fs::write(
        fixture.path("src/other.cs"),
        "namespace Other { public enum Color { Blue } }\n",
    )
    .unwrap();
    assert_success(update(fixture.root_str(), &db, "src/other.cs"));
    assert_eq!(
        identifier_target(&db, "Red"),
        None,
        "an ambiguous receiver type must clear the target even though `Red` itself is untouched"
    );
}

#[test]
fn ambiguous_receiver_type_clears_a_static_type_resolution() {
    // Closing the reporting leak created a new class of resolved identifier: one
    // written by the generic chain on a span whose covering pending edge failed.
    // Nothing propagates those, so the recheck sweep has to own them or a stale
    // target outlives the workspace change that invalidated it.
    //
    // This pins the end-to-end demotion. It does not isolate the recheck-ownership
    // narrowing: it passes with that narrowing reverted, because both re-extraction
    // and the delta sweep independently clear the row. A case that isolates it
    // needs a full pass over an artifact whose identifier rows survive, which no
    // current code path produces.
    let fixture = FixtureRoot::with_file(
        "src/fixture.cs",
        "namespace App { public class Fixture { public static int Create() { return 1; } } }\n",
    );
    std::fs::write(
        fixture.path("src/consumer.cs"),
        "namespace App { public class Consumer { public int Run() { return Fixture.Create(); } } }\n",
    )
    .unwrap();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    assert!(
        identifier_target(&db, "Create").is_some(),
        "the static-type receiver must resolve this call before the collision"
    );

    // A second `Fixture` makes the receiver type non-unique, so the tier must decline.
    std::fs::write(
        fixture.path("src/other.cs"),
        "namespace Other { public class Fixture { public int Create() { return 2; } } }\n",
    )
    .unwrap();
    assert_success(scan(fixture.root_str(), &db));
    assert_eq!(
        identifier_target(&db, "Create"),
        None,
        "an ambiguous receiver type must clear the previously resolved target"
    );
}

#[test]
fn scan_records_an_outcome_for_every_identifier() {
    let fixture = FixtureRoot::with_file("src/a.rs", "pub fn produce_widget() {}\n");
    std::fs::write(
        fixture.path("src/b.rs"),
        "pub fn consume() { produce_widget(); absent_external(); }\n",
    )
    .unwrap();
    let db = fixture.path("artifact.sqlite");
    scan(fixture.root_str(), &db);

    assert_eq!(
        identifiers_without_resolution(&db),
        0,
        "a reference site whose covering pending edge failed must still record an outcome"
    );
    assert_eq!(
        identifier_outcome(&db, "absent_external").as_deref(),
        Some("missing"),
        "an unresolvable call belongs in the report as missing, not absent from it"
    );
}

#[test]
fn scan_resolves_cross_file_call_and_propagates_to_identifier() {
    let fixture = cross_file_fixture();
    let db = fixture.path("artifact.sqlite");
    let report = json_report(&scan(fixture.root_str(), &db));
    let resolution = &report["languages"]["reference_resolution"];
    assert_eq!(resolution["status"], "partial");
    assert_eq!(resolution["gated_languages"][0], "rust");
    let origin_total = resolution["origin_totals"]
        .as_object()
        .expect("origin totals must expose each evidence-row domain")
        .values()
        .map(|totals| totals["total"].as_i64().unwrap())
        .sum::<i64>();
    assert_eq!(resolution["totals"]["total"].as_i64(), Some(origin_total));

    assert_eq!(table_count(&db, "pending_resolutions"), 1);
    let target = symbol_id_for(&db, "produce_widget").expect("produce_widget symbol exists");
    assert_eq!(
        identifier_target(&db, "produce_widget").as_deref(),
        Some(target.as_str()),
        "the co-located call identifier must be propagated to the definition"
    );
    assert_eq!(
        metadata_value(&db, "reference_resolution_status"),
        "partial"
    );
    assert_eq!(
        metadata_value(&db, "reference_resolution_version"),
        RESOLUTION_VERSION.to_string()
    );
}

#[test]
fn incremental_update_fk_demotes_then_re_resolves() {
    // INVARIANT: rewriting the TARGET file so the callee disappears CASCADE-demotes
    // the resolution while the pending context survives (FK-first invalidation);
    // restoring the callee re-resolves it.
    let fixture = cross_file_fixture();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    assert_eq!(table_count(&db, "pending_resolutions"), 1);
    assert!(identifier_target(&db, "produce_widget").is_some());

    // Rename the callee away: the old symbol dies -> CASCADE removes the
    // resolution; the pending row (unresolved context) stays.
    std::fs::write(fixture.path("src/a.rs"), "pub fn produce_gadget() {}\n").unwrap();
    assert_success(update(fixture.root_str(), &db, "src/a.rs"));
    assert_eq!(
        table_count(&db, "pending_resolutions"),
        0,
        "resolution must demote when the target symbol dies"
    );
    assert_eq!(
        table_count(&db, "pending_relationships"),
        1,
        "the unresolved pending context must survive demotion"
    );
    assert_eq!(
        identifier_target(&db, "produce_widget"),
        None,
        "the identifier target must be cleared when its resolution is gone"
    );

    // Restore the callee: the fill sweep re-resolves the pending edge.
    std::fs::write(fixture.path("src/a.rs"), "pub fn produce_widget() {}\n").unwrap();
    assert_success(update(fixture.root_str(), &db, "src/a.rs"));
    assert_eq!(
        table_count(&db, "pending_resolutions"),
        1,
        "restoring the target must re-resolve the pending edge"
    );
    let target = symbol_id_for(&db, "produce_widget").unwrap();
    assert_eq!(
        identifier_target(&db, "produce_widget").as_deref(),
        Some(target.as_str())
    );
}

#[test]
fn uniqueness_regression_demotes_then_removal_re_resolves() {
    // INVARIANT: adding a second same-name symbol makes the target ambiguous ->
    // the previously resolved edge demotes; removing the collision re-resolves it.
    let fixture = cross_file_fixture();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    assert_eq!(table_count(&db, "pending_resolutions"), 1);

    // Add a colliding produce_widget in a new file via update.
    std::fs::write(fixture.path("src/c.rs"), "pub fn produce_widget() {}\n").unwrap();
    assert_success(update(fixture.root_str(), &db, "src/c.rs"));
    assert_eq!(
        table_count(&db, "pending_resolutions"),
        0,
        "two same-name candidates must demote the resolved edge (no best-guess)"
    );
    assert_eq!(identifier_target(&db, "produce_widget"), None);

    // Remove the collision: the edge resolves again (unique once more).
    assert_success(delete(fixture.root_str(), &db, "src/c.rs"));
    assert_eq!(
        table_count(&db, "pending_resolutions"),
        1,
        "removing the collision must re-resolve the edge"
    );
    assert!(identifier_target(&db, "produce_widget").is_some());
}

#[test]
fn incremental_scan_demotes_uniqueness_regression_from_skipped_file() {
    // INVARIANT: a normal incremental scan can introduce a new same-name target
    // while the caller file is skipped as unchanged. The resolved overlay must
    // still be rechecked and demoted; otherwise `reference_resolution_status`
    // overstates trust in stale rows.
    let fixture = cross_file_fixture();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    assert_eq!(table_count(&db, "pending_resolutions"), 1);

    std::fs::write(fixture.path("src/c.rs"), "pub fn produce_widget() {}\n").unwrap();
    let report = json_report(&scan(fixture.root_str(), &db));
    assert_eq!(
        table_count(&db, "pending_resolutions"),
        0,
        "scan must demote a previously resolved edge when a skipped caller becomes ambiguous"
    );
    assert_eq!(identifier_target(&db, "produce_widget"), None);
    assert_eq!(
        report["languages"]["reference_resolution"]["status"], "partial",
        "the scan report must not mark stale or gated resolution data complete"
    );
    assert_eq!(
        metadata_value(&db, "reference_resolution_status"),
        "partial"
    );
}

#[test]
fn two_identical_scans_produce_byte_identical_resolution_tables() {
    // INVARIANT: determinism — the same source scanned into two fresh artifacts
    // produces byte-identical resolution overlay tables.
    let fixture = cross_file_fixture();
    let db_one = fixture.path("one.sqlite");
    let db_two = fixture.path("two.sqlite");
    assert_success(scan(fixture.root_str(), &db_one));
    assert_success(scan(fixture.root_str(), &db_two));
    assert_eq!(
        dump_resolution_tables(&db_one),
        dump_resolution_tables(&db_two),
        "identical scans must produce identical resolution tables"
    );
    assert!(
        !dump_resolution_tables(&db_one).is_empty(),
        "the determinism comparison must be over non-empty resolution tables"
    );
}

#[test]
fn artifact_without_resolution_metadata_requires_full_scan_before_update() {
    let fixture = cross_file_fixture();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    assert_eq!(table_count(&db, "pending_resolutions"), 1);

    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "DELETE FROM pending_resolutions; \
             DELETE FROM identifier_resolutions; \
             UPDATE identifiers SET target_symbol_id = NULL; \
             DELETE FROM artifact_metadata WHERE key LIKE 'reference_resolution%';",
        )
        .unwrap();
    }
    assert_eq!(table_count(&db, "pending_resolutions"), 0);

    std::fs::write(
        fixture.path("src/b.rs"),
        "// touched\npub fn consume() { produce_widget(); }\n",
    )
    .unwrap();
    let update_output = update(fixture.root_str(), &db, "src/b.rs");
    assert_eq!(update_output.status.code(), Some(3));
    let update_report = json_report(&update_output);
    assert_eq!(update_report["status"], "failed");
    assert_eq!(
        update_report["errors"][0]["code"],
        "schema_migration_required"
    );

    assert_success(scan(fixture.root_str(), &db));
    assert_eq!(
        table_count(&db, "pending_resolutions"),
        1,
        "a full scan must backfill the resolution overlay"
    );
    assert!(identifier_target(&db, "produce_widget").is_some());
    assert_eq!(
        metadata_value(&db, "reference_resolution_status"),
        "partial"
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
            "// module comment\n/// Alpha docs\npub fn alpha() { let message = \"hello\"; unsafe { core::ptr::read_volatile(&0); } }\npub fn helper() { alpha(); }\n",
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

fn source_region_kinds_for_path(db: &Path, path: &str) -> Vec<String> {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare("SELECT kind FROM source_regions WHERE path = ?1 ORDER BY kind, source_region_id")
        .unwrap();
    stmt.query_map([path], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn structural_facts_for_path(db: &Path, path: &str) -> Vec<(String, String, String)> {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT pattern_id, capture_name, node_kind
             FROM structural_facts
             WHERE path = ?1
             ORDER BY pattern_id, structural_fact_id",
        )
        .unwrap();
    stmt.query_map([path], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn route_facts_for_path(db: &Path, path: &str, pattern_id: &str) -> Vec<RouteFactRow> {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT structural_fact_id, metadata_json
             FROM structural_facts
             WHERE path = ?1 AND pattern_id = ?2
             ORDER BY json_extract(metadata_json, '$.verb')",
        )
        .unwrap();
    stmt.query_map([path, pattern_id], |row| {
        let structural_fact_id: String = row.get(0)?;
        let metadata_json: String = row.get(1)?;
        let metadata: Value = serde_json::from_str(&metadata_json).unwrap();
        Ok(RouteFactRow {
            structural_fact_id,
            verb: metadata["verb"].as_str().unwrap().to_string(),
            route_template: metadata["route_template"].as_str().unwrap().to_string(),
        })
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

fn assert_persisted_route_verbs<const N: usize>(
    db: &Path,
    path: &str,
    pattern_id: &str,
    route_template: &str,
    expected_verbs: [&str; N],
) {
    let rows = route_facts_for_path(db, path, pattern_id);
    assert_eq!(
        rows.len(),
        expected_verbs.len(),
        "one persisted row per verb for {pattern_id}: {rows:#?}"
    );

    let ids = rows
        .iter()
        .map(|row| row.structural_fact_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        ids.len(),
        expected_verbs.len(),
        "verb-specific rows need distinct IDs for {pattern_id}: {rows:#?}"
    );

    let verbs = rows.iter().map(|row| row.verb.as_str()).collect::<Vec<_>>();
    assert_eq!(verbs, expected_verbs);
    assert!(
        rows.iter().all(|row| row.route_template == route_template),
        "all rows should keep route template {route_template:?}: {rows:#?}"
    );
}

fn complexity_metric_scopes_for_path(db: &Path, path: &str) -> Vec<String> {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT scope
             FROM complexity_metrics
             WHERE path = ?1
             ORDER BY scope, complexity_metric_id",
        )
        .unwrap();
    stmt.query_map([path], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn language_kind_coverage(db: &Path, language: &str) -> Value {
    let conn = Connection::open(db).unwrap();
    let json: String = conn
        .query_row(
            "SELECT kind_coverage_json FROM language_capabilities WHERE language = ?1",
            [language],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&json).unwrap()
}

#[derive(Debug)]
struct RouteFactRow {
    structural_fact_id: String,
    verb: String,
    route_template: String,
}

#[derive(Debug)]
struct LiteralRow {
    literal_text: String,
    kind: String,
    carrier: Option<String>,
    arg_position: i64,
    containing_symbol_name: Option<String>,
}

fn literals_for_path(db: &Path, path: &str) -> Vec<LiteralRow> {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT l.literal_text, l.kind, l.carrier, l.arg_position, s.name
             FROM literals l
             LEFT JOIN symbols s ON s.symbol_id = l.containing_symbol_id
             WHERE l.path = ?1
             ORDER BY l.literal_text, l.carrier",
        )
        .unwrap();
    stmt.query_map([path], |row| {
        Ok(LiteralRow {
            literal_text: row.get(0)?,
            kind: row.get(1)?,
            carrier: row.get(2)?,
            arg_position: row.get(3)?,
            containing_symbol_name: row.get(4)?,
        })
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn scalar_i64(db: &Path, sql: &str) -> i64 {
    let conn = Connection::open(db).unwrap();
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn query_plan(db: &Path, sql: &str) -> String {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    stmt.query_map([], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n")
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

fn file_status_for_path(db: &Path, path: &str) -> String {
    let conn = Connection::open(db).unwrap();
    conn.query_row("SELECT status FROM files WHERE path = ?1", [path], |row| {
        row.get(0)
    })
    .unwrap()
}

fn file_language_for_path(db: &Path, path: &str) -> String {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT language FROM files WHERE path = ?1",
        [path],
        |row| row.get(0),
    )
    .unwrap()
}

fn parser_inventory_source(db: &Path, language: &str) -> String {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT source FROM parser_inventory WHERE language = ?1",
        [language],
        |row| row.get(0),
    )
    .unwrap()
}

fn diagnostics_for_path(db: &Path, path: &str) -> Vec<String> {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare("SELECT kind FROM parse_diagnostics WHERE path = ?1 ORDER BY diagnostic_id")
        .unwrap();
    stmt.query_map([path], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn latest_revision_operation_and_change(db: &Path) -> Option<(String, String)> {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT r.operation, c.change_kind
         FROM extraction_revisions r
         JOIN revision_file_changes c ON c.revision_id = r.revision_id
         ORDER BY r.revision_id DESC, c.path
         LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .unwrap()
}

fn metadata_value(db: &Path, key: &str) -> String {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT value FROM artifact_metadata WHERE key = ?1",
        [key],
        |row| row.get(0),
    )
    .unwrap()
}

fn latest_revision_counts(db: &Path) -> Value {
    let conn = Connection::open(db).unwrap();
    let counts_json: String = conn
        .query_row(
            "SELECT counts_json FROM extraction_revisions ORDER BY revision_id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&counts_json).unwrap()
}

fn sum_report_file_rows(file_rows: &[Value], domain: &str) -> i64 {
    file_rows
        .iter()
        .map(|entry| entry["rows"][domain].as_i64().unwrap())
        .sum()
}

fn assert_sha256_fingerprint(value: &str) {
    assert!(
        value.starts_with("sha256:"),
        "fingerprint must use sha256 prefix: {value}"
    );
    assert_eq!(value.len(), "sha256:".len() + 64);
    assert!(
        value["sha256:".len()..]
            .chars()
            .all(|ch| ch.is_ascii_digit() || ('a'..='f').contains(&ch)),
        "fingerprint digest must be lowercase hex: {value}"
    );
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
        "source_regions",
        "structural_facts",
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

#[derive(Debug)]
struct PendingSpanRow {
    terminal_name: String,
    start_line: i64,
    start_column: Option<i64>,
    end_line: Option<i64>,
    end_column: Option<i64>,
    start_byte: Option<i64>,
    end_byte: Option<i64>,
    pending_relationship_id: String,
}

fn pending_rows_for_path(db: &Path, path: &str) -> Vec<PendingSpanRow> {
    let conn = Connection::open(db).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT target_terminal_name, start_line, start_column, end_line, end_column,
                    start_byte, end_byte, pending_relationship_id
             FROM pending_relationships
             WHERE path = ?1
             ORDER BY start_byte, pending_relationship_id",
        )
        .unwrap();
    stmt.query_map([path], |row| {
        Ok(PendingSpanRow {
            terminal_name: row.get(0)?,
            start_line: row.get(1)?,
            start_column: row.get(2)?,
            end_line: row.get(3)?,
            end_column: row.get(4)?,
            start_byte: row.get(5)?,
            end_byte: row.get(6)?,
            pending_relationship_id: row.get(7)?,
        })
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

#[test]
fn scan_only_marks_audited_target_token_paths_as_exact() {
    let fixture = FixtureRoot::with_file(
        "src/caller.py",
        "from other import bar\n\n\ndef entry():\n    return bar()\n",
    );

    let cs = fixture.path("src/Source.cs");
    std::fs::write(
        &cs,
        "using OtherNs;\n\nnamespace Fixture;\n\npublic class Source\n{\n    public int Entry()\n    {\n        var x = new OtherClass();\n        return 0;\n    }\n}\n",
    )
    .unwrap();

    let ts = fixture.path("src/caller.ts");
    std::fs::write(
        &ts,
        "import { Foo } from './other';\n\nexport function entry(): number {\n    const x = new Foo();\n    return 0;\n}\n",
    )
    .unwrap();

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

    let exact_rows = pending_rows_for_path(&db, "src/caller.py");
    let exact = exact_rows
        .iter()
        .find(|row| row.terminal_name == "bar")
        .unwrap();
    assert!(exact.start_column.is_some());
    assert!(exact.end_line.is_some());
    assert!(exact.end_column.is_some());
    assert!(exact.start_byte.is_some());
    assert!(exact.end_byte.is_some());
    assert!(exact.end_byte.unwrap() > exact.start_byte.unwrap());

    for (path, terminal) in [("src/Source.cs", "OtherClass"), ("src/caller.ts", "Foo")] {
        let rows = pending_rows_for_path(&db, path);
        let row = rows
            .iter()
            .find(|r| r.terminal_name == terminal)
            .unwrap_or_else(|| {
                panic!("expected pending row for {terminal} in {path}; got {rows:#?}")
            });
        assert!(row.start_line >= 1);
        assert_eq!(row.start_column, None, "{path}: {row:?}");
        assert_eq!(row.end_line, None, "{path}: {row:?}");
        assert_eq!(row.end_column, None, "{path}: {row:?}");
        assert_eq!(row.start_byte, None, "{path}: {row:?}");
        assert_eq!(row.end_byte, None, "{path}: {row:?}");
    }
}

#[test]
fn scan_emits_distinct_pending_rows_for_same_line_duplicate_calls() {
    let fixture = FixtureRoot::with_file(
        "src/dup.py",
        "from other import bar\n\n\ndef entry():\n    return bar() + bar()\n",
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

    let rows: Vec<_> = pending_rows_for_path(&db, "src/dup.py")
        .into_iter()
        .filter(|r| r.terminal_name == "bar")
        .collect();
    assert_eq!(
        rows.len(),
        2,
        "two same-line bar() calls must yield two distinct pending rows; got {rows:#?}"
    );
    assert_eq!(
        rows[0].start_line, rows[1].start_line,
        "both occurrences are on the same source line"
    );
    assert_ne!(
        rows[0].pending_relationship_id, rows[1].pending_relationship_id,
        "same-line occurrences must have distinct pending_relationship_id"
    );
    assert_ne!(
        rows[0].start_byte, rows[1].start_byte,
        "same-line occurrences must have distinct start_byte"
    );
}

#[test]
fn scan_canonicalizes_one_attested_token_across_identifier_and_relationship_evidence() {
    let fixture = FixtureRoot::with_file(
        "src/main.c",
        "int target(void) { return 1; }\nint caller(void) { return target(); }\n",
    );
    let db = fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));

    let conn = Connection::open(db).unwrap();
    let shared_exact_sites = conn
        .query_row(
            "SELECT COUNT(*)
             FROM identifiers i
             JOIN relationships r ON r.reference_site_id = i.reference_site_id
             JOIN reference_sites s ON s.reference_site_id = i.reference_site_id
             WHERE i.name = 'target' AND r.kind = 'calls'
               AND s.is_exact = 1 AND s.provenance = 'target_token'
               AND r.metadata_json IS NULL",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();

    assert_eq!(shared_exact_sites, 1);
}
