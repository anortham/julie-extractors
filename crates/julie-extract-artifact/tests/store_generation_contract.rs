use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use julie_extract_artifact::store::{
    GenerationFence, PartialGenerationOwner, StoreConnectionError, StoreConnectionFactory,
    StoreLayout, StoreLayoutError, write_partial_generation_owner,
};
use rusqlite::Connection;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn opening_an_existing_generation_never_requests_a_store_write_lock() {
    let temp = TempStore::new("query-only-existing");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let blocker = Connection::open(layout.store_db()).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();

    let reopened = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();

    assert_eq!(reopened.generation_name(), "gen-001");
    blocker.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn missing_current_beside_a_named_generation_requires_explicit_recovery() {
    let temp = TempStore::new("missing-current");
    StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    fs::remove_file(temp.path().join("CURRENT")).unwrap();

    let error = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap_err();

    assert!(matches!(
        error,
        StoreLayoutError::CurrentRecoveryRequired { generations }
            if generations == vec!["gen-001".to_string()]
    ));
    assert!(!temp.path().join("CURRENT").exists());
}

#[test]
fn retired_and_replaced_generations_refuse_new_writers_but_existing_readers_survive() {
    let temp = TempStore::new("retired");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0");
    let reader = factory.open_reader().unwrap();
    Connection::open(layout.store_db())
        .unwrap()
        .execute(
            "UPDATE store_meta SET value = 'retired' WHERE key = 'generation_state'",
            [],
        )
        .unwrap();

    let error = factory.open_writer().unwrap_err();

    assert!(matches!(
        error,
        StoreConnectionError::GenerationNotServing { state } if state == "retired"
    ));
    assert_eq!(
        reader
            .query_row(
                "SELECT value FROM store_meta WHERE key = 'generation_state'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "retired"
    );

    Connection::open(layout.store_db())
        .unwrap()
        .execute(
            "UPDATE store_meta SET value = 'serving' WHERE key = 'generation_state'",
            [],
        )
        .unwrap();
    let replacement = temp.path().join("gen-002");
    fs::create_dir(&replacement).unwrap();
    fs::create_dir(replacement.join("bases")).unwrap();
    fs::copy(layout.store_db(), replacement.join("store.db")).unwrap();
    fs::write(temp.path().join("CURRENT"), "gen-002\n").unwrap();

    let error = factory.open_writer().unwrap_err();

    assert!(matches!(
        error,
        StoreConnectionError::CurrentGenerationChanged { expected, found }
            if expected == "gen-001" && found == "gen-002"
    ));
}

#[test]
fn foreign_maintenance_intent_blocks_writers_and_matching_fence_is_admitted() {
    let temp = TempStore::new("maintenance-fence");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "INSERT INTO maintenance_intent
             (resource, run_id, action, source_generation_name, owner_id, owner_pid,
              fencing_token, heartbeat_at, expires_at, started_at, plan_fingerprint,
              source_min_writer_version)
             VALUES ('store-maintenance', 'run-a', 'promote', 'gen-001', 'owner-a', 7,
                     41, 1, 9223372036854775807, 1, 'plan-a', '2.30.0')",
            [],
        )
        .unwrap();
    let factory = StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0");

    let error = factory.open_writer().unwrap_err();

    assert!(matches!(
        error,
        StoreConnectionError::MaintenanceInProgress { run_id } if run_id == "run-a"
    ));

    let holder_only = GenerationFence::writer(&layout, "owner-a", 7, 41, 10);
    let error = factory
        .clone()
        .with_generation_fence(holder_only)
        .open_writer()
        .unwrap_err();
    assert!(matches!(
        error,
        StoreConnectionError::MaintenanceInProgress { run_id } if run_id == "run-a"
    ));

    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "INSERT INTO writer_lease
             (resource, holder_id, holder_version, holder_pid, heartbeat_at, expires_at,
              fencing_token)
             VALUES ('store-writer', 'owner-a', '2.30.0', 7, 1, 9223372036854775807, 41)",
            [],
        )
        .unwrap();
    let wrong_pid_fence = GenerationFence::maintenance(&layout, "run-a", "owner-a", 8, 41, 10);
    let error = factory
        .clone()
        .with_generation_fence(wrong_pid_fence)
        .open_writer()
        .unwrap_err();
    assert!(matches!(
        error,
        StoreConnectionError::MaintenanceInProgress { run_id } if run_id == "run-a"
    ));

    let fence = GenerationFence::maintenance(&layout, "run-a", "owner-a", 7, 41, 10);
    let writer = factory.with_generation_fence(fence).open_writer().unwrap();
    assert_eq!(
        writer
            .query_row(
                "SELECT value FROM store_meta WHERE key = 'generation_state'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "serving"
    );
    drop(writer);
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    coordinator
        .execute_batch(
            "DELETE FROM writer_lease;
             DELETE FROM maintenance_intent;
             INSERT INTO maintenance_intent
             (resource, run_id, action, source_generation_name, owner_id, owner_pid,
              fencing_token, heartbeat_at, expires_at, started_at, plan_fingerprint,
              source_min_writer_version)
             VALUES ('store-maintenance', 'run-a', 'promote', 'gen-001', 'owner-a', 7,
                     41, 0, 1, 0, 'plan-a', '2.30.0');
             INSERT INTO writer_lease
             (resource, holder_id, holder_version, holder_pid, heartbeat_at, expires_at,
              fencing_token)
             VALUES ('store-writer', 'owner-a', '2.30.0', 7, 0, 1, 41);",
        )
        .unwrap();
    let expired = GenerationFence::maintenance(&layout, "run-a", "owner-a", 7, 41, 0);
    assert!(matches!(
        StoreConnectionFactory::new(layout, "family-a", "2.30.0")
            .with_generation_fence(expired)
            .open_writer(),
        Err(StoreConnectionError::WriterLeaseLost)
    ));
}

#[test]
fn writer_open_does_not_advance_binary_version_until_explicitly_requested() {
    let temp = TempStore::new("explicit-binary-advance");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout.clone(), "family-a", "2.31.0");

    let mut writer = factory.open_writer().unwrap();
    assert_eq!(metadata(&writer, "binary_version"), "2.30.0");

    factory.advance_binary_version(&mut writer).unwrap();
    assert_eq!(metadata(&writer, "binary_version"), "2.31.0");
}

#[test]
fn expired_writer_lease_cannot_authorize_a_later_write() {
    let temp = TempStore::new("expired-writer-lease");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout.clone(), "family-a", "2.31.0");
    let mut writer = factory.open_writer().unwrap();
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "UPDATE writer_lease SET expires_at = 1 WHERE resource = 'store-writer'",
            [],
        )
        .unwrap();

    let error = factory.advance_binary_version(&mut writer).unwrap_err();

    assert!(matches!(error, StoreConnectionError::WriterLeaseLost));
    assert_eq!(metadata(&writer, "binary_version"), "2.30.0");
}

#[cfg(unix)]
#[test]
fn partial_generation_cleanup_requires_a_dead_owner_and_absent_or_expired_intent() {
    let temp = TempStore::new("partial-ownership");
    StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let partial = temp.path().join(".gen-002.partial");
    fs::create_dir(&partial).unwrap();

    let error = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap_err();
    assert!(matches!(
        error,
        StoreLayoutError::PartialGenerationRecoveryRequired { .. }
    ));
    assert!(partial.exists());

    write_partial_generation_owner(
        &partial,
        &PartialGenerationOwner {
            run_id: "run-live".to_string(),
            owner_id: "owner-live".to_string(),
            owner_pid: std::process::id(),
            fencing_token: 91,
            expires_at: 1,
        },
    )
    .unwrap();
    Connection::open(temp.path().join("coord.db"))
        .unwrap()
        .execute(
            "INSERT INTO maintenance_intent
             (resource, run_id, action, source_generation_name, owner_id, owner_pid,
              fencing_token, heartbeat_at, expires_at, started_at, plan_fingerprint,
              source_min_writer_version)
             VALUES ('store-maintenance', 'run-live', 'promote', 'gen-001', 'owner-live',
                     ?1, 91, 0, 1, 0, 'plan', '2.30.0')",
            [i64::from(std::process::id())],
        )
        .unwrap();

    let error = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap_err();
    assert!(matches!(
        error,
        StoreLayoutError::PartialGenerationRecoveryRequired { .. }
    ));
    assert!(partial.exists());

    Connection::open(temp.path().join("coord.db"))
        .unwrap()
        .execute(
            "DELETE FROM maintenance_intent WHERE resource = 'store-maintenance'",
            [],
        )
        .unwrap();

    let mut child = std::process::Command::new("sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    let dead_pid = child.id();
    child.wait().unwrap();

    write_partial_generation_owner(
        &partial,
        &PartialGenerationOwner {
            run_id: "run-dead".to_string(),
            owner_id: "owner-dead".to_string(),
            owner_pid: dead_pid,
            fencing_token: 91,
            expires_at: 1,
        },
    )
    .unwrap();
    Connection::open(temp.path().join("coord.db"))
        .unwrap()
        .execute(
            "INSERT INTO maintenance_intent
             (resource, run_id, action, source_generation_name, owner_id, owner_pid,
              fencing_token, heartbeat_at, expires_at, started_at, plan_fingerprint,
              source_min_writer_version)
             VALUES ('store-maintenance', 'run-dead', 'promote', 'gen-001', 'owner-dead',
                     ?1, 91, 0, 1, 0, 'plan', '2.30.0')",
            [i64::from(dead_pid)],
        )
        .unwrap();

    StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    assert!(!partial.exists());
}

fn metadata(connection: &Connection, key: &str) -> String {
    connection
        .query_row(
            "SELECT value FROM store_meta WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .unwrap()
}

struct TempStore {
    path: PathBuf,
}

impl TempStore {
    fn new(name: &str) -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "julie-store-generation-{name}-{}-{id}",
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
