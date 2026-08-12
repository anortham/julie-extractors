#![cfg(feature = "test-store-resolution-contract")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use julie_extract_cli::store::args::{StoreCli, StoreCommand, StoreRootCommand};
use julie_extract_cli::store::test_support::{
    write_all_language_fixture, write_v3_extraction_oracle,
};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

struct TempDir(PathBuf);

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

const CHILD_TABLES: [&str; 14] = [
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

const GLOBAL_TABLES: [&str; 4] = [
    "parser_inventory",
    "language_capabilities",
    "language_capability_fixtures",
    "language_capability_gaps",
];

const LOCAL_ID_COLUMNS: [&str; 21] = [
    "symbol_id",
    "parent_symbol_id",
    "annotation_id",
    "reference_site_id",
    "identifier_id",
    "relationship_id",
    "from_symbol_id",
    "to_symbol_id",
    "pending_relationship_id",
    "caller_scope_symbol_id",
    "type_fact_id",
    "usage_id",
    "type_argument_id",
    "parent_type_argument_id",
    "literal_id",
    "containing_symbol_id",
    "source_region_id",
    "structural_fact_id",
    "complexity_metric_id",
    "diagnostic_id",
    "target_symbol_id",
];

impl TempDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "julie-store-resolution-adapters-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn julie_extract(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .expect("julie-extract should start")
}

fn create_full_store(temp: &TempDir) -> PathBuf {
    let root = temp.path().join("source");
    let store = temp.path().join("family");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 1 }\n",
    )
    .unwrap();
    let import = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "full",
        "--json",
    ]);
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stdout)
    );
    store
}

fn create_full_language_store(temp: &TempDir) -> PathBuf {
    let root = temp.path().join("language-source");
    let store = temp.path().join("language-family");
    fs::create_dir_all(&root).unwrap();
    write_all_language_fixture(&root).unwrap();
    let import = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "full",
        "--json",
    ]);
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stdout)
    );
    store
}

fn create_legacy_artifact(temp: &TempDir) -> (PathBuf, PathBuf) {
    let root = temp.path().join("legacy-source");
    let artifact = temp.path().join("legacy.db");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("lib.rs"),
        "pub fn answer() -> i32 { helper() }\nfn helper() -> i32 { 1 }\n",
    )
    .unwrap();
    let scan = julie_extract(&[
        "scan",
        "--root",
        root.to_str().unwrap(),
        "--db",
        artifact.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        scan.status.success(),
        "{}",
        String::from_utf8_lossy(&scan.stdout)
    );
    (root, artifact)
}

fn create_gated_partial_artifact(temp: &TempDir) -> (PathBuf, PathBuf) {
    let root = temp.path().join("partial-source");
    let artifact = temp.path().join("partial.db");
    fs::create_dir_all(&root).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/extraction/rust/basic/source.rs"),
        root.join("source.rs"),
    )
    .unwrap();
    let scan = julie_extract(&[
        "scan",
        "--root",
        root.to_str().unwrap(),
        "--db",
        artifact.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        scan.status.success(),
        "{}",
        String::from_utf8_lossy(&scan.stdout)
    );
    (root, artifact)
}

fn resolve(store: &Path) {
    let output = julie_extract(&[
        "store",
        "resolve",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn export(store: &Path, output: &Path) -> std::process::Output {
    julie_extract(&[
        "store",
        "export",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--out",
        output.to_str().unwrap(),
        "--json",
    ])
}

fn wait_for_path(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn export_parser_has_no_coordinator_request_controls() {
    let parsed = StoreCli::try_parse_from([
        "julie-extract",
        "store",
        "export",
        "--store",
        "/tmp/family",
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--view",
        "view-main",
        "--out",
        "/tmp/export.db",
        "--json",
    ])
    .expect("public export syntax should parse");

    let StoreRootCommand::Store(store) = parsed.command;
    let StoreCommand::Export(args) = store.command else {
        panic!("expected export command");
    };
    assert_eq!(args.store, PathBuf::from("/tmp/family"));
    assert_eq!(
        args.family.as_deref(),
        Some("9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11")
    );
    assert_eq!(args.view, "view-main");
    assert_eq!(args.out, PathBuf::from("/tmp/export.db"));
    assert!(args.json);

    for control in [
        "--request-id",
        "--idempotency-key",
        "--request-timeout-seconds",
    ] {
        let mut argv = vec![
            "julie-extract",
            "store",
            "export",
            "--store",
            "/tmp/family",
            "--view",
            "view-main",
            "--out",
            "/tmp/export.db",
            control,
        ];
        argv.push(if control == "--request-timeout-seconds" {
            "30"
        } else {
            "forbidden"
        });
        assert!(StoreCli::try_parse_from(argv).is_err());
    }
}

#[test]
fn from_artifact_import_is_a_public_import_mode() {
    let parsed = StoreCli::try_parse_from([
        "julie-extract",
        "store",
        "import",
        "--store",
        "/tmp/family",
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        "/tmp/source",
        "--view",
        "view-main",
        "--from-artifact",
        "/tmp/legacy.db",
        "--json",
    ])
    .expect("from-artifact import should parse");

    let StoreRootCommand::Store(store) = parsed.command;
    assert!(matches!(store.command, StoreCommand::Import(_)));
}

#[test]
fn from_artifact_rejects_extraction_level_and_scan_controls() {
    for conflicting in [
        ["--level", "l1"],
        ["--ignore-file", "/tmp/ignore"],
        ["--jobs", "1"],
        ["--spool-dir", "/tmp/spool"],
        ["--progress-file", "/tmp/progress"],
        ["--parent-pid", "1"],
    ] {
        let mut argv = vec![
            "julie-extract",
            "store",
            "import",
            "--store",
            "/tmp/family",
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            "/tmp/source",
            "--view",
            "view-main",
            "--from-artifact",
            "/tmp/legacy.db",
        ];
        argv.extend(conflicting);
        assert!(StoreCli::try_parse_from(argv).is_err(), "{conflicting:?}");
    }
}

#[test]
fn invalid_v3_from_artifact_never_creates_store_or_coordinator() {
    let temp = TempDir::new();
    let root = temp.path().join("source");
    let store = temp.path().join("family");
    let artifact = temp.path().join("legacy.db");
    fs::create_dir_all(&root).unwrap();
    Connection::open(&artifact).unwrap();

    let output = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["operation"], "from_artifact");
    assert_eq!(report["failure_class"], "store_incompatible");
    assert!(!store.exists());
}

#[test]
fn from_artifact_rejects_a_different_canonical_root_before_store_creation() {
    let temp = TempDir::new();
    let (_, artifact) = create_legacy_artifact(&temp);
    let different_root = temp.path().join("different-source");
    let store = temp.path().join("family");
    fs::create_dir_all(&different_root).unwrap();

    let output = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        different_root.to_str().unwrap(),
        "--view",
        "view-main",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["operation"], "from_artifact");
    assert_eq!(report["failure_class"], "view_root_mismatch");
    assert!(!store.exists());
}

#[test]
fn from_artifact_rejects_incomplete_resolution_before_store_creation() {
    let temp = TempDir::new();
    let (root, artifact) = create_legacy_artifact(&temp);
    let store = temp.path().join("family");
    Connection::open(&artifact)
        .unwrap()
        .execute(
            "UPDATE artifact_metadata SET value='failed'
             WHERE key='reference_resolution_status'",
            [],
        )
        .unwrap();

    let output = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "resolution_input_incomplete");
    assert!(
        report["error"]["message"]
            .as_str()
            .unwrap()
            .contains("reference_resolution_status")
    );
    assert!(!store.exists());
}

#[test]
fn from_artifact_accepts_current_gated_partial_as_exact_store_resolution() {
    let temp = TempDir::new();
    let (root, artifact) = create_gated_partial_artifact(&temp);
    let store = temp.path().join("family");
    let fresh_store = temp.path().join("fresh-family");
    let source = Connection::open(&artifact).unwrap();
    let (status, last_full_revision, current_revision): (String, i64, i64) = source
        .query_row(
            "SELECT status.value,CAST(last_full.value AS INTEGER),MAX(revision.revision_id)
             FROM artifact_metadata AS status
             JOIN artifact_metadata AS last_full
             JOIN extraction_revisions AS revision
             WHERE status.key='reference_resolution_status'
               AND last_full.key='reference_resolution_last_full_revision'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, "partial");
    assert_eq!(last_full_revision, current_revision);
    assert!(
        source
            .query_row(
                "SELECT COUNT(*) FROM pending_relationships
                 WHERE pending_relationship_id NOT IN
                   (SELECT pending_relationship_id FROM pending_resolutions)",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
            > 0
    );

    let output = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--json",
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["state"], "committed");
    assert_eq!(report["resolution"]["state"], "exact");
    assert_eq!(report["resolution"]["exact_at_matches"], true);
    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    assert!(
        connection
            .query_row(
                "SELECT resolution_state='exact'
                        AND resolution_exact_at=current_generation
                 FROM views WHERE view_id='view-main'",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap()
    );
    let fresh = julie_extract(&[
        "store",
        "import",
        "--store",
        fresh_store.to_str().unwrap(),
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--level",
        "full",
        "--json",
    ]);
    assert!(
        fresh.status.success(),
        "{}",
        String::from_utf8_lossy(&fresh.stdout)
    );
    resolve(&fresh_store);
    let partial_export = temp.path().join("partial-roundtrip.db");
    let fresh_export = temp.path().join("fresh-roundtrip.db");
    let exported = export(&store, &partial_export);
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stdout)
    );
    let exported = export(&fresh_store, &fresh_export);
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stdout)
    );
    let partial_rows = normalized_v3_rows_with_resolution(&partial_export);
    let fresh_rows = normalized_v3_rows_with_resolution(&fresh_export);
    for table in ["identifier_resolutions", "pending_resolutions"] {
        assert_eq!(partial_rows.get(table), fresh_rows.get(table), "{table}");
    }
}

#[test]
fn from_artifact_rejects_stale_complete_and_partial_before_store_creation() {
    for status in ["complete", "partial"] {
        let temp = TempDir::new();
        let (root, artifact) = create_gated_partial_artifact(&temp);
        let store = temp.path().join("family");
        let connection = Connection::open(&artifact).unwrap();
        connection
            .execute(
                "UPDATE artifact_metadata SET value=?1
                 WHERE key='reference_resolution_status'",
                [status],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO extraction_revisions
                 (revision_id,parent_revision_id,operation,mode,started_at,completed_at,
                  binary_version,extract_contract_version,sqlite_schema_version,input_root,counts_json)
                 SELECT revision_id+1,revision_id,'update','incremental',started_at,completed_at,
                        binary_version,extract_contract_version,sqlite_schema_version,input_root,counts_json
                 FROM extraction_revisions ORDER BY revision_id DESC LIMIT 1",
                [],
            )
            .unwrap();

        let output = julie_extract(&[
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--from-artifact",
            artifact.to_str().unwrap(),
            "--json",
        ]);

        assert_eq!(output.status.code(), Some(1), "{status}");
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["failure_class"], "resolution_input_incomplete");
        assert!(
            report["error"]["message"]
                .as_str()
                .unwrap()
                .contains(&format!("{status} resolution is stale")),
            "{status}"
        );
        assert!(!store.exists(), "{status}");
    }
}

#[test]
fn from_artifact_rejects_current_partial_with_missing_identifier_resolution() {
    let temp = TempDir::new();
    let (root, artifact) = create_gated_partial_artifact(&temp);
    let store = temp.path().join("family");
    Connection::open(&artifact)
        .unwrap()
        .execute(
            "DELETE FROM identifier_resolutions
             WHERE identifier_id=(SELECT identifier_id FROM identifier_resolutions LIMIT 1)",
            [],
        )
        .unwrap();

    let output = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["failure_class"], "resolution_input_incomplete");
    assert!(
        report["error"]["message"]
            .as_str()
            .unwrap()
            .contains("incomplete for identifiers")
    );
    assert!(!store.exists());
}

#[test]
fn from_artifact_preflight_rejects_path_hash_and_epoch_mismatch_without_mutation() {
    for mutation in [
        "UPDATE files SET path='../escape.rs'",
        "UPDATE files SET content_hash='sha256:not-blake3'",
        "UPDATE artifact_metadata SET value='stale' WHERE key='parser_inventory_fingerprint'",
    ] {
        let temp = TempDir::new();
        let (root, artifact) = create_legacy_artifact(&temp);
        let store = temp.path().join("family");
        Connection::open(&artifact)
            .unwrap()
            .execute(mutation, [])
            .unwrap();
        let output = julie_extract(&[
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--from-artifact",
            artifact.to_str().unwrap(),
            "--json",
        ]);
        assert_eq!(output.status.code(), Some(1), "{mutation}");
        assert!(!store.exists(), "{mutation}");
    }
}

#[test]
fn current_v3_artifact_imports_full_rows_and_binds_exact() {
    let temp = TempDir::new();
    let (root, artifact) = create_legacy_artifact(&temp);
    let store = temp.path().join("family");

    let output = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--request-id",
        "from-artifact-1",
        "--idempotency-key",
        "from-artifact-key",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["operation"], "from_artifact");
    assert_eq!(report["state"], "committed");
    assert_eq!(report["manifest"]["generation"], 1);
    assert_eq!(report["completion"]["l1"], true);
    assert_eq!(report["completion"]["l2"], true);
    assert_eq!(report["completion"]["l3"], true);
    assert_eq!(report["resolution"]["state"], "exact");
    assert_eq!(report["resolution"]["exact_at_matches"], true);
    resolve(&store);

    let source = Connection::open(&artifact).unwrap();
    let imported = Connection::open(store.join("gen-001/store.db")).unwrap();
    for (source_table, imported_table) in [
        ("files", "file_versions"),
        ("symbols", "symbols"),
        ("symbol_annotations", "symbol_annotations"),
        ("reference_sites", "reference_sites"),
        ("identifiers", "identifiers"),
        ("relationships", "relationships"),
        ("pending_relationships", "pending_relationships"),
        ("type_facts", "type_facts"),
        ("type_argument_usages", "type_argument_usages"),
        ("type_arguments", "type_arguments"),
        ("literals", "literals"),
        ("source_regions", "source_regions"),
        ("structural_facts", "structural_facts"),
        ("complexity_metrics", "complexity_metrics"),
        ("parse_diagnostics", "parse_diagnostics"),
    ] {
        let source_count = source
            .query_row(&format!("SELECT COUNT(*) FROM {source_table}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        let imported_count = imported
            .query_row(
                &format!("SELECT COUNT(*) FROM {imported_table}"),
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(imported_count, source_count, "{source_table}");
    }
    assert!(
        imported
            .query_row(
                "SELECT resolution_state='exact'
                        AND resolution_exact_at=current_generation
                 FROM views WHERE view_id='view-main'",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap()
    );
    let coordinator = Connection::open(store.join("coord.db")).unwrap();
    assert_eq!(
        coordinator
            .query_row(
                "SELECT COUNT(*) FROM requests
                 WHERE request_id='from-artifact-1' AND kind='from_artifact' AND state='committed'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );

    let roundtrip = temp.path().join("roundtrip.db");
    let export = julie_extract(&[
        "store",
        "export",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--out",
        roundtrip.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        export.status.success(),
        "{}",
        String::from_utf8_lossy(&export.stdout)
    );
    let roundtrip_rows = normalized_v3_rows_with_resolution(&roundtrip);
    let source_rows = normalized_v3_rows_with_resolution(&artifact);
    assert!(source_rows.contains_key("identifier_resolutions"));
    assert!(source_rows.contains_key("pending_resolutions"));
    for (table, expected) in source_rows {
        assert_eq!(roundtrip_rows.get(&table), Some(&expected), "{table}");
    }
}

#[test]
fn committed_from_artifact_replay_survives_source_deletion_without_new_effects() {
    let temp = TempDir::new();
    let (root, artifact) = create_legacy_artifact(&temp);
    let store = temp.path().join("family");
    let family = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
    let first = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        family,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--request-id",
        "from-artifact-original",
        "--idempotency-key",
        "from-artifact-replay-key",
        "--json",
    ]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    let original: Value = serde_json::from_slice(&first.stdout).unwrap();
    fs::remove_file(&artifact).unwrap();

    let replay = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        family,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--request-id",
        "from-artifact-retry",
        "--idempotency-key",
        "from-artifact-replay-key",
        "--json",
    ]);
    assert!(
        replay.status.success(),
        "{}",
        String::from_utf8_lossy(&replay.stdout)
    );
    let replayed: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replayed["request"]["id"], "from-artifact-original");
    assert_eq!(replayed["manifest"], original["manifest"]);
    assert_eq!(replayed["row_counts"], original["row_counts"]);
    assert_eq!(replayed["resolution"], original["resolution"]);
    let store_db = Connection::open(store.join("gen-001/store.db")).unwrap();
    assert_eq!(
        store_db
            .query_row(
                "SELECT COUNT(*) FROM store_log WHERE terminal=1 AND request_id='from-artifact-original'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        Connection::open(store.join("coord.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM requests", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn identical_from_artifact_retry_with_a_new_key_skips_file_materialization() {
    let temp = TempDir::new();
    let (root, artifact) = create_legacy_artifact(&temp);
    let store = temp.path().join("family");
    let family = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
    for (request_id, idempotency_key) in [
        ("from-artifact-original", "from-artifact-original-key"),
        ("from-artifact-retry", "from-artifact-retry-key"),
    ] {
        let output = julie_extract(&[
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            family,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--from-artifact",
            artifact.to_str().unwrap(),
            "--request-id",
            request_id,
            "--idempotency-key",
            idempotency_key,
            "--json",
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (retry_materialization_chunks, retry_terminal_reuse, manifests, versions): (
        i64,
        i64,
        i64,
        i64,
    ) = connection
        .query_row(
            "SELECT
                   (SELECT COUNT(*) FROM store_log
                    WHERE request_id='from-artifact-retry'
                      AND event_kind='store_from_artifact_versions_written'),
                   (SELECT COUNT(*) FROM store_log
                    WHERE request_id='from-artifact-retry' AND terminal=1
                      AND event_kind='store_from_artifact_reused'),
                   (SELECT COUNT(*) FROM manifests WHERE view_id='view-main'),
                   (SELECT COUNT(*) FROM file_versions)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(retry_materialization_chunks, 0);
    assert_eq!(retry_terminal_reuse, 1);
    assert_eq!(manifests, 1);
    assert_eq!(versions, 1);
}

#[test]
fn changed_from_artifact_content_with_a_new_key_materializes_a_new_version() {
    let temp = TempDir::new();
    let (root, artifact) = create_legacy_artifact(&temp);
    let store = temp.path().join("family");
    let family = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
    let original = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        family,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--request-id",
        "from-artifact-original",
        "--idempotency-key",
        "from-artifact-original-key",
        "--json",
    ]);
    assert!(original.status.success());
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 2 }\n").unwrap();
    let scan = julie_extract(&[
        "scan",
        "--root",
        root.to_str().unwrap(),
        "--db",
        artifact.to_str().unwrap(),
        "--force",
        "--json",
    ]);
    assert!(
        scan.status.success(),
        "{}",
        String::from_utf8_lossy(&scan.stdout)
    );
    let changed = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        family,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--request-id",
        "from-artifact-changed",
        "--idempotency-key",
        "from-artifact-changed-key",
        "--json",
    ]);
    assert!(
        changed.status.success(),
        "{}",
        String::from_utf8_lossy(&changed.stdout)
    );

    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let (materialization_chunks, manifests, versions): (i64, i64, i64) = connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM store_log
                WHERE request_id='from-artifact-changed'
                  AND event_kind='store_from_artifact_versions_written'),
               (SELECT COUNT(*) FROM manifests WHERE view_id='view-main'),
               (SELECT COUNT(*) FROM file_versions)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(materialization_chunks, 1);
    assert_eq!(manifests, 2);
    assert_eq!(versions, 2);
}

#[test]
fn incomplete_prior_from_artifact_request_does_not_authorize_reuse() {
    let temp = TempDir::new();
    let (root, artifact) = create_legacy_artifact(&temp);
    let store = temp.path().join("family");
    let family = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
    let original_args = [
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        family,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--request-id",
        "from-artifact-incomplete",
        "--idempotency-key",
        "from-artifact-incomplete-key",
        "--json",
    ];
    let crashed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(original_args)
        .env(
            "JULIE_EXTRACT_STORE_TEST_CRASH_AT",
            "terminal_after_store_commit",
        )
        .output()
        .unwrap();
    assert!(!crashed.status.success());

    let retry = julie_extract(&[
        "store",
        "import",
        "--store",
        store.to_str().unwrap(),
        "--family",
        family,
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--from-artifact",
        artifact.to_str().unwrap(),
        "--request-id",
        "from-artifact-after-incomplete",
        "--idempotency-key",
        "from-artifact-after-incomplete-key",
        "--json",
    ]);
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stdout)
    );

    let connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM store_log
                 WHERE request_id='from-artifact-after-incomplete'
                   AND event_kind='store_from_artifact_versions_written'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn from_artifact_crash_boundaries_resume_without_duplicate_versions_or_effects() {
    for boundary in [
        "child_rows_before_level_stamp",
        "level_stamp_before_store_commit",
        "from_artifact_manifest_before_publish",
        "from_artifact_manifest_after_publish_before_commit",
        "resolution_base_after_scratch_close",
        "from_artifact_base_before_catalog",
        "from_artifact_exact_before_cas",
        "from_artifact_exact_after_cas_before_commit",
        "terminal_before_store_commit",
        "terminal_after_store_commit",
        "post_store_pre_coord_reconcile",
    ] {
        let temp = TempDir::new();
        let (root, artifact) = create_legacy_artifact(&temp);
        let store = temp.path().join("family");
        let family = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
        let args = [
            "store",
            "import",
            "--store",
            store.to_str().unwrap(),
            "--family",
            family,
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-main",
            "--from-artifact",
            artifact.to_str().unwrap(),
            "--request-id",
            "from-artifact-crash",
            "--idempotency-key",
            "from-artifact-crash-key",
            "--json",
        ];
        let crashed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args(args)
            .env("JULIE_EXTRACT_STORE_TEST_CRASH_AT", boundary)
            .output()
            .unwrap();
        assert!(!crashed.status.success(), "{boundary}");
        let retried = julie_extract(&args);
        assert!(
            retried.status.success(),
            "{boundary}: {}",
            String::from_utf8_lossy(&retried.stdout)
        );
        let store_db = Connection::open(store.join("gen-001/store.db")).unwrap();
        let facts = store_db
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM file_versions),
                   (SELECT COUNT(*) FROM manifests WHERE view_id='view-main'),
                   (SELECT COUNT(*) FROM resolution_bases WHERE state='ready'),
                   (SELECT COUNT(*) FROM resolution_deltas WHERE view_id='view-main'),
                   (SELECT COUNT(*) FROM store_log
                    WHERE request_id='from-artifact-crash' AND terminal=1),
                   (SELECT COUNT(*)-COUNT(DISTINCT chunk_index) FROM request_chunks
                    WHERE request_id='from-artifact-crash')",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(facts, (1, 1, 1, 1, 1, 0), "{boundary}");
    }
}

#[test]
fn non_exact_view_refuses_export_without_output_or_partial() {
    let temp = TempDir::new();
    let store = create_full_store(&temp);
    let output_path = temp.path().join("export.db");
    let partial_path = temp.path().join("export.db.partial");
    let output = julie_extract(&[
        "store",
        "export",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--out",
        output_path.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["operation"], "export");
    assert_eq!(report["failure_class"], "resolution_not_exact");
    assert_eq!(report["family_id"], "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11");
    assert_eq!(report["view_id"], "view-main");
    assert!(!output_path.exists());
    assert!(!partial_path.exists());
}

#[test]
fn exact_view_exports_current_v3_artifact_with_resolution_overlay() {
    let temp = TempDir::new();
    let store = create_full_store(&temp);
    resolve(&store);
    let output_path = temp.path().join("export.db");
    let output = julie_extract(&[
        "store",
        "export",
        "--store",
        store.to_str().unwrap(),
        "--view",
        "view-main",
        "--out",
        output_path.to_str().unwrap(),
        "--json",
    ]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["operation"], "export");
    assert_eq!(report["export"]["disposition"], "created");
    assert_eq!(
        report["export"]["output"],
        output_path.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(output_path.is_file());
    assert!(!temp.path().join("export.db.partial").exists());

    let artifact = Connection::open(&output_path).unwrap();
    assert_eq!(
        artifact
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(
        artifact
            .query_row("PRAGMA foreign_key_check", [], |_| Ok(1))
            .optional()
            .unwrap(),
        None
    );
    assert_eq!(
        artifact
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(
        artifact
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| row
                .get::<_, i64>(0))
            .unwrap()
            > 0
    );
    assert!(
        artifact
            .query_row("SELECT COUNT(*) FROM identifier_resolutions", [], |row| row
                .get::<_, i64>(0))
            .unwrap()
            > 0
    );
    assert_eq!(
        artifact
            .query_row(
                "SELECT value FROM artifact_metadata WHERE key='store_view_id'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "view-main"
    );
    assert_eq!(
        artifact
            .query_row(
                "SELECT value FROM artifact_metadata WHERE key='reference_resolution_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        julie_extract_cli::resolution::RESOLUTION_VERSION.to_string()
    );
}

#[test]
fn exported_extraction_payload_matches_fresh_v3_oracle_for_every_table() {
    let temp = TempDir::new();
    let store = create_full_language_store(&temp);
    resolve(&store);
    let output_path = temp.path().join("export.db");
    let oracle_path = temp.path().join("oracle.db");
    write_v3_extraction_oracle(&temp.path().join("language-source"), &oracle_path).unwrap();
    let exported = export(&store, &output_path);
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stdout)
    );

    assert_eq!(
        normalized_v3_rows(&output_path),
        normalized_v3_rows(&oracle_path)
    );
    let store_connection = Connection::open(store.join("gen-001/store.db")).unwrap();
    let relative_path: String = store_connection
        .query_row(
            "SELECT base.relative_path
             FROM views AS view JOIN resolution_bases AS base
               ON base.base_id=view.resolution_base_id
             WHERE view.view_id='view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let base = Connection::open(store.join("gen-001").join(relative_path)).unwrap();
    let exported = Connection::open(&output_path).unwrap();
    assert_eq!(
        exported
            .query_row("SELECT COUNT(*) FROM identifier_resolutions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        base.query_row("SELECT COUNT(*) FROM identifier_resolutions", [], |row| row
            .get::<_, i64>(0))
            .unwrap()
    );
    assert_eq!(
        exported
            .query_row("SELECT COUNT(*) FROM pending_resolutions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        base.query_row("SELECT COUNT(*) FROM pending_resolutions", [], |row| row
            .get::<_, i64>(0))
            .unwrap()
    );
}

#[test]
fn retry_reuses_matching_output_and_regular_stale_partial_without_store_effects() {
    let temp = TempDir::new();
    let store = create_full_store(&temp);
    resolve(&store);
    let output_path = temp.path().join("export.db");
    let partial_path = temp.path().join("export.db.partial");
    fs::write(&partial_path, b"stale partial").unwrap();
    let store_db = store.join("gen-001/store.db");
    let before = Connection::open(&store_db)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM store_log", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();

    let created = export(&store, &output_path);
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stdout)
    );
    let created_report: Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(created_report["export"]["disposition"], "created");
    let bytes = fs::read(&output_path).unwrap();
    let reused = export(&store, &output_path);
    assert!(
        reused.status.success(),
        "{}",
        String::from_utf8_lossy(&reused.stdout)
    );
    let reused_report: Value = serde_json::from_slice(&reused.stdout).unwrap();
    assert_eq!(reused_report["export"]["disposition"], "reused");
    assert_eq!(fs::read(&output_path).unwrap(), bytes);
    assert!(!partial_path.exists());

    let connection = Connection::open(&store_db).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM store_log", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        before
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM resolution_pins", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn nonmatching_or_symlink_output_is_never_overwritten() {
    let temp = TempDir::new();
    let store = create_full_store(&temp);
    resolve(&store);
    let output_path = temp.path().join("export.db");
    let created = export(&store, &output_path);
    assert!(created.status.success());
    let original = fs::read(&output_path).unwrap();

    let root = temp.path().join("source");
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 2 }\n").unwrap();
    let update = julie_extract(&[
        "store",
        "update",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "lib.rs",
        "--level",
        "full",
        "--json",
    ]);
    assert!(
        update.status.success(),
        "{}",
        String::from_utf8_lossy(&update.stdout)
    );
    resolve(&store);
    let mismatch = export(&store, &output_path);
    assert_eq!(mismatch.status.code(), Some(1));
    let mismatch_report: Value = serde_json::from_slice(&mismatch.stdout).unwrap();
    assert_eq!(mismatch_report["failure_class"], "output_identity_mismatch");
    assert_eq!(fs::read(&output_path).unwrap(), original);

    let symlink_output = temp.path().join("symlink.db");
    let sentinel = temp.path().join("sentinel");
    fs::write(&sentinel, b"sentinel").unwrap();
    std::os::unix::fs::symlink(&sentinel, &symlink_output).unwrap();
    let refused = export(&store, &symlink_output);
    assert_eq!(refused.status.code(), Some(1));
    let refused_report: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(refused_report["failure_class"], "output_identity_mismatch");
    assert_eq!(fs::read(&sentinel).unwrap(), b"sentinel");

    let partial_symlink_output = temp.path().join("partial-symlink.db");
    let partial = temp.path().join("partial-symlink.db.partial");
    std::os::unix::fs::symlink(&sentinel, &partial).unwrap();
    let refused = export(&store, &partial_symlink_output);
    assert_eq!(refused.status.code(), Some(1));
    let refused_report: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(refused_report["failure_class"], "output_identity_mismatch");
    assert_eq!(fs::read(&sentinel).unwrap(), b"sentinel");
    assert!(!partial_symlink_output.exists());
}

#[test]
fn pin_keeps_export_on_one_generation_while_current_view_advances() {
    let temp = TempDir::new();
    let store = create_full_store(&temp);
    resolve(&store);
    let output_path = temp.path().join("export.db");
    let pause = temp.path().join("pause");
    fs::create_dir(&pause).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "export",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--out",
            output_path.to_str().unwrap(),
            "--json",
        ])
        .env("JULIE_EXTRACT_STORE_EXPORT_TEST_PAUSE_DIR", &pause)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_path(&pause.join("ready"));

    let store_db = store.join("gen-001/store.db");
    let pinned_generation = Connection::open(&store_db)
        .unwrap()
        .query_row(
            "SELECT manifest_generation FROM resolution_pins",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    let root = temp.path().join("source");
    fs::write(root.join("lib.rs"), "pub fn answer() -> i32 { 3 }\n").unwrap();
    let update = julie_extract(&[
        "store",
        "update",
        "--store",
        store.to_str().unwrap(),
        "--root",
        root.to_str().unwrap(),
        "--view",
        "view-main",
        "--file",
        "lib.rs",
        "--level",
        "full",
        "--json",
    ]);
    assert!(
        update.status.success(),
        "{}",
        String::from_utf8_lossy(&update.stdout)
    );
    resolve(&store);
    fs::write(pause.join("continue"), b"continue").unwrap();
    let exported = child.wait_with_output().unwrap();
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stdout)
    );

    let artifact = Connection::open(&output_path).unwrap();
    let exported_generation: i64 = artifact
        .query_row(
            "SELECT value FROM artifact_metadata WHERE key='store_manifest_generation'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(exported_generation, pinned_generation);
    let exported_hash: String = artifact
        .query_row(
            "SELECT content_hash FROM files WHERE path='lib.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let store_connection = Connection::open(&store_db).unwrap();
    let pinned_hash: String = store_connection
        .query_row(
            "SELECT version.content_hash
             FROM manifest_entries AS entry JOIN file_versions AS version USING(version_id)
             WHERE entry.view_id='view-main' AND entry.generation=?1 AND entry.path='lib.rs'",
            [pinned_generation],
            |row| row.get(0),
        )
        .unwrap();
    let current_generation: i64 = store_connection
        .query_row(
            "SELECT current_generation FROM views WHERE view_id='view-main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(current_generation > pinned_generation);
    assert_eq!(exported_hash, pinned_hash);
    assert_eq!(
        store_connection
            .query_row("SELECT COUNT(*) FROM resolution_pins", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn concurrent_same_output_never_removes_the_active_partial_or_overwrites() {
    let temp = TempDir::new();
    let store = create_full_store(&temp);
    resolve(&store);
    let output_path = temp.path().join("export.db");
    let pause = temp.path().join("pause");
    fs::create_dir(&pause).unwrap();
    let first = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "export",
            "--store",
            store.to_str().unwrap(),
            "--view",
            "view-main",
            "--out",
            output_path.to_str().unwrap(),
            "--json",
        ])
        .env("JULIE_EXTRACT_STORE_EXPORT_TEST_PAUSE_DIR", &pause)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for_path(&pause.join("ready"));

    let second = export(&store, &output_path);
    assert_eq!(second.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(report["failure_class"], "busy");
    assert!(!output_path.exists());
    fs::write(pause.join("continue"), b"continue").unwrap();
    let first = first.wait_with_output().unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    assert!(output_path.is_file());
    let artifact = Connection::open(&output_path).unwrap();
    assert_eq!(
        artifact
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(
        Connection::open(store.join("gen-001/store.db"))
            .unwrap()
            .query_row("SELECT COUNT(*) FROM resolution_pins", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn crash_at_validation_and_rename_boundaries_retries_to_one_valid_output() {
    let temp = TempDir::new();
    let store = create_full_store(&temp);
    resolve(&store);
    for boundary in ["before_validation", "after_validation", "after_rename"] {
        let output_path = temp.path().join(format!("{boundary}.db"));
        let crashed = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
            .args([
                "store",
                "export",
                "--store",
                store.to_str().unwrap(),
                "--view",
                "view-main",
                "--out",
                output_path.to_str().unwrap(),
                "--json",
            ])
            .env("JULIE_EXTRACT_STORE_EXPORT_TEST_CRASH_AT", boundary)
            .env("JULIE_EXTRACT_STORE_EXPORT_TEST_SHORT_PIN", "1")
            .output()
            .unwrap();
        assert!(!crashed.status.success(), "{boundary}");
        std::thread::sleep(Duration::from_millis(1_200));
        let retry = export(&store, &output_path);
        assert!(
            retry.status.success(),
            "{boundary}: {}",
            String::from_utf8_lossy(&retry.stdout)
        );
        let report: Value = serde_json::from_slice(&retry.stdout).unwrap();
        assert_eq!(
            report["export"]["disposition"],
            if boundary == "after_rename" {
                "reused"
            } else {
                "created"
            }
        );
        assert!(output_path.is_file());
        assert!(!PathBuf::from(format!("{}.partial", output_path.display())).exists());
        let artifact = Connection::open(&output_path).unwrap();
        assert_eq!(
            artifact
                .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        assert_eq!(
            artifact
                .query_row("PRAGMA foreign_key_check", [], |_| Ok(1))
                .optional()
                .unwrap(),
            None
        );
        assert_eq!(
            Connection::open(store.join("gen-001/store.db"))
                .unwrap()
                .query_row("SELECT COUNT(*) FROM resolution_pins", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

fn normalized_v3_rows(path: &Path) -> BTreeMap<String, Vec<String>> {
    let connection = Connection::open(path).unwrap();
    let mut rows = BTreeMap::new();
    rows.insert(
        "files".to_string(),
        query_rows(
            &connection,
            "SELECT path,language,content_hash,content_bytes,line_count,metadata_json FROM files",
            6,
        ),
    );
    for table in CHILD_TABLES {
        let columns = table_columns(&connection, table);
        let join = v3_path_join(table);
        let projection = columns
            .iter()
            .filter(|column| column.as_str() != "file_id")
            .map(|column| {
                if LOCAL_ID_COLUMNS.contains(&column.as_str()) {
                    format!(
                        "CASE WHEN t.{column} IS NULL THEN NULL
                         WHEN substr(t.{column},1,length(f.file_id)+1)=f.file_id||':'
                         THEN substr(t.{column},length(f.file_id)+2) ELSE t.{column} END"
                    )
                } else {
                    format!("t.{column}")
                }
            })
            .collect::<Vec<_>>();
        rows.insert(
            table.to_string(),
            query_rows(
                &connection,
                &format!(
                    "SELECT f.path,{} FROM {table} AS t {join}",
                    projection.join(",")
                ),
                projection.len() + 1,
            ),
        );
    }
    for table in GLOBAL_TABLES {
        let columns = table_columns(&connection, table);
        rows.insert(
            table.to_string(),
            query_rows(
                &connection,
                &format!("SELECT {} FROM {table}", columns.join(",")),
                columns.len(),
            ),
        );
    }
    rows
}

fn normalized_v3_rows_with_resolution(path: &Path) -> BTreeMap<String, Vec<String>> {
    let connection = Connection::open(path).unwrap();
    let mut rows = normalized_v3_rows(path);
    rows.insert(
        "identifier_resolutions".to_string(),
        query_rows(
            &connection,
            &format!(
                "SELECT f.path,{}, {},t.tier,t.confidence,t.method,t.outcome,t.candidates
                 FROM identifier_resolutions AS t
                 JOIN identifiers AS owner ON owner.identifier_id=t.identifier_id
                 JOIN files AS f ON f.file_id=owner.file_id",
                local_id_projection("identifier_id"),
                semantic_id_projection("target_symbol_id")
            ),
            8,
        ),
    );
    rows.insert(
        "pending_resolutions".to_string(),
        query_rows(
            &connection,
            &format!(
                "SELECT f.path,{}, {},t.tier,t.confidence,t.method
                 FROM pending_resolutions AS t
                 JOIN pending_relationships AS owner
                   ON owner.pending_relationship_id=t.pending_relationship_id
                 JOIN files AS f ON f.file_id=owner.file_id",
                local_id_projection("pending_relationship_id"),
                semantic_id_projection("target_symbol_id")
            ),
            6,
        ),
    );
    rows
}

fn local_id_projection(column: &str) -> String {
    format!(
        "CASE WHEN substr(t.{column},1,length(f.file_id)+1)=f.file_id||':'
         THEN substr(t.{column},length(f.file_id)+2) ELSE t.{column} END"
    )
}

fn semantic_id_projection(column: &str) -> String {
    format!(
        "CASE WHEN t.{column} IS NULL THEN NULL
         WHEN t.{column} LIKE 'store-version:%:%'
         THEN substr(substr(t.{column},15),instr(substr(t.{column},15),':')+1)
         WHEN instr(t.{column},':')>0 AND EXISTS(
             SELECT 1 FROM files AS target_file
             WHERE target_file.file_id=substr(t.{column},1,instr(t.{column},':')-1)
         ) THEN substr(t.{column},instr(t.{column},':')+1)
         ELSE t.{column} END"
    )
}

fn table_columns(connection: &Connection, table: &str) -> Vec<String> {
    connection
        .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
        .unwrap()
        .query_map([table], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn query_rows(connection: &Connection, sql: &str, width: usize) -> Vec<String> {
    let mut rows = connection
        .prepare(sql)
        .unwrap()
        .query_map([], |row| {
            (0..width)
                .map(|index| row.get::<_, rusqlite::types::Value>(index))
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .into_iter()
        .map(|row| format!("{row:?}"))
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

fn v3_path_join(table: &str) -> &'static str {
    match table {
        "symbol_annotations" => {
            "JOIN symbols AS owner ON owner.symbol_id=t.symbol_id
             JOIN files AS f ON f.file_id=owner.file_id"
        }
        "type_facts" => {
            "JOIN symbols AS owner ON owner.symbol_id=t.symbol_id
             JOIN files AS f ON f.file_id=owner.file_id"
        }
        "type_arguments" => {
            "JOIN type_argument_usages AS owner ON owner.usage_id=t.usage_id
             JOIN files AS f ON f.file_id=owner.file_id"
        }
        _ => "JOIN files AS f ON f.file_id=t.file_id",
    }
}
