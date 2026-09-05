use std::path::Path;
use std::process::{Command, Output};

use julie_extract_artifact::store::{
    CapacityProvider, MaintenanceClock, MaintenanceExecutor, MaintenanceInspector,
    MaintenanceLevel, MaintenanceRootKind, MaintenanceRun, StoreConnectionFactory, StoreLayout,
};
use rusqlite::{Connection, params};
use serde_json::Value;

const FAMILY_ID: &str = "family-reader-mixed-version";
const CURRENT_WRITER_VERSION: &str = env!("CARGO_PKG_VERSION");
const NEWER_FACTORY_VERSION: &str = "2.41.0";
const MAINTENANCE_NOW: i64 = 4_000_000_000_000;

#[test]
fn current_reader_aware_binary_marks_and_retains_registration_roots() {
    let family = ReaderRegisteredFamily::new("current-binary");
    assert_eq!(family.root_state(), RootState::protected());
    let registration_before = family.registration_snapshot();
    let version = julie_extract(&["--version"]);
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        format!("julie-extract {CURRENT_WRITER_VERSION}")
    );

    let output = julie_extract(&[
        "store",
        "maintain",
        "gc",
        "--store",
        family.root().to_str().unwrap(),
        "--apply",
        "--json",
    ]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["readers"]["protected_reader_count"], 1);
    assert_eq!(report["readers"]["definitively_dead_reader_count"], 0);
    assert_eq!(report["readers"]["retained_unknown_reader_count"], 0);
    assert_eq!(report["readers"]["removed_reader_count"], 0);

    assert_eq!(family.root_state(), RootState::protected());
    assert!(family.registration_snapshot() == registration_before);
}

#[test]
fn newer_version_factory_fixture_marks_and_retains_registration_roots() {
    let family = ReaderRegisteredFamily::new("newer-factory");
    assert_eq!(family.root_state(), RootState::protected());
    let registration_before = family.registration_snapshot();
    let factory =
        StoreConnectionFactory::new(family.layout.clone(), FAMILY_ID, NEWER_FACTORY_VERSION);
    let plan = MaintenanceInspector::new(factory.clone(), FixedClock, FixedCapacity)
        .with_window_size(1)
        .inspect()
        .unwrap();

    assert_eq!(plan.protected_readers.len(), 1);
    let reader = &plan.protected_readers[0];
    assert_eq!(reader.pin_id, "reader-mixed-version");
    assert_eq!(reader.view_id, "default");
    assert_eq!(reader.manifest_generation, 1);
    assert_eq!(reader.manifest_hash, "sha256:held");
    assert_eq!(reader.generation_name, "gen-001");
    assert_eq!(reader.min_retained_store_log_sequence, 0);
    let version = plan.version(101).unwrap();
    for level in [
        MaintenanceLevel::L1,
        MaintenanceLevel::L2,
        MaintenanceLevel::L3,
    ] {
        assert!(version.reasons(level).iter().any(|reason| {
            reason.kind == MaintenanceRootKind::ReaderRegistration
                && reason.reference == "reader-mixed-version"
        }));
    }

    let mut executor = MaintenanceExecutor::acquire(
        factory,
        MaintenanceRun::new(
            "newer-fixture-gc",
            "mixed-version-test",
            std::process::id(),
            MAINTENANCE_NOW,
            5_000,
        ),
        &plan,
        FixedCapacity,
    )
    .unwrap();
    executor.apply(&plan).unwrap();

    assert_eq!(family.root_state(), RootState::protected());
    assert!(family.registration_snapshot() == registration_before);
}

fn julie_extract(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args(args)
        .output()
        .unwrap()
}

struct ReaderRegisteredFamily {
    _fixture: tempfile::TempDir,
    layout: StoreLayout,
}

impl ReaderRegisteredFamily {
    fn new(label: &str) -> Self {
        let fixture = tempfile::Builder::new()
            .prefix(&format!("julie-reader-mixed-version-{label}-"))
            .tempdir()
            .unwrap();
        let root = fixture.path().join("store");
        let layout = StoreLayout::create(&root, FAMILY_ID, CURRENT_WRITER_VERSION, 7).unwrap();
        seed_history_and_reader(&layout);
        Self {
            _fixture: fixture,
            layout,
        }
    }

    fn root(&self) -> &Path {
        self.layout.root()
    }

    fn root_state(&self) -> RootState {
        let store = Connection::open(self.layout.store_db()).unwrap();
        let coordinator = Connection::open(self.layout.coordinator_db()).unwrap();
        RootState {
            reader_registrations: count(
                &coordinator,
                "SELECT COUNT(*) FROM reader_registrations WHERE pin_id='reader-mixed-version'",
            ),
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

    fn registration_snapshot(&self) -> RegistrationSnapshot {
        Connection::open(self.layout.coordinator_db())
            .unwrap()
            .query_row(
                "SELECT pin_id,owner_nonce,view_id,manifest_generation,generation_name,
                        owner_pid,owner_birth_identity,heartbeat_at,expires_at,
                        store_instance_id,manifest_hash,extraction_identity_epoch,
                        served_store_log_sequence,min_retained_store_log_sequence,
                        snapshot_fingerprint
                 FROM reader_registrations ORDER BY pin_id",
                [],
                |row| {
                    Ok(RegistrationSnapshot {
                        pin_id: row.get(0)?,
                        owner_nonce: row.get(1)?,
                        view_id: row.get(2)?,
                        manifest_generation: row.get(3)?,
                        generation_name: row.get(4)?,
                        owner_pid: row.get(5)?,
                        owner_birth_identity: row.get(6)?,
                        heartbeat_at: row.get(7)?,
                        expires_at: row.get(8)?,
                        store_instance_id: row.get(9)?,
                        manifest_hash: row.get(10)?,
                        extraction_identity_epoch: row.get(11)?,
                        served_store_log_sequence: row.get(12)?,
                        min_retained_store_log_sequence: row.get(13)?,
                        snapshot_fingerprint: row.get(14)?,
                    })
                },
            )
            .unwrap()
    }
}

#[derive(PartialEq, Eq)]
struct RegistrationSnapshot {
    pin_id: String,
    owner_nonce: String,
    view_id: String,
    manifest_generation: i64,
    generation_name: String,
    owner_pid: i64,
    owner_birth_identity: String,
    heartbeat_at: i64,
    expires_at: i64,
    store_instance_id: String,
    manifest_hash: String,
    extraction_identity_epoch: i64,
    served_store_log_sequence: i64,
    min_retained_store_log_sequence: i64,
    snapshot_fingerprint: String,
}

#[derive(Debug, PartialEq, Eq)]
struct RootState {
    reader_registrations: i64,
    held_manifests: i64,
    held_entries: i64,
    held_versions: i64,
    current_generation: i64,
}

impl RootState {
    fn protected() -> Self {
        Self {
            reader_registrations: 1,
            held_manifests: 1,
            held_entries: 1,
            held_versions: 1,
            current_generation: 2,
        }
    }
}

fn seed_history_and_reader(layout: &StoreLayout) {
    Connection::open(layout.store_db())
        .unwrap()
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO file_versions
               (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
                complete_l1,complete_l2,complete_l3)
             VALUES
               (101,'src/held.rs','blake3:held',1,'rust',100,2,101,102,103),
               (102,'src/current.rs','blake3:current',1,'rust',100,2,201,202,203);
             INSERT INTO views
               (view_id,root,current_generation,resolution_state,created_at,updated_at)
             VALUES ('default','/repo',NULL,'unbound','2026-09-04T00:00:00Z',
                     '2026-09-04T00:00:00Z');
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES
               ('default',1,'sha256:held','request-held','2026-09-04T00:00:00Z'),
               ('default',2,'sha256:current','request-current','2026-09-04T00:00:01Z');
             INSERT INTO manifest_entries
               (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES
               ('default',1,'src/held.rs','rust',101,'indexed','blake3:held',
                '2026-09-04T00:00:00Z'),
               ('default',2,'src/current.rs','rust',102,'indexed','blake3:current',
                '2026-09-04T00:00:01Z');
             UPDATE views SET current_generation=2 WHERE view_id='default';
             UPDATE store_meta SET value='1' WHERE key='retention_path_cap';
             COMMIT;",
        )
        .unwrap();
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "INSERT INTO reader_registrations
             (pin_id,owner_nonce,owner_label,family_id,view_id,manifest_generation,generation_name,
              owner_pid,owner_birth_identity,store_instance_id,manifest_hash,
              extraction_identity_epoch,served_store_log_sequence,acquired_at,heartbeat_at,
              expires_at,min_retained_store_log_sequence,snapshot_fingerprint)
             VALUES ('reader-mixed-version',?1,'miller',?2,'default',1,'gen-001',?3,?4,?5,
                     'sha256:held',7,0,1,1,?6,0,'snapshot-mixed-version')",
            params![
                "abababababababababababababababab",
                FAMILY_ID,
                std::process::id(),
                "synthetic-redacted-birth-identity",
                format!("{FAMILY_ID}:gen-001"),
                i64::MAX,
            ],
        )
        .unwrap();
}

fn count(connection: &Connection, sql: &str) -> i64 {
    connection.query_row(sql, [], |row| row.get(0)).unwrap()
}

#[derive(Clone, Copy)]
struct FixedClock;

impl MaintenanceClock for FixedClock {
    fn now_ms(&self) -> i64 {
        MAINTENANCE_NOW
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
