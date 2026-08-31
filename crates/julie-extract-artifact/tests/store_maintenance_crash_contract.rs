#![cfg(feature = "test-store-crash")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use julie_extract_artifact::store::{
    CapacityProvider, MaintenanceApplyPolicy, MaintenanceClock, MaintenanceExecutor,
    MaintenanceInspector, MaintenanceRun, StoreConnectionFactory, StoreLayout,
};
use rusqlite::Connection;

const DAY_MS: i64 = 86_400_000;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn store_demotions_are_atomic_on_both_sides_of_the_commit() {
    for (boundary, complete_l3) in [
        ("maintenance_store_before_commit", true),
        ("maintenance_store_after_commit", false),
    ] {
        let temp = TempStore::new("store");
        let output = run_worker(temp.path(), "maintenance_store_crash_worker", boundary);
        assert!(
            !output.status.success(),
            "boundary={boundary}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let layout = StoreLayout::open(temp.path()).unwrap();
        assert_valid(&Connection::open(layout.store_db()).unwrap());
        assert_valid(&Connection::open(layout.coordinator_db()).unwrap());
        let state = Connection::open(layout.store_db())
            .unwrap()
            .query_row(
                "SELECT complete_l3 IS NOT NULL,
                        (SELECT COUNT(*) FROM structural_facts WHERE version_id=1)
                 FROM file_versions WHERE version_id=1",
                [],
                |row| Ok((row.get::<_, bool>(0)?, row.get::<_, i64>(1)?)),
            )
            .unwrap();
        assert_eq!(
            state,
            (complete_l3, i64::from(complete_l3)),
            "boundary={boundary}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn coordinator_receipt_survives_death_before_store_log_pruning_and_retry_finishes() {
    let temp = TempStore::new("coordinator");
    let output = run_worker(
        temp.path(),
        "maintenance_coordinator_crash_worker",
        "maintenance_after_coordinator_archive",
    );
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let layout = StoreLayout::open(temp.path()).unwrap();
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    let store = Connection::open(layout.store_db()).unwrap();
    assert_valid(&coord);
    assert_valid(&store);
    assert_eq!(count(&coord, "requests"), 0);
    assert_eq!(count(&coord, "request_receipts"), 1);
    assert_eq!(count(&store, "store_log"), 1);

    thread::sleep(Duration::from_millis(300));
    let plan = inspect_plan(&layout);
    let mut executor = MaintenanceExecutor::acquire(
        factory(&layout),
        MaintenanceRun::new(
            "gc-retry",
            "retry-owner",
            std::process::id(),
            30 * DAY_MS + 1,
            5_000,
        ),
        &plan,
        FixedCapacity,
    )
    .unwrap();
    let report = executor
        .apply_with_policy(
            &plan,
            &MaintenanceApplyPolicy {
                request_safety_ms: 0,
                ..MaintenanceApplyPolicy::default()
            },
        )
        .unwrap();
    assert_eq!(report.pruned_log_rows, 1);
    assert_eq!(
        count(&Connection::open(layout.store_db()).unwrap(), "store_log"),
        0
    );
    assert_eq!(
        count(
            &Connection::open(layout.coordinator_db()).unwrap(),
            "request_receipts"
        ),
        1
    );
}

#[test]
fn leftover_resolution_files_are_gone_after_writer_open() {
    let temp = TempStore::new("reap");
    let layout = StoreLayout::create(temp.path(), "family-maintenance-crash", "2.30.0", 7).unwrap();
    fs::write(layout.bases_dir().join("base-orphan.db"), b"base-bytes").unwrap();
    fs::write(
        layout.scratch_dir().join("resolve-exact-failed-request.db"),
        b"scratch",
    )
    .unwrap();
    drop(factory(&layout).open_writer().unwrap());
    assert!(!layout.bases_dir().join("base-orphan.db").exists());
    assert!(
        !layout
            .scratch_dir()
            .join("resolve-exact-failed-request.db")
            .exists()
    );
}

#[test]
fn maintenance_store_crash_worker() {
    let Some((root, boundary)) = worker_context() else {
        return;
    };
    let layout = StoreLayout::create(root, "family-maintenance-crash", "2.30.0", 7).unwrap();
    seed_l3_candidate(&layout);
    run_gc(&layout, "gc-store-crash", 5_000);
    panic!("worker passed crash boundary {boundary}");
}

#[test]
fn maintenance_coordinator_crash_worker() {
    let Some((root, boundary)) = worker_context() else {
        return;
    };
    let layout = StoreLayout::create(root, "family-maintenance-crash", "2.30.0", 7).unwrap();
    seed_terminal_request(&layout);
    run_gc(&layout, "gc-coordinator-crash", 250);
    panic!("worker passed crash boundary {boundary}");
}

fn run_gc(layout: &StoreLayout, run_id: &str, lease_duration_ms: i64) {
    let plan = inspect_plan(layout);
    let mut executor = MaintenanceExecutor::acquire(
        factory(layout),
        MaintenanceRun::new(
            run_id,
            "crash-owner",
            std::process::id(),
            30 * DAY_MS,
            lease_duration_ms,
        ),
        &plan,
        FixedCapacity,
    )
    .unwrap();
    executor
        .apply_with_policy(
            &plan,
            &MaintenanceApplyPolicy {
                request_safety_ms: 0,
                ..MaintenanceApplyPolicy::default()
            },
        )
        .unwrap();
}

fn seed_l3_candidate(layout: &StoreLayout) {
    let store = Connection::open(layout.store_db()).unwrap();
    store
        .execute_batch(
            "PRAGMA foreign_keys=ON;
             BEGIN IMMEDIATE;
             INSERT INTO file_versions
             (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
              complete_l1,complete_l2,complete_l3)
             VALUES (1,'src/lib.rs','blake3:a',1,'rust',1,1,1,2,3);
             INSERT INTO structural_facts
             (version_id,structural_fact_id,path,language,pattern_id,capture_name,node_kind,
              start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (1,'fact','src/lib.rs','rust','test.fact','node','node',1,0,1,1,0,1,1.0);
             INSERT INTO views(view_id,root,current_generation,created_at,updated_at)
             VALUES ('view-scope','/repo',2,'2026-01-01T00:00:00Z','2026-01-02T00:00:00Z');
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES
               ('view-scope',1,'sha256:m1','request-1','2026-01-01T00:00:00Z'),
               ('view-scope',2,'sha256:m2','request-2','2026-01-02T00:00:00Z');
             COMMIT;",
        )
        .unwrap();
}

fn seed_terminal_request(layout: &StoreLayout) {
    Connection::open(layout.store_db())
        .unwrap()
        .execute(
            "INSERT INTO store_log
             (sequence,request_id,event_kind,terminal,payload_json,created_at)
             VALUES (1,'request-old','store_import_completed',1,'{}','1970-01-01T00:00:01Z')",
            [],
        )
        .unwrap();
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "INSERT INTO requests
             (request_id,idempotency_key,kind,payload_json,state,requester_id,requester_deadline,
              claim_owner,claim_heartbeat_at,terminal_log_sequence,result_json,error_json,
              created_at,updated_at)
             VALUES ('request-old','idem-old','import','{}','committed','cli',NULL,NULL,NULL,
                     1,'{}',NULL,1,1)",
            [],
        )
        .unwrap();
}

fn inspect_plan(layout: &StoreLayout) -> julie_extract_artifact::store::MaintenancePlan {
    MaintenanceInspector::new(factory(layout), FixedClock, FixedCapacity)
        .inspect()
        .unwrap()
}

fn factory(layout: &StoreLayout) -> StoreConnectionFactory {
    StoreConnectionFactory::new(layout.clone(), "family-maintenance-crash", "2.30.0")
}

fn worker_context() -> Option<(PathBuf, String)> {
    Some((
        std::env::var_os("JULIE_TEST_MAINTENANCE_ROOT")?.into(),
        std::env::var("JULIE_EXTRACT_STORE_TEST_CRASH_AT").ok()?,
    ))
}

fn run_worker(root: &Path, test: &str, boundary: &str) -> Output {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env("JULIE_TEST_MAINTENANCE_ROOT", root)
        .env("JULIE_EXTRACT_STORE_TEST_CRASH_AT", boundary)
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
        30 * DAY_MS
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

struct TempStore(PathBuf);

impl TempStore {
    fn new(name: &str) -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "julie-maintenance-crash-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
