use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use julie_extract_artifact::store::{
    CapacityProvider, CoordinatorRequest, DeltaVersionFact, MaintenanceApplyPolicy,
    MaintenanceApplyReport, MaintenanceCapacity, MaintenanceClock, MaintenanceError,
    MaintenanceExecutor, MaintenanceInspector, MaintenanceLevel, MaintenancePolicy,
    MaintenanceRootKind, MaintenanceRun, MaintenanceSnapshot, ManifestFact, ManifestVersionFact,
    PlanBinding, RequestKind, StoreConnectionError, StoreConnectionFactory, StoreCoordinator,
    StoreLayout, VersionFact, plan_maintenance,
};
use rusqlite::{Connection, params};

const DAY_MS: i64 = 86_400_000;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn pure_plan_preserves_level_roots_and_time_before_path_cap() {
    let now = 40 * DAY_MS;
    let mut snapshot = snapshot();
    snapshot.versions = (1..=31)
        .map(|version_id| VersionFact {
            version_id,
            path: "src/lib.rs".to_string(),
            logical_bytes: 100,
            complete_l1: true,
            complete_l2: true,
            complete_l3: true,
        })
        .collect();
    snapshot.manifests.push(ManifestFact {
        view_id: "view-a".to_string(),
        generation: 31,
        created_at_ms: now,
        current: true,
    });
    snapshot.manifest_versions.push(ManifestVersionFact {
        view_id: "view-a".to_string(),
        generation: 31,
        version_id: 31,
        path: "src/lib.rs".to_string(),
        failed_preserved: true,
    });
    for generation in 1..=30 {
        snapshot.manifests.push(ManifestFact {
            view_id: "view-a".to_string(),
            generation,
            created_at_ms: if generation == 1 {
                now - 6 * DAY_MS
            } else {
                now - 8 * DAY_MS
            },
            current: false,
        });
        snapshot.manifest_versions.push(ManifestVersionFact {
            view_id: "view-a".to_string(),
            generation,
            version_id: generation,
            path: "src/lib.rs".to_string(),
            failed_preserved: false,
        });
    }

    let plan = plan_maintenance(&snapshot, &MaintenancePolicy::default()).unwrap();

    let current = plan.version(31).unwrap();
    for level in [
        MaintenanceLevel::L1,
        MaintenanceLevel::L2,
        MaintenanceLevel::L3,
    ] {
        assert!(current.reasons(level).iter().any(|reason| {
            reason.kind == MaintenanceRootKind::CurrentManifest && reason.reference == "view-a:31"
        }));
    }
    assert!(
        plan.version(1)
            .unwrap()
            .reasons(MaintenanceLevel::L1)
            .iter()
            .any(|reason| reason.kind == MaintenanceRootKind::RetentionWindow)
    );
    assert!(!plan.eligible_manifest("view-a", 1));
    assert!(plan.eligible_manifest("view-a", 2));
    assert!(!plan.eligible_manifest("view-a", 8));
    assert_eq!(plan.retention.target_bytes, 120);
    assert_eq!(plan.retention.ceiling_bytes, 125);
}

#[test]
fn plan_refuses_unknown_roots_and_is_deterministic_under_shuffled_input() {
    let mut ordered = snapshot();
    ordered.versions = vec![version(1, "a.rs", 10), version(2, "b.rs", 20)];
    ordered.manifests = vec![ManifestFact {
        view_id: "view-a".to_string(),
        generation: 1,
        created_at_ms: 1,
        current: true,
    }];
    ordered.manifest_versions = vec![
        manifest_version(1, 1, "a.rs"),
        manifest_version(1, 2, "b.rs"),
    ];
    let mut shuffled = ordered.clone();
    shuffled.versions.reverse();
    shuffled.manifest_versions.reverse();

    let first = plan_maintenance(&ordered, &MaintenancePolicy::default()).unwrap();
    let second = plan_maintenance(&shuffled, &MaintenancePolicy::default()).unwrap();

    assert_eq!(first.fingerprint, second.fingerprint);
    assert_eq!(first.versions, second.versions);

    ordered
        .manifest_versions
        .push(manifest_version(1, 99, "lost.rs"));
    let error = plan_maintenance(&ordered, &MaintenancePolicy::default()).unwrap_err();
    assert_eq!(error.code(), "unknown_maintenance_root");
}

#[test]
fn delta_sources_and_targets_are_l2_roots_in_both_delta_tables() {
    let mut snapshot = snapshot();
    snapshot.versions = (1..=4)
        .map(|id| VersionFact {
            version_id: id,
            path: format!("src/{id}.rs"),
            logical_bytes: 1,
            complete_l1: true,
            complete_l2: true,
            complete_l3: true,
        })
        .collect();
    snapshot.identifier_delta_versions = vec![DeltaVersionFact {
        view_id: "view-a".to_string(),
        delta_generation: 1,
        source_version_id: 1,
        target_version_id: Some(2),
    }];
    snapshot.pending_delta_versions = vec![DeltaVersionFact {
        view_id: "view-a".to_string(),
        delta_generation: 1,
        source_version_id: 3,
        target_version_id: Some(4),
    }];

    let plan = plan_maintenance(&snapshot, &MaintenancePolicy::default()).unwrap();

    for (version_id, kind) in [
        (1, MaintenanceRootKind::IdentifierDeltaSource),
        (2, MaintenanceRootKind::IdentifierDeltaTarget),
        (3, MaintenanceRootKind::PendingDeltaSource),
        (4, MaintenanceRootKind::PendingDeltaTarget),
    ] {
        let decision = plan.version(version_id).unwrap();
        assert!(
            decision
                .reasons(MaintenanceLevel::L2)
                .iter()
                .any(|reason| reason.kind == kind)
        );
        assert!(!decision.l2_reasons.is_empty());
    }
    assert_eq!(plan.demotion_cohort.len(), 4);
    assert!(
        plan.demotion_cohort
            .iter()
            .all(|candidate| candidate.drop_l3 && !candidate.drop_l2)
    );
}

#[test]
fn capacity_is_conservative_and_demotion_cohort_is_bounded() {
    let mut snapshot = snapshot();
    snapshot.capacity = MaintenanceCapacity {
        free_bytes: 128 * 1024 * 1024,
        store_page_bytes: 40 * 1024 * 1024,
        store_freelist_bytes: 4 * 1024 * 1024,
        store_wal_bytes: 8 * 1024 * 1024,
        base_bytes: 16 * 1024 * 1024,
        scratch_bytes: 2 * 1024 * 1024,
        staged_generation_bytes: 48 * 1024 * 1024,
        retention_baseline_bytes: 0,
        retention_breach_streak: 0,
    };
    snapshot.versions = (1..=150)
        .map(|id| VersionFact {
            version_id: id,
            path: format!("src/{id}.rs"),
            logical_bytes: 1024 * 1024,
            complete_l1: true,
            complete_l2: true,
            complete_l3: true,
        })
        .collect();

    let plan = plan_maintenance(&snapshot, &MaintenancePolicy::default()).unwrap();

    assert_eq!(plan.demotion_cohort.len(), 64);
    assert!(plan.demotion_cohort.iter().all(|item| item.drop_l3));
    assert_eq!(plan.capacity.demotion_wal_headroom_bytes, 64 * 1024 * 1024);
    assert!(plan.capacity.promotion_required_bytes >= 48 * 1024 * 1024);
    assert!(!plan.capacity.promotion_fits);
}

#[test]
fn sqlite_inspection_covers_store_and_coordinator_roots_in_bounded_windows() {
    let temp = TempStore::new("sqlite-matrix");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    fs::create_dir(temp.path().join("gen-1000")).unwrap();
    seed_store_matrix(&layout);
    let factory = StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0");
    let inspector = MaintenanceInspector::new(factory, FixedClock(20 * DAY_MS), FixedCapacity)
        .with_window_size(2);

    let plan = inspector.inspect().unwrap();

    assert_eq!(plan.binding.current_generation, "gen-001");
    assert_eq!(plan.binding.store_log_max, 11);
    assert_eq!(plan.binding.request_watermark, 2);
    assert_eq!(plan.binding.allocator_marks.len(), 2);
    assert!(plan.binding.store_root_fingerprint.starts_with("sha256:"));
    assert!(
        plan.binding
            .coordinator_root_fingerprint
            .starts_with("sha256:")
    );
    assert!(plan.max_observed_window <= 2);
    let current = plan.version(1).unwrap();
    assert!(current.reasons(MaintenanceLevel::L2).iter().any(|reason| {
        reason.kind == MaintenanceRootKind::ResolutionBase && reason.reference == "base-a"
    }));
    assert!(current.reasons(MaintenanceLevel::L2).iter().any(|reason| {
        reason.kind == MaintenanceRootKind::IdentifierDeltaTarget && reason.reference == "view-a:1"
    }));
    assert!(current.reasons(MaintenanceLevel::L2).iter().any(|reason| {
        reason.kind == MaintenanceRootKind::ViewBinding && reason.reference == "view-a:base-a"
    }));
    assert!(
        current.reasons(MaintenanceLevel::L2).iter().any(|reason| {
            reason.kind == MaintenanceRootKind::Pin && reason.reference == "pin-a"
        })
    );
    assert!(plan.protected_requests.contains(&"request-a".to_string()));
    assert!(plan.protected_cursors.contains(&"consumer-a".to_string()));
    assert!(plan.protected_generations.contains(&"gen-001".to_string()));
    assert!(plan.protected_generations.contains(&"gen-1000".to_string()));
    assert!(plan.protected_pins.contains(&"pin-a".to_string()));
    assert_eq!(
        plan.expired_pins,
        vec!["pin-expired".to_string(), "pin-expired-delta".to_string()]
    );
    assert!(plan.eligible_bases.contains(&"base-orphan".to_string()));
    assert_eq!(plan.eligible_deltas.len(), 1);
    assert!(
        plan.protected_scratch
            .contains(&"request-live.scratch".to_string())
    );
    assert_eq!(
        plan.protected_failed_paths,
        vec!["view-a:1:rust:src/failed.rs".to_string()]
    );

    let repeated = MaintenanceInspector::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        FixedClock(20 * DAY_MS),
        FixedCapacity,
    )
    .with_window_size(1)
    .inspect()
    .unwrap();
    assert_eq!(plan.fingerprint, repeated.fingerprint);
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "UPDATE requests SET claim_owner='worker-b',claim_heartbeat_at=3,updated_at=3
             WHERE request_id='request-b'",
            [],
        )
        .unwrap();
    let changed = MaintenanceInspector::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        FixedClock(20 * DAY_MS),
        FixedCapacity,
    )
    .inspect()
    .unwrap();
    assert_ne!(plan.fingerprint, changed.fingerprint);
    assert_ne!(
        plan.binding.coordinator_root_fingerprint,
        changed.binding.coordinator_root_fingerprint
    );
}

#[test]
fn scope_predecessor_is_a_concrete_maintenance_root() {
    let temp = TempStore::new("scope-predecessor-root");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let store = Connection::open(layout.store_db()).unwrap();
    store
        .execute_batch(
            "PRAGMA foreign_keys=ON;
             BEGIN IMMEDIATE;
             INSERT INTO file_versions
               (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
                complete_l1,complete_l2,complete_l3)
             VALUES
               (1,'src/lib.rs','blake3:a',1,'rust',10,1,1,2,3),
               (2,'src/lib.rs','blake3:b',1,'rust',10,1,1,2,3);
             INSERT INTO views(view_id,root,current_generation,created_at,updated_at)
             VALUES ('view-a','/repo',3,'2026-01-01T00:00:00Z','2026-01-03T00:00:00Z');
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES
               ('view-a',1,'sha256:m1','request-1','2026-01-01T00:00:00Z'),
               ('view-a',2,'sha256:m2','request-2','2026-01-02T00:00:00Z'),
               ('view-a',3,'sha256:m3','request-3','2026-01-03T00:00:00Z');
             INSERT INTO manifest_entries
               (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES
               ('view-a',1,'src/lib.rs','rust',1,'indexed','blake3:a','2026-01-01T00:00:00Z'),
               ('view-a',2,'src/lib.rs','rust',2,'indexed','blake3:b','2026-01-02T00:00:00Z');
             INSERT INTO resolution_bases
               (base_id,manifest_hash,resolver_output_epoch,state,relative_path,identifier_count,
                pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
             VALUES ('base-scope','sha256:m1',7,'ready','bases/base-scope.db',1,0,10,
                     'sha256:base','request-base','2026-01-01T00:00:00Z',
                     '2026-01-01T00:00:00Z');
             INSERT INTO resolution_base_versions(base_id,version_id) VALUES ('base-scope',1);
             INSERT INTO resolution_deltas
               (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
                resolver_output_epoch,identifier_replacements,pending_replacements,
                pending_tombstones,exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
             VALUES ('view-a',1,'base-scope',1,'sha256:m1',7,1,0,0,0,0,'{}','request-exact',
                     '2026-01-01T00:00:00Z');
             INSERT INTO resolution_identifier_deltas
               (view_id,delta_generation,version_id,identifier_id,target_version_id,
                target_symbol_id,tier,confidence,method,outcome,candidates)
             VALUES ('view-a',1,1,'identifier-a',1,'symbol-a',1,1.0,'exact','resolved',1);
             INSERT INTO resolution_scope_batches
               (transition_id,view_id,from_manifest_generation,from_manifest_hash,
                to_manifest_generation,to_manifest_hash,scope_usable,
                predecessor_manifest_generation,predecessor_manifest_hash,base_id,
                delta_generation,resolver_output_epoch,change_count,change_hash,request_id,
                completed_at)
             VALUES (1,'view-a',1,'sha256:m1',2,'sha256:m2',1,1,'sha256:m1','base-scope',1,7,1,
                     'sha256:changes','request-2','2026-01-02T00:00:00Z');
             INSERT INTO resolution_scope_batches
               (transition_id,view_id,previous_transition_id,from_manifest_generation,
                from_manifest_hash,to_manifest_generation,to_manifest_hash,scope_usable,
                predecessor_manifest_generation,predecessor_manifest_hash,base_id,
                delta_generation,resolver_output_epoch,change_count,change_hash,request_id,
                completed_at)
             VALUES (2,'view-a',1,2,'sha256:m2',3,'sha256:m3',1,1,'sha256:m1','base-scope',1,7,1,
                     'sha256:changes-2','request-3','2026-01-03T00:00:00Z');
             INSERT INTO resolution_scope_journal
               (transition_id,path,change_kind,old_version_id,new_version_id,touched_names_json)
             VALUES (1,'src/lib.rs','content_replaced',1,2,'[]');
             INSERT INTO resolution_scope_journal
               (transition_id,path,change_kind,old_version_id,new_version_id,touched_names_json)
             VALUES (2,'src/lib.rs','path_deleted',2,NULL,'[]');
             INSERT INTO resolution_scope_state
               (view_id,predecessor_manifest_generation,predecessor_manifest_hash,base_id,
                delta_generation,resolver_output_epoch,current_manifest_generation,
                current_manifest_hash,journal_through_transition_id)
             VALUES ('view-a',1,'sha256:m1','base-scope',1,7,3,'sha256:m3',2);
             COMMIT;",
        )
        .unwrap();

    let plan = MaintenanceInspector::new(
        StoreConnectionFactory::new(layout, "family-a", "2.30.0"),
        FixedClock(20 * DAY_MS),
        FixedCapacity,
    )
    .inspect()
    .unwrap();

    assert!(plan.protected_bases.contains(&"base-scope".to_string()));
    assert!(!plan.eligible_bases.contains(&"base-scope".to_string()));
    assert!(
        plan.protected_deltas
            .contains(&"view-a:00000000000000000001".to_string())
    );
    assert!(
        !plan
            .eligible_deltas
            .contains(&"view-a:00000000000000000001".to_string())
    );
    assert!(
        plan.version(1)
            .unwrap()
            .reasons(MaintenanceLevel::L1)
            .iter()
            .any(|reason| {
                reason.kind == MaintenanceRootKind::CurrentManifest
                    && reason.reference == "view-a:1"
            })
    );
    assert!(
        plan.version(2)
            .unwrap()
            .reasons(MaintenanceLevel::L2)
            .iter()
            .any(|reason| {
                reason.kind == MaintenanceRootKind::ViewBinding
                    && reason.reference == "view-a:scope:2"
            })
    );
}

#[test]
fn paged_inspection_refuses_a_concurrent_coordinator_commit() {
    let temp = TempStore::new("inspection-race");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_store_matrix(&layout);
    let inspector = MaintenanceInspector::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        FixedClock(20 * DAY_MS),
        RacingCapacity {
            coordinator_db: Mutex::new(Some(layout.coordinator_db().to_path_buf())),
        },
    )
    .with_window_size(1);

    let error = inspector.inspect().unwrap_err();

    assert_eq!(error.code(), "maintenance_inspection_raced");
}

#[cfg(unix)]
#[test]
fn dead_maintenance_owner_is_replaced_before_its_expiry() {
    let temp = TempStore::new("dead-maintenance-owner");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let mut child = std::process::Command::new("sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    let dead_pid = child.id();
    child.wait().unwrap();
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "INSERT INTO maintenance_intent
             (resource,run_id,action,source_generation_name,owner_id,owner_pid,fencing_token,
              heartbeat_at,expires_at,started_at,plan_fingerprint,source_min_writer_version)
             VALUES ('store-maintenance','dead-run','gc','gen-001','dead-owner',?1,1,
                     1,?2,1,'dead-plan','2.30.0')",
            params![i64::from(dead_pid), i64::MAX],
        )
        .unwrap();

    let plan = inspect_plan(&layout, 30 * DAY_MS);
    let executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new(
            "successor-run",
            "successor-owner",
            std::process::id(),
            30 * DAY_MS,
            5_000,
        ),
        &plan,
        FixedCapacity,
    )
    .unwrap();

    let coord = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(
        coord
            .query_row(
                "SELECT run_id FROM maintenance_intent WHERE resource='store-maintenance'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "successor-run"
    );
    drop(executor);
}

#[test]
fn gc_demotes_l3_then_l2_and_only_then_purges_whole_unrooted_versions() {
    let temp = TempStore::new("gc-level-order");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_gc_level_matrix(&layout);

    let first = inspect_plan(&layout, 30 * DAY_MS);
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new("gc-1", "owner", std::process::id(), 30 * DAY_MS, 5_000),
        &first,
        FixedCapacity,
    )
    .unwrap();
    let first_report = executor.apply(&first).unwrap();
    assert_eq!(first_report.demoted_l3, 2);
    assert_eq!(first_report.demoted_l2, 0);
    assert_eq!(first_report.purged_versions, 0);
    assert_eq!(first_report.last_version_cursor, Some(3));
    assert_eq!(
        first_report.checkpoint_order,
        ["checkpoint", "incremental_vacuum", "truncate_checkpoint"]
    );
    assert!(!include_str!("../src/store/maintenance.rs").contains("VACUUM;"));
    assert_level_state(&layout, 2, true, false, 1, 0);
    assert_level_state(&layout, 3, true, false, 1, 0);

    let second = inspect_plan(&layout, 30 * DAY_MS);
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new("gc-2", "owner", std::process::id(), 30 * DAY_MS + 1, 5_000),
        &second,
        FixedCapacity,
    )
    .unwrap();
    let second_report = executor.apply(&second).unwrap();
    assert_eq!(second_report.demoted_l3, 0);
    assert_eq!(second_report.demoted_l2, 2);
    assert_eq!(second_report.purged_versions, 0);
    assert_level_state(&layout, 2, false, false, 0, 0);
    assert_level_state(&layout, 3, false, false, 0, 0);

    let third = inspect_plan(&layout, 30 * DAY_MS);
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new("gc-3", "owner", std::process::id(), 30 * DAY_MS + 2, 5_000),
        &third,
        FixedCapacity,
    )
    .unwrap();
    let third_report = executor.apply(&third).unwrap();
    assert_eq!(third_report.purged_versions, 1);
    let store = Connection::open(layout.store_db()).unwrap();
    assert_eq!(
        store
            .query_row(
                "SELECT group_concat(version_id,',') FROM file_versions ORDER BY version_id",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "1,2"
    );
}

#[test]
fn gc_steps_incremental_vacuum_until_the_freelist_is_empty() {
    let temp = TempStore::new("gc-physical-reclaim");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_gc_level_matrix(&layout);
    let mut store = Connection::open(layout.store_db()).unwrap();
    store
        .execute_batch("CREATE TABLE gc_pressure (payload BLOB NOT NULL);")
        .unwrap();
    let transaction = store.transaction().unwrap();
    for _ in 0..4096 {
        transaction
            .execute(
                "INSERT INTO gc_pressure(payload) VALUES (?1)",
                [vec![0_u8; 4096]],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    store.execute("DELETE FROM gc_pressure", []).unwrap();
    let freelist_before: i64 = store
        .query_row("PRAGMA freelist_count", [], |row| row.get(0))
        .unwrap();
    assert!(freelist_before > 64);
    let bytes_before = fs::metadata(layout.store_db()).unwrap().len();

    let plan = inspect_plan(&layout, 30 * DAY_MS);
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new(
            "gc-physical",
            "owner",
            std::process::id(),
            30 * DAY_MS,
            5_000,
        ),
        &plan,
        FixedCapacity,
    )
    .unwrap();
    let policy = MaintenanceApplyPolicy {
        incremental_vacuum_pages: usize::try_from(freelist_before).unwrap(),
        ..MaintenanceApplyPolicy::default()
    };
    let report = executor.apply_with_policy(&plan, &policy).unwrap();

    let bytes_after = fs::metadata(layout.store_db()).unwrap().len();
    assert_eq!(report.freelist_pages_after_vacuum, 0);
    assert!(report.vacuum_pages >= freelist_before as u64);
    assert!(bytes_after < bytes_before);
}

#[test]
fn gc_persists_physical_retention_breaches_and_requests_compaction() {
    let temp = TempStore::new("gc-physical-retention");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_gc_level_matrix(&layout);
    Connection::open(layout.store_db())
        .unwrap()
        .execute(
            "INSERT INTO store_meta(key,value)
             VALUES ('retention_physical_baseline_bytes','1')
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [],
        )
        .unwrap();

    let mut report = MaintenanceApplyReport::default();
    for run_number in 1..=3 {
        let plan = inspect_plan(&layout, 30 * DAY_MS + run_number);
        assert!(plan.retention.physical_current_bytes > plan.retention.physical_ceiling_bytes);
        let mut executor = MaintenanceExecutor::acquire(
            StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
            MaintenanceRun::new(
                format!("gc-physical-{run_number}"),
                "owner",
                std::process::id(),
                30 * DAY_MS + run_number,
                5_000,
            ),
            &plan,
            FixedCapacity,
        )
        .unwrap();
        report = executor.apply(&plan).unwrap();
    }

    assert!(report.physical_bytes_after > report.physical_target_bytes);
    assert_eq!(report.physical_breach_streak, 3);
    assert!(report.compaction_required);
    assert_eq!(
        Connection::open(layout.store_db())
            .unwrap()
            .query_row(
                "SELECT value FROM store_meta
                 WHERE key='retention_physical_breach_streak'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "3"
    );
}

#[test]
fn gc_retires_inactive_scope_batches_before_their_parent_manifests() {
    let temp = TempStore::new("gc-manifest-entry-order");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let mut store = Connection::open(layout.store_db()).unwrap();
    store.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    let transaction = store.transaction().unwrap();
    transaction
        .execute(
            "INSERT INTO views(view_id,root,current_generation,created_at,updated_at)
             VALUES ('view-a','/repo',NULL,'1970-01-01T00:00:01Z','1970-01-01T00:00:01Z')",
            [],
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO file_versions
             (version_id,path,content_hash,extraction_epoch,language,content_bytes,
              line_count,metadata_json,complete_l1,complete_l2,complete_l3)
             VALUES (1,'src/lib.rs','content-hash',1,'rust',1,1,'{}',1,1,1)",
            [],
        )
        .unwrap();
    for generation in 1..=26_i64 {
        transaction
            .execute(
                "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
                 VALUES ('view-a',?1,?2,?3,'1970-01-01T00:00:01Z')",
                params![
                    generation,
                    format!("manifest-{generation}"),
                    format!("request-{generation}")
                ],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO manifest_entries
                 (view_id,generation,path,language,version_id,status,observed_content_hash,
                  indexed_at,error_class,error_json)
                 VALUES ('view-a',?1,'src/lib.rs','rust',1,'indexed','content-hash',
                         '1970-01-01T00:00:01Z',NULL,NULL)",
                [generation],
            )
            .unwrap();
    }
    transaction
        .execute(
            "UPDATE views SET current_generation=26 WHERE view_id='view-a'",
            [],
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO resolution_scope_batches
             (view_id,previous_transition_id,from_manifest_generation,from_manifest_hash,
              to_manifest_generation,to_manifest_hash,scope_usable,
              predecessor_manifest_generation,predecessor_manifest_hash,base_id,
              delta_generation,resolver_output_epoch,change_count,change_hash,request_id,
              completed_at)
             VALUES ('view-a',NULL,1,'manifest-1',2,'manifest-2',0,
                     NULL,NULL,NULL,NULL,NULL,0,'blake3:empty','scope-fallback',
                     '1970-01-01T00:00:01Z')",
            [],
        )
        .unwrap();
    transaction.commit().unwrap();

    let plan = inspect_plan(&layout, 30 * DAY_MS);
    assert_eq!(plan.eligible_manifests, [("view-a".to_string(), 1)]);
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new(
            "gc-manifest-entry-order",
            "owner",
            std::process::id(),
            30 * DAY_MS,
            5_000,
        ),
        &plan,
        FixedCapacity,
    )
    .unwrap();
    let report = executor.apply(&plan).unwrap();

    assert_eq!(report.removed_manifests, 1);
    assert_eq!(
        store
            .query_row("SELECT COUNT(*) FROM resolution_scope_batches", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .query_row("SELECT COUNT(*) FROM manifests", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        25
    );
    assert_eq!(
        store
            .query_row("SELECT COUNT(*) FROM manifest_entries", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        25
    );
    assert_eq!(
        store
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}

#[test]
fn gc_archives_terminal_requests_before_pruning_their_store_log() {
    let temp = TempStore::new("gc-request-pruning");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let store = Connection::open(layout.store_db()).unwrap();
    store
        .execute(
            "INSERT INTO store_log
             (sequence,request_id,event_kind,terminal,payload_json,created_at)
             VALUES (1,'request-old','store_import_completed',1,'{\"generation\":1}',
                     '1970-01-01T00:00:01Z')",
            [],
        )
        .unwrap();
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    coord
        .execute(
            "INSERT INTO requests
             (request_id,idempotency_key,kind,payload_json,state,requester_id,requester_deadline,
              claim_owner,claim_heartbeat_at,terminal_log_sequence,result_json,error_json,
              created_at,updated_at)
             VALUES ('request-old','idem-old','import','{}','committed','cli',NULL,NULL,NULL,
                     1,'{\"generation\":1}',NULL,1,1)",
            [],
        )
        .unwrap();

    let plan = inspect_plan(&layout, 30 * DAY_MS);
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new(
            "gc-request-pruning",
            "owner",
            std::process::id(),
            30 * DAY_MS,
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

    assert_eq!(report.archived_requests, 1);
    assert_eq!(report.pruned_log_rows, 1);
    assert_eq!(
        coord
            .query_row("SELECT COUNT(*) FROM requests", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        coord
            .query_row("SELECT COUNT(*) FROM request_receipts", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .query_row("SELECT COUNT(*) FROM store_log", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let replay = coordinator
        .enqueue(CoordinatorRequest::new(
            "request-retry",
            "idem-old",
            RequestKind::Import,
            "{}",
            "cli",
            31 * DAY_MS,
            31 * DAY_MS,
        ))
        .unwrap();
    assert_eq!(replay.request.request_id, "request-old");
    assert!(!replay.inserted);
}

#[test]
fn malformed_or_ahead_consumer_cursors_block_request_log_pruning() {
    for (name, generation, sequence) in
        [("malformed", "gen-bad", 0_i64), ("ahead", "gen-001", 2_i64)]
    {
        let temp = TempStore::new(name);
        let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
        seed_terminal_request(&layout);
        Connection::open(layout.coordinator_db())
            .unwrap()
            .execute(
                "INSERT INTO consumer_cursors
                 (consumer_id,generation_name,store_log_sequence,updated_at)
                 VALUES ('consumer',?1,?2,1)",
                rusqlite::params![generation, sequence],
            )
            .unwrap();

        let plan = inspect_plan(&layout, 30 * DAY_MS);
        let mut executor = MaintenanceExecutor::acquire(
            StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
            MaintenanceRun::new(name, "owner", std::process::id(), 30 * DAY_MS, 5_000),
            &plan,
            FixedCapacity,
        )
        .unwrap();
        let error = executor
            .apply_with_policy(
                &plan,
                &MaintenanceApplyPolicy {
                    request_safety_ms: 0,
                    ..MaintenanceApplyPolicy::default()
                },
            )
            .unwrap_err();

        assert_eq!(error.code(), "invalid_maintenance_metadata");
        assert_eq!(
            Connection::open(layout.store_db())
                .unwrap()
                .query_row("SELECT COUNT(*) FROM store_log", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let coord = Connection::open(layout.coordinator_db()).unwrap();
        assert_eq!(
            coord
                .query_row("SELECT COUNT(*) FROM requests", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            coord
                .query_row("SELECT COUNT(*) FROM request_receipts", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0
        );
    }
}

#[test]
fn hundred_version_cohorts_persist_a_cursor_and_resume_without_duplicates_or_gaps() {
    let temp = TempStore::new("gc-cohort-cursor");
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
                    format!("blake3:{version_id}")
                ],
            )
            .unwrap();
    }
    transaction.commit().unwrap();

    let first = inspect_plan(&layout, 30 * DAY_MS);
    assert_eq!(first.demotion_cohort.len(), 100);
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new(
            "gc-cursor-1",
            "owner",
            std::process::id(),
            30 * DAY_MS,
            5_000,
        ),
        &first,
        FixedCapacity,
    )
    .unwrap();
    let first_report = executor.apply(&first).unwrap();
    assert_eq!(first_report.demoted_l3, 100);
    assert_eq!(first_report.last_version_cursor, Some(100));
    assert_eq!(maintenance_cursor(&layout), 100);

    let second = inspect_plan(&layout, 30 * DAY_MS + 1);
    assert_eq!(
        second
            .demotion_cohort
            .iter()
            .map(|candidate| candidate.version_id)
            .collect::<Vec<_>>(),
        vec![101]
    );
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new(
            "gc-cursor-2",
            "owner",
            std::process::id(),
            30 * DAY_MS + 1,
            5_000,
        ),
        &second,
        FixedCapacity,
    )
    .unwrap();
    let second_report = executor.apply(&second).unwrap();
    assert_eq!(second_report.demoted_l3, 1);
    assert_eq!(second_report.last_version_cursor, Some(101));
    assert_eq!(maintenance_cursor(&layout), 101);
    assert_eq!(
        Connection::open(layout.store_db())
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM file_versions WHERE complete_l3 IS NOT NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn terminal_request_scratch_is_reaped_while_live_request_scratch_is_preserved() {
    let temp = TempStore::new("gc-scratch");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    coord
        .execute(
            "INSERT INTO requests
             (request_id,idempotency_key,kind,payload_json,state,requester_id,requester_deadline,
              claim_owner,claim_heartbeat_at,terminal_log_sequence,result_json,error_json,
              created_at,updated_at)
             VALUES ('failed-request','failed-idem','resolve','{}','failed','cli',NULL,NULL,NULL,
                     NULL,NULL,'{}',1,1),
                    ('live-request','live-idem','resolve','{}','queued','cli',NULL,NULL,NULL,
                     NULL,NULL,NULL,1,1)",
            [],
        )
        .unwrap();
    for name in [
        "resolve-exact-failed-request.db",
        "resolve-delta-failed-request.db",
        "resolve-exact-live-request.db",
    ] {
        fs::write(layout.scratch_dir().join(name), b"scratch").unwrap();
    }

    let plan = inspect_plan(&layout, 30 * DAY_MS);
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new(
            "gc-scratch",
            "owner",
            std::process::id(),
            30 * DAY_MS,
            5_000,
        ),
        &plan,
        FixedCapacity,
    )
    .unwrap();
    let report = executor.apply(&plan).unwrap();

    assert_eq!(report.removed_scratch_files, 2);
    assert!(
        !layout
            .scratch_dir()
            .join("resolve-exact-failed-request.db")
            .exists()
    );
    assert!(
        !layout
            .scratch_dir()
            .join("resolve-delta-failed-request.db")
            .exists()
    );
    assert!(
        layout
            .scratch_dir()
            .join("resolve-exact-live-request.db")
            .exists()
    );
}

#[test]
fn apply_refuses_when_live_free_bytes_drop_below_required_headroom() {
    let temp = TempStore::new("gc-capacity-reprobe");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_gc_level_matrix(&layout);
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "INSERT INTO requests
             (request_id,idempotency_key,kind,payload_json,state,requester_id,requester_deadline,
              claim_owner,claim_heartbeat_at,terminal_log_sequence,result_json,error_json,
              created_at,updated_at)
             VALUES ('failed-request','failed-idem','resolve','{}','failed','cli',NULL,NULL,NULL,
                     NULL,NULL,'{}',1,1)",
            [],
        )
        .unwrap();
    fs::write(
        layout.scratch_dir().join("resolve-exact-failed-request.db"),
        b"scratch",
    )
    .unwrap();

    let plan = inspect_plan(&layout, 30 * DAY_MS);
    assert!(plan.capacity.gc_fits);
    assert!(plan.capacity.gc_required_bytes > 0);
    assert!(!plan.demotion_cohort.is_empty());

    let free_bytes = Arc::new(AtomicU64::new(512 * 1024 * 1024));
    let capacity = ControllableCapacity {
        free_bytes: Arc::clone(&free_bytes),
    };
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        MaintenanceRun::new(
            "gc-capacity",
            "owner",
            std::process::id(),
            30 * DAY_MS,
            5_000,
        ),
        &plan,
        capacity,
    )
    .unwrap();

    free_bytes.store(0, Ordering::SeqCst);
    let error = executor.apply(&plan).unwrap_err();
    assert!(
        matches!(error, MaintenanceError::CapacityInsufficient),
        "expected capacity_insufficient, got {error:?}"
    );

    assert!(
        layout
            .scratch_dir()
            .join("resolve-exact-failed-request.db")
            .exists(),
        "scratch must remain when capacity re-probe refuses apply"
    );
    assert_level_state(&layout, 2, true, true, 1, 1);
    assert_level_state(&layout, 3, true, true, 1, 1);
}

#[test]
fn apply_error_after_floor_raise_restores_floor_and_clears_intent_on_drop() {
    let temp = TempStore::new("floor-restore-on-error");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_gc_level_matrix(&layout);
    let plan = inspect_plan(&layout, 30 * DAY_MS);
    let free_bytes = Arc::new(AtomicU64::new(512 * 1024 * 1024));
    let capacity = ControllableCapacity {
        free_bytes: Arc::clone(&free_bytes),
    };
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.31.0"),
        MaintenanceRun::new(
            "floor-err-run",
            "owner",
            std::process::id(),
            30 * DAY_MS,
            5_000,
        ),
        &plan,
        capacity,
    )
    .unwrap();
    assert_eq!(
        meta(
            &Connection::open(layout.store_db()).unwrap(),
            "min_writer_version"
        ),
        "2.31.0"
    );
    free_bytes.store(0, Ordering::SeqCst);
    let error = executor.apply(&plan).unwrap_err();
    assert!(
        matches!(error, MaintenanceError::CapacityInsufficient),
        "expected capacity_insufficient, got {error:?}"
    );
    drop(executor);

    let store = Connection::open(layout.store_db()).unwrap();
    assert_eq!(meta(&store, "min_writer_version"), "2.30.0");
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
            .query_row("SELECT COUNT(*) FROM maintenance_intent", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        coord
            .query_row("SELECT COUNT(*) FROM writer_lease", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn acquire_raises_source_writer_floor_and_mirrors_intent() {
    let temp = TempStore::new("floor-raise");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let plan = inspect_plan(&layout, 30 * DAY_MS);
    let executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.31.0"),
        MaintenanceRun::new("floor-run", "owner", std::process::id(), 30 * DAY_MS, 5_000),
        &plan,
        FixedCapacity,
    )
    .unwrap();

    let store = Connection::open(layout.store_db()).unwrap();
    assert_eq!(meta(&store, "min_writer_version"), "2.31.0");
    assert_eq!(meta(&store, "maintenance_tmp_run_id"), "floor-run");
    assert_eq!(meta(&store, "maintenance_tmp_action"), "gc");
    assert_eq!(
        meta(&store, "maintenance_tmp_source_min_writer_version"),
        "2.30.0"
    );
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(
        coord
            .query_row(
                "SELECT source_min_writer_version FROM maintenance_intent
                 WHERE resource='store-maintenance'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "2.30.0"
    );

    let older = StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0");
    let error = older.open_writer().unwrap_err();
    assert!(
        matches!(
            error,
            StoreConnectionError::MaintenanceInProgress { ref run_id }
                if run_id == "floor-run"
        ),
        "live intent must refuse ordinary writers first: {error:?}"
    );
    // Clear intent so the raised floor is the only remaining barrier for older binaries.
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute_batch(
            "DELETE FROM writer_lease;
             DELETE FROM maintenance_intent;",
        )
        .unwrap();
    let error = older.open_writer().unwrap_err();
    match error {
        StoreConnectionError::WriterVersionTooOld {
            ref running,
            ref required,
        } if running == "2.30.0" && required == "2.31.0" => {}
        other => panic!("expected WriterVersionTooOld after intent expiry, got {other:?}"),
    }
    drop(executor);
}

#[test]
fn finish_restores_serving_source_floor_and_clears_intent_mirrors() {
    let temp = TempStore::new("floor-restore");
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    seed_gc_level_matrix(&layout);
    let plan = inspect_plan(&layout, 30 * DAY_MS);
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.31.0"),
        MaintenanceRun::new(
            "restore-run",
            "owner",
            std::process::id(),
            30 * DAY_MS,
            5_000,
        ),
        &plan,
        FixedCapacity,
    )
    .unwrap();
    assert_eq!(
        meta(
            &Connection::open(layout.store_db()).unwrap(),
            "min_writer_version"
        ),
        "2.31.0"
    );
    executor.apply(&plan).unwrap();

    let store = Connection::open(layout.store_db()).unwrap();
    assert_eq!(meta(&store, "min_writer_version"), "2.30.0");
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
            .query_row("SELECT COUNT(*) FROM maintenance_intent", [], |row| row
                .get::<_, i64>(0),)
            .unwrap(),
        0
    );
    assert_eq!(
        coord
            .query_row("SELECT COUNT(*) FROM writer_lease", [], |row| row
                .get::<_, i64>(0),)
            .unwrap(),
        0
    );
}

fn meta(connection: &Connection, key: &str) -> String {
    connection
        .query_row("SELECT value FROM store_meta WHERE key=?1", [key], |row| {
            row.get(0)
        })
        .unwrap()
}

fn maintenance_cursor(layout: &StoreLayout) -> i64 {
    Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM store_meta
             WHERE key='maintenance_gc_version_cursor'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn seed_terminal_request(layout: &StoreLayout) {
    Connection::open(layout.store_db())
        .unwrap()
        .execute(
            "INSERT INTO store_log
             (sequence,request_id,event_kind,terminal,payload_json,created_at)
             VALUES (1,'request-old','store_import_completed',1,'{\"generation\":1}',
                     '1970-01-01T00:00:01Z')",
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
                     1,'{\"generation\":1}',NULL,1,1)",
            [],
        )
        .unwrap();
}

fn snapshot() -> MaintenanceSnapshot {
    MaintenanceSnapshot {
        binding: PlanBinding {
            family_id: "family-a".to_string(),
            current_generation: "gen-001".to_string(),
            store_root_fingerprint: "sha256:store".to_string(),
            coordinator_root_fingerprint: "sha256:coord".to_string(),
            store_log_max: 0,
            request_watermark: 0,
            allocator_marks: Vec::new(),
        },
        now_ms: 40 * DAY_MS,
        capacity: MaintenanceCapacity::default(),
        ..MaintenanceSnapshot::default()
    }
}

fn version(version_id: i64, path: &str, logical_bytes: u64) -> VersionFact {
    VersionFact {
        version_id,
        path: path.to_string(),
        logical_bytes,
        complete_l1: true,
        complete_l2: false,
        complete_l3: false,
    }
}

fn manifest_version(generation: i64, version_id: i64, path: &str) -> ManifestVersionFact {
    ManifestVersionFact {
        view_id: "view-a".to_string(),
        generation,
        version_id,
        path: path.to_string(),
        failed_preserved: false,
    }
}

fn seed_store_matrix(layout: &StoreLayout) {
    let store = Connection::open(layout.store_db()).unwrap();
    store
        .execute_batch("PRAGMA foreign_keys=ON; BEGIN IMMEDIATE;")
        .unwrap();
    store
        .execute(
            "INSERT INTO file_versions
         (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
          metadata_json,complete_l1,complete_l2,complete_l3)
         VALUES (1,'src/lib.rs','blake3:a',1,'rust',100,2,NULL,1,2,3)",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO manifest_entries
         (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at,
          error_class,error_json)
         VALUES ('view-a',1,'src/failed.rs','rust',NULL,'failed','blake3:failed',
                 '2026-01-01T00:00:00Z','parse','{}')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO views
         (view_id,root,current_generation,resolution_state,resolution_base_id,
          resolution_delta_generation,resolution_exact_at,created_at,updated_at)
         VALUES ('view-a','/repo',NULL,'unbound',NULL,NULL,NULL,
                 '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
         VALUES ('view-a',1,'sha256:m','request-a','2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO manifest_entries
         (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at,
          error_class,error_json)
         VALUES ('view-a',1,'src/lib.rs','rust',1,'failed_preserved','blake3:a',
                 '2026-01-01T00:00:00Z','parse','{}')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO resolution_bases
         (base_id,manifest_hash,resolver_output_epoch,state,relative_path,identifier_count,
          pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
         VALUES ('base-a','sha256:m',1,'ready','bases/base-a.db',1,1,10,'sha256:b',
                 'request-a','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO resolution_base_versions VALUES ('base-a',1)",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO resolution_bases
         (base_id,manifest_hash,resolver_output_epoch,state,relative_path,identifier_count,
          pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
         VALUES ('base-orphan','sha256:orphan',1,'ready','bases/base-orphan.db',1,1,10,
                 'sha256:o','request-a','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO resolution_deltas
         (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
          resolver_output_epoch,identifier_replacements,pending_replacements,pending_tombstones,
          exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
         VALUES ('view-a',1,'base-a',1,'sha256:m',1,1,1,0,0,0,'{}','request-a',
                 '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO resolution_deltas
         (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
          resolver_output_epoch,identifier_replacements,pending_replacements,pending_tombstones,
          exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
         VALUES ('view-a',2,'base-a',1,'sha256:m',1,0,0,0,0,0,'{}','request-a',
                 '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO resolution_identifier_deltas
         (view_id,delta_generation,version_id,identifier_id,target_version_id,target_symbol_id,
          tier,confidence,method,outcome,candidates)
         VALUES ('view-a',1,1,'id-a',1,'symbol-a',1,1.0,'exact','resolved',1)",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO resolution_pins
         (pin_id,owner_kind,owner_id,view_id,manifest_generation,base_id,delta_generation,
          expires_at,created_at)
         VALUES ('pin-expired','reader','reader-old','view-a',1,'base-orphan',NULL,
                 '1970-01-02T00:00:00Z','1970-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO resolution_pins
         (pin_id,owner_kind,owner_id,view_id,manifest_generation,base_id,delta_generation,
          expires_at,created_at)
         VALUES ('pin-expired-delta','reader','reader-old','view-a',1,'base-a',2,
                 '1970-01-02T00:00:00Z','1970-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO resolution_pending_deltas
         (view_id,delta_generation,version_id,pending_relationship_id,operation,
          target_version_id,target_symbol_id,tier,confidence,method)
         VALUES ('view-a',1,1,'pending-a','replace',1,'symbol-a',1,1.0,'exact')",
            [],
        )
        .unwrap();
    store.execute(
        "UPDATE views SET current_generation=1,resolution_state='exact',resolution_base_id='base-a',
          resolution_delta_generation=1,resolution_exact_at=1 WHERE view_id='view-a'",
        [],
    ).unwrap();
    store
        .execute(
            "INSERT INTO resolution_pins
         (pin_id,owner_kind,owner_id,view_id,manifest_generation,base_id,delta_generation,
          expires_at,created_at)
         VALUES ('pin-a','reader','reader-a','view-a',1,'base-a',1,
                 '2026-12-31T00:00:00Z','2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    store.execute(
        "INSERT INTO store_log
         (sequence,request_id,event_kind,view_id,generation,version_id,level,terminal,payload_json,created_at)
         VALUES (11,'request-a','store_import_completed','view-a',1,1,3,1,'{}',
                 '2026-01-01T00:00:00Z')",
        [],
    ).unwrap();
    store.execute_batch("COMMIT;").unwrap();

    let coord = Connection::open(layout.coordinator_db()).unwrap();
    coord.execute(
        "INSERT INTO requests
         (request_id,idempotency_key,kind,payload_json,state,requester_id,requester_deadline,
          claim_owner,claim_heartbeat_at,terminal_log_sequence,result_json,error_json,created_at,updated_at)
         VALUES ('request-a','idem-a','import','{}','committed','cli',NULL,NULL,NULL,11,'{}',NULL,1,1)",
        [],
    ).unwrap();
    coord.execute(
        "INSERT INTO requests
         (request_id,idempotency_key,kind,payload_json,state,requester_id,requester_deadline,
          claim_owner,claim_heartbeat_at,terminal_log_sequence,result_json,error_json,created_at,updated_at)
         VALUES ('request-b','idem-b','resolve','{}','claimed','cli',NULL,'worker-a',2,
                 NULL,NULL,NULL,2,2)",
        [],
    ).unwrap();
    coord
        .execute(
            "INSERT INTO consumer_cursors VALUES ('consumer-a','gen-001',10,1)",
            [],
        )
        .unwrap();
    coord
        .execute(
            "INSERT INTO family_allocator_marks VALUES ('file_version','',1,1)",
            [],
        )
        .unwrap();
    coord
        .execute(
            "INSERT INTO family_allocator_marks VALUES ('store_log','',11,1)",
            [],
        )
        .unwrap();
    fs::write(layout.bases_dir().join("base-a.db"), b"base-bytes").unwrap();
    fs::write(layout.bases_dir().join("base-orphan.db"), b"orphan-bytes").unwrap();
    fs::write(
        layout.scratch_dir().join("request-live.scratch"),
        b"scratch",
    )
    .unwrap();
}

fn seed_gc_level_matrix(layout: &StoreLayout) {
    let store = Connection::open(layout.store_db()).unwrap();
    store
        .execute_batch("PRAGMA foreign_keys=ON; BEGIN IMMEDIATE;")
        .unwrap();
    for version_id in 1..=3 {
        store
            .execute(
                "INSERT INTO file_versions
                 (version_id,path,content_hash,extraction_epoch,language,content_bytes,line_count,
                  complete_l1,complete_l2,complete_l3)
                 VALUES (?1,?2,?3,1,'rust',4096,1,1,2,3)",
                rusqlite::params![
                    version_id,
                    format!("src/{version_id}.rs"),
                    format!("blake3:{version_id}")
                ],
            )
            .unwrap();
        store
            .execute(
                "INSERT INTO reference_sites
                 (version_id,reference_site_id,path,language,containing_symbol_id,start_line,
                  start_column,end_line,end_column,start_byte,end_byte,is_exact,provenance,level)
                 VALUES (?1,'site',?2,'rust',NULL,1,0,1,1,0,1,1,'target_token',2)",
                rusqlite::params![version_id, format!("src/{version_id}.rs")],
            )
            .unwrap();
        store
            .execute(
                "INSERT INTO identifiers
                 (version_id,identifier_id,reference_site_id,path,language,name,kind,
                  containing_symbol_id,start_line,start_column,end_line,end_column,start_byte,
                  end_byte,confidence,code_context,metadata_json)
                 VALUES (?1,'identifier','site',?2,'rust','value','variable_ref',NULL,
                         1,0,1,1,0,1,1.0,NULL,NULL)",
                rusqlite::params![version_id, format!("src/{version_id}.rs")],
            )
            .unwrap();
        store
            .execute(
                "INSERT INTO structural_facts
                 (version_id,structural_fact_id,path,language,pattern_id,capture_name,node_kind,
                  containing_symbol_id,start_line,start_column,end_line,end_column,start_byte,
                  end_byte,confidence,metadata_json)
                 VALUES (?1,'fact',?2,'rust','test.fact','node','node',NULL,
                         1,0,1,1,0,1,1.0,NULL)",
                rusqlite::params![version_id, format!("src/{version_id}.rs")],
            )
            .unwrap();
    }
    store
        .execute(
            "INSERT INTO views
             (view_id,root,current_generation,resolution_state,created_at,updated_at)
             VALUES ('view-a','/repo',2,'unbound','1970-01-01T00:00:00Z','1970-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'sha256:old','old','1970-01-25T00:00:00Z'),
                    ('view-a',2,'sha256:current','current','1970-01-30T00:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO manifest_entries
             (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/2.rs','rust',2,'indexed','blake3:2','1970-01-25T00:00:00Z'),
                    ('view-a',2,'src/1.rs','rust',1,'indexed','blake3:1','1970-01-30T00:00:00Z')",
            [],
        )
        .unwrap();
    store.execute_batch("COMMIT;").unwrap();
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

fn assert_level_state(
    layout: &StoreLayout,
    version_id: i64,
    l2_complete: bool,
    l3_complete: bool,
    identifiers: i64,
    facts: i64,
) {
    let store = Connection::open(layout.store_db()).unwrap();
    let (l2, l3) = store
        .query_row(
            "SELECT complete_l2 IS NOT NULL,complete_l3 IS NOT NULL
             FROM file_versions WHERE version_id=?1",
            [version_id],
            |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?)),
        )
        .unwrap();
    assert_eq!((l2, l3), (l2_complete, l3_complete));
    assert_eq!(
        store
            .query_row(
                "SELECT COUNT(*) FROM identifiers WHERE version_id=?1",
                [version_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        identifiers
    );
    assert_eq!(
        store
            .query_row(
                "SELECT COUNT(*) FROM structural_facts WHERE version_id=?1",
                [version_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        facts
    );
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

#[derive(Clone)]
struct ControllableCapacity {
    free_bytes: Arc<AtomicU64>,
}

impl CapacityProvider for ControllableCapacity {
    fn free_bytes(&self, _path: &Path) -> Result<u64, std::io::Error> {
        Ok(self.free_bytes.load(Ordering::SeqCst))
    }

    fn staged_generation_bytes(&self, _path: &Path) -> Result<u64, std::io::Error> {
        Ok(64 * 1024 * 1024)
    }
}

struct RacingCapacity {
    coordinator_db: Mutex<Option<PathBuf>>,
}

impl CapacityProvider for RacingCapacity {
    fn free_bytes(&self, _path: &Path) -> Result<u64, std::io::Error> {
        if let Some(path) = self.coordinator_db.lock().unwrap().take() {
            Connection::open(path)
                .unwrap()
                .execute(
                    "UPDATE requests SET updated_at=updated_at+1 WHERE request_id='request-b'",
                    [],
                )
                .unwrap();
        }
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
            "julie-store-maintenance-{name}-{}-{id}",
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
