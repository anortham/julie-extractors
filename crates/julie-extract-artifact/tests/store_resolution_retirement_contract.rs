use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::store::{
    CapacityProvider, MaintenanceClock, MaintenanceExecutor, MaintenanceInspector, MaintenanceRun,
    STORE_SQLITE_SCHEMA_VERSION, StoreConnectionFactory, StoreLayout, create_coordinator_schema,
    create_store_schema,
};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};

#[test]
fn fresh_store_has_no_resolution_schema_objects() {
    let store = Connection::open_in_memory().unwrap();
    create_store_schema(&store).unwrap();
    let coordinator = Connection::open_in_memory().unwrap();
    create_coordinator_schema(&coordinator).unwrap();

    assert_eq!(user_version(&store), STORE_SQLITE_SCHEMA_VERSION);
    assert_eq!(retired_schema_objects(&store), Vec::<String>::new());
    assert_eq!(retired_schema_objects(&coordinator), Vec::<String>::new());
    assert!(foreign_key_check(&store).is_empty());
    assert!(foreign_key_check(&coordinator).is_empty());
}

#[test]
fn legacy_store_reader_is_inert_and_writer_retires_resolution_objects() {
    let temp = TempStore::new("legacy-retirement");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0", 7).unwrap();
    seed_legacy_resolution_world(&layout);

    let before_tables = retired_schema_objects(&Connection::open(layout.store_db()).unwrap());
    assert!(
        before_tables
            .iter()
            .any(|name| name == "resolution_bases" || name.ends_with("resolution_bases")),
        "legacy seed must install resolution tables, got {before_tables:?}"
    );
    let before_views = view_rows(&Connection::open(layout.store_db()).unwrap());
    let before_facts = fact_table_digest(&Connection::open(layout.store_db()).unwrap());
    let before_files = leftover_resolution_files(&layout);
    assert!(!before_files.is_empty());
    let before_mtime = store_db_mtime(&layout);

    let factory = StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0");
    let reader = factory.open_reader().unwrap();
    assert_eq!(retired_schema_objects(&reader), before_tables);
    assert_eq!(view_rows(&reader), before_views);
    assert_eq!(fact_table_digest(&reader), before_facts);
    drop(reader);
    assert_eq!(leftover_resolution_files(&layout), before_files);
    assert_eq!(store_db_mtime(&layout), before_mtime);

    let before_marker: Option<String> = Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT value FROM store_meta WHERE key = 'resolution_retired'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(before_marker, None);

    let writer = factory.open_writer().unwrap();
    assert_eq!(retired_schema_objects(&writer), Vec::<String>::new());
    assert_eq!(view_rows(&writer), before_views);
    assert_eq!(fact_table_digest(&writer), before_facts);
    assert!(foreign_key_check(&writer).is_empty());
    drop(writer);
    assert_eq!(leftover_resolution_files(&layout), Vec::<PathBuf>::new());
    let after_marker: String = Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT value FROM store_meta WHERE key = 'resolution_retired'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(after_marker, "1");

    let second = factory.open_writer().unwrap();
    assert_eq!(retired_schema_objects(&second), Vec::<String>::new());
    assert_eq!(view_rows(&second), before_views);
    assert_eq!(fact_table_digest(&second), before_facts);
    drop(second);
    let second_marker: String = Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT value FROM store_meta WHERE key = 'resolution_retired'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(second_marker, "1");

    let fresh = Connection::open_in_memory().unwrap();
    create_store_schema(&fresh).unwrap();
    assert_eq!(
        catalog_hash(&Connection::open(layout.store_db()).unwrap()),
        catalog_hash(&fresh)
    );

    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert!(
        !retired_schema_objects(&coordinator)
            .iter()
            .any(|name| name == "uidx_coord_one_claimed_resolve")
    );
}

#[test]
fn first_gc_apply_on_unmigrated_legacy_store_does_not_stale_plan() {
    let temp = TempStore::new("legacy-gc-apply");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0", 7).unwrap();
    seed_legacy_resolution_world(&layout);
    assert!(!leftover_resolution_files(&layout).is_empty());

    let factory = StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0");
    let plan = MaintenanceInspector::new(
        factory.clone(),
        FixedClock(1_700_000_000_000),
        FixedCapacity,
    )
    .inspect()
    .unwrap();
    assert!(plan.protected_bases.is_empty());
    assert!(plan.eligible_bases.is_empty());
    assert!(plan.protected_deltas.is_empty());
    assert!(plan.eligible_deltas.is_empty());
    assert!(plan.protected_pins.is_empty());
    assert!(plan.expired_pins.is_empty());
    assert_eq!(plan.capacity.facts.base_bytes, 0);
    assert!(
        !plan
            .protected_scratch
            .iter()
            .any(|name| name.starts_with("resolve-") || name.starts_with("resolution-"))
    );

    let mut executor = MaintenanceExecutor::acquire(
        factory,
        MaintenanceRun::new(
            "legacy-gc",
            "owner",
            std::process::id(),
            1_700_000_000_000,
            5_000,
        ),
        &plan,
        FixedCapacity,
    )
    .unwrap();
    executor.apply(&plan).unwrap();
    assert_eq!(leftover_resolution_files(&layout), Vec::<PathBuf>::new());
}

#[test]
fn writer_open_runs_retirement_once_and_skips_on_subsequent_opens() {
    let temp = TempStore::new("retirement-marker-skip");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0", 7).unwrap();
    seed_legacy_resolution_world(&layout);

    // Seed a capability gap matching reference_resolution.% that a pre-2.34 binary could have left
    {
        let conn = Connection::open(layout.store_db()).unwrap();
        conn.execute_batch(
            "INSERT INTO language_capabilities
               (extraction_epoch, language, parser_package, extensions_json, dependency_status,
                target_symbols, target_relationships, target_pending_relationships,
                target_identifiers, target_types, actual_symbols, actual_relationships,
                actual_pending_relationships, actual_identifiers, actual_types,
                kind_coverage_json)
             VALUES (1, 'rust', 'tree-sitter-rust', '[\"rs\"]', 'bundled',
                     1, 1, 1, 1, 1, 1, 1, 1, 1, 1, '{}');
             INSERT INTO language_capability_gaps
               (extraction_epoch, gap_id, language, capability, status, reason, required_closure, evidence_json)
             VALUES (1, 'gap-1', 'rust', 'reference_resolution.test_gap', 'open', 'reason', 'closure', '{}');",
        )
        .unwrap();
    }

    // 1. Verify that before writer open, marker is absent
    {
        let conn = Connection::open(layout.store_db()).unwrap();
        let marker: Option<String> = conn
            .query_row(
                "SELECT value FROM store_meta WHERE key = 'resolution_retired'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(
            marker, None,
            "unmigrated store must not have resolution_retired"
        );
    }

    let factory = StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0");

    // 2. First open_writer performs all retirement work and records resolution_retired = '1'
    let writer = factory.open_writer().unwrap();
    assert_eq!(retired_schema_objects(&writer), Vec::<String>::new());
    let gap_count: i64 = writer
        .query_row(
            "SELECT COUNT(*) FROM language_capability_gaps WHERE capability LIKE 'reference_resolution.%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(gap_count, 0, "first open must reap capability gaps");
    assert_eq!(leftover_resolution_files(&layout), Vec::<PathBuf>::new());
    let marker: String = writer
        .query_row(
            "SELECT value FROM store_meta WHERE key = 'resolution_retired'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        marker, "1",
        "first open must write resolution_retired = '1'"
    );
    drop(writer);

    // 3. Plant probe artifacts that retirement steps would remove if they executed:
    // - A capability gap matching reference_resolution.% (would be reaped by reap_retired_resolution_capability_gaps)
    // - A base file in bases_dir (would be reaped by reap_retired_resolution_files)
    // - A scratch file in scratch_dir (would be reaped by reap_retired_resolution_files)
    {
        let conn = Connection::open(layout.store_db()).unwrap();
        conn.execute_batch(
            "INSERT OR IGNORE INTO language_capabilities
               (extraction_epoch, language, parser_package, extensions_json, dependency_status,
                target_symbols, target_relationships, target_pending_relationships,
                target_identifiers, target_types, actual_symbols, actual_relationships,
                actual_pending_relationships, actual_identifiers, actual_types,
                kind_coverage_json)
             VALUES (1, 'rust', 'tree-sitter-rust', '[\"rs\"]', 'bundled',
                     1, 1, 1, 1, 1, 1, 1, 1, 1, 1, '{}');
             INSERT INTO language_capability_gaps
               (extraction_epoch, gap_id, language, capability, status, reason, required_closure, evidence_json)
             VALUES (1, 'probe-gap', 'rust', 'reference_resolution.probe_gap', 'open', 'reason', 'closure', '{}');",
        )
        .unwrap();
    }
    let probe_base = layout.bases_dir().join("probe-base.db");
    let probe_scratch = layout.scratch_dir().join("resolve-probe.db");
    fs::write(&probe_base, b"probe_base").unwrap();
    fs::write(&probe_scratch, b"probe_scratch").unwrap();

    // 4. Second open_writer: resolution_retired == '1', so all retirement steps MUST be skipped
    let second_writer = factory.open_writer().unwrap();
    let probe_gap_count: i64 = second_writer
        .query_row(
            "SELECT COUNT(*) FROM language_capability_gaps WHERE capability = 'reference_resolution.probe_gap'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        probe_gap_count, 1,
        "second open must skip reap_retired_resolution_capability_gaps when marker is present"
    );
    assert!(
        probe_base.exists(),
        "second open must skip reap_retired_resolution_files (bases_dir) when marker is present"
    );
    assert!(
        probe_scratch.exists(),
        "second open must skip reap_retired_resolution_files (scratch_dir) when marker is present"
    );
    drop(second_writer);

    // 5. If the marker is deleted, subsequent open_writer MUST re-run retirement steps
    {
        let conn = Connection::open(layout.store_db()).unwrap();
        conn.execute(
            "DELETE FROM store_meta WHERE key = 'resolution_retired'",
            [],
        )
        .unwrap();
    }
    let third_writer = factory.open_writer().unwrap();
    let reaped_gap_count: i64 = third_writer
        .query_row(
            "SELECT COUNT(*) FROM language_capability_gaps WHERE capability = 'reference_resolution.probe_gap'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        reaped_gap_count, 0,
        "unmarked open must run retirement steps and reap capability gaps"
    );
    assert!(
        !probe_base.exists(),
        "unmarked open must run retirement steps and reap base files"
    );
    assert!(
        !probe_scratch.exists(),
        "unmarked open must run retirement steps and reap scratch files"
    );
    let marker_restored: String = third_writer
        .query_row(
            "SELECT value FROM store_meta WHERE key = 'resolution_retired'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        marker_restored, "1",
        "retirement must restore marker to '1'"
    );
    drop(third_writer);

    // 6. If marker is present with non-'1' value (e.g. '0'), retirement must also run
    {
        let conn = Connection::open(layout.store_db()).unwrap();
        conn.execute(
            "UPDATE store_meta SET value = '0' WHERE key = 'resolution_retired'",
            [],
        )
        .unwrap();
    }
    fs::write(&probe_base, b"probe_base_2").unwrap();
    let fourth_writer = factory.open_writer().unwrap();
    assert!(
        !probe_base.exists(),
        "open with non-'1' marker must run retirement steps"
    );
    let marker_updated: String = fourth_writer
        .query_row(
            "SELECT value FROM store_meta WHERE key = 'resolution_retired'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(marker_updated, "1", "retirement must update marker to '1'");
    drop(fourth_writer);
}

#[test]
fn fresh_store_writer_open_records_marker_and_skips_subsequent_retirement() {
    let temp = TempStore::new("fresh-retirement-marker");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0", 7).unwrap();

    // Before writer open, freshly created store has resolution_retired = '1' from layout initialization
    {
        let conn = Connection::open(layout.store_db()).unwrap();
        let marker: Option<String> = conn
            .query_row(
                "SELECT value FROM store_meta WHERE key = 'resolution_retired'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(
            marker,
            Some("1".to_string()),
            "fresh store initializes resolution_retired marker"
        );
    }

    let factory = StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0");
    let writer = factory.open_writer().unwrap();
    let marker: String = writer
        .query_row(
            "SELECT value FROM store_meta WHERE key = 'resolution_retired'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        marker, "1",
        "writer open preserves resolution_retired = '1'"
    );
    drop(writer);

    // Plant probe scratch file
    let probe_scratch = layout.scratch_dir().join("resolve-fresh-probe.db");
    fs::write(&probe_scratch, b"fresh_probe").unwrap();

    // Second open skips retirement
    let second_writer = factory.open_writer().unwrap();
    assert!(
        probe_scratch.exists(),
        "subsequent writer open on fresh store must skip retirement steps"
    );
    drop(second_writer);
}

fn seed_legacy_resolution_world(layout: &StoreLayout) {
    let connection = Connection::open(layout.store_db()).unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE;")
        .unwrap();
    connection
        .execute(
            "DELETE FROM store_meta WHERE key = 'resolution_retired'",
            [],
        )
        .unwrap();
    connection
        .execute_batch(LEGACY_RESOLUTION_OBJECTS_SQL)
        .unwrap();
    connection
        .execute_batch(
            "CREATE TABLE views__legacy (
  view_id TEXT PRIMARY KEY CHECK (length(view_id) > 0),
  root TEXT NOT NULL CHECK (length(root) > 0),
  current_generation INTEGER,
  resolution_state TEXT NOT NULL DEFAULT 'unbound',
  resolution_base_id TEXT,
  resolution_delta_generation INTEGER,
  resolution_exact_at INTEGER,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  CHECK (
    (resolution_state = 'unbound'
      AND resolution_base_id IS NULL
      AND resolution_delta_generation IS NULL
      AND resolution_exact_at IS NULL)
    OR
    (resolution_state = 'converging'
      AND current_generation IS NOT NULL
      AND resolution_base_id IS NOT NULL
      AND resolution_delta_generation IS NOT NULL
      AND resolution_exact_at IS NULL)
    OR
    (resolution_state = 'exact'
      AND current_generation IS NOT NULL
      AND resolution_base_id IS NOT NULL
      AND resolution_delta_generation IS NOT NULL
      AND resolution_exact_at = current_generation)
  ),
  FOREIGN KEY (view_id, current_generation)
    REFERENCES manifests(view_id, generation)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (resolution_base_id) REFERENCES resolution_bases(base_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (view_id, resolution_delta_generation)
    REFERENCES resolution_deltas(view_id, delta_generation)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;
INSERT INTO views__legacy
  (view_id,root,current_generation,resolution_state,resolution_base_id,
   resolution_delta_generation,resolution_exact_at,created_at,updated_at)
SELECT view_id,root,current_generation,resolution_state,resolution_base_id,
       resolution_delta_generation,resolution_exact_at,created_at,updated_at
FROM views;
DROP TABLE views;
ALTER TABLE views__legacy RENAME TO views;",
        )
        .unwrap();
    connection
        .execute_batch("COMMIT; PRAGMA foreign_keys=ON;")
        .unwrap();

    let timestamp = "2026-08-18T12:00:00Z";
    connection
        .execute_batch("BEGIN IMMEDIATE; PRAGMA defer_foreign_keys=ON;")
        .unwrap();
    connection
        .execute(
            "INSERT INTO file_versions
             (version_id,path,content_hash,extraction_epoch,language,content_bytes,complete_l1)
             VALUES (1,'src/a.rs','blake3:a',1,'rust',4,1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO views
             (view_id,root,current_generation,resolution_state,resolution_base_id,
              resolution_delta_generation,resolution_exact_at,created_at,updated_at)
             VALUES ('view-a','/repo',1,'exact','base-a',1,1,?1,?1)",
            [timestamp],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'hash-a','request-a',?1)",
            [timestamp],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_bases
             (base_id,manifest_hash,resolver_output_epoch,state,relative_path,
              identifier_count,pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
             VALUES ('base-a','hash-a',1,'ready','bases/base-a.db',0,0,4,'sha256:a','request-a',?1,?1)",
            [timestamp],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_deltas
             (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
              resolver_output_epoch,identifier_replacements,pending_replacements,
              pending_tombstones,exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
             VALUES ('view-a',1,'base-a',1,'hash-a',1,0,0,0,0,0,'[]','request-a',?1)",
            [timestamp],
        )
        .unwrap();
    connection.execute_batch("COMMIT;").unwrap();
    drop(connection);

    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    coordinator
        .execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS uidx_coord_one_claimed_resolve
             ON requests(kind) WHERE kind = 'resolve' AND state = 'claimed';",
        )
        .unwrap();
    drop(coordinator);

    fs::write(layout.bases_dir().join("base-a.db"), b"base").unwrap();
    fs::write(
        layout.scratch_dir().join("resolve-exact-request.db"),
        b"scratch",
    )
    .unwrap();
    fs::write(
        layout.scratch_dir().join("resolve-exact-request.db-wal"),
        b"wal",
    )
    .unwrap();
    fs::write(
        layout.scratch_dir().join("resolve-exact-request.db-shm"),
        b"shm",
    )
    .unwrap();
    fs::write(
        layout
            .scratch_dir()
            .join("resolution-base-a-request.partial.db"),
        b"partial",
    )
    .unwrap();
    fs::write(
        layout
            .scratch_dir()
            .join("resolution-base-a-request.partial.db-wal"),
        b"pwal",
    )
    .unwrap();
    fs::write(
        layout
            .scratch_dir()
            .join("resolution-base-a-request.partial.db-shm"),
        b"pshm",
    )
    .unwrap();
}

fn leftover_resolution_files(layout: &StoreLayout) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(layout.bases_dir()) {
        for entry in entries.flatten() {
            files.push(entry.path());
        }
    }
    if let Ok(entries) = fs::read_dir(layout.scratch_dir()) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("resolve-") || name.starts_with("resolution-") {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    files
}

fn view_rows(connection: &Connection) -> Vec<ViewRow> {
    connection
        .prepare(
            "SELECT view_id,root,current_generation,resolution_state,resolution_base_id,
                    resolution_delta_generation,resolution_exact_at,created_at,updated_at
             FROM views ORDER BY view_id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(ViewRow {
                view_id: row.get(0)?,
                root: row.get(1)?,
                current_generation: row.get(2)?,
                resolution_state: row.get(3)?,
                resolution_base_id: row.get(4)?,
                resolution_delta_generation: row.get(5)?,
                resolution_exact_at: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn fact_table_digest(connection: &Connection) -> String {
    let mut digest = Sha256::new();
    for table in [
        "file_versions",
        "manifests",
        "manifest_entries",
        "symbols",
        "identifiers",
        "store_log",
    ] {
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        if !exists {
            continue;
        }
        let sql = format!("SELECT * FROM {table} ORDER BY rowid");
        let mut statement = connection.prepare(&sql).unwrap();
        let column_count = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        digest.update(table.as_bytes());
        while let Some(row) = rows.next().unwrap() {
            for index in 0..column_count {
                match row.get_ref(index).unwrap() {
                    rusqlite::types::ValueRef::Null => digest.update(0u8.to_be_bytes()),
                    rusqlite::types::ValueRef::Integer(value) => {
                        digest.update(1u8.to_be_bytes());
                        digest.update(value.to_be_bytes());
                    }
                    rusqlite::types::ValueRef::Real(value) => {
                        digest.update(2u8.to_be_bytes());
                        digest.update(value.to_be_bytes());
                    }
                    rusqlite::types::ValueRef::Text(value)
                    | rusqlite::types::ValueRef::Blob(value) => {
                        digest.update(3u8.to_be_bytes());
                        digest.update((value.len() as u64).to_be_bytes());
                        digest.update(value);
                    }
                }
            }
        }
    }
    format!("{:x}", digest.finalize())
}

fn retired_schema_objects(connection: &Connection) -> Vec<String> {
    connection
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE name LIKE 'resolution_%'
                OR name LIKE '%resolution_scope_%'
                OR name = 'uidx_coord_one_claimed_resolve'
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn foreign_key_check(connection: &Connection) -> Vec<String> {
    connection
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query_map([], |row| {
            Ok(format!(
                "{}:{}",
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn catalog_hash(connection: &Connection) -> String {
    let catalog = connection
        .prepare(
            "SELECT type, name, tbl_name, sql
             FROM sqlite_master
             WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%'
             ORDER BY type, name",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(format!(
                "{}|{}|{}|{}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                compact_whitespace(&row.get::<_, String>(3)?),
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    format!("{:x}", Sha256::digest(catalog.as_bytes()))
}

fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn user_version(connection: &Connection) -> i64 {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

fn store_db_mtime(layout: &StoreLayout) -> std::time::SystemTime {
    fs::metadata(layout.store_db()).unwrap().modified().unwrap()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ViewRow {
    view_id: String,
    root: String,
    current_generation: Option<i64>,
    resolution_state: String,
    resolution_base_id: Option<String>,
    resolution_delta_generation: Option<i64>,
    resolution_exact_at: Option<i64>,
    created_at: String,
    updated_at: String,
}

struct TempStore {
    path: PathBuf,
}

impl TempStore {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "julie-store-retirement-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Clone, Copy)]
struct FixedClock(i64);

impl MaintenanceClock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.0
    }
}

#[derive(Clone, Copy)]
struct FixedCapacity;

impl CapacityProvider for FixedCapacity {
    fn free_bytes(&self, _path: &Path) -> Result<u64, std::io::Error> {
        Ok(512 * 1024 * 1024)
    }

    fn staged_generation_bytes(&self, _path: &Path) -> Result<u64, std::io::Error> {
        Ok(0)
    }
}

const LEGACY_RESOLUTION_OBJECTS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS resolution_bases (
  base_id TEXT PRIMARY KEY CHECK (length(base_id) > 0),
  manifest_hash TEXT NOT NULL CHECK (length(manifest_hash) > 0),
  resolver_output_epoch INTEGER NOT NULL CHECK (resolver_output_epoch > 0),
  state TEXT NOT NULL CHECK (state IN ('building', 'ready')),
  relative_path TEXT NOT NULL CHECK (length(relative_path) > 0),
  identifier_count INTEGER NOT NULL CHECK (identifier_count >= 0),
  pending_count INTEGER NOT NULL CHECK (pending_count >= 0),
  file_bytes INTEGER,
  file_sha256 TEXT,
  request_id TEXT NOT NULL CHECK (length(request_id) > 0),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS resolution_base_versions (
  base_id TEXT NOT NULL,
  version_id INTEGER NOT NULL,
  PRIMARY KEY (base_id, version_id)
) STRICT;
CREATE TABLE IF NOT EXISTS resolution_deltas (
  view_id TEXT NOT NULL,
  delta_generation INTEGER NOT NULL CHECK (delta_generation > 0),
  base_id TEXT NOT NULL,
  manifest_generation INTEGER NOT NULL CHECK (manifest_generation > 0),
  manifest_hash TEXT NOT NULL CHECK (length(manifest_hash) > 0),
  resolver_output_epoch INTEGER NOT NULL CHECK (resolver_output_epoch > 0),
  identifier_replacements INTEGER NOT NULL CHECK (identifier_replacements >= 0),
  pending_replacements INTEGER NOT NULL CHECK (pending_replacements >= 0),
  pending_tombstones INTEGER NOT NULL CHECK (pending_tombstones >= 0),
  exact_gap_rows INTEGER NOT NULL CHECK (exact_gap_rows >= 0),
  exact_gap_files INTEGER NOT NULL CHECK (exact_gap_files >= 0),
  exact_gap_json TEXT NOT NULL CHECK (length(exact_gap_json) > 0),
  request_id TEXT NOT NULL CHECK (length(request_id) > 0),
  created_at TEXT NOT NULL,
  PRIMARY KEY (view_id, delta_generation)
) STRICT;
CREATE TABLE IF NOT EXISTS resolution_identifier_deltas (
  view_id TEXT NOT NULL,
  delta_generation INTEGER NOT NULL,
  version_id INTEGER NOT NULL,
  identifier_id TEXT NOT NULL,
  outcome TEXT NOT NULL,
  PRIMARY KEY (view_id, delta_generation, version_id, identifier_id)
) STRICT;
CREATE TABLE IF NOT EXISTS resolution_pending_deltas (
  view_id TEXT NOT NULL,
  delta_generation INTEGER NOT NULL,
  version_id INTEGER NOT NULL,
  pending_relationship_id TEXT NOT NULL,
  operation TEXT NOT NULL,
  PRIMARY KEY (view_id, delta_generation, version_id, pending_relationship_id)
) STRICT;
CREATE TABLE IF NOT EXISTS resolution_pins (
  pin_id TEXT PRIMARY KEY,
  owner_kind TEXT NOT NULL,
  owner_id TEXT NOT NULL,
  view_id TEXT NOT NULL,
  manifest_generation INTEGER NOT NULL,
  base_id TEXT NOT NULL,
  delta_generation INTEGER,
  expires_at TEXT NOT NULL,
  created_at TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS resolution_scope_state (
  view_id TEXT PRIMARY KEY,
  predecessor_manifest_generation INTEGER,
  predecessor_manifest_hash TEXT,
  base_id TEXT,
  delta_generation INTEGER,
  resolver_output_epoch INTEGER,
  current_manifest_generation INTEGER,
  current_manifest_hash TEXT,
  journal_through_transition_id INTEGER
) STRICT;
CREATE TABLE IF NOT EXISTS resolution_scope_batches (
  transition_id INTEGER PRIMARY KEY,
  view_id TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS resolution_scope_journal (
  transition_id INTEGER NOT NULL,
  path TEXT NOT NULL,
  PRIMARY KEY (transition_id, path)
) STRICT;
"#;
