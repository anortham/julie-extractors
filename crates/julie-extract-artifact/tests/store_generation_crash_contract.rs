#![cfg(feature = "test-store-crash")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use julie_extract_artifact::store::{
    CapacityProvider, GenerationLifecycle, GenerationPolicy, MaintenanceAction, MaintenanceClock,
    MaintenanceInspector, MaintenanceRun, StoreConnectionError, StoreConnectionFactory,
    StoreLayout,
};
use rusqlite::Connection;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn every_promotion_boundary_recovers_the_same_generation_without_duplicates() {
    for boundary in [
        "maintenance_after_intent_before_floor",
        "generation_after_partial_owner",
        "generation_after_logical_copy",
        "generation_after_validation",
        "generation_before_directory_rename",
        "generation_after_directory_rename",
        "generation_after_source_retired",
        "generation_after_current_publish",
        "generation_after_destination_serving",
        "generation_after_maintenance_finish",
    ] {
        let temp = TempStore::new(boundary);
        let layout =
            StoreLayout::create(temp.path(), "family-generation-crash", "2.30.0", 7).unwrap();
        seed_source(&layout);
        let output = run_worker(temp.path(), boundary);
        assert!(
            !output.status.success(),
            "boundary={boundary}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        thread::sleep(Duration::from_millis(300));
        let current = StoreLayout::open(temp.path()).unwrap();
        let plan = inspect_plan(&current);
        let mut retry = GenerationLifecycle::acquire(
            factory(&current),
            MaintenanceRun::new(
                format!("retry-{boundary}"),
                "retry-owner",
                std::process::id(),
                2_000,
                5_000,
            ),
            &plan,
            MaintenanceAction::Promote,
            FixedCapacity,
        )
        .unwrap();
        let report = retry.promote(&plan, &GenerationPolicy::default()).unwrap();
        assert_eq!(report.destination_generation, "gen-002", "{boundary}");
        let serving = StoreLayout::open(temp.path()).unwrap();
        assert_eq!(serving.generation_name(), "gen-002", "{boundary}");
        assert!(!temp.path().join("gen-003").exists(), "{boundary}");
        assert!(!temp.path().join(".gen-002.partial").exists(), "{boundary}");
        assert!(
            !serving.generation_dir().join("OWNER.json").exists(),
            "{boundary}"
        );
        let store = Connection::open(serving.store_db()).unwrap();
        let coord = Connection::open(serving.coordinator_db()).unwrap();
        assert_valid(&store);
        assert_valid(&coord);
        assert_eq!(count(&store, "file_versions"), 1, "{boundary}");
        assert_eq!(count(&store, "manifests"), 2, "{boundary}");
        assert_eq!(count(&store, "store_log"), 1, "{boundary}");
        assert_eq!(count(&coord, "maintenance_intent"), 0, "{boundary}");
        assert_eq!(count(&coord, "writer_lease"), 0, "{boundary}");
    }
}

#[test]
fn dead_partial_owner_is_replaced_before_its_expiry() {
    let temp = TempStore::new("dead-partial-before-expiry");
    let layout = StoreLayout::create(temp.path(), "family-generation-crash", "2.30.0", 7).unwrap();
    seed_source(&layout);
    let output = run_worker_with_lease(
        temp.path(),
        "generation_after_partial_owner",
        Duration::from_secs(5),
    );
    assert!(!output.status.success());

    let current = StoreLayout::open(temp.path()).unwrap();
    let plan = inspect_plan(&current);
    let mut retry = GenerationLifecycle::acquire(
        factory(&current),
        MaintenanceRun::new(
            "retry-dead-partial",
            "retry-owner",
            std::process::id(),
            2_000,
            5_000,
        ),
        &plan,
        MaintenanceAction::Promote,
        FixedCapacity,
    )
    .unwrap();
    let report = retry.promote(&plan, &GenerationPolicy::default()).unwrap();
    assert_eq!(report.destination_generation, "gen-002");
    assert!(!temp.path().join(".gen-002.partial").exists());
}

#[test]
fn crash_between_intent_and_floor_blocks_foreign_writers_via_intent_alone() {
    let temp = TempStore::new("intent-before-floor");
    let layout = StoreLayout::create(temp.path(), "family-generation-crash", "2.30.0", 7).unwrap();
    seed_source(&layout);
    let output = run_worker_with_lease(
        temp.path(),
        "maintenance_after_intent_before_floor",
        Duration::from_secs(5),
    );
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let store = Connection::open(layout.store_db()).unwrap();
    assert_eq!(
        store
            .query_row(
                "SELECT value FROM store_meta WHERE key='min_writer_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "2.30.0"
    );
    let tmp_count: i64 = store
        .query_row(
            "SELECT COUNT(*) FROM store_meta WHERE key LIKE 'maintenance_tmp_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tmp_count, 0);
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(
        coord
            .query_row(
                "SELECT COUNT(*) FROM maintenance_intent WHERE expires_at > 0",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );

    let error = factory(&layout).open_writer().unwrap_err();
    match error {
        StoreConnectionError::MaintenanceInProgress { ref run_id } if run_id == "crash-run" => {}
        other => panic!("expected MaintenanceInProgress for crash-run, got {other:?}"),
    }

    thread::sleep(Duration::from_millis(300));
    // Drop dead crash-owner ownership so a successor can take over.
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute_batch(
            "DELETE FROM writer_lease;
             DELETE FROM maintenance_intent;",
        )
        .unwrap();
    let current = StoreLayout::open(temp.path()).unwrap();
    let plan = inspect_plan(&current);
    let mut retry = GenerationLifecycle::acquire(
        factory(&current),
        MaintenanceRun::new(
            "retry-intent-before-floor",
            "retry-owner",
            std::process::id(),
            2_000,
            5_000,
        ),
        &plan,
        MaintenanceAction::Promote,
        FixedCapacity,
    )
    .unwrap();
    let report = retry.promote(&plan, &GenerationPolicy::default()).unwrap();
    assert_eq!(report.destination_generation, "gen-002");
}

#[test]
fn forward_rollback_crashes_recover_with_scope_explicitly_invalidated() {
    for boundary in [
        "generation_after_logical_copy",
        "generation_after_directory_rename",
        "generation_after_current_publish",
    ] {
        let temp = TempStore::new(boundary);
        let initial =
            StoreLayout::create(temp.path(), "family-generation-crash", "2.30.0", 7).unwrap();
        seed_source(&initial);
        let initial_plan = inspect_plan(&initial);
        let mut promotion = GenerationLifecycle::acquire(
            factory(&initial),
            MaintenanceRun::new(
                format!("prepare-{boundary}"),
                "prepare-owner",
                std::process::id(),
                500,
                5_000,
            ),
            &initial_plan,
            MaintenanceAction::Promote,
            FixedCapacity,
        )
        .unwrap();
        promotion
            .promote(&initial_plan, &GenerationPolicy::default())
            .unwrap();

        let crashed = run_rollback_worker(temp.path(), boundary);
        assert!(
            !crashed.status.success(),
            "boundary={boundary}: {}",
            String::from_utf8_lossy(&crashed.stderr)
        );
        thread::sleep(Duration::from_millis(300));
        let current = StoreLayout::open(temp.path()).unwrap();
        let plan = inspect_plan(&current);
        let mut retry = GenerationLifecycle::acquire(
            factory(&current),
            MaintenanceRun::new(
                format!("retry-rollback-{boundary}"),
                "retry-owner",
                std::process::id(),
                2_000,
                5_000,
            ),
            &plan,
            MaintenanceAction::Rollback,
            FixedCapacity,
        )
        .unwrap();
        let report = retry
            .rollback(&plan, &GenerationPolicy::default(), "gen-001")
            .unwrap();
        assert_eq!(report.destination_generation, "gen-003", "{boundary}");
        let serving = StoreLayout::open(temp.path()).unwrap();
        let store = Connection::open(serving.store_db()).unwrap();
        assert_valid(&store);
        assert_eq!(count(&store, "file_versions"), 1, "{boundary}");
        assert_eq!(count(&store, "manifests"), 2, "{boundary}");
    }
}

#[test]
fn generation_promotion_crash_worker() {
    let Some(root) = std::env::var_os("JULIE_TEST_GENERATION_ROOT") else {
        return;
    };
    let layout = StoreLayout::open(PathBuf::from(root)).unwrap();
    let plan = inspect_plan(&layout);
    let lease_duration_ms = std::env::var("JULIE_TEST_GENERATION_LEASE_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(100);
    let action = if std::env::var_os("JULIE_TEST_GENERATION_ROLLBACK").is_some() {
        MaintenanceAction::Rollback
    } else {
        MaintenanceAction::Promote
    };
    let mut lifecycle = GenerationLifecycle::acquire(
        factory(&layout),
        MaintenanceRun::new(
            "crash-run",
            "crash-owner",
            std::process::id(),
            1_000,
            lease_duration_ms,
        ),
        &plan,
        action,
        FixedCapacity,
    )
    .unwrap();
    if action == MaintenanceAction::Rollback {
        lifecycle
            .rollback(&plan, &GenerationPolicy::default(), "gen-001")
            .unwrap();
    } else {
        lifecycle
            .promote(&plan, &GenerationPolicy::default())
            .unwrap();
    }
    panic!("worker passed crash boundary");
}

fn seed_source(layout: &StoreLayout) {
    Connection::open(layout.store_db())
        .unwrap()
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO file_versions
               (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
                complete_l1,complete_l2,complete_l3)
             VALUES (5,'src/lib.rs','blake3:a',1,'rust',10,1,1,2,3);
             INSERT INTO views
               (view_id,root,current_generation,resolution_state,resolution_base_id,
                resolution_delta_generation,resolution_exact_at,created_at,updated_at)
             VALUES ('view-a','/repo',2,'unbound',NULL,NULL,NULL,
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES
               ('view-a',1,'sha256:m1','request-predecessor','2025-12-31T00:00:00Z'),
               ('view-a',2,'sha256:m2','request-a','2026-01-01T00:00:00Z');
             INSERT INTO manifest_entries
               (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES
               ('view-a',1,'src/lib.rs','rust',5,'indexed','blake3:a',
                '2025-12-31T00:00:00Z'),
               ('view-a',2,'src/lib.rs','rust',5,'indexed','blake3:a',
                '2026-01-01T00:00:00Z');
             INSERT INTO store_log
               (sequence,request_id,event_kind,view_id,generation,terminal,payload_json,created_at)
             VALUES (7,'request-a','store_import_completed','view-a',2,1,'{}',
                     '2026-01-01T00:00:00Z');
             COMMIT;",
        )
        .unwrap();
}

fn inspect_plan(layout: &StoreLayout) -> julie_extract_artifact::store::MaintenancePlan {
    MaintenanceInspector::new(factory(layout), FixedClock, FixedCapacity)
        .inspect()
        .unwrap()
}

fn factory(layout: &StoreLayout) -> StoreConnectionFactory {
    StoreConnectionFactory::new(layout.clone(), "family-generation-crash", "2.30.0")
}

fn run_worker(root: &Path, boundary: &str) -> Output {
    run_worker_with_lease(root, boundary, Duration::from_millis(100))
}

fn run_worker_with_lease(root: &Path, boundary: &str, lease_duration: Duration) -> Output {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "generation_promotion_crash_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("JULIE_TEST_GENERATION_ROOT", root)
        .env("JULIE_EXTRACT_STORE_TEST_CRASH_AT", boundary)
        .env(
            "JULIE_TEST_GENERATION_LEASE_MS",
            lease_duration.as_millis().to_string(),
        )
        .output()
        .unwrap()
}

fn run_rollback_worker(root: &Path, boundary: &str) -> Output {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "generation_promotion_crash_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("JULIE_TEST_GENERATION_ROOT", root)
        .env("JULIE_EXTRACT_STORE_TEST_CRASH_AT", boundary)
        .env("JULIE_TEST_GENERATION_ROLLBACK", "1")
        .env("JULIE_TEST_GENERATION_LEASE_MS", "100")
        .output()
        .unwrap()
}

fn assert_valid(connection: &Connection) {
    assert_eq!(
        connection
            .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(count_query(connection, "PRAGMA foreign_key_check"), 0);
}

fn count(connection: &Connection, table: &str) -> i64 {
    count_query(connection, &format!("SELECT * FROM {table}"))
}

fn count_query(connection: &Connection, sql: &str) -> i64 {
    let mut statement = connection.prepare(sql).unwrap();
    let mut rows = statement.query([]).unwrap();
    let mut count = 0;
    while rows.next().unwrap().is_some() {
        count += 1;
    }
    count
}

#[derive(Clone, Copy)]
struct FixedClock;

impl MaintenanceClock for FixedClock {
    fn now_ms(&self) -> i64 {
        1_000
    }
}

#[derive(Clone, Copy)]
struct FixedCapacity;

impl CapacityProvider for FixedCapacity {
    fn free_bytes(&self, _path: &Path) -> Result<u64, std::io::Error> {
        Ok(512 * 1024 * 1024)
    }

    fn staged_generation_bytes(&self, _path: &Path) -> Result<u64, std::io::Error> {
        Ok(64 * 1024 * 1024)
    }
}

struct TempStore {
    path: PathBuf,
}

impl TempStore {
    fn new(name: &str) -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "julie-store-generation-crash-{name}-{}-{id}",
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
