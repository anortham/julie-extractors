use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use julie_extract_artifact::store::{
    MaintenanceError, MaintenanceExecutor, MaintenanceRun, StoreConnectionFactory, StoreLayout,
    create_coordinator_schema,
};
use rusqlite::Connection;

#[test]
fn wholly_absent_legacy_catalog_installs_atomically_and_idempotently() {
    let temp = TempStore::new("legacy-reader-catalog");
    let layout = legacy_store(&temp);
    let before = Connection::open(layout.coordinator_db()).unwrap();
    assert!(reader_catalog(&before).is_empty());
    drop(before);

    activate(&layout, "legacy-reader-catalog-first").unwrap();

    let fresh = Connection::open_in_memory().unwrap();
    create_coordinator_schema(&fresh).unwrap();
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(reader_catalog(&coordinator), reader_catalog(&fresh));
    assert_eq!(registration_count(&coordinator), 0);
    assert_eq!(store_floor(&layout), "2.40.0");
    drop(coordinator);

    activate(&layout, "legacy-reader-catalog-second").unwrap();

    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(reader_catalog(&coordinator), reader_catalog(&fresh));
    assert_eq!(registration_count(&coordinator), 0);
    assert_eq!(store_floor(&layout), "2.40.0");
}

#[test]
fn complete_empty_catalog_below_reader_floor_is_accepted() {
    let temp = TempStore::new("complete-empty-reader-catalog");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.39.0", 9).unwrap();
    let before = reader_catalog(&Connection::open(layout.coordinator_db()).unwrap());

    activate(&layout, "complete-empty-reader-catalog").unwrap();

    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(reader_catalog(&coordinator), before);
    assert_eq!(registration_count(&coordinator), 0);
    assert_eq!(store_floor(&layout), "2.40.0");
}

#[test]
fn partial_catalog_refuses_without_mutation() {
    for object in [
        "idx_read_reader_registrations_generation",
        "idx_read_reader_registrations_expiry",
        "trg_reader_registrations_immutable_identity",
        "trg_reader_registrations_liveness_coherent",
    ] {
        let temp = TempStore::new(object);
        let layout = StoreLayout::create(temp.path(), "family-a", "2.39.0", 9).unwrap();
        let coordinator = Connection::open(layout.coordinator_db()).unwrap();
        coordinator
            .execute_batch(&format!(
                "DROP INDEX IF EXISTS {object}; DROP TRIGGER IF EXISTS {object};"
            ))
            .unwrap();
        let before = reader_catalog(&coordinator);
        drop(coordinator);

        let error = activate(&layout, "partial-reader-catalog").unwrap_err();

        assert_reader_catalog_refusal(error);
        let coordinator = Connection::open(layout.coordinator_db()).unwrap();
        assert_eq!(reader_catalog(&coordinator), before);
        assert_eq!(maintenance_owner_count(&coordinator), 0);
        assert_eq!(store_floor(&layout), "2.39.0");
    }
}

#[test]
fn wrong_immutability_trigger_refuses_without_repair() {
    let temp = TempStore::new("wrong-reader-trigger");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.39.0", 9).unwrap();
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    coordinator
        .execute_batch(
            "DROP TRIGGER trg_reader_registrations_immutable_identity;
             CREATE TRIGGER trg_reader_registrations_immutable_identity
             BEFORE UPDATE ON reader_registrations
             BEGIN
               SELECT RAISE(ABORT, 'wrong reader trigger');
             END;",
        )
        .unwrap();
    let before = reader_catalog(&coordinator);
    drop(coordinator);

    let error = activate(&layout, "wrong-reader-trigger").unwrap_err();

    assert_reader_catalog_refusal(error);
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(reader_catalog(&coordinator), before);
    assert_eq!(maintenance_owner_count(&coordinator), 0);
    assert_eq!(store_floor(&layout), "2.39.0");
}

#[test]
fn reader_enabled_missing_catalog_refuses_without_recreation() {
    let temp = TempStore::new("reader-enabled-missing-catalog");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.40.0", 9).unwrap();
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute_batch("DROP TABLE reader_registrations;")
        .unwrap();

    let error = activate(&layout, "reader-enabled-missing-catalog").unwrap_err();

    assert_reader_catalog_refusal(error);
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert!(reader_catalog(&coordinator).is_empty());
    assert_eq!(maintenance_owner_count(&coordinator), 0);
    assert_eq!(store_floor(&layout), "2.40.0");
}

#[test]
fn nonempty_catalog_below_reader_floor_refuses_without_losing_rows() {
    let temp = TempStore::new("nonempty-reader-catalog");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.39.0", 9).unwrap();
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    insert_registration(&coordinator);
    drop(coordinator);

    let error = activate(&layout, "nonempty-reader-catalog").unwrap_err();

    assert_reader_catalog_refusal(error);
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(registration_count(&coordinator), 1);
    assert_eq!(maintenance_owner_count(&coordinator), 0);
    assert_eq!(store_floor(&layout), "2.39.0");
}

#[test]
fn foreign_maintenance_or_writer_owner_refuses_legacy_installation() {
    for blocker in ["maintenance_intent", "writer_lease"] {
        let temp = TempStore::new(blocker);
        let layout = legacy_store(&temp);
        let coordinator = Connection::open(layout.coordinator_db()).unwrap();
        match blocker {
            "maintenance_intent" => coordinator
                .execute(
                    "INSERT INTO maintenance_intent
                     (resource,run_id,action,source_generation_name,owner_id,owner_pid,
                      fencing_token,heartbeat_at,expires_at,started_at,plan_fingerprint,
                      source_min_writer_version)
                     VALUES ('store-maintenance','foreign','gc','gen-000001','foreign',?1,
                             1,1,9223372036854775807,1,'foreign-plan','2.39.0')",
                    [std::process::id()],
                )
                .unwrap(),
            "writer_lease" => coordinator
                .execute(
                    "INSERT INTO writer_lease
                     (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,
                      fencing_token)
                     VALUES ('store-writer','foreign','2.40.0',?1,1,9223372036854775807,1)",
                    [std::process::id()],
                )
                .unwrap(),
            _ => unreachable!(),
        };
        drop(coordinator);

        let error = activate(&layout, "blocked-reader-catalog").unwrap_err();

        assert_eq!(error.code(), "maintenance_busy");
        assert!(reader_catalog(&Connection::open(layout.coordinator_db()).unwrap()).is_empty());
        assert_eq!(store_floor(&layout), "2.39.0");
    }
}

#[cfg(feature = "test-store-crash")]
#[test]
fn aborted_catalog_install_publishes_neither_objects_nor_registrations() {
    let temp = TempStore::new("aborted-reader-catalog");
    let layout = legacy_store(&temp);
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "reader_catalog_install_crash_child"])
        .env("JULIE_READER_CATALOG_CRASH_ROOT", temp.path())
        .env(
            "JULIE_EXTRACT_STORE_TEST_CRASH_AT",
            "reader_catalog_installed_before_floor",
        )
        .status()
        .unwrap();
    assert!(!status.success());

    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert!(reader_catalog(&coordinator).is_empty());
    assert_eq!(
        coordinator
            .query_row(
                "SELECT source_min_writer_version FROM maintenance_intent
                 WHERE resource='store-maintenance'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "2.39.0"
    );
    assert_eq!(store_floor(&layout), "2.40.0");
    drop(coordinator);

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match activate(&layout, "aborted-reader-catalog-recovery") {
            Ok(()) => break,
            Err(MaintenanceError::MaintenanceBusy) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => panic!("reader catalog recovery failed: {error}"),
        }
    }

    let fresh = Connection::open_in_memory().unwrap();
    create_coordinator_schema(&fresh).unwrap();
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(reader_catalog(&coordinator), reader_catalog(&fresh));
    assert_eq!(registration_count(&coordinator), 0);
    assert_eq!(maintenance_owner_count(&coordinator), 0);
    assert_eq!(store_floor(&layout), "2.40.0");
}

#[cfg(feature = "test-store-crash")]
#[test]
#[ignore = "subprocess crash probe"]
fn reader_catalog_install_crash_child() {
    let root = std::env::var_os("JULIE_READER_CATALOG_CRASH_ROOT").unwrap();
    let layout = StoreLayout::open(root).unwrap();
    MaintenanceExecutor::activate_reader_writer_floor(
        StoreConnectionFactory::new(layout, "family-a", "2.40.0"),
        MaintenanceRun::new(
            "aborted-reader-catalog-child",
            "catalog-owner",
            std::process::id(),
            100,
            1_000,
        ),
    )
    .unwrap();
}

fn legacy_store(temp: &TempStore) -> StoreLayout {
    let layout = StoreLayout::create(temp.path(), "family-a", "2.39.0", 9).unwrap();
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute_batch("DROP TABLE reader_registrations;")
        .unwrap();
    layout
}

fn activate(
    layout: &StoreLayout,
    run_id: &str,
) -> Result<(), julie_extract_artifact::store::MaintenanceError> {
    MaintenanceExecutor::activate_reader_writer_floor(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.40.0"),
        MaintenanceRun::new(run_id, "catalog-owner", std::process::id(), 100, 30_000),
    )
}

fn reader_catalog(connection: &Connection) -> Vec<(String, String, String, Option<String>)> {
    connection
        .prepare(
            "SELECT type,name,tbl_name,sql FROM sqlite_schema
             WHERE name='reader_registrations' OR tbl_name='reader_registrations'
             ORDER BY type,name",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn registration_count(connection: &Connection) -> i64 {
    connection
        .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn maintenance_owner_count(connection: &Connection) -> i64 {
    connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM maintenance_intent) +
               (SELECT COUNT(*) FROM writer_lease)",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn insert_registration(connection: &Connection) {
    connection
        .execute(
            "INSERT INTO reader_registrations
             (pin_id,owner_nonce,owner_label,family_id,view_id,manifest_generation,generation_name,
              owner_pid,owner_birth_identity,store_instance_id,manifest_hash,
              extraction_identity_epoch,served_store_log_sequence,acquired_at,heartbeat_at,
              expires_at,min_retained_store_log_sequence,snapshot_fingerprint)
             VALUES ('reader-1','0123456789abcdef0123456789abcdef','miller','family-a','view-a',
                     42,'gen-000001',1234,'birth-1','family-a:gen-000001','manifest-hash',9,
                     800,100,100,200,700,'snapshot-fingerprint')",
            [],
        )
        .unwrap();
}

fn assert_reader_catalog_refusal(error: MaintenanceError) {
    assert_eq!(error.code(), "invalid_maintenance_metadata");
    assert!(error.to_string().contains("reader catalog"));
}

fn store_floor(layout: &StoreLayout) -> String {
    Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT value FROM store_meta WHERE key='min_writer_version'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

struct TempStore {
    path: PathBuf,
}

impl TempStore {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "julie-reader-catalog-{label}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
