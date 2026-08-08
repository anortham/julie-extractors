#![cfg(feature = "test-store-resolution-contract")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use julie_extract_cli::store::args::{StoreCli, StoreCommand, StoreRootCommand};
use julie_extract_cli::store::test_support::write_v3_extraction_oracle;
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
    let store = create_full_store(&temp);
    resolve(&store);
    let output_path = temp.path().join("export.db");
    let oracle_path = temp.path().join("oracle.db");
    write_v3_extraction_oracle(&temp.path().join("source"), &oracle_path).unwrap();
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
