#![cfg(feature = "test-store-maintenance-contract")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use julie_extract_artifact::store::StoreLayout;
use rusqlite::{Connection, types::Value};
use sha2::{Digest, Sha256};

const FAMILY_ID: &str = "90d44d72-c939-4a14-8a27-72568b06af4c";

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
    seed_resolution_rows(&source);

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
    for table in [
        "resolution_bases",
        "resolution_base_versions",
        "resolution_deltas",
        "resolution_identifier_deltas",
        "store_log",
    ] {
        assert!(!table_rows(&destination, table).is_empty(), "table={table}");
    }
    assert_valid(&source);
    assert_valid(&destination);
    assert_valid(&Connection::open(store.join("coord.db")).unwrap());
}

fn seed_resolution_rows(layout: &StoreLayout) {
    let bytes = b"lifecycle resolution base";
    let relative_path = "bases/lifecycle-base.db";
    std::fs::write(layout.generation_dir().join(relative_path), bytes).unwrap();
    let sha = format!("{:x}", Sha256::digest(bytes));
    let connection = Connection::open(layout.store_db()).unwrap();
    let (generation, manifest_hash, version_id): (i64, String, i64) = connection
        .query_row(
            "SELECT v.current_generation,m.manifest_hash,MIN(me.version_id)
             FROM views v
             JOIN manifests m ON m.view_id=v.view_id AND m.generation=v.current_generation
             JOIN manifest_entries me ON me.view_id=v.view_id AND me.generation=v.current_generation
             WHERE v.view_id='view-all-languages' AND me.version_id IS NOT NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_bases
             (base_id,manifest_hash,resolver_output_epoch,state,relative_path,identifier_count,
              pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
             VALUES ('lifecycle-base',?1,1,'ready',?2,1,0,?3,?4,'lifecycle-language-import',
                     '2026-08-09T00:00:00Z','2026-08-09T00:00:00Z')",
            rusqlite::params![manifest_hash, relative_path, bytes.len() as i64, sha],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_base_versions(base_id,version_id)
             VALUES ('lifecycle-base',?1)",
            [version_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_deltas
             (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
              resolver_output_epoch,identifier_replacements,pending_replacements,pending_tombstones,
              exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
             VALUES ('view-all-languages',1,'lifecycle-base',?1,?2,1,1,0,0,0,0,'{}',
                     'lifecycle-language-import','2026-08-09T00:00:00Z')",
            rusqlite::params![generation, manifest_hash],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_identifier_deltas
             (view_id,delta_generation,version_id,identifier_id,target_version_id,target_symbol_id,
              tier,confidence,method,outcome,candidates)
             VALUES ('view-all-languages',1,?1,'lifecycle-identifier',?1,'lifecycle-symbol',
                     1,1.0,'exact','resolved',1)",
            [version_id],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE views SET resolution_state='exact',resolution_base_id='lifecycle-base',
              resolution_delta_generation=1,resolution_exact_at=?1
             WHERE view_id='view-all-languages'",
            [generation],
        )
        .unwrap();
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
