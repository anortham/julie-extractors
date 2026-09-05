use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use julie_extract_artifact::store::{
    MaintenanceExecutor, MaintenanceRun, READER_MIN_WRITER_VERSION, ReaderAcquireRequest,
    ReaderManifestSnapshot, ReaderReleaseRequest, ReaderRenewRequest, StoreConnectionError,
    StoreConnectionFactory, StoreLayout, create_coordinator_schema, create_store_schema,
};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

#[test]
fn schema_rejects_invalid_reader_identity() {
    let coordinator = Connection::open_in_memory().unwrap();
    create_coordinator_schema(&coordinator).unwrap();

    insert_registration(&coordinator, RegistrationFields::valid()).unwrap();

    for invalid in [
        RegistrationFields {
            pin_id: "",
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            pin_id: &"p".repeat(129),
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            owner_nonce: &"n".repeat(31),
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            owner_nonce: &"n".repeat(513),
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            owner_label: "",
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            owner_label: &"o".repeat(129),
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            family_id: "",
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            family_id: &"f".repeat(129),
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            view_id: "",
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            view_id: &"v".repeat(129),
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            manifest_generation: 0,
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            generation_name: "",
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            generation_name: &"g".repeat(129),
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            owner_pid: 0,
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            owner_birth_identity: "",
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            owner_birth_identity: &"b".repeat(513),
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            store_instance_id: "",
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            store_instance_id: &"s".repeat(513),
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            manifest_hash: "",
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            manifest_hash: &"h".repeat(513),
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            extraction_identity_epoch: 0,
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            served_store_log_sequence: -1,
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            acquired_at: -1,
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            heartbeat_at: 99,
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            expires_at: 100,
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            min_retained_store_log_sequence: 801,
            ..RegistrationFields::valid()
        },
        RegistrationFields {
            snapshot_fingerprint: "",
            ..RegistrationFields::valid()
        },
    ] {
        let isolated = Connection::open_in_memory().unwrap();
        create_coordinator_schema(&isolated).unwrap();
        assert!(insert_registration(&isolated, invalid).is_err());
    }

    let mut duplicate_nonce = RegistrationFields::valid();
    duplicate_nonce.pin_id = "reader-2";
    assert!(insert_registration(&coordinator, duplicate_nonce).is_err());
}

#[test]
fn coordinator_schema_is_exact_idempotent_and_keeps_reader_identity_immutable() {
    let coordinator = Connection::open_in_memory().unwrap();
    create_coordinator_schema(&coordinator).unwrap();
    create_coordinator_schema(&coordinator).unwrap();

    let columns = coordinator
        .prepare("SELECT name FROM pragma_table_info('reader_registrations') ORDER BY cid")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        columns,
        [
            "pin_id",
            "owner_nonce",
            "owner_label",
            "family_id",
            "view_id",
            "manifest_generation",
            "generation_name",
            "owner_pid",
            "owner_birth_identity",
            "store_instance_id",
            "manifest_hash",
            "extraction_identity_epoch",
            "served_store_log_sequence",
            "acquired_at",
            "heartbeat_at",
            "expires_at",
            "min_retained_store_log_sequence",
            "snapshot_fingerprint",
        ]
    );
    let indexes = coordinator
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type='index' AND tbl_name='reader_registrations' AND sql IS NOT NULL
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        indexes,
        [
            "idx_read_reader_registrations_expiry",
            "idx_read_reader_registrations_generation",
        ]
    );

    insert_registration(&coordinator, RegistrationFields::valid()).unwrap();
    for column in [
        "pin_id",
        "owner_nonce",
        "owner_label",
        "family_id",
        "view_id",
        "manifest_generation",
        "generation_name",
        "owner_pid",
        "owner_birth_identity",
        "store_instance_id",
        "manifest_hash",
        "extraction_identity_epoch",
        "served_store_log_sequence",
        "acquired_at",
        "min_retained_store_log_sequence",
        "snapshot_fingerprint",
    ] {
        assert!(
            coordinator
                .execute(
                    &format!(
                        "UPDATE reader_registrations SET {column}=CASE typeof({column})
                         WHEN 'integer' THEN {column}+1 ELSE {column} || '-changed' END
                         WHERE pin_id='reader-1'"
                    ),
                    [],
                )
                .is_err()
        );
    }
    coordinator
        .execute(
            "UPDATE reader_registrations SET heartbeat_at=150,expires_at=250
             WHERE pin_id='reader-1'",
            [],
        )
        .unwrap();
    assert!(
        coordinator
            .execute(
                "UPDATE reader_registrations SET heartbeat_at=149 WHERE pin_id='reader-1'",
                [],
            )
            .is_err()
    );
}

#[test]
fn store_schema_has_no_reader_registration_objects() {
    let store = Connection::open_in_memory().unwrap();
    create_store_schema(&store).unwrap();

    assert_eq!(
        store
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE '%reader_registration%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn one_registration_roots_each_manifest_size() {
    for entry_count in [1_000_i64, 10_000, 100_000] {
        let store = Connection::open_in_memory().unwrap();
        create_store_schema(&store).unwrap();
        store
            .execute_batch(
                "INSERT INTO views(view_id,root,current_generation,created_at,updated_at)
                 VALUES ('view-a','/repo',NULL,'2026-09-04T00:00:00Z','2026-09-04T00:00:00Z');
                 INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
                 VALUES ('view-a',42,'manifest-hash','request-a','2026-09-04T00:00:00Z');",
            )
            .unwrap();
        store
            .execute(
                "WITH RECURSIVE entries(number) AS (
                   SELECT 1 UNION ALL SELECT number+1 FROM entries WHERE number<?1
                 )
                 INSERT INTO manifest_entries
                   (view_id,generation,path,language,version_id,status,observed_content_hash,
                    indexed_at,error_class,error_json)
                 SELECT 'view-a',42,printf('src/%06d.rs',number),'rust',NULL,'failed','hash',
                        '2026-09-04T00:00:00Z','parse','{}'
                 FROM entries",
                [entry_count],
            )
            .unwrap();

        let coordinator = Connection::open_in_memory().unwrap();
        create_coordinator_schema(&coordinator).unwrap();
        insert_registration(&coordinator, RegistrationFields::valid()).unwrap();

        assert_eq!(
            store
                .query_row("SELECT COUNT(*) FROM manifest_entries", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            entry_count
        );
        assert_eq!(
            coordinator
                .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            1
        );
    }
}

#[test]
fn reader_models_freeze_identity_and_derive_snapshot_facts() {
    assert_eq!(READER_MIN_WRITER_VERSION, "2.40.0");
    let acquire = ReaderAcquireRequest::new(
        "family-a",
        "view-a",
        "gen-000042",
        "miller",
        1234,
        "0123456789abcdef0123456789abcdef",
        30_000,
    );
    assert_eq!(acquire.family_id(), "family-a");
    assert_eq!(acquire.view_id(), "view-a");
    assert_eq!(acquire.generation_name(), "gen-000042");
    assert_eq!(acquire.owner_label(), "miller");
    assert_eq!(acquire.owner_pid(), 1234);
    assert_eq!(acquire.owner_nonce(), "0123456789abcdef0123456789abcdef");
    assert_eq!(acquire.lease_ms(), 30_000);

    let snapshot = ReaderManifestSnapshot::new(
        "family-a",
        "view-a",
        42,
        "gen-000042",
        "manifest-hash",
        9,
        800,
        700,
    );
    assert_eq!(snapshot.store_instance_id(), "family-a:gen-000042");
    assert_eq!(
        snapshot.snapshot_fingerprint(),
        "0fac79b573ab9eafc7a1fdd31198da0c51657c13d894b1d8cedb08387fed8450"
    );

    assert_eq!(snapshot.family_id(), "family-a");
    assert_eq!(snapshot.view_id(), "view-a");
    assert_eq!(snapshot.manifest_generation(), 42);
    assert_eq!(snapshot.manifest_hash(), "manifest-hash");
    assert_eq!(snapshot.extraction_identity_epoch(), 9);
    assert_eq!(snapshot.served_store_log_sequence(), 800);
    assert_eq!(snapshot.min_retained_store_log_sequence(), 700);

    for changed in [
        ReaderManifestSnapshot::new(
            "family-b",
            "view-a",
            42,
            "gen-000042",
            "manifest-hash",
            9,
            800,
            700,
        ),
        ReaderManifestSnapshot::new(
            "family-a",
            "view-b",
            42,
            "gen-000042",
            "manifest-hash",
            9,
            800,
            700,
        ),
        ReaderManifestSnapshot::new(
            "family-a",
            "view-a",
            43,
            "gen-000042",
            "manifest-hash",
            9,
            800,
            700,
        ),
        ReaderManifestSnapshot::new(
            "family-a",
            "view-a",
            42,
            "gen-000043",
            "manifest-hash",
            9,
            800,
            700,
        ),
        ReaderManifestSnapshot::new(
            "family-a",
            "view-a",
            42,
            "gen-000042",
            "manifest-hash-b",
            9,
            800,
            700,
        ),
        ReaderManifestSnapshot::new(
            "family-a",
            "view-a",
            42,
            "gen-000042",
            "manifest-hash",
            10,
            800,
            700,
        ),
        ReaderManifestSnapshot::new(
            "family-a",
            "view-a",
            42,
            "gen-000042",
            "manifest-hash",
            9,
            801,
            700,
        ),
        ReaderManifestSnapshot::new(
            "family-a",
            "view-a",
            42,
            "gen-000042",
            "manifest-hash",
            9,
            800,
            699,
        ),
    ] {
        assert_ne!(
            changed.snapshot_fingerprint(),
            snapshot.snapshot_fingerprint()
        );
    }
    assert_ne!(
        ReaderManifestSnapshot::new("ab", "c", 1, "d", "e", 1, 1, 1).snapshot_fingerprint(),
        ReaderManifestSnapshot::new("a", "bc", 1, "d", "e", 1, 1, 1).snapshot_fingerprint()
    );

    let renew = ReaderRenewRequest::new("family-a", "reader-1", "nonce", 1234, 30_000);
    let release = ReaderReleaseRequest::new("family-a", "reader-1", "nonce");
    assert_eq!(renew.pin_id(), "reader-1");
    assert_eq!(release.pin_id(), "reader-1");
}

#[test]
fn permanent_reader_floor_activation_survives_drop_and_is_idempotent() {
    let temp = TempStore::new("reader-floor");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.39.0", 9).unwrap();
    let store = Connection::open(layout.store_db()).unwrap();
    store
        .execute(
            "INSERT INTO views(view_id,root,created_at,updated_at)
             VALUES ('view-a','/repo','2026-09-04T00:00:00Z','2026-09-04T00:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',42,'manifest-hash','request-a','2026-09-04T00:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "WITH RECURSIVE entries(number) AS (
               SELECT 1 UNION ALL SELECT number+1 FROM entries WHERE number<100000
             )
             INSERT INTO manifest_entries
               (view_id,generation,path,language,version_id,status,observed_content_hash,
                indexed_at,error_class,error_json)
             SELECT 'view-a',42,printf('src/%06d.rs',number),'rust',NULL,'failed','hash',
                    '2026-09-04T00:00:00Z','parse','{}'
             FROM entries",
            [],
        )
        .unwrap();
    drop(store);
    MaintenanceExecutor::activate_reader_writer_floor(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.40.0"),
        MaintenanceRun::new("reader-floor-run", "owner", std::process::id(), 100, 30_000),
    )
    .unwrap();
    MaintenanceExecutor::activate_reader_writer_floor(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.40.0"),
        MaintenanceRun::new(
            "reader-floor-run-2",
            "owner",
            std::process::id(),
            100,
            30_000,
        ),
    )
    .unwrap();

    let store = Connection::open(layout.store_db()).unwrap();
    assert_eq!(metadata(&store, "min_writer_version"), "2.40.0");
    assert_eq!(
        store
            .query_row(
                "SELECT COUNT(*) FROM store_meta WHERE key LIKE 'maintenance_tmp_%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(
        coordinator
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM maintenance_intent) +
                   (SELECT COUNT(*) FROM writer_lease)",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    let error = StoreConnectionFactory::new(layout, "family-a", "2.39.0")
        .open_writer()
        .unwrap_err();
    assert!(matches!(
        error,
        StoreConnectionError::WriterVersionTooOld { running, required }
            if running == "2.39.0" && required == "2.40.0"
    ));
}

#[test]
fn reader_floor_activation_refuses_live_maintenance_and_writer_owners() {
    for blocker in ["maintenance_intent", "writer_lease"] {
        let temp = TempStore::new(blocker);
        let layout = StoreLayout::create(temp.path(), "family-a", "2.39.0", 9).unwrap();
        let coordinator = Connection::open(layout.coordinator_db()).unwrap();
        match blocker {
            "maintenance_intent" => {
                coordinator
                    .execute(
                        "INSERT INTO maintenance_intent
                         (resource,run_id,action,source_generation_name,owner_id,owner_pid,
                          fencing_token,heartbeat_at,expires_at,started_at,plan_fingerprint,
                          source_min_writer_version)
                         VALUES ('store-maintenance','foreign','gc','gen-001','foreign',?1,
                                 1,1,9223372036854775807,1,'foreign-plan','2.39.0')",
                        [std::process::id()],
                    )
                    .unwrap();
            }
            "writer_lease" => {
                coordinator
                    .execute(
                        "INSERT INTO writer_lease
                         (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,
                          fencing_token)
                         VALUES ('store-writer','foreign','2.40.0',?1,1,9223372036854775807,1)",
                        [std::process::id()],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        drop(coordinator);

        let error = MaintenanceExecutor::activate_reader_writer_floor(
            StoreConnectionFactory::new(layout.clone(), "family-a", "2.40.0"),
            MaintenanceRun::new("reader-floor-run", "owner", std::process::id(), 100, 30_000),
        )
        .unwrap_err();
        assert_eq!(error.code(), "maintenance_busy");
        assert_eq!(
            metadata(
                &Connection::open(layout.store_db()).unwrap(),
                "min_writer_version"
            ),
            "2.39.0"
        );
    }
}

#[test]
fn expired_maintenance_source_floor_cannot_restore_below_reader_floor() {
    let temp = TempStore::new("expired-floor");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.39.0", 9).unwrap();
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "INSERT INTO maintenance_intent
             (resource,run_id,action,source_generation_name,owner_id,owner_pid,fencing_token,
              heartbeat_at,expires_at,started_at,plan_fingerprint,source_min_writer_version)
             VALUES ('store-maintenance','expired','gc','gen-001','expired',?1,1,0,1,0,
                     'expired-plan','2.40.0')",
            [std::process::id()],
        )
        .unwrap();

    MaintenanceExecutor::activate_reader_writer_floor(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.40.0"),
        MaintenanceRun::new("reader-floor-run", "owner", std::process::id(), 100, 30_000),
    )
    .unwrap();

    assert_eq!(
        metadata(
            &Connection::open(layout.store_db()).unwrap(),
            "min_writer_version"
        ),
        "2.40.0"
    );
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(
        coordinator
            .query_row("SELECT COUNT(*) FROM maintenance_intent", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn old_runtime_refuses_floor_activation_before_coordinator_mutation() {
    let temp = TempStore::new("old-activation");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.39.0", 9).unwrap();

    let error = MaintenanceExecutor::activate_reader_writer_floor(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.39.0"),
        MaintenanceRun::new("reader-floor-run", "owner", std::process::id(), 100, 30_000),
    )
    .unwrap_err();

    assert_eq!(error.code(), "invalid_maintenance_metadata");
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(
        coordinator
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM maintenance_intent) +
                   (SELECT COUNT(*) FROM writer_lease)",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
#[ignore = "requires JULIE_EXTRACT_2_39_BIN"]
fn v239_maintenance_refuses_reader_registered_family_before_mutation() {
    const FAMILY_ID: &str = "8d3c1258-8d85-47f1-8d86-c8666393b6b7";
    let binary = std::env::var_os("JULIE_EXTRACT_2_39_BIN")
        .expect("JULIE_EXTRACT_2_39_BIN must name julie-extract 2.39.0");
    let version = std::process::Command::new(&binary)
        .arg("--version")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        "julie-extract 2.39.0"
    );

    let temp = TempStore::new("old-writer");
    let layout = StoreLayout::create(temp.path(), FAMILY_ID, "2.39.0", 9).unwrap();
    Connection::open(layout.store_db())
        .unwrap()
        .execute(
            "INSERT INTO views(view_id,root,created_at,updated_at)
             VALUES ('view-a','/repo','2026-09-04T00:00:00Z','2026-09-04T00:00:00Z')",
            [],
        )
        .unwrap();
    MaintenanceExecutor::activate_reader_writer_floor(
        StoreConnectionFactory::new(layout.clone(), FAMILY_ID, "2.40.0"),
        MaintenanceRun::new("reader-floor-run", "owner", std::process::id(), 100, 30_000),
    )
    .unwrap();
    let mut registration = RegistrationFields::valid();
    registration.family_id = FAMILY_ID;
    insert_registration(
        &Connection::open(layout.coordinator_db()).unwrap(),
        registration,
    )
    .unwrap();
    checkpoint(layout.store_db());
    checkpoint(layout.coordinator_db());
    let before = family_state(&layout);
    println!("before={before:?}");

    for arguments in [
        vec!["store", "maintain", "gc", "--apply", "--json"],
        vec!["store", "maintain", "repair", "--apply", "--json"],
        vec!["store", "maintain", "promote", "--apply", "--json"],
        vec![
            "store",
            "maintain",
            "retire-view",
            "--view",
            "view-a",
            "--apply",
            "--json",
        ],
    ] {
        let output = std::process::Command::new(&binary)
            .args(&arguments)
            .arg("--store")
            .arg(temp.path())
            .arg("--family")
            .arg(FAMILY_ID)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3), "{arguments:?}: {output:?}");
        let report = if output.stdout.is_empty() {
            &output.stderr
        } else {
            &output.stdout
        };
        let report: serde_json::Value = serde_json::from_slice(report).unwrap();
        assert_eq!(report["failure_class"], "incompatible_store");
        let after = family_state(&layout);
        println!("{}={after:?}", arguments[2]);
        assert_eq!(after, before);
    }
}

struct RegistrationFields<'a> {
    pin_id: &'a str,
    owner_nonce: &'a str,
    owner_label: &'a str,
    family_id: &'a str,
    view_id: &'a str,
    manifest_generation: i64,
    generation_name: &'a str,
    owner_pid: i64,
    owner_birth_identity: &'a str,
    store_instance_id: &'a str,
    manifest_hash: &'a str,
    extraction_identity_epoch: i64,
    served_store_log_sequence: i64,
    acquired_at: i64,
    heartbeat_at: i64,
    expires_at: i64,
    min_retained_store_log_sequence: i64,
    snapshot_fingerprint: &'a str,
}

impl RegistrationFields<'static> {
    fn valid() -> Self {
        Self {
            pin_id: "reader-1",
            owner_nonce: "0123456789abcdef0123456789abcdef",
            owner_label: "miller",
            family_id: "family-a",
            view_id: "view-a",
            manifest_generation: 42,
            generation_name: "gen-000042",
            owner_pid: 1234,
            owner_birth_identity: "birth-1",
            store_instance_id: "family-a:gen-000042",
            manifest_hash: "manifest-hash",
            extraction_identity_epoch: 9,
            served_store_log_sequence: 800,
            acquired_at: 100,
            heartbeat_at: 100,
            expires_at: 200,
            min_retained_store_log_sequence: 700,
            snapshot_fingerprint: "snapshot-fingerprint",
        }
    }
}

fn insert_registration(
    coordinator: &Connection,
    fields: RegistrationFields<'_>,
) -> rusqlite::Result<usize> {
    coordinator.execute(
        "INSERT INTO reader_registrations
         (pin_id,owner_nonce,owner_label,family_id,view_id,manifest_generation,generation_name,
          owner_pid,owner_birth_identity,store_instance_id,manifest_hash,
          extraction_identity_epoch,served_store_log_sequence,acquired_at,heartbeat_at,expires_at,
          min_retained_store_log_sequence,snapshot_fingerprint)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
        params![
            fields.pin_id,
            fields.owner_nonce,
            fields.owner_label,
            fields.family_id,
            fields.view_id,
            fields.manifest_generation,
            fields.generation_name,
            fields.owner_pid,
            fields.owner_birth_identity,
            fields.store_instance_id,
            fields.manifest_hash,
            fields.extraction_identity_epoch,
            fields.served_store_log_sequence,
            fields.acquired_at,
            fields.heartbeat_at,
            fields.expires_at,
            fields.min_retained_store_log_sequence,
            fields.snapshot_fingerprint,
        ],
    )
}

fn metadata(connection: &Connection, key: &str) -> String {
    connection
        .query_row("SELECT value FROM store_meta WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .unwrap()
}

fn checkpoint(path: &Path) {
    Connection::open(path)
        .unwrap()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
}

#[derive(Debug, PartialEq, Eq)]
struct FamilyState {
    store_sha256: String,
    coordinator_sha256: String,
    current: String,
    min_writer_version: String,
    reader_registrations: i64,
    maintenance_intents: i64,
    writer_leases: i64,
    views: i64,
}

fn family_state(layout: &StoreLayout) -> FamilyState {
    let store = Connection::open(layout.store_db()).unwrap();
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    FamilyState {
        store_sha256: file_sha256(layout.store_db()),
        coordinator_sha256: file_sha256(layout.coordinator_db()),
        current: std::fs::read_to_string(layout.root().join("CURRENT")).unwrap(),
        min_writer_version: metadata(&store, "min_writer_version"),
        reader_registrations: coordinator
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get(0)
            })
            .unwrap(),
        maintenance_intents: coordinator
            .query_row("SELECT COUNT(*) FROM maintenance_intent", [], |row| {
                row.get(0)
            })
            .unwrap(),
        writer_leases: coordinator
            .query_row("SELECT COUNT(*) FROM writer_lease", [], |row| row.get(0))
            .unwrap(),
        views: store
            .query_row("SELECT COUNT(*) FROM views", [], |row| row.get(0))
            .unwrap(),
    }
}

fn file_sha256(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

static NEXT_TEMP_STORE: AtomicU64 = AtomicU64::new(1);

struct TempStore {
    path: std::path::PathBuf,
}

impl TempStore {
    fn new(label: &str) -> Self {
        let sequence = NEXT_TEMP_STORE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "julie-reader-contract-{label}-{}-{sequence}",
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
