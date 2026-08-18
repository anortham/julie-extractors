use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

use julie_extract_cli::limits::MAX_SOURCE_FILE_BYTES;
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
    assert_eq!(open_reference_resolution_gaps, 0);
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
fn scan_profile_splits_artifact_write_into_additive_sub_phases() {
    let fixture = FixtureRoot::with_file("src/lib.rs", "pub fn alpha() { beta(); }\n");
    std::fs::write(fixture.path("src/app.js"), "function run() { return 1; }\n").unwrap();
    let db = fixture.path("artifact.sqlite");

    let report = json_report(&julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));

    let phases = report["profile"]["phases"]
        .as_object()
        .expect("scan reports should include profile phases");
    let artifact_write = phases["artifact_write"]
        .as_u64()
        .expect("artifact_write phase should be present");

    let sub_phases = [
        "artifact_write_plan",
        "artifact_write_file_symbol_insert",
        "artifact_write_child_rows",
        "artifact_write_index_build",
        "artifact_write_foreign_key_check",
        "artifact_write_commit",
        "artifact_write_wal_checkpoint",
    ];
    let mut sum = 0;
    for key in sub_phases {
        sum += phases[key]
            .as_u64()
            .unwrap_or_else(|| panic!("sub-phase {key} should be present: {phases:#?}"));
    }

    assert!(
        sum <= artifact_write + 2,
        "sub-phases must partition artifact_write, not exceed it: sum={sum} artifact_write={artifact_write} {phases:#?}"
    );
    assert!(
        artifact_write - sum <= 30,
        "sub-phases must account for artifact_write: sum={sum} artifact_write={artifact_write} {phases:#?}"
    );
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
fn scan_writes_facts_only_and_omits_resolution_metadata() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");

    assert_success(scan(fixture.root_str(), &db));

    assert_eq!(table_count(&db, "pending_resolutions"), 0);
    assert_eq!(table_count(&db, "identifier_resolutions"), 0);
    assert_eq!(
        metadata_optional(&db, "reference_resolution_version"),
        None
    );
    assert_eq!(
        metadata_optional(&db, "reference_resolution_status"),
        None
    );
    assert_eq!(
        metadata_optional(&db, "reference_resolution_last_full_revision"),
        None
    );
}

#[test]
fn opening_a_prior_artifact_with_resolution_metadata_does_not_force_reextract() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));

    let connection = Connection::open(&db).unwrap();
    connection
        .execute(
            "INSERT OR REPLACE INTO artifact_metadata (key, value)
             VALUES ('reference_resolution_version', '1')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT OR REPLACE INTO artifact_metadata (key, value)
             VALUES ('reference_resolution_status', 'complete')",
            [],
        )
        .unwrap();
    let identifier_id: String = connection
        .query_row(
            "SELECT identifier_id FROM identifiers LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute(
            "INSERT OR REPLACE INTO identifier_resolutions
             (identifier_id, target_symbol_id, tier, confidence, method, outcome,
              candidates, resolved_at_revision)
             VALUES (?1, NULL, 1, 0.0, 'prior', 'missing', 0, 1)",
            [&identifier_id],
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
    assert_eq!(report["status"], "no_change");
    assert_eq!(report["revision"]["created_revision_id"], Value::Null);
    assert_eq!(report["counts"]["files_unchanged"], 2);
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|warning| warning["code"] != "resolution_upgraded")
    );
    assert_eq!(table_count(&db, "extraction_revisions"), 1);

    let update_output = update(fixture.root_str(), &db, "src/b.rs");
    assert_eq!(
        update_output.status.code(),
        Some(0),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&update_output.stdout),
        String::from_utf8_lossy(&update_output.stderr)
    );
    assert_ne!(
        json_report(&update_output)["errors"][0]["code"],
        "schema_migration_required"
    );

    let delete_output = delete(fixture.root_str(), &db, "src/b.rs");
    assert_eq!(
        delete_output.status.code(),
        Some(0),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&delete_output.stdout),
        String::from_utf8_lossy(&delete_output.stderr)
    );
    assert_eq!(json_report(&delete_output)["status"], "ok");
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
fn scan_writes_no_identifier_code_context() {
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

    assert_success(output);
    assert!(table_count(&db, "identifiers") > 0);
    let with_context = Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM identifiers WHERE code_context IS NOT NULL",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(with_context, 0);
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
    // 11 base metadata keys + the `index_level` key every scan stamps, minus the
    // deleted `updated_at`.
    assert_eq!(table_count(&db, "artifact_metadata"), 11);
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
    assert_eq!(report["artifact"]["jsonl_schema_version"], 5);
    assert_eq!(report["counts"]["rows_written"]["files"], 2);
    assert_eq!(report["counts"]["rows_written"]["symbols"], 3);
    let records = std::fs::read_to_string(&out).unwrap();
    let parsed = records
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(parsed[0]["kind"], "artifact");
    assert_eq!(parsed[0]["op"], "snapshot");
    assert_eq!(parsed[0]["jsonl_schema_version"], 5);
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
fn scan_removes_existing_rows_when_source_file_becomes_oversized() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    assert!(!symbols_for_path(&db, "src/a.rs").is_empty());

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
    assert_eq!(report["status"], "ok");
    assert_eq!(report["counts"]["files_deleted"], 1);
    assert_eq!(report["warnings"][0]["code"], "slow_file_skipped");
    assert_eq!(report["warnings"][0]["root_relative_path"], "src/a.rs");
    for domain in ["files", "symbols", "identifiers"] {
        assert_eq!(rows_for_path(&db, domain, "src/a.rs"), 0, "{domain}");
    }
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);

    let converged = json_report(&scan(fixture.root_str(), &db));
    assert_eq!(converged["status"], "no_change");
    assert_eq!(converged["counts"]["files_deleted"], 0);
}

#[test]
fn scan_indexes_a_source_file_at_exactly_the_size_limit() {
    let fixture = FixtureRoot::with_file(
        "src/limit.rs",
        &rust_source_of_exact_size("at_limit", MAX_SOURCE_FILE_BYTES),
    );
    let db = fixture.path("artifact.sqlite");

    let output = scan(fixture.root_str(), &db);

    assert_success(output);
    assert_eq!(symbols_for_path(&db, "src/limit.rs"), vec!["at_limit"]);
}

#[test]
fn scan_skips_a_source_file_one_byte_over_the_size_limit() {
    let fixture = FixtureRoot::with_file(
        "src/over.rs",
        &rust_source_of_exact_size("over_limit", MAX_SOURCE_FILE_BYTES + 1),
    );
    let db = fixture.path("artifact.sqlite");

    let output = scan(fixture.root_str(), &db);

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["counts"]["files_unsupported"], 1);
    assert_eq!(report["warnings"][0]["code"], "slow_file_skipped");
    assert_eq!(rows_for_path(&db, "files", "src/over.rs"), 0);
}

#[test]
fn update_oversized_supported_file_removes_rows_and_reports_slow_file_skipped() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    assert!(!symbols_for_path(&db, "src/a.rs").is_empty());

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
    assert_eq!(report["status"], "unsupported");
    assert_eq!(report["counts"]["files_unsupported"], 1);
    assert_eq!(report["counts"]["files_deleted"], 1);
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

    for domain in ["files", "symbols", "identifiers"] {
        assert_eq!(rows_for_path(&db, domain, "src/a.rs"), 0, "{domain}");
    }
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);
}

#[test]
fn update_reindexes_a_file_that_shrinks_back_under_the_size_limit() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    std::fs::write(
        fixture.path("src/a.rs"),
        "x".repeat(MAX_SOURCE_FILE_BYTES + 1),
    )
    .unwrap();
    assert_success(update(fixture.root_str(), &db, "src/a.rs"));
    assert_eq!(rows_for_path(&db, "files", "src/a.rs"), 0);

    std::fs::write(fixture.path("src/a.rs"), "pub fn regrown() {}\n").unwrap();
    let output = update(fixture.root_str(), &db, "src/a.rs");

    assert_success(output);
    assert_eq!(symbols_for_path(&db, "src/a.rs"), vec!["regrown"]);
}

#[test]
fn update_indexes_a_source_file_at_exactly_the_size_limit() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    std::fs::write(
        fixture.path("src/a.rs"),
        rust_source_of_exact_size("at_limit", MAX_SOURCE_FILE_BYTES),
    )
    .unwrap();

    let output = update(fixture.root_str(), &db, "src/a.rs");

    assert_success(output);
    assert_eq!(symbols_for_path(&db, "src/a.rs"), vec!["at_limit"]);
}

#[test]
fn update_skips_a_source_file_one_byte_over_the_size_limit() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(scan(fixture.root_str(), &db));
    std::fs::write(
        fixture.path("src/a.rs"),
        rust_source_of_exact_size("over_limit", MAX_SOURCE_FILE_BYTES + 1),
    )
    .unwrap();

    let output = update(fixture.root_str(), &db, "src/a.rs");

    assert_eq!(output.status.code(), Some(0));
    let report = json_report(&output);
    assert_eq!(report["status"], "unsupported");
    assert_eq!(report["warnings"][0]["code"], "slow_file_skipped");
    assert_eq!(rows_for_path(&db, "files", "src/a.rs"), 0);
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

#[test]
fn absent_fleet_safety_flags_leave_the_scan_report_and_the_filesystem_unchanged() {
    let bare_fixture = FixtureRoot::new();
    let bare_output = TempDir::new().unwrap();
    let db = bare_output.path().join("artifact.sqlite");
    // A spool this test owns the name of. A flagless scan must not reap the
    // system temporary directory, which is where it puts its own spool.
    let planted = std::env::temp_dir().join(format!(
        "julie-extract-scan-owned-spool-{}-1754000000000000000.jsonl",
        std::process::id()
    ));
    let planted_sentinel = std::path::PathBuf::from(format!("{}.lock", planted.display()));
    std::fs::write(&planted, b"planted").unwrap();
    std::fs::File::create(&planted_sentinel)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    let root_before = entry_names(&bare_fixture.root);

    let bare = json_report(&julie_extract(&[
        "scan",
        "--root",
        bare_fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]));

    assert_eq!(
        entry_names(&bare_fixture.root),
        root_before,
        "a flagless scan must add nothing to the scanned tree"
    );
    assert!(
        entry_names(bare_output.path()).iter().all(|name| [
            "artifact.sqlite",
            "artifact.sqlite-wal",
            "artifact.sqlite-shm"
        ]
        .contains(&name.as_str())),
        "a flagless scan must write nothing beside the artifact: {:?}",
        entry_names(bare_output.path())
    );
    assert!(
        planted.exists() && planted_sentinel.exists(),
        "reaping is opt-in; a flagless scan must not touch the system temporary directory"
    );
    std::fs::remove_file(&planted).unwrap();
    std::fs::remove_file(&planted_sentinel).unwrap();

    let flagged_fixture = FixtureRoot::new();
    let flagged_output = TempDir::new().unwrap();
    let progress = flagged_output.path().join("scan.progress");
    let flagged = json_report(&julie_extract(&[
        "scan",
        "--root",
        flagged_fixture.root_str(),
        "--db",
        path_str(&flagged_output.path().join("artifact.sqlite")),
        "--force",
        "--spool-dir",
        path_str(&flagged_output.path().join("spools")),
        "--progress-file",
        path_str(&progress),
        "--parent-pid",
        &std::process::id().to_string(),
        "--json",
    ]));

    assert_eq!(stable_report(&bare), stable_report(&flagged));
    assert!(progress.exists(), "the flagged scan should write progress");
}

#[test]
fn a_progress_file_pointed_at_the_artifact_or_a_sidecar_is_refused_by_the_name_rule() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]));
    let artifact_bytes = std::fs::metadata(&db).unwrap().len();
    assert!(artifact_bytes > 0);

    for collision in [
        db.clone(),
        std::path::PathBuf::from(format!("{}-wal", db.display())),
        std::path::PathBuf::from(format!("{}-shm", db.display())),
    ] {
        let output = julie_extract(&[
            "scan",
            "--root",
            fixture.root_str(),
            "--db",
            path_str(&db),
            "--progress-file",
            path_str(&collision),
            "--json",
        ]);

        assert_eq!(output.status.code(), Some(1));
        let report = json_report(&output);
        assert_eq!(report["status"], "failed");
        assert_eq!(report["errors"][0]["code"], "invalid_path");
        assert!(
            report["errors"][0]["message"]
                .as_str()
                .unwrap()
                .contains("`.progress`"),
            "{} must be refused by the name rule it actually hits: {report:#?}",
            collision.display()
        );
        assert_eq!(
            std::fs::metadata(&db).unwrap().len(),
            artifact_bytes,
            "{} must not truncate the artifact",
            collision.display()
        );
    }
}

#[test]
fn a_progress_file_that_is_an_artifact_named_progress_is_refused_by_the_collision_guard() {
    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    let db = output.path().join("index.progress");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]));
    let artifact_bytes = std::fs::metadata(&db).unwrap().len();
    assert!(artifact_bytes > 0);

    let refused = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--progress-file",
        path_str(&db),
        "--json",
    ]);

    assert_eq!(refused.status.code(), Some(1));
    let report = json_report(&refused);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "invalid_path");
    assert!(
        report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("must not be the artifact database"),
        "{report:#?}"
    );
    assert_eq!(
        std::fs::metadata(&db).unwrap().len(),
        artifact_bytes,
        "the artifact must not be truncated"
    );
}

#[test]
fn a_progress_file_named_only_progress_is_accepted() {
    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    let progress = output.path().join(".progress");

    let report = json_report(&julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&output.path().join("artifact.sqlite")),
        "--force",
        "--progress-file",
        path_str(&progress),
        "--json",
    ]));

    assert_eq!(report["status"], "ok");
    assert!(
        !progress_records(&progress).is_empty(),
        "a bare `.progress` dotfile is the obvious hidden spelling and must work"
    );
}

#[cfg(unix)]
#[test]
fn a_db_and_a_progress_file_that_are_one_file_through_a_symlink_are_refused() {
    let fixture = FixtureRoot::new();
    let store = TempDir::new().unwrap();
    let db = store.path().join("index.progress");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]));
    let artifact_bytes = std::fs::metadata(&db).unwrap().len();
    assert!(artifact_bytes > 0);

    let link = fixture.path("index.progress");
    std::os::unix::fs::symlink(&db, &link).unwrap();

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&link),
        "--progress-file",
        path_str(&link),
        "--json",
    ]);

    assert_eq!(
        std::fs::metadata(&db).unwrap().len(),
        artifact_bytes,
        "a symlinked artifact must not be truncated through the progress file"
    );
    assert_eq!(output.status.code(), Some(1));
    let report = json_report(&output);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "invalid_path");
}

#[test]
fn a_progress_file_hard_linked_to_the_artifact_leaves_it_byte_identical() {
    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    let db = output.path().join("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]));
    let before = std::fs::read(&db).unwrap();
    assert!(!before.is_empty());

    let progress = output.path().join("scan.progress");
    std::fs::hard_link(&db, &progress).unwrap();

    let refused = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--progress-file",
        path_str(&progress),
        "--json",
    ]);

    assert_eq!(
        std::fs::read(&db).unwrap(),
        before,
        "a second name for the artifact must not truncate it"
    );
    assert_eq!(refused.status.code(), Some(1));
    let report = json_report(&refused);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "invalid_path");
    assert!(
        report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("must not be the artifact database"),
        "{report:#?}"
    );
}

#[test]
fn a_progress_file_hard_linked_to_an_artifact_sidecar_is_refused() {
    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    let db = output.path().join("artifact.sqlite");
    let wal = output.path().join("artifact.sqlite-wal");
    std::fs::write(&wal, b"write-ahead log").unwrap();
    let progress = output.path().join("scan.progress");
    std::fs::hard_link(&wal, &progress).unwrap();

    let refused = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--progress-file",
        path_str(&progress),
        "--json",
    ]);

    assert_eq!(refused.status.code(), Some(1));
    let report = json_report(&refused);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "invalid_path");
    assert!(
        report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("-wal"),
        "{report:#?}"
    );
    assert_eq!(std::fs::read(&wal).unwrap(), b"write-ahead log");
    assert!(
        !db.exists(),
        "the refusal must happen before any scan work runs"
    );
}

#[test]
fn a_progress_file_a_case_insensitive_volume_makes_the_artifact_is_refused_after_it_is_created() {
    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    if !ignores_case(output.path()) {
        return;
    }
    let db = output.path().join("index.progress");
    let progress = output.path().join("INDEX.PROGRESS");

    let refused = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--progress-file",
        path_str(&progress),
        "--json",
    ]);

    assert_eq!(refused.status.code(), Some(1));
    let report = json_report(&refused);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "invalid_path");
    assert!(
        report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("must not be the artifact database"),
        "{report:#?}"
    );
    assert!(
        !db.exists(),
        "the empty progress file the refusal created must be removed again"
    );
}

#[cfg(unix)]
#[test]
fn a_progress_file_symlinked_to_another_progress_file_is_refused() {
    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    let target = output.path().join("real.progress");
    std::fs::write(&target, b"kept").unwrap();
    let link = output.path().join("link.progress");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let refused = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&output.path().join("artifact.sqlite")),
        "--progress-file",
        path_str(&link),
        "--json",
    ]);

    assert_eq!(refused.status.code(), Some(1));
    let report = json_report(&refused);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "invalid_path");
    assert_eq!(report["errors"][0]["path"], path_str(&link));
    assert_eq!(std::fs::read(&target).unwrap(), b"kept");
}

#[cfg(unix)]
#[test]
fn a_progress_file_symlinked_to_a_source_file_is_refused_by_the_name_rule() {
    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    let source = fixture.path("src/a.rs");
    let before = std::fs::read(&source).unwrap();
    let link = fixture.path("scan.progress");
    std::os::unix::fs::symlink(&source, &link).unwrap();

    let refused = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&output.path().join("artifact.sqlite")),
        "--progress-file",
        path_str(&link),
        "--json",
    ]);

    assert_eq!(
        std::fs::read(&source).unwrap(),
        before,
        "the name rule must apply to what the path resolves to"
    );
    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(json_report(&refused)["errors"][0]["code"], "invalid_path");
}

#[test]
fn a_progress_file_without_the_progress_extension_is_refused_before_it_truncates() {
    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    let source = fixture.path("src/a.rs");
    let before = std::fs::read(&source).unwrap();
    assert!(!before.is_empty());

    let refused = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&output.path().join("artifact.sqlite")),
        "--progress-file",
        path_str(&source),
        "--json",
    ]);

    assert_eq!(refused.status.code(), Some(1));
    let report = json_report(&refused);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "invalid_path");
    assert_eq!(report["errors"][0]["path"], path_str(&source));
    assert_eq!(
        std::fs::read(&source).unwrap(),
        before,
        "a mistyped progress path must never truncate the file it names"
    );
    assert!(
        !output.path().join("artifact.sqlite").exists(),
        "the refusal must happen before any scan work runs"
    );
}

#[test]
fn a_spool_dir_inside_the_root_does_not_change_the_scan_counts() {
    let baseline_fixture = FixtureRoot::new();
    let baseline_output = TempDir::new().unwrap();
    let baseline = json_report(&julie_extract(&[
        "scan",
        "--root",
        baseline_fixture.root_str(),
        "--db",
        path_str(&baseline_output.path().join("artifact.sqlite")),
        "--force",
        "--json",
    ]));

    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    let spool_dir = fixture.path("spools");
    std::fs::create_dir_all(&spool_dir).unwrap();
    // Held by a "concurrent scan" so the reaper leaves it alone and discovery is
    // the only thing standing between a live spool and being extracted as source.
    let (survivor, survivor_sentinel) = plant_spool_pair(&spool_dir, 4242, Duration::from_secs(1));
    std::fs::write(&survivor, "{\"root_relative_path\":\"src/a.rs\"}\n").unwrap();
    let holder = std::fs::File::open(&survivor_sentinel).unwrap();
    holder.lock().unwrap();

    let with_spool_dir = json_report(&julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&output.path().join("artifact.sqlite")),
        "--force",
        "--spool-dir",
        path_str(&spool_dir),
        "--json",
    ]));
    holder.unlock().unwrap();

    assert!(survivor.exists(), "a locked spool must survive the scan");
    assert_eq!(
        baseline["counts"]["files_scanned"], with_spool_dir["counts"]["files_scanned"],
        "a spool directory inside the root must not be discovered"
    );
    assert_eq!(
        baseline["counts"]["files_unsupported"],
        with_spool_dir["counts"]["files_unsupported"]
    );
    assert_eq!(
        stable_report_without_warnings(&baseline),
        stable_report_without_warnings(&with_spool_dir)
    );
}

#[test]
fn a_spool_dir_inside_the_root_warns_that_its_contents_are_excluded() {
    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    let spool_dir = fixture.path("src");

    let report = json_report(&julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&output.path().join("artifact.sqlite")),
        "--force",
        "--spool-dir",
        path_str(&spool_dir),
        "--json",
    ]));

    assert_eq!(report["status"], "ok");
    let warning = report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|warning| warning["code"] == "spool_dir_excluded")
        .unwrap_or_else(|| {
            panic!("a scan that silently drops a subtree must warn: {report:#?}");
        });
    assert_eq!(warning["root_relative_path"], "src");
    assert!(
        warning["message"]
            .as_str()
            .unwrap()
            .contains(&spool_dir.canonicalize().unwrap().display().to_string()),
        "the warning must name the excluded directory: {warning:#?}"
    );
    assert_eq!(
        report["counts"]["files_scanned"], 0,
        "the excluded subtree really is missing from the scan: {report:#?}"
    );
}

#[test]
fn a_spool_dir_outside_the_root_does_not_warn() {
    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();

    let report = json_report(&julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&output.path().join("artifact.sqlite")),
        "--force",
        "--spool-dir",
        path_str(&output.path().join("spools")),
        "--json",
    ]));

    assert_eq!(report["status"], "ok");
    assert_eq!(report["warnings"].as_array().unwrap().len(), 0);
}

#[test]
fn a_dedicated_scratch_spool_dir_inside_the_root_does_not_warn_on_any_scan() {
    // The per-workspace scratch path a consumer actually wires up. It excludes no
    // content, so a warning here would be permanent and unactionable on every
    // scan — and a warning channel nobody can act on stops being read.
    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    let spool_dir = fixture.path(".miller/spool");
    let db = output.path().join("artifact.sqlite");

    for pass in 0..2 {
        let report = json_report(&julie_extract(&[
            "scan",
            "--root",
            fixture.root_str(),
            "--db",
            path_str(&db),
            "--force",
            "--spool-dir",
            path_str(&spool_dir),
            "--json",
        ]));

        assert_eq!(report["status"], "ok");
        assert_eq!(
            report["warnings"].as_array().unwrap().len(),
            0,
            "pass {pass} must be silent: {report:#?}"
        );
    }
}

#[test]
fn a_failing_scan_still_reports_the_spool_dir_it_excluded() {
    // The run most likely to be read closely. Attached only on the success path,
    // the operator fixes the failure, reruns, sees `ok`, and never learns that
    // the first run excluded `src/`.
    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    let db = output.path().join("artifact.sqlite");
    std::fs::create_dir(&db).unwrap();

    let failed = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--spool-dir",
        path_str(&fixture.path("src")),
        "--json",
    ]);

    assert_eq!(failed.status.code(), Some(1));
    let report = json_report(&failed);
    assert_eq!(report["status"], "failed");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning["code"] == "spool_dir_excluded"),
        "a failed scan must still say what it excluded: {report:#?}"
    );
}

#[test]
fn a_force_scan_aborted_by_the_watchdog_leaves_an_unopenable_artifact_on_disk() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    std::fs::write(&db, b"not an artifact this scan can open").unwrap();
    let before = std::fs::read(&db).unwrap();
    let not_the_parent = std::process::id() + 1;

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--parent-pid",
        &not_the_parent.to_string(),
        "--json",
    ]);

    if cfg!(unix) {
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(json_report(&output)["errors"][0]["code"], "parent_exited");
        assert_eq!(
            std::fs::read(&db).unwrap(),
            before,
            "parent_exited is documented as leaving the artifact untouched"
        );
    } else {
        assert_success(output);
    }
}

#[test]
fn spool_dir_holds_the_spool_and_is_empty_after_a_successful_scan() {
    let fixture = FixtureRoot::new();
    let spool_dir = fixture.path("spools");
    std::fs::create_dir_all(&spool_dir).unwrap();

    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&fixture.path("artifact.sqlite")),
        "--force",
        "--spool-dir",
        path_str(&spool_dir),
        "--json",
    ]));

    assert_eq!(
        std::fs::read_dir(&spool_dir).unwrap().count(),
        0,
        "a completed scan must remove its own spool"
    );
}

#[test]
fn scan_startup_reaps_only_spools_that_no_live_process_owns() {
    let fixture = FixtureRoot::new();
    let spool_dir = fixture.path("spools");
    std::fs::create_dir_all(&spool_dir).unwrap();
    let (unowned, unowned_sentinel) = plant_spool_pair(&spool_dir, 4242, Duration::from_secs(3600));
    let (owned, owned_sentinel) = plant_spool_pair(&spool_dir, 4243, Duration::from_secs(3600));
    let (just_created, _) = plant_spool_pair(&spool_dir, 4244, Duration::ZERO);
    let sentinel_free = spool_dir.join("julie-extract-scan-owned-spool-4245-1754.jsonl");
    std::fs::File::create(&sentinel_free)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();

    let holder = std::fs::File::open(&owned_sentinel).unwrap();
    holder.lock().unwrap();
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&fixture.path("artifact.sqlite")),
        "--force",
        "--spool-dir",
        path_str(&spool_dir),
        "--json",
    ]));
    holder.unlock().unwrap();

    assert!(!unowned.exists(), "an unowned spool should be reaped");
    assert!(!unowned_sentinel.exists(), "its sentinel goes with it");
    assert!(owned.exists(), "a live process's spool must survive");
    assert!(
        just_created.exists(),
        "a spool whose sentinel is younger than the creation window must survive"
    );
    assert!(
        sentinel_free.exists(),
        "a spool with no sentinel can never be proved unowned"
    );
}

#[test]
fn scan_startup_keeps_a_sentinel_whose_spool_it_could_not_remove() {
    let fixture = FixtureRoot::new();
    let spool_dir = fixture.path("spools");
    std::fs::create_dir_all(&spool_dir).unwrap();
    let (spool, sentinel) = plant_spool_pair(&spool_dir, 4242, Duration::from_secs(3600));
    // A spool the reaper cannot unlink. The sentinel is the only thing that will
    // ever make it a candidate again, so removing it would leak the spool forever.
    std::fs::remove_file(&spool).unwrap();
    std::fs::create_dir(&spool).unwrap();
    std::fs::write(spool.join("blocker"), b"holds the directory").unwrap();

    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&fixture.path("artifact.sqlite")),
        "--force",
        "--spool-dir",
        path_str(&spool_dir),
        "--json",
    ]));

    assert!(spool.exists(), "the spool removal must have failed");
    assert!(
        sentinel.exists(),
        "a reapable leak must not be converted into a permanent one"
    );
}

#[test]
fn scan_without_a_spool_dir_never_touches_a_planted_spool() {
    let fixture = FixtureRoot::new();
    let spool_dir = fixture.path("spools");
    std::fs::create_dir_all(&spool_dir).unwrap();
    let (unowned, unowned_sentinel) = plant_spool_pair(&spool_dir, 4242, Duration::from_secs(3600));

    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&fixture.path("artifact.sqlite")),
        "--force",
        "--json",
    ]));

    assert!(
        unowned.exists() && unowned_sentinel.exists(),
        "reaping is opt-in; a scan without --spool-dir must remove nothing"
    );
}

#[test]
fn progress_file_records_advancing_counters_through_to_the_artifact_write() {
    let fixture = FixtureRoot::new();
    let progress = fixture.path("scan.progress");

    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&fixture.path("artifact.sqlite")),
        "--force",
        "--progress-file",
        path_str(&progress),
        "--json",
    ]));

    let records = progress_records(&progress);
    assert!(records.len() >= 2, "expected several records: {records:#?}");
    for record in &records {
        assert_eq!(record["progress_schema_version"], 1);
        assert!(record["pid"].as_u64().is_some());
        assert!(record["elapsed_ms"].as_u64().is_some());
    }
    let phases = records
        .iter()
        .map(|record| record["phase"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert!(phases.contains(&"discovery".to_string()));
    assert!(phases.contains(&"extraction_spool".to_string()));
    assert_eq!(phases.last().map(String::as_str), Some("artifact_write"));

    for counter in [
        "files_discovered",
        "files_supported",
        "files_extracted",
        "files_spooled",
    ] {
        let values = records
            .iter()
            .map(|record| record[counter].as_u64().unwrap())
            .collect::<Vec<_>>();
        assert!(
            values.windows(2).all(|pair| pair[0] <= pair[1]),
            "{counter} must never decrease: {values:?}"
        );
    }
    assert_eq!(records.last().unwrap()["files_spooled"], 2);
}

#[test]
fn a_progress_file_inside_the_root_is_not_scanned() {
    let baseline_fixture = FixtureRoot::new();
    let baseline_output = TempDir::new().unwrap();
    let baseline = json_report(&julie_extract(&[
        "scan",
        "--root",
        baseline_fixture.root_str(),
        "--db",
        path_str(&baseline_output.path().join("artifact.sqlite")),
        "--force",
        "--json",
    ]));

    let fixture = FixtureRoot::new();
    let output = TempDir::new().unwrap();
    let inside = fixture.path("src/scan.progress");
    let with_progress = json_report(&julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&output.path().join("artifact.sqlite")),
        "--force",
        "--progress-file",
        path_str(&inside),
        "--json",
    ]));

    assert!(
        inside.exists(),
        "the progress file should have been written"
    );
    assert_eq!(
        baseline["counts"]["files_scanned"], with_progress["counts"]["files_scanned"],
        "a progress file inside the root must not be discovered"
    );
    assert_eq!(
        baseline["counts"]["files_unsupported"],
        with_progress["counts"]["files_unsupported"]
    );
}

#[test]
fn parent_pid_that_is_not_the_parent_aborts_before_the_artifact_is_written() {
    let fixture = FixtureRoot::new();
    let db = fixture.path("artifact.sqlite");
    let spool_dir = fixture.path("spools");
    std::fs::create_dir_all(&spool_dir).unwrap();
    let not_the_parent = std::process::id() + 1;

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--spool-dir",
        path_str(&spool_dir),
        "--parent-pid",
        &not_the_parent.to_string(),
        "--json",
    ]);

    if cfg!(unix) {
        assert_eq!(output.status.code(), Some(1));
        let report = json_report(&output);
        assert_eq!(report["status"], "failed");
        assert_eq!(report["errors"][0]["code"], "parent_exited");
        assert_eq!(
            report["errors"][0]["details"]["expected_parent_pid"],
            not_the_parent
        );
        assert_eq!(
            report["errors"][0]["details"]["observed_parent_pid"],
            std::process::id()
        );
        assert!(!db.exists(), "an aborted scan must not create the artifact");
        assert_eq!(
            std::fs::read_dir(&spool_dir).unwrap().count(),
            0,
            "the abort must unwind through Drop and leave no spool"
        );
    } else {
        assert_success(output);
    }
}

fn stable_report(report: &Value) -> Value {
    let mut report = report.clone();
    let object = report.as_object_mut().unwrap();
    for volatile in ["profile", "revision", "artifact", "input"] {
        object.remove(volatile);
    }
    report
}

fn stable_report_without_warnings(report: &Value) -> Value {
    let mut report = stable_report(report);
    report.as_object_mut().unwrap().remove("warnings");
    report
}

fn progress_records(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// A spool plus the sentinel that makes it a removal candidate. The age is the
/// sentinel's, because the sentinel is what the reaper ages out.
fn plant_spool_pair(
    dir: &Path,
    pid: u32,
    age: Duration,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let spool = dir.join(format!(
        "julie-extract-scan-owned-spool-{pid}-1754000000000000000.jsonl"
    ));
    let sentinel = std::path::PathBuf::from(format!("{}.lock", spool.display()));
    std::fs::File::create(&spool).unwrap();
    std::fs::File::create(&sentinel)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
    (spool, sentinel)
}

fn ignores_case(dir: &Path) -> bool {
    let probe = dir.join("case-probe");
    std::fs::write(&probe, b"").unwrap();
    let ignored = dir.join("CASE-PROBE").exists();
    std::fs::remove_file(&probe).unwrap();
    ignored
}

fn entry_names(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

const TS_LEVELS_FIXTURE: &str = r#"
import { BroadcastMessage, AppSetting, Parameter } from "@/models"
import axios from "./apiConfig"

export async function getActiveMessages() {
    let response = await axios.get<BroadcastMessage[]>("/api/messages/active")
    return response.data
}

export async function saveParameter(parameter: Parameter) {
    let response = await axios.put<Parameter>("/api/parameter", parameter)
    return response.data
}
"#;

const SYMBOLS_LEVEL_GATED_DOMAINS: &[&str] = &[
    "identifiers",
    "literals",
    "type_argument_usages",
    "type_arguments",
    "source_regions",
    "structural_facts",
];

const SYMBOLS_LEVEL_KEPT_DOMAINS: &[&str] = &[
    "files",
    "symbols",
    "symbol_annotations",
    "relationships",
    "pending_relationships",
    "type_facts",
    "complexity_metrics",
    "parse_diagnostics",
];

fn index_level_metadata(db: &Path) -> Option<String> {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT value FROM artifact_metadata WHERE key = 'index_level'",
        [],
        |row| row.get(0),
    )
    .optional()
    .unwrap()
}

fn resolution_status_metadata(db: &Path) -> Option<String> {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT value FROM artifact_metadata WHERE key = 'reference_resolution_status'",
        [],
        |row| row.get(0),
    )
    .optional()
    .unwrap()
}

#[test]
fn scan_level_symbols_builds_a_symbol_core_artifact() {
    let fixture = FixtureRoot::with_file("src/messagesService.ts", TS_LEVELS_FIXTURE);
    let db = fixture.path("artifact.sqlite");

    let output = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--level",
        "symbols",
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
    assert_eq!(report["artifact"]["index_level"], "symbols");
    assert!(
        report["counts"]["rows_written"]["symbols"]
            .as_i64()
            .unwrap()
            >= 2
    );
    for domain in SYMBOLS_LEVEL_GATED_DOMAINS {
        assert_eq!(
            report["counts"]["totals"][*domain], 0,
            "{domain} must be empty at symbols level"
        );
        assert_eq!(
            table_count(&db, domain),
            0,
            "{domain} table must be empty at symbols level"
        );
    }
    assert_eq!(table_count(&db, "identifier_resolutions"), 0);
    assert_eq!(index_level_metadata(&db).as_deref(), Some("symbols"));
    assert_eq!(resolution_status_metadata(&db), None);
}

#[test]
fn symbols_level_keeps_symbol_core_domains_equal_to_full() {
    const GENERICS_FIXTURE: &str = "pub fn collect() { let ids: Vec<String> = Vec::<String>::new(); let n = \"42\".parse::<u32>().unwrap(); process(ids, n); }\npub fn process(v: Vec<String>, n: u32) {}\n";

    let full_fixture = FixtureRoot::with_file("src/messagesService.ts", TS_LEVELS_FIXTURE);
    std::fs::write(full_fixture.root.join("src/generics.rs"), GENERICS_FIXTURE).unwrap();
    let full_db = full_fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        full_fixture.root_str(),
        "--db",
        path_str(&full_db),
        "--json",
    ]));

    let symbols_fixture = FixtureRoot::with_file("src/messagesService.ts", TS_LEVELS_FIXTURE);
    std::fs::write(
        symbols_fixture.root.join("src/generics.rs"),
        GENERICS_FIXTURE,
    )
    .unwrap();
    let symbols_db = symbols_fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        symbols_fixture.root_str(),
        "--db",
        path_str(&symbols_db),
        "--level",
        "symbols",
        "--json",
    ]));

    for domain in SYMBOLS_LEVEL_KEPT_DOMAINS {
        assert_eq!(
            table_count(&full_db, domain),
            table_count(&symbols_db, domain),
            "{domain} must be identical between full and symbols levels"
        );
    }
    for domain in &[
        "identifiers",
        "literals",
        "type_argument_usages",
        "type_arguments",
        "source_regions",
    ] {
        assert!(
            table_count(&full_db, domain) > 0,
            "fixture must exercise {domain} at full level or the gating assertions are vacuous"
        );
        assert_eq!(table_count(&symbols_db, domain), 0);
    }
    assert_eq!(index_level_metadata(&full_db).as_deref(), Some("full"));
    assert_eq!(
        index_level_metadata(&symbols_db).as_deref(),
        Some("symbols")
    );
}

#[test]
fn scan_level_is_inherited_and_conflicts_are_usage_errors() {
    let fixture = FixtureRoot::with_file("src/messagesService.ts", TS_LEVELS_FIXTURE);
    let db = fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--level",
        "symbols",
        "--json",
    ]));

    std::fs::write(
        fixture.root.join("src/extra.ts"),
        "export function extra() { return 1 }\n",
    )
    .unwrap();
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
    assert_eq!(
        report["artifact"]["index_level"], "symbols",
        "a rescan without --level inherits the artifact's recorded level"
    );
    assert_eq!(table_count(&db, "identifiers"), 0);

    let conflict = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--level",
        "full",
        "--json",
    ]);
    assert_eq!(conflict.status.code(), Some(2));
    let report = json_report(&conflict);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "usage_error");
    assert_eq!(
        report["errors"][0]["details"]["artifact_index_level"],
        "symbols"
    );
    assert_eq!(
        report["errors"][0]["details"]["requested_index_level"],
        "full"
    );

    let force_conflict = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--level",
        "full",
        "--json",
    ]);
    assert_eq!(
        force_conflict.status.code(),
        Some(2),
        "--force does not license an in-place level change"
    );

    let force_inherit = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--force",
        "--json",
    ]);
    assert_eq!(force_inherit.status.code(), Some(0));
    let report = json_report(&force_inherit);
    assert_eq!(report["artifact"]["index_level"], "symbols");
    assert_eq!(table_count(&db, "identifiers"), 0);
}

#[test]
fn scan_default_level_is_full_and_symbols_conflict_errors() {
    let fixture = FixtureRoot::with_file("src/messagesService.ts", TS_LEVELS_FIXTURE);
    let db = fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));
    assert_eq!(index_level_metadata(&db).as_deref(), Some("full"));
    assert!(table_count(&db, "identifiers") > 0);

    let conflict = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--level",
        "symbols",
        "--json",
    ]);
    assert_eq!(conflict.status.code(), Some(2));
    assert_eq!(json_report(&conflict)["errors"][0]["code"], "usage_error");
}

#[test]
fn update_on_a_symbols_artifact_stays_at_symbols_level() {
    let fixture = FixtureRoot::with_file("src/messagesService.ts", TS_LEVELS_FIXTURE);
    let db = fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--level",
        "symbols",
        "--json",
    ]));

    std::fs::write(
        fixture.root.join("src/messagesService.ts"),
        TS_LEVELS_FIXTURE.replace("getActiveMessages", "getActiveMessagesV2"),
    )
    .unwrap();
    let output = julie_extract(&[
        "update",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--file",
        "src/messagesService.ts",
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
    assert_eq!(
        report["artifact"]["index_level"], "symbols",
        "a single-file update reports the artifact's recorded level"
    );
    assert_eq!(
        table_count(&db, "identifiers"),
        0,
        "a single-file update re-extracts at the artifact's recorded level"
    );
    assert_eq!(table_count(&db, "literals"), 0);
    let conn = Connection::open(&db).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM symbols ORDER BY name")
        .unwrap();
    let symbol_names = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<String>, _>>()
        .unwrap();
    assert!(
        symbol_names
            .iter()
            .any(|name| name == "getActiveMessagesV2"),
        "the update re-extracted the new symbol: {symbol_names:?}"
    );
}

#[test]
fn scan_and_update_on_an_unknown_index_level_fail_closed() {
    let fixture = FixtureRoot::with_file("src/messagesService.ts", TS_LEVELS_FIXTURE);
    let db = fixture.path("artifact.sqlite");
    assert_success(julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]));

    // The forward-compatibility shape: a newer julie-extract built this artifact at a level this
    // binary does not know. Extracting into it anyway would mix levels behind a wrong stamp.
    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE artifact_metadata SET value = 'minimal' WHERE key = 'index_level'",
        [],
    )
    .unwrap();
    drop(conn);

    std::fs::write(
        fixture.root.join("src/extra.ts"),
        "export function extra() { return 1 }\n",
    )
    .unwrap();
    let scan = julie_extract(&[
        "scan",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--json",
    ]);
    assert_eq!(scan.status.code(), Some(3));
    let report = json_report(&scan);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["errors"][0]["code"], "schema_incompatible");
    assert_eq!(
        report["errors"][0]["details"]["artifact_index_level"],
        "minimal"
    );

    std::fs::write(
        fixture.root.join("src/messagesService.ts"),
        TS_LEVELS_FIXTURE.replace("getActiveMessages", "getActiveMessagesV3"),
    )
    .unwrap();
    let update = julie_extract(&[
        "update",
        "--root",
        fixture.root_str(),
        "--db",
        path_str(&db),
        "--file",
        "src/messagesService.ts",
        "--json",
    ]);
    assert_eq!(update.status.code(), Some(3));
    assert_eq!(
        json_report(&update)["errors"][0]["code"],
        "schema_incompatible"
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

fn rows_for_path(db: &Path, table: &str, path: &str) -> i64 {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE path = ?1"),
        [path],
        |row| row.get(0),
    )
    .unwrap()
}

fn rust_source_of_exact_size(symbol: &str, size: usize) -> String {
    let mut source = format!("pub fn {symbol}() {{}}\n// ");
    assert!(source.len() <= size);
    let padding = size - source.len();
    source.push_str(&"x".repeat(padding));
    source
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

fn metadata_optional(db: &Path, key: &str) -> Option<String> {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT value FROM artifact_metadata WHERE key = ?1",
        [key],
        |row| row.get(0),
    )
    .optional()
    .unwrap()
}

fn metadata_value(db: &Path, key: &str) -> String {
    metadata_optional(db, key).unwrap_or_else(|| panic!("missing artifact metadata key {key}"))
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
