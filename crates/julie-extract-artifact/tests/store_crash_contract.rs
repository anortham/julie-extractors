#![cfg(feature = "test-store-crash")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::model::{
    ArtifactCapabilityFlags, ArtifactCapabilitySnapshot, ArtifactFile, ArtifactIdentifier,
    ArtifactLanguageCapabilityRow, ArtifactLiteral, ArtifactParserInventoryRow, ArtifactSymbol,
    FileStatus, ReferenceSiteProvenance,
};
use julie_extract_artifact::store::{
    CoordinatorRequest, ManifestEntry, ManifestStore, RequestKind, RequestState,
    StoreConnectionFactory, StoreCoordinator, StoreFileVersion, StoreLayout, StoreLevel, StoreLog,
    StoreLogEntry, StoreWriteRequest, StoreWriter,
};
use rusqlite::Connection;

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "julie-store-crash-{}-{nonce}-{sequence}",
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

#[test]
fn hard_kill_after_l1_stamp_reopens_valid_store_with_committed_level() {
    let temp = TempDir::new();
    let output = run_crash_worker(temp.path(), "after_l1_commit");

    assert!(
        !output.status.success(),
        "worker must terminate at the crash boundary"
    );
    let layout = StoreLayout::open(temp.path()).unwrap();
    let connection = Connection::open(layout.store_db()).unwrap();
    assert_store_is_valid(&connection);
    assert_store_is_valid(&Connection::open(layout.coordinator_db()).unwrap());
    let completed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM file_versions WHERE complete_l1 IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        completed,
        1,
        "worker stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(table_count(&connection, "symbols"), 1);
}

#[test]
fn hard_kill_before_l1_chunk_commit_rolls_back_incomplete_level() {
    let temp = TempDir::new();
    let output = run_crash_worker(temp.path(), "before_l1_commit");

    assert!(!output.status.success());
    let layout = StoreLayout::open(temp.path()).unwrap();
    let connection = Connection::open(layout.store_db()).unwrap();
    assert_store_is_valid(&connection);
    assert_store_is_valid(&Connection::open(layout.coordinator_db()).unwrap());
    let versions: i64 = connection
        .query_row("SELECT COUNT(*) FROM file_versions", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        versions,
        0,
        "worker stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(table_count(&connection, "symbols"), 0);
}

#[test]
fn manifest_flip_is_atomic_on_both_sides_of_the_commit_boundary() {
    for (point, expected_generation) in [
        ("before_manifest_commit", None),
        ("after_manifest_commit", Some(1_i64)),
    ] {
        let temp = TempDir::new();
        let output = run_specific_worker(temp.path(), "manifest_crash_worker", point);
        assert!(!output.status.success());
        let layout = StoreLayout::open(temp.path()).unwrap();
        let connection = Connection::open(layout.store_db()).unwrap();
        assert_store_is_valid(&connection);
        assert_store_is_valid(&Connection::open(layout.coordinator_db()).unwrap());
        let generation = connection
            .query_row(
                "SELECT current_generation FROM views WHERE view_id = 'view-main'",
                [],
                |row| row.get::<_, Option<i64>>(0),
            )
            .unwrap();
        assert_eq!(generation, expected_generation, "point={point}");
        let orphan_entries: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM manifest_entries me
                 LEFT JOIN manifests m ON m.view_id = me.view_id AND m.generation = me.generation
                 WHERE m.generation IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(orphan_entries, 0, "point={point}");
    }
}

#[test]
fn terminal_store_commit_reconciles_the_separate_coordinator_database_once() {
    for (point, expected_state, expected_terminal) in [
        ("before_terminal_commit", RequestState::Queued, 0_i64),
        ("after_terminal_commit", RequestState::Committed, 1_i64),
    ] {
        let temp = TempDir::new();
        let output = run_specific_worker(temp.path(), "terminal_crash_worker", point);
        assert!(!output.status.success());
        let layout = StoreLayout::open(temp.path()).unwrap();
        assert_store_is_valid(&Connection::open(layout.store_db()).unwrap());
        assert_store_is_valid(&Connection::open(layout.coordinator_db()).unwrap());
        let mut coordinator = StoreCoordinator::open(&layout).unwrap();
        coordinator.reconcile("request-terminal").unwrap();
        assert_eq!(
            coordinator.request("request-terminal").unwrap().state,
            expected_state
        );
        let terminal: i64 = Connection::open(layout.store_db())
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM store_log WHERE request_id = 'request-terminal' AND terminal = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(terminal, expected_terminal, "point={point}");
    }
}

#[test]
fn deep_level_transaction_is_atomic_before_l3_and_before_commit() {
    for (point, expected_deep) in [
        ("after_l2_before_l3", false),
        ("before_deep_commit", false),
        ("after_deep_commit", true),
    ] {
        let temp = TempDir::new();
        let output = run_specific_worker(temp.path(), "deep_crash_worker", point);
        assert!(!output.status.success());
        let layout = StoreLayout::open(temp.path()).unwrap();
        let connection = Connection::open(layout.store_db()).unwrap();
        assert_store_is_valid(&connection);
        assert_store_is_valid(&Connection::open(layout.coordinator_db()).unwrap());
        let stamps: (bool, bool, bool) = connection
            .query_row(
                "SELECT complete_l1 IS NOT NULL, complete_l2 IS NOT NULL, complete_l3 IS NOT NULL
                 FROM file_versions WHERE path = 'src/lib.rs'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            stamps,
            (true, expected_deep, expected_deep),
            "point={point}"
        );
        assert_eq!(table_count(&connection, "symbols"), 1, "point={point}");
        assert_eq!(
            table_count(&connection, "identifiers"),
            i64::from(expected_deep),
            "point={point}"
        );
        assert_eq!(
            table_count(&connection, "literals"),
            i64::from(expected_deep),
            "point={point}"
        );
    }
}

#[test]
fn chunk_progress_is_atomic_on_both_sides_of_commit() {
    for (point, expected) in [
        ("before_progress_commit", 0_i64),
        ("after_progress_commit", 1_i64),
    ] {
        let temp = TempDir::new();
        let output = run_specific_worker(temp.path(), "progress_crash_worker", point);
        assert!(!output.status.success());
        let layout = StoreLayout::open(temp.path()).unwrap();
        let connection = Connection::open(layout.store_db()).unwrap();
        assert_store_is_valid(&connection);
        assert_store_is_valid(&Connection::open(layout.coordinator_db()).unwrap());
        let counts: (i64, i64) = connection
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM store_log WHERE request_id = 'request-progress'),
                   (SELECT COUNT(*) FROM request_chunks WHERE request_id = 'request-progress')",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(counts, (expected, expected), "point={point}");
    }
}

fn run_crash_worker(root: &Path, point: &str) -> std::process::Output {
    run_specific_worker(root, "crash_after_l1_stamp_worker", point)
}

fn run_specific_worker(root: &Path, worker: &str, point: &str) -> std::process::Output {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", worker, "--nocapture", "--test-threads=1"])
        .env("JULIE_TEST_STORE_CRASH_ROOT", root)
        .env("JULIE_TEST_STORE_CRASH_POINT", point)
        .output()
        .unwrap()
}

#[test]
fn manifest_crash_worker() {
    let Ok(root) = std::env::var("JULIE_TEST_STORE_CRASH_ROOT") else {
        return;
    };
    let point = std::env::var("JULIE_TEST_STORE_CRASH_POINT").unwrap();
    let layout = StoreLayout::create(&root, "family-crash", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-crash", "2.30.0");
    let version = StoreFileVersion::try_from_artifact_file(1, &fixture_file()).unwrap();
    let mut writer = StoreWriter::open(&factory).unwrap();
    writer.stage_capability_snapshot(1, capability_snapshot());
    let written = writer
        .write_level(
            &StoreWriteRequest::routine("request-level", "2026-08-08T00:00:00Z"),
            &version,
            StoreLevel::L1,
        )
        .unwrap();
    drop(writer);
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-main", "/repo")
        .unwrap();
    let entry = ManifestEntry::indexed(
        "src/lib.rs",
        "rust",
        written.version_id,
        "blake3:crash",
        "2026-08-08T00:00:00Z",
    );
    if point == "before_manifest_commit" {
        let transaction = connection.transaction().unwrap();
        ManifestStore::publish_in_transaction(
            &transaction,
            "view-main",
            None,
            [entry],
            "request-manifest",
        )
        .unwrap();
        std::process::abort();
    }
    ManifestStore::new(&mut connection)
        .publish("view-main", None, [entry], "request-manifest")
        .unwrap();
    std::process::abort();
}

#[test]
fn terminal_crash_worker() {
    let Ok(root) = std::env::var("JULIE_TEST_STORE_CRASH_ROOT") else {
        return;
    };
    let point = std::env::var("JULIE_TEST_STORE_CRASH_POINT").unwrap();
    let layout = StoreLayout::create(&root, "family-crash", "2.30.0").unwrap();
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            "request-terminal",
            "idem-terminal",
            RequestKind::Update,
            "{}",
            "requester",
            10_000,
            1,
        ))
        .unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-crash", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    let transaction = connection.transaction().unwrap();
    StoreLog::append_terminal(
        &transaction,
        &StoreLogEntry::new(
            "request-terminal",
            "request_completed",
            r#"{"state":"committed"}"#,
            "2026-08-08T00:00:00Z",
        ),
    )
    .unwrap();
    if point == "before_terminal_commit" {
        std::process::abort();
    }
    transaction.commit().unwrap();
    std::process::abort();
}

#[test]
fn progress_crash_worker() {
    let Ok(root) = std::env::var("JULIE_TEST_STORE_CRASH_ROOT") else {
        return;
    };
    let point = std::env::var("JULIE_TEST_STORE_CRASH_POINT").unwrap();
    let layout = StoreLayout::create(&root, "family-crash", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-crash", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    let transaction = connection.transaction().unwrap();
    StoreLog::append_progress(
        &transaction,
        &StoreLogEntry::new(
            "request-progress",
            "l1_chunk",
            r#"{"completed_files":1}"#,
            "2026-08-08T00:00:00Z",
        )
        .with_level(StoreLevel::L1),
        0,
    )
    .unwrap();
    if point == "before_progress_commit" {
        std::process::abort();
    }
    transaction.commit().unwrap();
    std::process::abort();
}

#[test]
fn deep_crash_worker() {
    let Ok(root) = std::env::var("JULIE_TEST_STORE_CRASH_ROOT") else {
        return;
    };
    let point = std::env::var("JULIE_TEST_STORE_CRASH_POINT").unwrap();
    let layout = StoreLayout::create(&root, "family-crash", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-crash", "2.30.0");
    let version = StoreFileVersion::try_from_artifact_file(1, &fixture_file()).unwrap();
    let request = StoreWriteRequest::routine("request-deep", "2026-08-08T00:00:00Z");
    let mut writer = StoreWriter::open(&factory).unwrap();
    writer.stage_capability_snapshot(1, capability_snapshot());
    writer
        .write_level(&request, &version, StoreLevel::L1)
        .unwrap();
    drop(writer);

    let mut connection = factory.open_writer().unwrap();
    let transaction = connection.transaction().unwrap();
    StoreWriter::write_level_in_transaction(&transaction, &request, None, &version, StoreLevel::L2)
        .unwrap();
    if point == "after_l2_before_l3" {
        std::process::abort();
    }
    StoreWriter::write_level_in_transaction(&transaction, &request, None, &version, StoreLevel::L3)
        .unwrap();
    if point == "before_deep_commit" {
        std::process::abort();
    }
    transaction.commit().unwrap();
    std::process::abort();
}

#[test]
fn crash_after_l1_stamp_worker() {
    let Ok(root) = std::env::var("JULIE_TEST_STORE_CRASH_ROOT") else {
        return;
    };
    let layout = StoreLayout::create(&root, "family-crash", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-crash", "2.30.0");
    let version = StoreFileVersion::try_from_artifact_file(1, &fixture_file()).unwrap();
    let request = StoreWriteRequest::routine("request-crash", "2026-08-08T00:00:00Z");
    if std::env::var("JULIE_TEST_STORE_CRASH_POINT").as_deref() == Ok("before_l1_commit") {
        let mut connection = factory.open_writer().unwrap();
        let transaction = connection.transaction().unwrap();
        StoreWriter::write_level_in_transaction(
            &transaction,
            &request,
            Some(&capability_snapshot()),
            &version,
            StoreLevel::L1,
        )
        .unwrap();
        std::process::abort();
    }
    let mut writer = StoreWriter::open(&factory).unwrap();
    writer.stage_capability_snapshot(1, capability_snapshot());
    writer
        .write_level(&request, &version, StoreLevel::L1)
        .unwrap();
    std::process::abort();
}

fn capability_snapshot() -> ArtifactCapabilitySnapshot {
    ArtifactCapabilitySnapshot {
        parser_inventory: vec![ArtifactParserInventoryRow {
            language: "rust".to_string(),
            parser_package: "tree-sitter-rust".to_string(),
            parser_version: None,
            grammar_version: None,
            source: None,
            metadata: None,
        }],
        languages: vec![ArtifactLanguageCapabilityRow {
            language: "rust".to_string(),
            parser_package: "tree-sitter-rust".to_string(),
            extensions: vec![".rs".to_string()],
            dependency_status: "available".to_string(),
            target_capabilities: ArtifactCapabilityFlags::default(),
            actual_capabilities: ArtifactCapabilityFlags::default(),
            kind_coverage: serde_json::json!({}),
            fixtures: Vec::new(),
            gaps: Vec::new(),
        }],
    }
}

fn fixture_file() -> ArtifactFile {
    ArtifactFile {
        file_id: "file-crash".to_string(),
        path: "src/lib.rs".to_string(),
        language: "rust".to_string(),
        content_hash: "blake3:crash".to_string(),
        content_bytes: 1,
        line_count: Some(1),
        indexed_at: "2026-08-08T00:00:00Z".to_string(),
        status: FileStatus::Indexed,
        metadata_json: None,
        symbols: vec![ArtifactSymbol {
            symbol_id: "symbol-root".to_string(),
            name: "root".to_string(),
            kind: "function".to_string(),
            start_line: 1,
            end_line: 1,
            end_byte: 1,
            ..ArtifactSymbol::default()
        }],
        symbol_annotations: Vec::new(),
        identifiers: vec![ArtifactIdentifier {
            identifier_id: "identifier-call".to_string(),
            reference_site_id: "site-call".to_string(),
            name: "callee".to_string(),
            kind: "call".to_string(),
            containing_symbol_id: Some("symbol-root".to_string()),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 1,
            start_byte: 0,
            end_byte: 1,
            site_is_exact: true,
            site_provenance: ReferenceSiteProvenance::TargetToken,
            confidence: 1.0,
            code_context: None,
            metadata_json: None,
        }],
        relationships: Vec::new(),
        pending_relationships: Vec::new(),
        type_facts: Vec::new(),
        type_argument_usages: Vec::new(),
        type_arguments: Vec::new(),
        literals: vec![ArtifactLiteral {
            literal_id: "literal-value".to_string(),
            literal_text: "value".to_string(),
            kind: "string".to_string(),
            carrier: Some("callee".to_string()),
            arg_position: 0,
            containing_symbol_id: Some("symbol-root".to_string()),
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 1,
            start_byte: 0,
            end_byte: 1,
            confidence: 1.0,
            metadata_json: None,
        }],
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        parse_diagnostics: Vec::new(),
    }
}

fn table_count(connection: &Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn assert_store_is_valid(connection: &Connection) {
    let quick_check: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(quick_check, "ok");
    let foreign_key_errors: i64 = connection
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(foreign_key_errors, 0);
}
