use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(feature = "test-store-contract")]
use std::sync::{Arc, Barrier};

use julie_extract_artifact::store::{
    CoordinatorError, CoordinatorRequest, LeaseHolder, MaintenanceExecutor, MaintenanceRun,
    ManifestStore, READER_MIN_WRITER_VERSION, ReaderAcquireRequest, ReaderManifestSnapshot,
    ReaderReleaseRequest, ReaderRenewRequest, RequestKind, StoreConnectionError,
    StoreConnectionFactory, StoreCoordinator, StoreLayout, StoreLog, StoreLogEntry,
    StoreSchemaError, create_coordinator_schema, create_store_schema,
};

#[cfg(feature = "test-store-contract")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct AdmissionStatementEvidence {
    label: String,
    vm_steps: i32,
    fullscan_steps: i32,
    query_plan: Vec<String>,
}
#[cfg(feature = "test-store-contract")]
use julie_extract_artifact::store::{
    ProcessIdentityObservation, ProcessIdentityProbe, ProcessIdentityUnknownReason,
    ProcessInstanceIdentity,
};
use rusqlite::{Connection, TransactionBehavior, params};
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
fn authenticated_inspection_refuses_a_malformed_store_instance() {
    let temp = TempStore::new("malformed-store-instance");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.40.0", 9).unwrap();
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    let mut fields = RegistrationFields::valid();
    fields.store_instance_id = "family-a:gen-wrong";
    fields.snapshot_fingerprint = Box::leak(
        ReaderManifestSnapshot::new(
            "family-a",
            "view-a",
            42,
            "gen-000042",
            "manifest-hash",
            9,
            800,
            700,
        )
        .snapshot_fingerprint()
        .to_string()
        .into_boxed_str(),
    );
    insert_registration(&coordinator, fields).unwrap();
    drop(coordinator);

    let request =
        ReaderReleaseRequest::new("family-a", "reader-1", "0123456789abcdef0123456789abcdef");
    assert!(matches!(
        StoreCoordinator::open(&layout)
            .unwrap()
            .reader_registration(&request),
        Err(CoordinatorError::ReaderStaleSnapshot)
    ));
}

#[test]
fn below_floor_is_classified_before_legacy_reader_catalog_lookup() {
    let temp = TempStore::new("legacy-floor-classification");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.39.0", 9).unwrap();
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute_batch(
            "DROP TRIGGER trg_reader_registrations_immutable_identity;
             DROP TABLE reader_registrations;",
        )
        .unwrap();
    let request = ReaderAcquireRequest::new(
        "family-a",
        "view-a",
        "gen-001",
        "miller",
        std::process::id(),
        "0123456789abcdef0123456789abcdef",
        30_000,
    );
    let result = StoreCoordinator::open(&layout)
        .unwrap()
        .acquire_reader(&request);
    assert!(matches!(
        result,
        Err(CoordinatorError::ReaderWriterFloorRequired)
    ));
}

#[cfg(feature = "test-store-contract")]
#[test]
fn acquire_refuses_permanent_writer_floor_above_compiled_version_without_mutation() {
    let temp = TempStore::new("newer-permanent-writer-floor");
    let layout = seeded_admission_store(&temp, 0);
    Connection::open(layout.store_db())
        .unwrap()
        .execute(
            "UPDATE store_meta SET value='999.0.0' WHERE key='min_writer_version'",
            [],
        )
        .unwrap();
    let owner_pid = std::process::id();
    let request = ReaderAcquireRequest::new(
        "family-a",
        "view-a",
        "gen-001",
        "miller",
        owner_pid,
        "0123456789abcdef0123456789abcdef",
        30_000,
    );
    let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(owner_pid, "opaque-birth-newer-floor"),
    ));
    let result = StoreCoordinator::open(&layout)
        .unwrap()
        .acquire_reader_with_probe(&request, &owner);
    let error = match result {
        Ok(_) => panic!("newer permanent writer floor admitted a reader"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        CoordinatorError::WriterVersionTooOld { running, required }
            if running == env!("CARGO_PKG_VERSION") && required == "999.0.0"
    ));
    assert_eq!(
        Connection::open(layout.coordinator_db())
            .unwrap()
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}

#[cfg(feature = "test-store-contract")]
#[test]
fn future_coordinator_schema_refuses_open_before_retired_object_cleanup() {
    let temp = TempStore::new("future-coordinator-schema");
    let layout = seeded_admission_store(&temp, 0);
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    coordinator
        .execute_batch(
            "CREATE INDEX uidx_coord_one_claimed_resolve ON requests(request_id);
             PRAGMA user_version=99;",
        )
        .unwrap();
    drop(coordinator);

    let result = StoreCoordinator::open(&layout);
    assert!(matches!(
        result,
        Err(CoordinatorError::StoreConnection(
            StoreConnectionError::Schema(StoreSchemaError::NewerSchema {
                database: "coord.db",
                found: 99,
                supported: 2,
            })
        ))
    ));
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(
        coordinator
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema
                 WHERE type='index' AND name='uidx_coord_one_claimed_resolve'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        coordinator
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        99
    );
    assert_eq!(
        coordinator
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}

#[cfg(feature = "test-store-contract")]
#[test]
fn future_store_schema_refuses_reader_constructor_before_coordinator_cleanup() {
    let temp = TempStore::new("future-store-schema");
    let layout = seeded_admission_store(&temp, 0);
    Connection::open(layout.store_db())
        .unwrap()
        .execute_batch("PRAGMA user_version=99;")
        .unwrap();
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    coordinator
        .execute_batch("CREATE INDEX uidx_coord_one_claimed_resolve ON requests(request_id);")
        .unwrap();
    drop(coordinator);

    let result = StoreCoordinator::open(&layout);
    assert!(matches!(
        result,
        Err(CoordinatorError::StoreConnection(
            StoreConnectionError::Schema(StoreSchemaError::NewerSchema {
                database: "store.db",
                found: 99,
                supported: 2,
            })
        ))
    ));
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(
        coordinator
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema
                 WHERE type='index' AND name='uidx_coord_one_claimed_resolve'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        coordinator
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}

#[test]
fn reader_enabled_missing_catalog_fails_closed_without_recreation() {
    let temp = TempStore::new("missing-reader-catalog");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.40.0", 9).unwrap();
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute_batch(
            "DROP TRIGGER trg_reader_registrations_immutable_identity;
             DROP TABLE reader_registrations;",
        )
        .unwrap();
    let request = ReaderAcquireRequest::new(
        "family-a",
        "view-a",
        "gen-001",
        "miller",
        std::process::id(),
        "0123456789abcdef0123456789abcdef",
        30_000,
    );
    assert!(matches!(
        StoreCoordinator::open(&layout)
            .unwrap()
            .acquire_reader(&request),
        Err(CoordinatorError::ReaderOperational)
    ));
    assert_eq!(
        Connection::open(layout.coordinator_db())
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='reader_registrations'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[cfg(feature = "test-store-contract")]
#[test]
fn reader_enabled_corrupt_catalog_fails_closed() {
    let temp = TempStore::new("corrupt-reader-catalog");
    let layout = seeded_admission_store(&temp, 0);
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute_batch("DROP TRIGGER trg_reader_registrations_immutable_identity;")
        .unwrap();
    let request = ReaderAcquireRequest::new(
        "family-a",
        "view-a",
        "gen-001",
        "miller",
        std::process::id(),
        "0123456789abcdef0123456789abcdef",
        30_000,
    );
    assert!(matches!(
        StoreCoordinator::open(&layout)
            .unwrap()
            .acquire_reader(&request),
        Err(CoordinatorError::ReaderOperational)
    ));
}

#[cfg(feature = "test-store-contract")]
#[test]
fn acquire_uses_retained_committed_high_water_after_manifest_log_is_pruned() {
    for retain_later_log in [false, true] {
        let temp = TempStore::new(if retain_later_log {
            "pruned-manifest-later-log"
        } else {
            "pruned-manifest-empty-log"
        });
        let (layout, _, expected_served_sequence) =
            seeded_manifest_log_store(&temp, retain_later_log, true);
        let owner_pid = std::process::id();
        let request = ReaderAcquireRequest::new(
            "family-a",
            "view-a",
            "gen-001",
            "miller",
            owner_pid,
            "0123456789abcdef0123456789abcdef",
            30_000,
        );
        let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
            ProcessInstanceIdentity::new(owner_pid, "opaque-birth-pruned-log"),
        ));
        let acquired = StoreCoordinator::open(&layout)
            .unwrap()
            .acquire_reader_with_probe(&request, &owner)
            .unwrap();
        assert_eq!(
            acquired
                .registration()
                .snapshot()
                .served_store_log_sequence(),
            expected_served_sequence
        );
        assert_eq!(
            acquired
                .registration()
                .snapshot()
                .min_retained_store_log_sequence(),
            expected_served_sequence
        );
    }
}

#[cfg(feature = "test-store-contract")]
#[test]
fn acquire_preserves_retained_original_manifest_floor_below_served_high_water() {
    let temp = TempStore::new("retained-manifest-later-log");
    let (layout, original_manifest_sequence, served_sequence) =
        seeded_manifest_log_store(&temp, true, false);
    let owner_pid = std::process::id();
    let request = ReaderAcquireRequest::new(
        "family-a",
        "view-a",
        "gen-001",
        "miller",
        owner_pid,
        "0123456789abcdef0123456789abcdef",
        30_000,
    );
    let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(owner_pid, "opaque-birth-retained-manifest"),
    ));
    let acquired = StoreCoordinator::open(&layout)
        .unwrap()
        .acquire_reader_with_probe(&request, &owner)
        .unwrap();
    assert!(original_manifest_sequence < served_sequence);
    assert_eq!(
        acquired
            .registration()
            .snapshot()
            .served_store_log_sequence(),
        served_sequence
    );
    assert_eq!(
        acquired
            .registration()
            .snapshot()
            .min_retained_store_log_sequence(),
        original_manifest_sequence
    );
}

#[test]
fn acquire_is_idempotent_by_nonce() {
    let temp = TempStore::new("acquire-idempotent");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.40.0", 9).unwrap();
    let store = Connection::open(layout.store_db()).unwrap();
    store
        .execute_batch(
            "INSERT INTO views(view_id,root,current_generation,created_at,updated_at)
             VALUES ('view-a','/repo',NULL,'2026-09-04T00:00:00Z','2026-09-04T00:00:00Z');
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',42,'manifest-hash','request-a','2026-09-04T00:00:00Z');
             UPDATE views SET current_generation=42 WHERE view_id='view-a';
             INSERT INTO store_log
               (sequence,request_id,event_kind,view_id,generation,payload_json,created_at)
             VALUES (700,'request-a','manifest_flipped','view-a',42,'{}','2026-09-04T00:00:00Z');",
        )
        .unwrap();
    drop(store);
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    coordinator
        .execute(
            "INSERT INTO family_allocator_marks(allocator_kind,scope_id,high_water,updated_at)
             VALUES ('store_log','',700,1)",
            [],
        )
        .unwrap();
    drop(coordinator);

    let request = ReaderAcquireRequest::new(
        "family-a",
        "view-a",
        "gen-001",
        "miller",
        std::process::id(),
        "0123456789abcdef0123456789abcdef",
        30_000,
    );
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let first = coordinator.acquire_reader(&request).unwrap();
    assert!(
        first
            .registration()
            .identity()
            .pin_id()
            .starts_with("reader-")
    );
    assert_eq!(first.registration().identity().pin_id().len(), 39);
    assert!(
        first.registration().identity().pin_id()[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    assert!(first.registration().identity().pin_id() != request.owner_nonce());

    let store = Connection::open(layout.store_db()).unwrap();
    store
        .execute_batch(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',43,'new-manifest-hash','request-c','2026-09-04T00:00:02Z');
             UPDATE views SET current_generation=43 WHERE view_id='view-a';",
        )
        .unwrap();
    drop(store);

    let replayed = coordinator.acquire_reader(&request).unwrap();
    assert!(replayed.registration() == first.registration());
    assert_eq!(replayed.registration().snapshot().manifest_generation(), 42);
    assert_eq!(
        replayed.registration().snapshot().manifest_hash(),
        "manifest-hash"
    );
    let renewed = coordinator
        .renew_reader(&ReaderRenewRequest::new(
            "family-a",
            replayed.registration().identity().pin_id(),
            request.owner_nonce(),
            request.owner_pid(),
            60_000,
        ))
        .unwrap();
    assert_eq!(renewed.snapshot(), first.registration().snapshot());
    assert_eq!(
        Connection::open(layout.coordinator_db())
            .unwrap()
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
}

#[cfg(feature = "test-store-contract")]
#[test]
fn renew_release_and_inspection_require_the_registered_owner() {
    let temp = TempStore::new("renew-release");
    let layout = seeded_admission_store(&temp, 0);
    let owner_pid = std::process::id();
    let nonce = "0123456789abcdef0123456789abcdef";
    let request = ReaderAcquireRequest::new(
        "family-a", "view-a", "gen-001", "miller", owner_pid, nonce, 30_000,
    );
    let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(owner_pid, "opaque-birth-a"),
    ));
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let acquired = coordinator
        .acquire_reader_with_probe(&request, &owner)
        .unwrap()
        .into_registration();
    let pin_id = acquired.identity().pin_id().to_string();

    let wrong_nonce =
        ReaderReleaseRequest::new("family-a", &pin_id, "ffffffffffffffffffffffffffffffff");
    assert!(matches!(
        coordinator.reader_registration(&wrong_nonce),
        Err(CoordinatorError::ReaderOwnerMismatch)
    ));
    assert!(matches!(
        coordinator.release_reader(&wrong_nonce),
        Err(CoordinatorError::ReaderOwnerMismatch)
    ));
    assert!(matches!(
        coordinator.renew_reader_with_probe(
            &ReaderRenewRequest::new(
                "family-a",
                &pin_id,
                "ffffffffffffffffffffffffffffffff",
                owner_pid,
                60_000,
            ),
            &owner,
        ),
        Err(CoordinatorError::ReaderOwnerMismatch)
    ));

    let renewed = coordinator
        .renew_reader_with_probe(
            &ReaderRenewRequest::new("family-a", &pin_id, nonce, owner_pid, 60_000),
            &owner,
        )
        .unwrap();
    assert!(renewed.identity() == acquired.identity());
    assert_eq!(renewed.snapshot(), acquired.snapshot());
    assert_eq!(renewed.acquired_at(), acquired.acquired_at());
    assert!(renewed.heartbeat_at() >= acquired.heartbeat_at());
    assert!(renewed.expires_at() > acquired.expires_at());

    let changed_owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(owner_pid, "opaque-birth-b"),
    ));
    assert!(matches!(
        coordinator.renew_reader_with_probe(
            &ReaderRenewRequest::new("family-a", &pin_id, nonce, owner_pid, 60_000),
            &changed_owner,
        ),
        Err(CoordinatorError::ReaderOwnerMismatch)
    ));
    let wrong_pid = owner_pid.checked_add(1).unwrap();
    let wrong_process = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(wrong_pid, "opaque-birth-a"),
    ));
    assert!(matches!(
        coordinator.renew_reader_with_probe(
            &ReaderRenewRequest::new("family-a", &pin_id, nonce, wrong_pid, 60_000),
            &wrong_process,
        ),
        Err(CoordinatorError::ReaderOwnerMismatch)
    ));

    let release = ReaderReleaseRequest::new("family-a", &pin_id, nonce);
    assert!(coordinator.reader_registration(&release).unwrap().is_some());
    assert!(coordinator.release_reader(&release).unwrap());
    assert!(!coordinator.release_reader(&release).unwrap());
    assert!(!coordinator.release_reader(&wrong_nonce).unwrap());
    assert!(coordinator.reader_registration(&release).unwrap().is_none());
}

#[cfg(feature = "test-store-contract")]
#[test]
fn acquire_replay_refuses_identity_and_snapshot_mismatches() {
    let temp = TempStore::new("acquire-mismatch");
    let layout = seeded_admission_store(&temp, 0);
    let owner_pid = std::process::id();
    let nonce = "0123456789abcdef0123456789abcdef";
    let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(owner_pid, "opaque-birth-a"),
    ));
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    coordinator
        .acquire_reader_with_probe(
            &ReaderAcquireRequest::new(
                "family-a", "view-a", "gen-001", "miller", owner_pid, nonce, 30_000,
            ),
            &owner,
        )
        .unwrap();

    assert!(matches!(
        coordinator.acquire_reader_with_probe(
            &ReaderAcquireRequest::new(
                "family-a",
                "view-a",
                "gen-001",
                "different-owner",
                owner_pid,
                nonce,
                30_000,
            ),
            &owner,
        ),
        Err(CoordinatorError::ReaderOwnerMismatch)
    ));
    let changed_birth = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(owner_pid, "opaque-birth-b"),
    ));
    assert!(matches!(
        coordinator.acquire_reader_with_probe(
            &ReaderAcquireRequest::new(
                "family-a", "view-a", "gen-001", "miller", owner_pid, nonce, 30_000,
            ),
            &changed_birth,
        ),
        Err(CoordinatorError::ReaderOwnerMismatch)
    ));
    for request in [
        ReaderAcquireRequest::new(
            "family-b", "view-a", "gen-001", "miller", owner_pid, nonce, 30_000,
        ),
        ReaderAcquireRequest::new(
            "family-a", "view-b", "gen-001", "miller", owner_pid, nonce, 30_000,
        ),
        ReaderAcquireRequest::new(
            "family-a", "view-a", "gen-002", "miller", owner_pid, nonce, 30_000,
        ),
    ] {
        assert!(matches!(
            coordinator.acquire_reader_with_probe(&request, &owner),
            Err(CoordinatorError::ReaderStaleSnapshot)
        ));
    }
    assert_eq!(
        Connection::open(layout.coordinator_db())
            .unwrap()
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
}

#[cfg(feature = "test-store-contract")]
#[test]
fn acquire_captures_process_identity_inside_admission_transaction() {
    let temp = TempStore::new("acquire-probe-order");
    let layout = seeded_admission_store(&temp, 0);
    let observed_transaction = Arc::new(AtomicBool::new(false));
    let probe = TransactionObservingProbe::new(
        layout.coordinator_db(),
        Arc::clone(&observed_transaction),
        "opaque-birth-acquire-order",
    );
    let owner_pid = std::process::id();
    StoreCoordinator::open(&layout)
        .unwrap()
        .acquire_reader_with_probe(
            &ReaderAcquireRequest::new(
                "family-a",
                "view-a",
                "gen-001",
                "miller",
                owner_pid,
                "0123456789abcdef0123456789abcdef",
                30_000,
            ),
            &probe,
        )
        .unwrap();
    assert!(observed_transaction.load(Ordering::SeqCst));
}

#[cfg(feature = "test-store-contract")]
#[test]
fn renew_reprobes_process_identity_inside_admission_transaction() {
    let temp = TempStore::new("renew-probe-order");
    let layout = seeded_admission_store(&temp, 0);
    let owner_pid = std::process::id();
    let nonce = "0123456789abcdef0123456789abcdef";
    let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(owner_pid, "opaque-birth-renew-order"),
    ));
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let acquired = coordinator
        .acquire_reader_with_probe(
            &ReaderAcquireRequest::new(
                "family-a", "view-a", "gen-001", "miller", owner_pid, nonce, 30_000,
            ),
            &owner,
        )
        .unwrap();
    let observed_transaction = Arc::new(AtomicBool::new(false));
    let probe = TransactionObservingProbe::new(
        layout.coordinator_db(),
        Arc::clone(&observed_transaction),
        "opaque-birth-renew-order",
    );
    coordinator
        .renew_reader_with_probe(
            &ReaderRenewRequest::new(
                "family-a",
                acquired.registration().identity().pin_id(),
                nonce,
                owner_pid,
                60_000,
            ),
            &probe,
        )
        .unwrap();
    assert!(observed_transaction.load(Ordering::SeqCst));
}

#[cfg(feature = "test-store-contract")]
#[test]
fn unknown_identity_and_invalid_lease_refuse_before_registration() {
    let temp = TempStore::new("identity-unknown");
    let layout = seeded_admission_store(&temp, 0);
    let owner_pid = std::process::id();
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let request = ReaderAcquireRequest::new(
        "family-a",
        "view-a",
        "gen-001",
        "miller",
        owner_pid,
        "0123456789abcdef0123456789abcdef",
        30_000,
    );
    for observation in [
        ProcessIdentityObservation::Absent,
        ProcessIdentityObservation::Terminated(ProcessInstanceIdentity::new(
            owner_pid,
            "opaque-terminated-birth",
        )),
        ProcessIdentityObservation::Unknown(ProcessIdentityUnknownReason::IdentityDomainUnverified),
    ] {
        assert!(matches!(
            coordinator.acquire_reader_with_probe(&request, &FixedProcessIdentity(observation),),
            Err(CoordinatorError::ReaderIdentityUnknown)
        ));
    }
    let overflowing = ReaderAcquireRequest::new(
        "family-a",
        "view-a",
        "gen-001",
        "miller",
        owner_pid,
        "fedcba9876543210fedcba9876543210",
        u64::MAX,
    );
    assert!(matches!(
        coordinator.acquire_reader_with_probe(
            &overflowing,
            &FixedProcessIdentity(ProcessIdentityObservation::Absent),
        ),
        Err(CoordinatorError::InvalidTime { .. })
    ));
    assert_eq!(
        Connection::open(layout.coordinator_db())
            .unwrap()
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}

#[cfg(feature = "test-store-contract")]
#[test]
fn admission_recovers_committed_log_watermark_without_writer_reconciliation() {
    for initial_mark in [700, 900] {
        let temp = TempStore::new("admission-watermark-recovery");
        let layout = seeded_admission_store(&temp, 0);
        let durable_sequence = append_unreconciled_log(&layout);
        let coord = Connection::open(layout.coordinator_db()).unwrap();
        coord
            .execute(
                "UPDATE family_allocator_marks SET high_water=?1,updated_at=9223372036854775807
             WHERE allocator_kind='store_log' AND scope_id=''",
                [initial_mark],
            )
            .unwrap();
        drop(coord);
        let request = ReaderAcquireRequest::new(
            "family-a",
            "view-a",
            "gen-001",
            "miller",
            std::process::id(),
            "0123456789abcdef0123456789abcdef",
            30_000,
        );
        StoreCoordinator::open(&layout)
            .unwrap()
            .acquire_reader(&request)
            .unwrap();
        let coord = Connection::open(layout.coordinator_db()).unwrap();
        let (mark, updated): (i64, i64) = coord
            .query_row(
                "SELECT high_water,updated_at FROM family_allocator_marks
             WHERE allocator_kind='store_log' AND scope_id=''",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(mark, initial_mark.max(durable_sequence));
        assert_eq!(updated, i64::MAX);
        let (served, retained): (i64, i64) = coord
            .query_row(
                "SELECT served_store_log_sequence,min_retained_store_log_sequence
             FROM reader_registrations",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(served, durable_sequence);
        assert_eq!(retained, 700);
    }
}

#[cfg(feature = "test-store-contract")]
#[test]
fn admission_recovery_does_not_observe_an_expired_writers_uncommitted_log() {
    let temp = TempStore::new("admission-pending-log");
    let layout = seeded_admission_store(&temp, 0);
    let durable_sequence = append_unreconciled_log(&layout);
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "INSERT INTO writer_lease
         (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,fencing_token)
         VALUES ('store-writer','expired','2.40.0',?1,1,2,1)",
            [std::process::id()],
        )
        .unwrap();
    let mut writer = Connection::open(layout.store_db()).unwrap();
    let pending = writer.transaction().unwrap();
    let pending_sequence = StoreLog::append_terminal(
        &pending,
        &StoreLogEntry::new(
            "pending-request",
            "store_import_completed",
            "{}",
            "2026-09-05T00:00:01Z",
        ),
    )
    .unwrap();
    assert!(pending_sequence > durable_sequence);
    let request = ReaderAcquireRequest::new(
        "family-a",
        "view-a",
        "gen-001",
        "miller",
        std::process::id(),
        "0123456789abcdef0123456789abcdef",
        30_000,
    );
    StoreCoordinator::open(&layout)
        .unwrap()
        .acquire_reader(&request)
        .unwrap();
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(coord.query_row(
        "SELECT high_water FROM family_allocator_marks WHERE allocator_kind='store_log' AND scope_id=''",
        [], |row| row.get::<_, i64>(0),
    ).unwrap(), durable_sequence);
    assert_eq!(
        coord
            .query_row(
                "SELECT served_store_log_sequence FROM reader_registrations",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        durable_sequence
    );
    pending.rollback().unwrap();
}

#[cfg(feature = "test-store-contract")]
#[test]
fn admission_recovery_refuses_a_missing_allocator_mark() {
    let temp = TempStore::new("admission-missing-log-mark");
    let layout = seeded_admission_store(&temp, 0);
    append_unreconciled_log(&layout);
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    coord
        .execute(
            "DELETE FROM family_allocator_marks WHERE allocator_kind='store_log' AND scope_id=''",
            [],
        )
        .unwrap();
    let request = ReaderAcquireRequest::new(
        "family-a",
        "view-a",
        "gen-001",
        "miller",
        std::process::id(),
        "0123456789abcdef0123456789abcdef",
        30_000,
    );
    assert!(matches!(
        StoreCoordinator::open(&layout)
            .unwrap()
            .acquire_reader(&request),
        Err(CoordinatorError::ReaderStaleSnapshot)
    ));
    assert_eq!(
        coord
            .query_row("SELECT COUNT(*) FROM family_allocator_marks", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        coord
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        0
    );
}

#[cfg(feature = "test-store-contract")]
fn append_unreconciled_log(layout: &StoreLayout) -> i64 {
    let mut store = Connection::open(layout.store_db()).unwrap();
    let tx = store.transaction().unwrap();
    let sequence = StoreLog::append_terminal(
        &tx,
        &StoreLogEntry::new(
            "interrupted-request",
            "store_import_completed",
            "{}",
            "2026-09-05T00:00:00Z",
        ),
    )
    .unwrap();
    tx.commit().unwrap();
    sequence
}

#[cfg(feature = "test-store-contract")]
#[test]
fn acquire_metadata_work_is_constant_across_manifest_sizes() {
    let mut baseline = None;
    for entry_count in [1_000_i64, 10_000, 100_000] {
        let temp = TempStore::new(&format!("admission-{entry_count}"));
        let layout = seeded_admission_store(&temp, entry_count);
        let owner_pid = std::process::id();
        let request = ReaderAcquireRequest::new(
            "family-a",
            "view-a",
            "gen-001",
            "miller",
            owner_pid,
            format!("{entry_count:032x}"),
            30_000,
        );
        let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
            ProcessInstanceIdentity::new(owner_pid, format!("opaque-birth-{entry_count}")),
        ));
        let mut coordinator = StoreCoordinator::open(&layout).unwrap();
        let mut evidence = Vec::new();
        coordinator
            .acquire_reader_with_probe_and_metrics(
                &request,
                &owner,
                |label, vm_steps, fullscan_steps, query_plan| {
                    evidence.push(AdmissionStatementEvidence {
                        label: label.to_owned(),
                        vm_steps,
                        fullscan_steps,
                        query_plan: query_plan.to_vec(),
                    });
                },
            )
            .unwrap();
        assert!(!evidence.is_empty());
        assert!(
            evidence
                .iter()
                .all(|statement| statement.fullscan_steps == 0)
        );
        assert!(
            evidence
                .iter()
                .flat_map(|statement| &statement.query_plan)
                .all(|detail| {
                    !detail.contains("manifest_entries") && !detail.contains("file_versions")
                })
        );
        assert!(
            evidence
                .iter()
                .flat_map(|statement| &statement.query_plan)
                .any(|detail| detail.contains("sqlite_autoindex_manifests_1"))
        );
        assert!(
            evidence
                .iter()
                .flat_map(|statement| &statement.query_plan)
                .any(|detail| detail.contains("idx_read_store_log_request"))
        );
        if let Some(expected) = &baseline {
            assert_eq!(&evidence, expected);
        } else {
            baseline = Some(evidence.clone());
        }
        eprintln!(
            "reader admission entries={entry_count} data_statements={} vm_steps={} fullscan_steps={}",
            evidence.len(),
            evidence
                .iter()
                .map(|statement| statement.vm_steps)
                .sum::<i32>(),
            evidence
                .iter()
                .map(|statement| statement.fullscan_steps)
                .sum::<i32>()
        );
        for statement in &evidence {
            eprintln!(
                "reader admission statement={} vm_steps={} fullscan_steps={} plan={:?}",
                statement.label, statement.vm_steps, statement.fullscan_steps, statement.query_plan
            );
        }
        let store = Connection::open(layout.store_db()).unwrap();
        assert_eq!(
            store
                .query_row("SELECT COUNT(*) FROM manifest_entries", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            entry_count
        );
        assert_eq!(
            Connection::open(layout.coordinator_db())
                .unwrap()
                .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            1
        );
    }
}

#[cfg(feature = "test-store-contract")]
#[test]
fn coordinator_admission_fence_serializes_maintenance_intent_creation() {
    let temp = TempStore::new("maintenance-race");
    let layout = seeded_admission_store(&temp, 0);
    let entered = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let worker_layout = layout.clone();
    let worker_entered = Arc::clone(&entered);
    let worker_resume = Arc::clone(&resume);
    let worker = std::thread::spawn(move || {
        let owner_pid = std::process::id();
        let request = ReaderAcquireRequest::new(
            "family-a",
            "view-a",
            "gen-001",
            "miller",
            owner_pid,
            "0123456789abcdef0123456789abcdef",
            30_000,
        );
        let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
            ProcessInstanceIdentity::new(owner_pid, "opaque-birth-maintenance"),
        ));
        StoreCoordinator::open(&worker_layout)
            .unwrap()
            .acquire_reader_with_probe_and_barrier(&request, &owner, || {
                worker_entered.wait();
                worker_resume.wait();
            })
    });
    entered.wait();

    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    coordinator.busy_timeout(std::time::Duration::ZERO).unwrap();
    let blocked = coordinator.execute(
        "INSERT INTO maintenance_intent
         (resource,run_id,action,source_generation_name,owner_id,owner_pid,
          fencing_token,heartbeat_at,expires_at,started_at,plan_fingerprint,
          source_min_writer_version)
         VALUES ('store-maintenance','foreign','gc','gen-001','foreign',?1,
                 1,1,9223372036854775807,1,'foreign-plan','2.40.0')",
        [std::process::id()],
    );
    assert!(matches!(
        blocked,
        Err(rusqlite::Error::SqliteFailure(error, _))
            if matches!(error.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)
    ));
    resume.wait();
    worker.join().unwrap().unwrap();

    coordinator
        .execute(
            "INSERT INTO maintenance_intent
             (resource,run_id,action,source_generation_name,owner_id,owner_pid,
              fencing_token,heartbeat_at,expires_at,started_at,plan_fingerprint,
              source_min_writer_version)
             VALUES ('store-maintenance','foreign','gc','gen-001','foreign',?1,
                     1,1,9223372036854775807,1,'foreign-plan','2.40.0')",
            [std::process::id()],
        )
        .unwrap();
    assert_eq!(
        coordinator
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
}

#[cfg(feature = "test-store-contract")]
#[test]
fn admission_refuses_generation_and_view_mutations_without_partial_rows() {
    for mutation in [
        "family-generation",
        "manifest-publication",
        "view-retirement",
    ] {
        let temp = TempStore::new(mutation);
        let layout = seeded_admission_store(&temp, 0);
        append_unreconciled_log(&layout);
        let entered = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        let worker_layout = layout.clone();
        let worker_entered = Arc::clone(&entered);
        let worker_resume = Arc::clone(&resume);
        let worker = std::thread::spawn(move || {
            let owner_pid = std::process::id();
            let request = ReaderAcquireRequest::new(
                "family-a",
                "view-a",
                "gen-001",
                "miller",
                owner_pid,
                "0123456789abcdef0123456789abcdef",
                30_000,
            );
            let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
                ProcessInstanceIdentity::new(owner_pid, "opaque-birth-mutation"),
            ));
            StoreCoordinator::open(&worker_layout)
                .unwrap()
                .acquire_reader_with_probe_and_barrier(&request, &owner, || {
                    worker_entered.wait();
                    worker_resume.wait();
                })
        });
        entered.wait();

        match mutation {
            "family-generation" => {
                std::fs::write(layout.root().join("CURRENT"), "gen-002\n").unwrap();
            }
            "manifest-publication" => {
                Connection::open(layout.store_db())
                    .unwrap()
                    .execute_batch(
                        "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
                         VALUES ('view-a',43,'manifest-new','request-new','2026-09-04T00:00:01Z');
                         UPDATE views SET current_generation=43 WHERE view_id='view-a';",
                    )
                    .unwrap();
            }
            "view-retirement" => {
                Connection::open(layout.store_db())
                    .unwrap()
                    .execute(
                        "UPDATE views SET current_generation=NULL WHERE view_id='view-a'",
                        [],
                    )
                    .unwrap();
            }
            _ => unreachable!(),
        }
        resume.wait();
        match worker.join().unwrap() {
            Err(CoordinatorError::ReaderStaleSnapshot) => {}
            Err(error) => panic!("{mutation} returned {error}"),
            Ok(_) => panic!("{mutation} admission unexpectedly succeeded"),
        }
        assert_eq!(Connection::open(layout.coordinator_db()).unwrap().query_row(
            "SELECT high_water FROM family_allocator_marks WHERE allocator_kind='store_log' AND scope_id=''",
            [], |row| row.get::<_, i64>(0),
        ).unwrap(), 700);
        assert_eq!(
            Connection::open(layout.coordinator_db())
                .unwrap()
                .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
    }
}

#[cfg(feature = "test-store-contract")]
#[test]
fn admission_refuses_live_writer_and_maintenance_owners_without_partial_rows() {
    for blocker in ["writer", "maintenance"] {
        let temp = TempStore::new(blocker);
        let layout = seeded_admission_store(&temp, 0);
        append_unreconciled_log(&layout);
        let coordinator = Connection::open(layout.coordinator_db()).unwrap();
        if blocker == "writer" {
            coordinator
                .execute(
                    "INSERT INTO writer_lease
                     (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,
                      fencing_token)
                     VALUES ('store-writer','foreign','2.40.0',?1,1,9223372036854775807,1)",
                    [std::process::id()],
                )
                .unwrap();
        } else {
            Connection::open(layout.store_db())
                .unwrap()
                .execute(
                    "UPDATE store_meta SET value='999.0.0' WHERE key='min_writer_version'",
                    [],
                )
                .unwrap();
            coordinator
                .execute(
                    "INSERT INTO maintenance_intent
                     (resource,run_id,action,source_generation_name,owner_id,owner_pid,
                      fencing_token,heartbeat_at,expires_at,started_at,plan_fingerprint,
                      source_min_writer_version)
                     VALUES ('store-maintenance','foreign','gc','gen-001','foreign',?1,
                             1,1,9223372036854775807,1,'foreign-plan','2.40.0')",
                    [std::process::id()],
                )
                .unwrap();
        }
        drop(coordinator);
        let owner_pid = std::process::id();
        let request = ReaderAcquireRequest::new(
            "family-a",
            "view-a",
            "gen-001",
            "miller",
            owner_pid,
            "0123456789abcdef0123456789abcdef",
            30_000,
        );
        let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
            ProcessInstanceIdentity::new(owner_pid, "opaque-birth-blocked"),
        ));
        let result = StoreCoordinator::open(&layout)
            .unwrap()
            .acquire_reader_with_probe(&request, &owner);
        let error = match result {
            Ok(_) => panic!("blocked admission unexpectedly succeeded"),
            Err(error) => error,
        };
        if blocker == "writer" {
            assert!(matches!(error, CoordinatorError::ReaderAdmissionBusy));
        } else {
            assert!(matches!(error, CoordinatorError::StoreConnection(_)));
        }
        assert_eq!(Connection::open(layout.coordinator_db()).unwrap().query_row(
            "SELECT high_water FROM family_allocator_marks WHERE allocator_kind='store_log' AND scope_id=''",
            [], |row| row.get::<_, i64>(0),
        ).unwrap(), 700);
        assert_eq!(
            Connection::open(layout.coordinator_db())
                .unwrap()
                .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
    }
}

#[cfg(feature = "test-store-contract")]
#[test]
fn first_time_floor_activation_allows_bounded_admission() {
    let temp = TempStore::new("first-floor-admission");
    let layout = seeded_admission_store_with_version(&temp, 100_000, "2.39.0");
    let owner_pid = std::process::id();
    let request = ReaderAcquireRequest::new(
        "family-a",
        "view-a",
        "gen-001",
        "miller",
        owner_pid,
        "0123456789abcdef0123456789abcdef",
        30_000,
    );
    let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(owner_pid, "opaque-birth-floor"),
    ));
    let before = StoreCoordinator::open(&layout)
        .unwrap()
        .acquire_reader_with_probe(&request, &owner);
    assert!(matches!(
        before,
        Err(CoordinatorError::ReaderWriterFloorRequired)
    ));

    MaintenanceExecutor::activate_reader_writer_floor(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.40.0"),
        MaintenanceRun::new("first-floor-run", "owner", std::process::id(), 100, 30_000),
    )
    .unwrap();
    let mut evidence = Vec::new();
    StoreCoordinator::open(&layout)
        .unwrap()
        .acquire_reader_with_probe_and_metrics(
            &request,
            &owner,
            |label, vm_steps, fullscan_steps, query_plan| {
                evidence.push(AdmissionStatementEvidence {
                    label: label.to_owned(),
                    vm_steps,
                    fullscan_steps,
                    query_plan: query_plan.to_vec(),
                });
            },
        )
        .unwrap();
    assert!(!evidence.is_empty());
    assert!(
        evidence
            .iter()
            .all(|statement| statement.fullscan_steps == 0)
    );
    assert!(
        evidence
            .iter()
            .flat_map(|statement| &statement.query_plan)
            .all(|detail| {
                !detail.contains("manifest_entries") && !detail.contains("file_versions")
            })
    );
    eprintln!(
        "first reader activation admission data_statements={} vm_steps={} fullscan_steps={}",
        evidence.len(),
        evidence
            .iter()
            .map(|statement| statement.vm_steps)
            .sum::<i32>(),
        evidence
            .iter()
            .map(|statement| statement.fullscan_steps)
            .sum::<i32>()
    );
}

#[cfg(feature = "test-store-contract")]
#[test]
fn renew_and_release_refuse_foreign_maintenance_intent() {
    let temp = TempStore::new("mutation-maintenance");
    let layout = seeded_admission_store(&temp, 0);
    let owner_pid = std::process::id();
    let nonce = "0123456789abcdef0123456789abcdef";
    let request = ReaderAcquireRequest::new(
        "family-a", "view-a", "gen-001", "miller", owner_pid, nonce, 30_000,
    );
    let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(owner_pid, "opaque-birth-maintenance-mutation"),
    ));
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let acquired = coordinator
        .acquire_reader_with_probe(&request, &owner)
        .unwrap();
    let pin_id = acquired.registration().identity().pin_id();
    let intent = Connection::open(layout.coordinator_db()).unwrap();
    intent
        .execute(
            "INSERT INTO maintenance_intent
             (resource,run_id,action,source_generation_name,owner_id,owner_pid,
              fencing_token,heartbeat_at,expires_at,started_at,plan_fingerprint,
              source_min_writer_version)
             VALUES ('store-maintenance','foreign','gc','gen-001','foreign',?1,
                     1,1,9223372036854775807,1,'foreign-plan','2.40.0')",
            [owner_pid],
        )
        .unwrap();

    assert!(matches!(
        coordinator.renew_reader_with_probe(
            &ReaderRenewRequest::new("family-a", pin_id, nonce, owner_pid, 60_000),
            &owner,
        ),
        Err(CoordinatorError::StoreConnection(_))
    ));
    assert!(matches!(
        coordinator.release_reader(&ReaderReleaseRequest::new("family-a", pin_id, nonce)),
        Err(CoordinatorError::StoreConnection(_))
    ));
    assert_eq!(
        intent
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
}

#[cfg(feature = "test-store-contract")]
#[test]
fn renew_succeeds_while_ordinary_writer_lease_is_live() {
    let temp = TempStore::new("renew-with-writer");
    let layout = seeded_admission_store(&temp, 0);
    let owner_pid = std::process::id();
    let nonce = "0123456789abcdef0123456789abcdef";
    let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(owner_pid, "opaque-birth-renew-writer"),
    ));
    let mut reader = StoreCoordinator::open(&layout).unwrap();
    let acquired = reader
        .acquire_reader_with_probe(
            &ReaderAcquireRequest::new(
                "family-a", "view-a", "gen-001", "miller", owner_pid, nonce, 30_000,
            ),
            &owner,
        )
        .unwrap();
    let mut writer = StoreCoordinator::open(&layout).unwrap();
    assert!(
        writer
            .try_acquire_or_takeover_now(LeaseHolder::new("writer", "2.40.0", owner_pid))
            .unwrap()
            .acquired()
    );

    let renewed = reader
        .renew_reader_with_probe(
            &ReaderRenewRequest::new(
                "family-a",
                acquired.registration().identity().pin_id(),
                nonce,
                owner_pid,
                60_000,
            ),
            &owner,
        )
        .unwrap();
    assert!(renewed.expires_at() > acquired.registration().expires_at());
}

#[cfg(feature = "test-store-contract")]
#[test]
fn release_succeeds_while_ordinary_writer_lease_is_live() {
    let temp = TempStore::new("release-with-writer");
    let layout = seeded_admission_store(&temp, 0);
    let owner_pid = std::process::id();
    let nonce = "0123456789abcdef0123456789abcdef";
    let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(owner_pid, "opaque-birth-release-writer"),
    ));
    let mut reader = StoreCoordinator::open(&layout).unwrap();
    let acquired = reader
        .acquire_reader_with_probe(
            &ReaderAcquireRequest::new(
                "family-a", "view-a", "gen-001", "miller", owner_pid, nonce, 30_000,
            ),
            &owner,
        )
        .unwrap();
    let mut writer = StoreCoordinator::open(&layout).unwrap();
    assert!(
        writer
            .try_acquire_or_takeover_now(LeaseHolder::new("writer", "2.40.0", owner_pid))
            .unwrap()
            .acquired()
    );

    assert!(
        reader
            .release_reader(&ReaderReleaseRequest::new(
                "family-a",
                acquired.registration().identity().pin_id(),
                nonce,
            ))
            .unwrap()
    );
}

#[cfg(feature = "test-store-contract")]
#[test]
fn reader_validation_matches_sqlite_character_lengths_and_refuses_overflow() {
    let temp = TempStore::new("reader-validation");
    let layout = seeded_admission_store(&temp, 0);
    let owner_pid = std::process::id();
    let owner = FixedProcessIdentity(ProcessIdentityObservation::Alive(
        ProcessInstanceIdentity::new(owner_pid, "opaque-birth-validation"),
    ));
    let nonce = "0123456789abcdef0123456789abcdef";
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let acquired = coordinator
        .acquire_reader_with_probe(
            &ReaderAcquireRequest::new(
                "family-a",
                "view-a",
                "gen-001",
                "é".repeat(128),
                owner_pid,
                nonce,
                30_000,
            ),
            &owner,
        )
        .unwrap();
    let pin_id = acquired.registration().identity().pin_id();
    assert!(matches!(
        coordinator.acquire_reader_with_probe(
            &ReaderAcquireRequest::new(
                "family-a",
                "view-a",
                "gen-001",
                "é".repeat(129),
                owner_pid,
                "fedcba9876543210fedcba9876543210",
                30_000,
            ),
            &owner,
        ),
        Err(CoordinatorError::InvalidRequest)
    ));
    assert!(matches!(
        coordinator.renew_reader_with_probe(
            &ReaderRenewRequest::new("family-a", pin_id, nonce, owner_pid, u64::MAX),
            &owner,
        ),
        Err(CoordinatorError::InvalidTime { .. })
    ));
    assert!(matches!(
        coordinator.release_reader(&ReaderReleaseRequest::new(
            "family-a",
            pin_id,
            "short-nonce"
        )),
        Err(CoordinatorError::InvalidRequest)
    ));
    assert_eq!(
        Connection::open(layout.coordinator_db())
            .unwrap()
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
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

fn exited_floor_requester() -> String {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    assert!(child.wait().unwrap().success());
    format!("cli-{pid}")
}

fn seed_legacy_floor_request(
    layout: &StoreLayout,
    requester: &str,
    owner: &str,
    deadline: Option<i64>,
) {
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    coordinator
        .execute_batch("DROP TABLE reader_registrations;")
        .unwrap();
    coordinator
        .execute(
            "INSERT INTO requests
             (request_id,idempotency_key,kind,payload_json,state,requester_id,
              requester_deadline,claim_owner,claim_heartbeat_at,created_at,updated_at)
             VALUES ('orphan','idem-orphan','update','{}','claimed',?1,?2,?3,1,1,1)",
            params![requester, deadline, owner],
        )
        .unwrap();
}

#[test]
fn reader_floor_activation_recovers_expired_dead_request() {
    let dead = exited_floor_requester();
    let temp = TempStore::new("floor-dead-request");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.39.0", 9).unwrap();
    seed_legacy_floor_request(&layout, &dead, &dead, Some(1));

    MaintenanceExecutor::activate_reader_writer_floor(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.40.0"),
        MaintenanceRun::new("floor-recovery", "owner", std::process::id(), 100, 30_000),
    )
    .unwrap();

    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    let (state, owner, error): (String, Option<String>, String) = coordinator
        .query_row(
            "SELECT state,claim_owner,error_json FROM requests WHERE request_id='orphan'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(state, "failed");
    assert!(owner.is_none());
    assert!(error.contains("coordinator_requester_dead"));
    assert_eq!(
        metadata(
            &Connection::open(layout.store_db()).unwrap(),
            "min_writer_version"
        ),
        "2.40.0"
    );
    assert_eq!(
        coordinator
            .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        0
    );
    assert_eq!(coordinator.query_row("SELECT (SELECT COUNT(*) FROM maintenance_intent) + (SELECT COUNT(*) FROM writer_lease)", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn reader_floor_activation_preserves_live_or_recoverable_requests() {
    let dead = exited_floor_requester();
    let live = format!("cli-{}", std::process::id());
    for (label, requester, owner, deadline, writer) in [
        ("live-owner", dead.as_str(), live.as_str(), Some(1), false),
        (
            "live-requester",
            live.as_str(),
            dead.as_str(),
            Some(1),
            false,
        ),
        (
            "unexpired",
            dead.as_str(),
            dead.as_str(),
            Some(i64::MAX),
            false,
        ),
        ("no-deadline", dead.as_str(), dead.as_str(), None, false),
        (
            "unknown-requester",
            "external-owner",
            dead.as_str(),
            Some(1),
            false,
        ),
        (
            "live-writer-rollback",
            dead.as_str(),
            dead.as_str(),
            Some(1),
            true,
        ),
    ] {
        let temp = TempStore::new(label);
        let layout = StoreLayout::create(temp.path(), "family-a", "2.39.0", 9).unwrap();
        seed_legacy_floor_request(&layout, requester, owner, deadline);
        let coordinator = Connection::open(layout.coordinator_db()).unwrap();
        if writer {
            coordinator.execute(
                "INSERT INTO writer_lease
                 (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,fencing_token)
                 VALUES ('store-writer','foreign','2.39.0',?1,1,9223372036854775807,1)",
                [std::process::id()],
            ).unwrap();
        }
        let error = MaintenanceExecutor::activate_reader_writer_floor(
            StoreConnectionFactory::new(layout.clone(), "family-a", "2.40.0"),
            MaintenanceRun::new("floor-refusal", "owner", std::process::id(), 100, 30_000),
        )
        .unwrap_err();
        assert_eq!(error.code(), "maintenance_busy", "{label}");
        let row: (String, String, Option<String>, Option<i64>, i64) = coordinator
            .query_row(
                "SELECT state,claim_owner,error_json,claim_heartbeat_at,updated_at
             FROM requests WHERE request_id='orphan'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            row,
            ("claimed".to_string(), owner.to_string(), None, Some(1), 1),
            "{label}"
        );
        assert_eq!(
            metadata(
                &Connection::open(layout.store_db()).unwrap(),
                "min_writer_version"
            ),
            "2.39.0",
            "{label}"
        );
        assert_eq!(
            coordinator
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name='reader_registrations'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0,
            "{label}"
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

#[cfg(feature = "test-store-contract")]
struct FixedProcessIdentity(ProcessIdentityObservation);

#[cfg(feature = "test-store-contract")]
impl ProcessIdentityProbe for FixedProcessIdentity {
    fn inspect(&self, _pid: u32) -> ProcessIdentityObservation {
        self.0.clone()
    }
}

#[cfg(feature = "test-store-contract")]
struct TransactionObservingProbe {
    coordinator_db: std::path::PathBuf,
    observed_transaction: Arc<AtomicBool>,
    birth_identity: String,
}

#[cfg(feature = "test-store-contract")]
impl TransactionObservingProbe {
    fn new(
        coordinator_db: &Path,
        observed_transaction: Arc<AtomicBool>,
        birth_identity: &str,
    ) -> Self {
        Self {
            coordinator_db: coordinator_db.to_path_buf(),
            observed_transaction,
            birth_identity: birth_identity.to_string(),
        }
    }
}

#[cfg(feature = "test-store-contract")]
impl ProcessIdentityProbe for TransactionObservingProbe {
    fn inspect(&self, pid: u32) -> ProcessIdentityObservation {
        let mut connection = Connection::open(&self.coordinator_db).unwrap();
        connection.busy_timeout(std::time::Duration::ZERO).unwrap();
        let held = match connection.transaction_with_behavior(TransactionBehavior::Immediate) {
            Ok(transaction) => {
                drop(transaction);
                false
            }
            Err(rusqlite::Error::SqliteFailure(error, _))
                if matches!(
                    error.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                ) =>
            {
                true
            }
            Err(error) => panic!("identity probe transaction check failed: {error}"),
        };
        self.observed_transaction.store(held, Ordering::SeqCst);
        ProcessIdentityObservation::Alive(ProcessInstanceIdentity::new(pid, &self.birth_identity))
    }
}

#[cfg(feature = "test-store-contract")]
fn seeded_admission_store(temp: &TempStore, manifest_entries: i64) -> StoreLayout {
    seeded_admission_store_with_version(temp, manifest_entries, "2.40.0")
}

#[cfg(feature = "test-store-contract")]
fn seeded_admission_store_with_version(
    temp: &TempStore,
    manifest_entries: i64,
    creator_version: &str,
) -> StoreLayout {
    let layout = StoreLayout::create(temp.path(), "family-a", creator_version, 9).unwrap();
    let store = Connection::open(layout.store_db()).unwrap();
    store
        .execute_batch(
            "INSERT INTO views(view_id,root,current_generation,created_at,updated_at)
             VALUES ('view-a','/repo',NULL,'2026-09-04T00:00:00Z','2026-09-04T00:00:00Z');
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',42,'manifest-hash','request-a','2026-09-04T00:00:00Z');
             UPDATE views SET current_generation=42 WHERE view_id='view-a';
             INSERT INTO store_log
               (sequence,request_id,event_kind,view_id,generation,payload_json,created_at)
             VALUES (700,'request-a','manifest_flipped','view-a',42,'{}','2026-09-04T00:00:00Z');",
        )
        .unwrap();
    if manifest_entries > 0 {
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
                [manifest_entries],
            )
            .unwrap();
    }
    drop(store);
    let coordinator = Connection::open(layout.coordinator_db()).unwrap();
    coordinator
        .execute(
            "INSERT INTO family_allocator_marks(allocator_kind,scope_id,high_water,updated_at)
             VALUES ('store_log','',700,1)",
            [],
        )
        .unwrap();
    layout
}

#[cfg(feature = "test-store-contract")]
fn seeded_manifest_log_store(
    temp: &TempStore,
    retain_later_log: bool,
    prune_manifest_log: bool,
) -> (StoreLayout, i64, i64) {
    let layout = StoreLayout::create(temp.path(), "family-a", "2.40.0", 9).unwrap();
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            "request-a",
            "request-a-key",
            RequestKind::Import,
            "{}",
            "reader-test",
            i64::MAX,
            1,
        ))
        .unwrap();
    let mut store = Connection::open(layout.store_db()).unwrap();
    ManifestStore::new(&mut store)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let published = ManifestStore::new(&mut store)
        .publish("view-a", None, std::iter::empty(), "request-a")
        .unwrap();
    let original_manifest_sequence = published.effect_sequence.unwrap();
    let transaction = store.transaction().unwrap();
    let terminal_a = StoreLog::append_terminal(
        &transaction,
        &StoreLogEntry::new(
            "request-a",
            "store_import_completed",
            "{}",
            "2026-09-04T00:00:01Z",
        )
        .with_view("view-a")
        .with_generation(published.generation),
    )
    .unwrap();
    transaction.commit().unwrap();
    drop(store);
    coordinator.reconcile("request-a").unwrap();
    assert_eq!(
        coordinator
            .archive_terminal_requests("gen-001", i64::MAX, terminal_a, 10)
            .unwrap()
            .len(),
        1
    );

    let expected_served_sequence = if retain_later_log {
        coordinator
            .enqueue(CoordinatorRequest::new(
                "request-b",
                "request-b-key",
                RequestKind::Update,
                "{}",
                "reader-test",
                i64::MAX,
                2,
            ))
            .unwrap();
        let mut store = Connection::open(layout.store_db()).unwrap();
        let transaction = store.transaction().unwrap();
        let terminal_b = StoreLog::append_terminal(
            &transaction,
            &StoreLogEntry::new(
                "request-b",
                "store_update_completed",
                "{}",
                "2026-09-04T00:00:02Z",
            ),
        )
        .unwrap();
        transaction.commit().unwrap();
        drop(store);
        coordinator.reconcile("request-b").unwrap();
        terminal_b
    } else {
        0
    };
    let store = Connection::open(layout.store_db()).unwrap();
    if prune_manifest_log {
        let transaction = store.unchecked_transaction().unwrap();
        assert_eq!(
            StoreLog::prune_receipted_request(&transaction, "request-a", terminal_a).unwrap(),
            2
        );
        transaction.commit().unwrap();
    }
    assert_eq!(
        store
            .query_row("SELECT COUNT(*) FROM store_log", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        i64::from(retain_later_log) + if prune_manifest_log { 0 } else { 2 }
    );
    (layout, original_manifest_sequence, expected_served_sequence)
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
