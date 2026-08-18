use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use julie_extract_artifact::store::{
    CapacityProvider, GenerationLifecycle, GenerationPolicy, MaintenanceAction, MaintenanceClock,
    MaintenanceInspector, MaintenanceRun, RepairDisposition, StoreConnectionFactory, StoreLayout,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn promotion_streams_exact_rows_and_advances_current() {
    let temp = TempStore::new("promotion");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_source(&layout);
    seed_receipt(&layout, 31);
    let plan = MaintenanceInspector::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        FixedClock(1_000),
        FixedCapacity,
    )
    .inspect()
    .unwrap();
    let mut lifecycle = GenerationLifecycle::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new("promote-1", "owner", std::process::id(), 1_000, 30_000),
        &plan,
        MaintenanceAction::Promote,
        FixedCapacity,
    )
    .unwrap();

    let report = lifecycle
        .promote(&plan, &GenerationPolicy::default())
        .unwrap();

    assert_eq!(report.source_generation, "gen-001");
    assert_eq!(report.destination_generation, "gen-002");
    assert_eq!(report.copied_file_versions, 2);
    assert_eq!(report.selected_generation, None);
    let current = StoreLayout::open(temp.path()).unwrap();
    assert_eq!(current.generation_name(), "gen-002");
    let source = Connection::open(temp.path().join("gen-001/store.db")).unwrap();
    let destination = Connection::open(current.store_db()).unwrap();
    assert_eq!(metadata(&source, "generation_state"), "retired");
    assert_eq!(metadata(&destination, "generation_state"), "serving");
    assert_eq!(version_rows(&source), version_rows(&destination));
    assert_eq!(manifest_rows(&source), manifest_rows(&destination));
    let coord = Connection::open(current.coordinator_db()).unwrap();
    assert_eq!(allocator(&coord, "file_version", ""), 11);
    assert_eq!(allocator(&coord, "store_log", ""), 31);
    assert_eq!(allocator(&coord, "manifest_generation", "view-a"), 4);
    assert_eq!(
        allocator(&coord, "resolution_delta_generation", "view-a"),
        9
    );
    assert_eq!(
        destination
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name='store_log'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        31
    );
}

#[test]
fn promotion_does_not_create_resolution_scope_objects() {
    let temp = TempStore::new("promotion-scope-journal");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    Connection::open(layout.store_db())
        .unwrap()
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO views(view_id,root,current_generation,created_at,updated_at)
             VALUES ('view-a','/repo',1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'sha256:m1','request-a','2026-01-01T00:00:00Z');
             COMMIT;",
        )
        .unwrap();

    let current = promote_once(
        &layout,
        "promote-scope-journal",
        1_000,
        &GenerationPolicy::default(),
    );
    let destination = Connection::open(current.store_db()).unwrap();
    assert_eq!(
        destination
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'resolution_%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn promotion_ignores_leftover_base_files() {
    let temp = TempStore::new("base-identity");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_source(&layout);
    seed_base(&layout, b"valid resolution base");
    fs::write(layout.bases_dir().join("base-a.db"), b"corrupt").unwrap();
    let current = promote_once(&layout, "promote-base", 1_000, &GenerationPolicy::default());
    assert_eq!(current.generation_name(), "gen-002");
}

#[test]
fn retained_cleanup_keeps_only_the_configured_retired_generations() {
    let temp = TempStore::new("generation-pins");
    let initial = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_source(&initial);
    let policy = GenerationPolicy {
        retained_generation_limit: 1,
        rollback_safety_ms: 0,
        ..GenerationPolicy::default()
    };

    let second = promote_once(&initial, "promote-pin-1", 1_000, &policy);
    let third = promote_once(&second, "promote-pin-2", 2_000, &policy);
    let fourth = promote_once(&third, "promote-pin-3", 3_000, &policy);

    assert_eq!(fourth.generation_name(), "gen-004");
    assert!(!temp.path().join("gen-001").exists());
    assert!(!temp.path().join("gen-002").exists());
    assert!(temp.path().join("gen-003").exists());
}

#[test]
fn repair_stops_after_checkpoint_when_the_serving_generation_is_valid() {
    let temp = TempStore::new("repair-checkpoint");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_source(&layout);
    let plan = inspect_plan(&layout, 1_000);
    let mut lifecycle = GenerationLifecycle::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new("repair-1", "owner", std::process::id(), 1_000, 30_000),
        &plan,
        MaintenanceAction::Repair,
        FixedCapacity,
    )
    .unwrap();

    let report = lifecycle
        .repair(&plan, &GenerationPolicy::default())
        .unwrap();

    assert_eq!(
        report.repair_disposition,
        Some(RepairDisposition::CheckpointRecovered)
    );
    assert_eq!(report.source_generation, "gen-001");
    assert_eq!(report.destination_generation, "gen-001");
    assert!(!temp.path().join("gen-002").exists());
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(
        coord
            .query_row("SELECT COUNT(*) FROM maintenance_intent", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}

#[test]
fn logical_copy_is_bounded_by_the_configured_primary_key_window() {
    let temp = TempStore::new("copy-window");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let mut store = Connection::open(layout.store_db()).unwrap();
    let transaction = store.transaction().unwrap();
    for version_id in 1..=101_i64 {
        transaction
            .execute(
                "INSERT INTO file_versions
                 (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
                  complete_l1,complete_l2,complete_l3)
                 VALUES (?1,?2,?3,1,'rust',1,1,1,2,3)",
                rusqlite::params![
                    version_id,
                    format!("src/{version_id}.rs"),
                    format!("blake3:{version_id}"),
                ],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    let plan = inspect_plan(&layout, 1_000);
    let mut lifecycle = GenerationLifecycle::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new("promote-window", "owner", std::process::id(), 1_000, 30_000),
        &plan,
        MaintenanceAction::Promote,
        FixedCapacity,
    )
    .unwrap();

    let report = lifecycle
        .promote(
            &plan,
            &GenerationPolicy {
                copy_window: 7,
                ..GenerationPolicy::default()
            },
        )
        .unwrap();

    assert_eq!(report.copied_file_versions, 101);
    assert_eq!(report.max_observed_copy_window, 7);
    assert_eq!(
        Connection::open(StoreLayout::open(temp.path()).unwrap().store_db())
            .unwrap()
            .query_row("SELECT COUNT(*) FROM file_versions", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        101
    );
}

#[test]
fn family_allocators_scan_all_named_generations_and_receipts() {
    let temp = TempStore::new("family-allocators");
    let initial = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_source(&initial);
    seed_base(&initial, b"valid resolution base");
    let second = promote_once(
        &initial,
        "promote-allocator-1",
        1_000,
        &GenerationPolicy::default(),
    );
    Connection::open(temp.path().join("gen-001/store.db"))
        .unwrap()
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO file_versions
               (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
                complete_l1,complete_l2,complete_l3)
             VALUES (42,'src/retained.rs','blake3:retained',1,'rust',1,1,1,2,3);
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',20,'sha256:m20','request-retained','2026-01-03T00:00:00Z');
             INSERT INTO manifest_entries
               (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',20,'src/retained.rs','rust',42,'indexed','blake3:retained',
                     '2026-01-03T00:00:00Z');
             INSERT INTO store_log
               (sequence,request_id,event_kind,terminal,payload_json,created_at)
             VALUES (40,'request-retained','store_import_completed',1,'{}',
                     '2026-01-03T00:00:00Z');
             COMMIT;",
        )
        .unwrap();
    seed_receipt(&second, 50);

    let third = promote_once(
        &second,
        "promote-allocator-2",
        2_000,
        &GenerationPolicy::default(),
    );

    let coord = Connection::open(third.coordinator_db()).unwrap();
    assert_eq!(allocator(&coord, "file_version", ""), 42);
    assert_eq!(allocator(&coord, "store_log", ""), 50);
    assert_eq!(allocator(&coord, "manifest_generation", "view-a"), 20);
    let store = Connection::open(third.store_db()).unwrap();
    assert_eq!(
        store
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name='file_versions'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        42
    );
    assert_eq!(
        store
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name='store_log'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        50
    );
}

#[test]
fn rollback_safety_window_retains_unpinned_generations() {
    let temp = TempStore::new("rollback-safety");
    let initial = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_source(&initial);
    let policy = GenerationPolicy {
        retained_generation_limit: 1,
        rollback_safety_ms: 60_000,
        ..GenerationPolicy::default()
    };

    let second = promote_once(&initial, "promote-safety-1", 1_000, &policy);
    let third = promote_once(&second, "promote-safety-2", 2_000, &policy);

    assert_eq!(third.generation_name(), "gen-003");
    assert!(temp.path().join("gen-001").exists());
    assert!(temp.path().join("gen-002").exists());
}

#[test]
fn retired_generation_cleanup_orders_numeric_suffixes() {
    let temp = TempStore::new("numeric-generation-retention");
    let initial = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_source(&initial);
    fs::rename(temp.path().join("gen-001"), temp.path().join("gen-998")).unwrap();
    fs::write(temp.path().join("CURRENT"), "gen-998\n").unwrap();
    let initial = StoreLayout::open(temp.path()).unwrap();
    let policy = GenerationPolicy {
        retained_generation_limit: 1,
        rollback_safety_ms: 0,
        ..GenerationPolicy::default()
    };

    let generation_999 = promote_once(&initial, "promote-999", 1_000, &policy);
    let generation_1000 = promote_once(&generation_999, "promote-1000", 2_000, &policy);
    let generation_1001 = promote_once(&generation_1000, "promote-1001", 3_000, &policy);

    assert_eq!(generation_1001.generation_name(), "gen-1001");
    assert!(!temp.path().join("gen-999").exists());
    assert!(temp.path().join("gen-1000").exists());
}

#[test]
fn forward_rollback_preserves_latest_logs_and_allocators_with_new_visible_identity() {
    let temp = TempStore::new("rollback");
    let initial = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_source(&initial);
    let first_plan = inspect_plan(&initial, 1_000);
    let mut promotion = GenerationLifecycle::acquire(
        StoreConnectionFactory::new(initial.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new("promote-1", "owner", std::process::id(), 1_000, 30_000),
        &first_plan,
        MaintenanceAction::Promote,
        FixedCapacity,
    )
    .unwrap();
    promotion
        .promote(&first_plan, &GenerationPolicy::default())
        .unwrap();
    let latest = StoreLayout::open(temp.path()).unwrap();
    seed_latest_state(&latest);
    let rollback_plan = inspect_plan(&latest, 2_000);
    let mut rollback = GenerationLifecycle::acquire(
        StoreConnectionFactory::new(latest.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new("rollback-1", "owner", std::process::id(), 2_000, 30_000),
        &rollback_plan,
        MaintenanceAction::Rollback,
        FixedCapacity,
    )
    .unwrap();

    let report = rollback
        .rollback(&rollback_plan, &GenerationPolicy::default(), "gen-001")
        .unwrap();

    assert_eq!(report.source_generation, "gen-002");
    assert_eq!(report.destination_generation, "gen-003");
    assert_eq!(report.selected_generation.as_deref(), Some("gen-001"));
    let current = StoreLayout::open(temp.path()).unwrap();
    let store = Connection::open(current.store_db()).unwrap();
    let current_manifest = store
        .query_row(
            "SELECT current_generation FROM views WHERE view_id='view-a'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(current_manifest, 9);
    assert_eq!(
        store
            .query_row(
                "SELECT group_concat(path,',') FROM (
                   SELECT path FROM manifest_entries
                   WHERE view_id='view-a' AND generation=9 ORDER BY path
                 )",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "src/a.rs,src/b.rs"
    );
    assert_eq!(
        store
            .query_row("SELECT MAX(version_id) FROM file_versions", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        19
    );
    assert_eq!(
        store
            .query_row("SELECT MAX(sequence) FROM store_log", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        23
    );
    let coord = Connection::open(current.coordinator_db()).unwrap();
    assert_eq!(allocator(&coord, "manifest_generation", "view-a"), 9);
    assert_eq!(allocator(&coord, "file_version", ""), 19);
    assert_eq!(allocator(&coord, "store_log", ""), 23);
    assert_eq!(
        coord
            .query_row(
                "SELECT generation_name || ':' || store_log_sequence
                 FROM consumer_cursors WHERE consumer_id='consumer-a'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "gen-002:23"
    );
    assert_eq!(
        coord
            .query_row(
                "SELECT terminal_generation_name || ':' || terminal_log_sequence
                 FROM request_receipts WHERE request_id='request-b'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "gen-002:23"
    );
}

#[test]
fn forward_rollback_refuses_conflicting_immutable_identity() {
    let temp = TempStore::new("rollback-conflict");
    let initial = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_source(&initial);
    let second = promote_once(
        &initial,
        "promote-conflict",
        1_000,
        &GenerationPolicy::default(),
    );
    Connection::open(second.store_db())
        .unwrap()
        .execute(
            "UPDATE file_versions SET content_hash='blake3:conflict' WHERE version_id=7",
            [],
        )
        .unwrap();
    let plan = inspect_plan(&second, 2_000);
    let mut rollback = GenerationLifecycle::acquire(
        StoreConnectionFactory::new(second.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new(
            "rollback-conflict",
            "owner",
            std::process::id(),
            2_000,
            30_000,
        ),
        &plan,
        MaintenanceAction::Rollback,
        FixedCapacity,
    )
    .unwrap();

    let error = rollback
        .rollback(&plan, &GenerationPolicy::default(), "gen-001")
        .unwrap_err();

    assert_eq!(error.code(), "generation_identity_conflict");
    assert_eq!(
        StoreLayout::open(temp.path()).unwrap().generation_name(),
        "gen-002"
    );
    assert!(!temp.path().join("gen-003").exists());
}

#[test]
fn forward_rollback_rebinds_exact_resolution_with_fresh_manifest_and_delta_ids() {
    let temp = TempStore::new("rollback-resolution");
    let initial = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_source(&initial);
    seed_base(&initial, b"valid resolution base");
    seed_exact_binding(&initial);
    let second = promote_once(
        &initial,
        "promote-resolution",
        1_000,
        &GenerationPolicy::default(),
    );
    seed_latest_unbound(&second);
    let plan = inspect_plan(&second, 2_000);
    let mut rollback = GenerationLifecycle::acquire(
        StoreConnectionFactory::new(second.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new(
            "rollback-resolution",
            "owner",
            std::process::id(),
            2_000,
            30_000,
        ),
        &plan,
        MaintenanceAction::Rollback,
        FixedCapacity,
    )
    .unwrap();

    rollback
        .rollback(&plan, &GenerationPolicy::default(), "gen-001")
        .unwrap();

    let current = StoreLayout::open(temp.path()).unwrap();
    let store = Connection::open(current.store_db()).unwrap();
    assert_eq!(
        store
            .query_row(
                "SELECT current_generation || ':' || resolution_state || ':' ||
                        resolution_base_id || ':' || resolution_delta_generation || ':' ||
                        resolution_exact_at
                 FROM views WHERE view_id='view-a'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "9:exact:base-a:9:9"
    );
    assert_eq!(
        store
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'resolution_%'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    let coord = Connection::open(current.coordinator_db()).unwrap();
    assert_eq!(allocator(&coord, "manifest_generation", "view-a"), 9);
}

fn seed_source(layout: &StoreLayout) {
    let store = Connection::open(layout.store_db()).unwrap();
    store
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO file_versions
               (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
                metadata_json,complete_l1,complete_l2,complete_l3)
             VALUES
               (7,'src/a.rs','blake3:a',1,'rust',10,1,NULL,1,2,3),
               (11,'src/b.rs','blake3:b',1,'rust',20,2,NULL,1,2,3);
             INSERT INTO views
               (view_id,root,current_generation,resolution_state,resolution_base_id,
                resolution_delta_generation,resolution_exact_at,created_at,updated_at)
             VALUES
               ('view-a','/repo',4,'unbound',NULL,NULL,NULL,
                '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',4,'sha256:m4','request-a','2026-01-01T00:00:00Z');
             INSERT INTO manifest_entries
               (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES
               ('view-a',4,'src/a.rs','rust',7,'indexed','blake3:a','2026-01-01T00:00:00Z'),
               ('view-a',4,'src/b.rs','rust',11,'indexed','blake3:b','2026-01-01T00:00:00Z');
             INSERT INTO store_log
               (sequence,request_id,event_kind,view_id,generation,terminal,payload_json,created_at)
             VALUES
               (13,'request-a','manifest_flipped','view-a',4,0,'{}','2026-01-01T00:00:00Z'),
               (17,'request-a','store_import_completed','view-a',4,1,'{}','2026-01-01T00:00:01Z');
             COMMIT;",
        )
        .unwrap();
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    coord
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO family_allocator_marks(allocator_kind,scope_id,high_water,updated_at)
             VALUES
               ('file_version','',11,1),
               ('store_log','',17,1),
               ('manifest_generation','view-a',4,1),
               ('resolution_delta_generation','view-a',9,1);
             COMMIT;",
        )
        .unwrap();
}

fn seed_latest_state(layout: &StoreLayout) {
    let store = Connection::open(layout.store_db()).unwrap();
    store
        .execute_batch(
            "BEGIN IMMEDIATE;
             INSERT INTO file_versions
               (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
                metadata_json,complete_l1,complete_l2,complete_l3)
             VALUES (19,'src/c.rs','blake3:c',1,'rust',30,3,NULL,1,2,3);
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',8,'sha256:m8','request-b','2026-01-02T00:00:00Z');
             INSERT INTO manifest_entries
               (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',8,'src/c.rs','rust',19,'indexed','blake3:c',
                     '2026-01-02T00:00:00Z');
             UPDATE views SET current_generation=8,updated_at='2026-01-02T00:00:00Z'
             WHERE view_id='view-a';
             INSERT INTO store_log
               (sequence,request_id,event_kind,view_id,generation,terminal,payload_json,created_at)
             VALUES (23,'request-b','store_update_completed','view-a',8,1,'{}',
                     '2026-01-02T00:00:00Z');
             COMMIT;",
        )
        .unwrap();
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    coord
        .execute_batch(
            "BEGIN IMMEDIATE;
             UPDATE family_allocator_marks SET high_water=19,updated_at=updated_at+1
             WHERE allocator_kind='file_version' AND scope_id='';
             UPDATE family_allocator_marks SET high_water=23,updated_at=updated_at+1
             WHERE allocator_kind='store_log' AND scope_id='';
             UPDATE family_allocator_marks SET high_water=8,updated_at=updated_at+1
             WHERE allocator_kind='manifest_generation' AND scope_id='view-a';
             INSERT INTO consumer_cursors
               (consumer_id,generation_name,store_log_sequence,updated_at)
             VALUES ('consumer-a','gen-002',23,2);
             INSERT INTO request_receipts
               (request_id,idempotency_key,kind,payload_json,terminal_result_json,
                terminal_generation_name,terminal_log_sequence,completed_at)
             VALUES ('request-b','idem-b','update','{}','{}','gen-002',23,2);
             COMMIT;",
        )
        .unwrap();
}

fn seed_receipt(layout: &StoreLayout, sequence: i64) {
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "INSERT INTO request_receipts
             (request_id,idempotency_key,kind,payload_json,terminal_result_json,
              terminal_generation_name,terminal_log_sequence,completed_at)
             VALUES ('receipt-request','receipt-idem','import','{}','{}','gen-001',?1,1)",
            [sequence],
        )
        .unwrap();
}

fn seed_base(layout: &StoreLayout, bytes: &[u8]) {
    let relative = "bases/base-a.db";
    let path = layout.generation_dir().join(relative);
    fs::write(&path, bytes).unwrap();
    let store = Connection::open(layout.store_db()).unwrap();
    if !table_exists(&store, "resolution_bases") {
        return;
    }
    let sha = format!("{:x}", Sha256::digest(bytes));
    store
        .execute(
            "INSERT INTO resolution_bases
             (base_id,manifest_hash,resolver_output_epoch,state,relative_path,identifier_count,
              pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
             VALUES ('base-a','sha256:m4',1,'ready',?1,0,0,?2,?3,'request-a',
                     '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            rusqlite::params![relative, bytes.len() as i64, sha],
        )
        .unwrap();
}

fn table_exists(connection: &Connection, table: &str) -> bool {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |row| row.get(0),
        )
        .unwrap()
}

fn seed_exact_binding(layout: &StoreLayout) {
    let store = Connection::open(layout.store_db()).unwrap();
    if table_exists(&store, "resolution_deltas") {
        store
            .execute_batch(
                "BEGIN IMMEDIATE;
                 INSERT INTO resolution_deltas
                 (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
                  resolver_output_epoch,identifier_replacements,pending_replacements,pending_tombstones,
                  exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
                 VALUES ('view-a',9,'base-a',4,'sha256:m4',1,1,0,0,0,0,'{}','request-a',
                         '2026-01-01T00:00:00Z');
                 INSERT INTO resolution_identifier_deltas
                 (view_id,delta_generation,version_id,identifier_id,target_version_id,target_symbol_id,
                  tier,confidence,method,outcome,candidates)
                 VALUES ('view-a',9,7,'identifier-a',7,'symbol-a',1,1.0,'exact','resolved',1);
                 UPDATE views SET resolution_state='exact',resolution_base_id='base-a',
                   resolution_delta_generation=9,resolution_exact_at=4 WHERE view_id='view-a';
                 COMMIT;",
            )
            .unwrap();
        return;
    }
    store
        .execute(
            "UPDATE views SET resolution_state='exact',resolution_base_id='base-a',
               resolution_delta_generation=9,resolution_exact_at=4 WHERE view_id='view-a'",
            [],
        )
        .unwrap();
}

fn seed_latest_unbound(layout: &StoreLayout) {
    let store = Connection::open(layout.store_db()).unwrap();
    store
        .execute_batch(
            "BEGIN IMMEDIATE;
             UPDATE views SET current_generation=NULL,resolution_state='unbound',
               resolution_base_id=NULL,resolution_delta_generation=NULL,resolution_exact_at=NULL
             WHERE view_id='view-a';
             INSERT INTO file_versions
               (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
                metadata_json,complete_l1,complete_l2,complete_l3)
             VALUES (19,'src/c.rs','blake3:c',1,'rust',30,3,NULL,1,2,3);
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',8,'sha256:m8','request-b','2026-01-02T00:00:00Z');
             INSERT INTO manifest_entries
               (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',8,'src/c.rs','rust',19,'indexed','blake3:c',
                     '2026-01-02T00:00:00Z');
             UPDATE views SET current_generation=8,updated_at='2026-01-02T00:00:00Z'
             WHERE view_id='view-a';
             INSERT INTO store_log
               (sequence,request_id,event_kind,view_id,generation,terminal,payload_json,created_at)
             VALUES (23,'request-b','store_update_completed','view-a',8,1,'{}',
                     '2026-01-02T00:00:00Z');
             COMMIT;",
        )
        .unwrap();
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    coord
        .execute_batch(
            "BEGIN IMMEDIATE;
             UPDATE family_allocator_marks SET high_water=19,updated_at=updated_at+1
             WHERE allocator_kind='file_version' AND scope_id='';
             UPDATE family_allocator_marks SET high_water=23,updated_at=updated_at+1
             WHERE allocator_kind='store_log' AND scope_id='';
             UPDATE family_allocator_marks SET high_water=8,updated_at=updated_at+1
             WHERE allocator_kind='manifest_generation' AND scope_id='view-a';
             COMMIT;",
        )
        .unwrap();
}

fn inspect_plan(
    layout: &StoreLayout,
    now_ms: i64,
) -> julie_extract_artifact::store::MaintenancePlan {
    MaintenanceInspector::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        FixedClock(now_ms),
        FixedCapacity,
    )
    .inspect()
    .unwrap()
}

fn promote_once(
    layout: &StoreLayout,
    run_id: &str,
    now_ms: i64,
    policy: &GenerationPolicy,
) -> StoreLayout {
    let plan = inspect_plan(layout, now_ms);
    let mut lifecycle = GenerationLifecycle::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new(run_id, "owner", std::process::id(), now_ms, 30_000),
        &plan,
        MaintenanceAction::Promote,
        FixedCapacity,
    )
    .unwrap();
    lifecycle.promote(&plan, policy).unwrap();
    StoreLayout::open(layout.root()).unwrap()
}

fn allocator(connection: &Connection, kind: &str, scope: &str) -> i64 {
    connection
        .query_row(
            "SELECT high_water FROM family_allocator_marks
             WHERE allocator_kind=?1 AND scope_id=?2",
            [kind, scope],
            |row| row.get(0),
        )
        .unwrap()
}

fn metadata(connection: &Connection, key: &str) -> String {
    connection
        .query_row("SELECT value FROM store_meta WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .unwrap()
}

fn version_rows(connection: &Connection) -> Vec<(i64, String, String)> {
    let mut statement = connection
        .prepare("SELECT version_id,path,content_hash FROM file_versions ORDER BY version_id")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn manifest_rows(connection: &Connection) -> Vec<(String, i64, String, i64)> {
    let mut statement = connection
        .prepare(
            "SELECT view_id,generation,path,version_id
             FROM manifest_entries ORDER BY view_id,generation,path",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
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
