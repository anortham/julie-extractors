use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Output};

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use tempfile::TempDir;

const CAPABILITIES_JSON: &str = include_str!("../../../fixtures/extraction/capabilities.json");

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
    assert_eq!(symbols_for_path(&db, "src/a.rs"), vec!["alpha", "helper"]);
    assert_eq!(symbols_for_path(&db, "src/b.rs"), vec!["beta"]);
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
    assert_eq!(table_count(&db, "artifact_metadata"), 10);
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
    assert_eq!(report["artifact"]["jsonl_schema_version"], 2);
    assert_eq!(report["counts"]["rows_written"]["files"], 2);
    assert_eq!(report["counts"]["rows_written"]["symbols"], 3);
    let records = std::fs::read_to_string(&out).unwrap();
    let parsed = records
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(parsed[0]["kind"], "artifact");
    assert_eq!(parsed[0]["op"], "snapshot");
    assert_eq!(parsed[0]["jsonl_schema_version"], 2);
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
        assert!(language["fixtures"].as_i64().unwrap() > 0);
    }
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
            "// module comment\n/// Alpha docs\npub fn alpha() { let message = \"hello\"; }\npub fn helper() { alpha(); }\n",
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
