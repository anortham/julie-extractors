#![cfg(any(target_os = "linux", windows))]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use julie_extract_artifact::store::{
    CapacityProvider, CoordinatorError, MaintenanceClock, MaintenanceExecutor,
    MaintenanceInspector, MaintenanceLevel, MaintenanceRootKind, MaintenanceRun,
    ReaderAcquireRequest, ReaderRegistration, ReaderRenewRequest, StoreConnectionError,
    StoreConnectionFactory, StoreCoordinator, StoreLayout,
};
use rusqlite::Connection;

const FAMILY_ID: &str = "family-reader-renew-gc";
const WRITER_VERSION: &str = "2.40.0";
const OWNER_NONCE: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

#[test]
fn renewal_commits_before_gc_and_the_renewed_reader_roots_survive() {
    let family = ReaderGcFamily::new("renew-first");
    let before = family.root_state();
    let renewed = StoreCoordinator::open(&family.layout)
        .unwrap()
        .renew_reader(&ReaderRenewRequest::new(
            FAMILY_ID,
            family.reader.identity().pin_id(),
            OWNER_NONCE,
            std::process::id(),
            60_000,
        ))
        .unwrap();
    assert!(renewed.expires_at() > family.reader.expires_at());
    assert!(renewed.snapshot() == family.reader.snapshot());
    assert!(family.authenticated_reader() == renewed);

    let plan = family.plan();
    assert_reader_roots(&plan, family.reader.identity().pin_id());
    family.apply("renew-first-gc", &plan);

    assert_eq!(family.root_state(), before);
    assert!(family.authenticated_reader() == renewed);
}

#[test]
fn gc_fence_refuses_renewal_and_keeps_the_live_reader_roots() {
    let family = ReaderGcFamily::new("gc-first");
    let before = family.root_state();
    let registration_before = family.authenticated_reader();
    let plan = family.plan();
    assert_reader_roots(&plan, family.reader.identity().pin_id());
    let mut executor = MaintenanceExecutor::acquire(
        family.factory(),
        MaintenanceRun::new(
            "gc-first",
            "reader-renew-gc-test",
            std::process::id(),
            family.maintenance_now,
            5_000,
        ),
        &plan,
        FixedCapacity,
    )
    .unwrap();

    let renewal = StoreCoordinator::open(&family.layout)
        .unwrap()
        .renew_reader(&ReaderRenewRequest::new(
            FAMILY_ID,
            family.reader.identity().pin_id(),
            OWNER_NONCE,
            std::process::id(),
            60_000,
        ));
    assert!(matches!(
        renewal,
        Err(CoordinatorError::StoreConnection(
            StoreConnectionError::MaintenanceInProgress {
            run_id
        })) if run_id == "gc-first"
    ));
    assert!(family.authenticated_reader() == registration_before);

    executor.apply(&plan).unwrap();

    assert_eq!(family.root_state(), before);
    assert!(family.authenticated_reader() == registration_before);
}

#[test]
fn renewal_after_planning_makes_the_old_plan_stale_without_root_changes() {
    let family = ReaderGcFamily::new("renew-after-plan");
    let before = family.root_state();
    let plan = family.plan();
    assert_reader_roots(&plan, family.reader.identity().pin_id());
    let renewed = StoreCoordinator::open(&family.layout)
        .unwrap()
        .renew_reader(&ReaderRenewRequest::new(
            FAMILY_ID,
            family.reader.identity().pin_id(),
            OWNER_NONCE,
            std::process::id(),
            60_000,
        ))
        .unwrap();
    assert!(renewed.expires_at() > family.reader.expires_at());
    assert!(family.authenticated_reader() == renewed);

    let result = MaintenanceExecutor::acquire(
        family.factory(),
        MaintenanceRun::new(
            "renew-after-plan-gc",
            "reader-renew-gc-test",
            std::process::id(),
            family.maintenance_now,
            5_000,
        ),
        &plan,
        FixedCapacity,
    );
    let error = match result {
        Ok(_) => panic!("renewed registration admitted a stale maintenance plan"),
        Err(error) => error,
    };
    assert_eq!(error.code(), "maintenance_plan_stale");
    assert_eq!(family.root_state(), before);
    assert!(family.authenticated_reader() == renewed);
}

fn assert_reader_roots(
    plan: &julie_extract_artifact::store::MaintenancePlan,
    expected_reader_id: &str,
) {
    assert_eq!(plan.protected_readers.len(), 1);
    assert_eq!(plan.protected_readers[0].pin_id, expected_reader_id);
    let version = plan.version(101).unwrap();
    for level in [
        MaintenanceLevel::L1,
        MaintenanceLevel::L2,
        MaintenanceLevel::L3,
    ] {
        assert!(version.reasons(level).iter().any(|reason| {
            reason.kind == MaintenanceRootKind::ReaderRegistration
                && reason.reference == expected_reader_id
        }));
    }
}

struct ReaderGcFamily {
    _temp: TempStore,
    layout: StoreLayout,
    reader: ReaderRegistration,
    maintenance_now: i64,
}

impl ReaderGcFamily {
    fn new(label: &str) -> Self {
        let temp = TempStore::new(label);
        let layout = StoreLayout::create(temp.path(), FAMILY_ID, WRITER_VERSION, 7).unwrap();
        seed_held_manifest(&layout);
        let reader = StoreCoordinator::open(&layout)
            .unwrap()
            .acquire_reader(&ReaderAcquireRequest::new(
                FAMILY_ID,
                "default",
                layout.generation_name(),
                "miller",
                std::process::id(),
                OWNER_NONCE,
                30_000,
            ))
            .unwrap()
            .registration()
            .clone();
        assert_eq!(reader.snapshot().manifest_generation(), 1);
        assert_eq!(reader.snapshot().manifest_hash(), "sha256:held");
        assert_eq!(reader.snapshot().served_store_log_sequence(), 800);
        assert_eq!(reader.snapshot().min_retained_store_log_sequence(), 800);
        let maintenance_now = reader.heartbeat_at();
        publish_current_manifest(&layout);
        Self {
            _temp: temp,
            layout,
            reader,
            maintenance_now,
        }
    }

    fn factory(&self) -> StoreConnectionFactory {
        StoreConnectionFactory::new(self.layout.clone(), FAMILY_ID, WRITER_VERSION)
    }

    fn plan(&self) -> julie_extract_artifact::store::MaintenancePlan {
        MaintenanceInspector::new(
            self.factory(),
            FixedClock(self.maintenance_now),
            FixedCapacity,
        )
        .with_window_size(1)
        .inspect()
        .unwrap()
    }

    fn apply(&self, run_id: &str, plan: &julie_extract_artifact::store::MaintenancePlan) {
        let mut executor = MaintenanceExecutor::acquire(
            self.factory(),
            MaintenanceRun::new(
                run_id,
                "reader-renew-gc-test",
                std::process::id(),
                self.maintenance_now,
                5_000,
            ),
            plan,
            FixedCapacity,
        )
        .unwrap();
        executor.apply(plan).unwrap();
    }

    fn authenticated_reader(&self) -> ReaderRegistration {
        StoreCoordinator::open(&self.layout)
            .unwrap()
            .reader_registration(&julie_extract_artifact::store::ReaderReleaseRequest::new(
                FAMILY_ID,
                self.reader.identity().pin_id(),
                OWNER_NONCE,
            ))
            .unwrap()
            .unwrap()
    }

    fn root_state(&self) -> RootState {
        let store = Connection::open(self.layout.store_db()).unwrap();
        let coordinator = Connection::open(self.layout.coordinator_db()).unwrap();
        RootState {
            reader_registrations: count(&coordinator, "SELECT COUNT(*) FROM reader_registrations"),
            held_manifests: count(
                &store,
                "SELECT COUNT(*) FROM manifests WHERE view_id='default' AND generation=1 AND manifest_hash='sha256:held'",
            ),
            held_entries: count(
                &store,
                "SELECT COUNT(*) FROM manifest_entries WHERE view_id='default' AND generation=1 AND version_id=101",
            ),
            held_versions: count(
                &store,
                "SELECT COUNT(*) FROM file_versions WHERE version_id=101 AND complete_l1=101 AND complete_l2=102 AND complete_l3=103",
            ),
            current_generation: store
                .query_row(
                    "SELECT current_generation FROM views WHERE view_id='default'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct RootState {
    reader_registrations: i64,
    held_manifests: i64,
    held_entries: i64,
    held_versions: i64,
    current_generation: i64,
}

fn seed_held_manifest(layout: &StoreLayout) {
    Connection::open(layout.store_db())
        .unwrap()
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO file_versions
               (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
                complete_l1,complete_l2,complete_l3)
             VALUES (101,'src/held.rs','blake3:held',1,'rust',100,2,101,102,103);
             INSERT INTO views
               (view_id,root,current_generation,resolution_state,created_at,updated_at)
             VALUES ('default','/repo',1,'unbound','2026-09-04T00:00:00Z',
                     '2026-09-04T00:00:00Z');
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('default',1,'sha256:held','request-held','2026-09-04T00:00:00Z');
             INSERT INTO manifest_entries
               (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('default',1,'src/held.rs','rust',101,'indexed','blake3:held',
                     '2026-09-04T00:00:00Z');
             INSERT INTO store_log
               (sequence,request_id,event_kind,view_id,generation,payload_json,created_at)
             VALUES (800,'request-held','manifest_flipped','default',1,'{}',
                     '2026-09-04T00:00:00Z');
             COMMIT;",
        )
        .unwrap();
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "INSERT INTO family_allocator_marks(allocator_kind,scope_id,high_water,updated_at)
             VALUES ('store_log','',800,1)",
            [],
        )
        .unwrap();
}

fn publish_current_manifest(layout: &StoreLayout) {
    Connection::open(layout.store_db())
        .unwrap()
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO file_versions
               (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
                complete_l1,complete_l2,complete_l3)
             VALUES (102,'src/current.rs','blake3:current',1,'rust',100,2,201,202,203);
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('default',2,'sha256:current','request-current','2026-09-04T00:00:01Z');
             INSERT INTO manifest_entries
               (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('default',2,'src/current.rs','rust',102,'indexed','blake3:current',
                     '2026-09-04T00:00:01Z');
             UPDATE views SET current_generation=2 WHERE view_id='default';
             UPDATE store_meta SET value='1' WHERE key='retention_path_cap';
             COMMIT;",
        )
        .unwrap();
}

fn count(connection: &Connection, sql: &str) -> i64 {
    connection.query_row(sql, [], |row| row.get(0)).unwrap()
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
        Ok(1)
    }
}

struct TempStore {
    path: PathBuf,
}

impl TempStore {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "julie-reader-renew-gc-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
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
