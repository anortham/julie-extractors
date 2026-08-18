#![cfg(feature = "test-store-maintenance-contract")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use julie_extract_artifact::store::StoreLayout;
use rusqlite::{Connection, types::Value};

const FAMILY_ID: &str = "90d44d72-c939-4a14-8a27-72568b06af4c";

#[test]
fn post_promotion_writes_advance_family_allocator_marks() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    let root = fixture.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("lib.rs"), "pub fn value() -> u32 { 1 }\n").unwrap();

    assert_success(&run_store(&[
        "store",
        "import",
        "--store",
        path(&store),
        "--family",
        FAMILY_ID,
        "--root",
        path(&root),
        "--view",
        "view-main",
        "--level",
        "l1",
        "--request-id",
        "allocator-import",
        "--idempotency-key",
        "allocator-import",
        "--json",
    ]));
    assert_success(&run_store(&[
        "store",
        "maintain",
        "promote",
        "--store",
        path(&store),
        "--apply",
        "--json",
    ]));

    std::fs::write(root.join("lib.rs"), "pub fn value() -> u32 { 2 }\n").unwrap();
    assert_success(&run_store(&[
        "store",
        "update",
        "--store",
        path(&store),
        "--root",
        path(&root),
        "--view",
        "view-main",
        "--file",
        "lib.rs",
        "--level",
        "l1",
        "--request-id",
        "allocator-update",
        "--idempotency-key",
        "allocator-update",
        "--json",
    ]));

    let serving = StoreLayout::open(&store).unwrap();
    assert_eq!(serving.generation_name(), "gen-002");
    let catalog = Connection::open(serving.store_db()).unwrap();
    let coordinator = Connection::open(serving.coordinator_db()).unwrap();
    for (kind, scope, maximum) in [
        (
            "file_version",
            "",
            catalog
                .query_row("SELECT MAX(version_id) FROM file_versions", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
        ),
        (
            "store_log",
            "",
            catalog
                .query_row("SELECT MAX(sequence) FROM store_log", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
        ),
        (
            "manifest_generation",
            "view-main",
            catalog
                .query_row(
                    "SELECT MAX(generation) FROM manifests WHERE view_id='view-main'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
        ),
    ] {
        let high_water = coordinator
            .query_row(
                "SELECT high_water FROM family_allocator_marks
                 WHERE allocator_kind=?1 AND scope_id=?2",
                rusqlite::params![kind, scope],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert!(high_water >= maximum, "kind={kind} scope={scope}");
    }
}

#[test]
fn every_supported_language_and_natural_store_row_survives_public_promotion() {
    let fixture = tempfile::tempdir().unwrap();
    let store = fixture.path().join("store");
    let root = workspace_root().join("fixtures/extraction");
    let imported = run_store(&[
        "store",
        "import",
        "--store",
        path(&store),
        "--family",
        FAMILY_ID,
        "--root",
        path(&root),
        "--view",
        "view-all-languages",
        "--level",
        "full",
        "--jobs",
        "4",
        "--request-id",
        "lifecycle-language-import",
        "--idempotency-key",
        "lifecycle-language-import",
        "--request-timeout-seconds",
        "300",
        "--json",
    ]);
    assert_success(&imported);

    let source = StoreLayout::open(&store).unwrap();
    let source_db = source.store_db().to_path_buf();
    let observed = Connection::open(&source_db)
        .unwrap()
        .prepare(
            "SELECT DISTINCT language FROM file_versions
             WHERE complete_l3 IS NOT NULL ORDER BY language",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<BTreeSet<_>, _>>()
        .unwrap();
    let expected = julie_extractors::language::supported_languages()
        .iter()
        .map(|language| (*language).to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(observed, expected);

    let promoted = run_store(&[
        "store",
        "maintain",
        "promote",
        "--store",
        path(&store),
        "--apply",
        "--json",
    ]);
    assert_success(&promoted);
    let destination = StoreLayout::open(&store).unwrap();
    assert_eq!(destination.generation_name(), "gen-002");

    let source = Connection::open(source_db).unwrap();
    let destination = Connection::open(destination.store_db()).unwrap();
    for table in logical_tables(&source) {
        assert_eq!(
            table_rows(&source, &table),
            table_rows(&destination, &table),
            "table={table}"
        );
    }
    assert!(!table_rows(&destination, "store_log").is_empty());
    assert_valid(&source);
    assert_valid(&destination);
    assert_valid(&Connection::open(store.join("coord.db")).unwrap());
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn run_store(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .env("MILLER_STORE_CHUNK_VERSIONS", "8")
        .output()
        .unwrap()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn logical_tables(connection: &Connection) -> Vec<String> {
    connection
        .prepare(
            "SELECT name FROM sqlite_schema
             WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name <> 'store_meta'
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn table_rows(connection: &Connection, table: &str) -> Vec<String> {
    let width = connection
        .prepare("SELECT COUNT(*) FROM pragma_table_info(?1)")
        .unwrap()
        .query_row([table], |row| row.get::<_, i64>(0))
        .unwrap() as usize;
    let mut rows = connection
        .prepare(&format!("SELECT * FROM \"{table}\""))
        .unwrap()
        .query_map([], |row| {
            (0..width)
                .map(|index| row.get::<_, Value>(index))
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

fn assert_valid(connection: &Connection) {
    assert_eq!(
        connection
            .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}
