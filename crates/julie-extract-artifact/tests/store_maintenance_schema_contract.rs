use julie_extract_artifact::store::{
    FamilyAllocatorKind, GenerationState, MaintenanceAction, create_coordinator_schema,
    create_store_schema,
};
use rusqlite::Connection;

#[test]
fn resolution_delta_gc_indexes_start_with_version_id() {
    let connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();

    assert_eq!(
        index_columns(&connection, "idx_gc_resolution_identifier_deltas_version"),
        vec!["version_id", "view_id", "delta_generation", "identifier_id"]
    );
    assert_eq!(
        index_columns(&connection, "idx_gc_resolution_pending_deltas_version"),
        vec![
            "version_id",
            "view_id",
            "delta_generation",
            "pending_relationship_id",
        ]
    );
}

#[test]
fn request_receipts_reserve_request_and_idempotency_identity_immutably() {
    let connection = open_coordinator();

    assert_eq!(
        table_columns(&connection, "request_receipts"),
        vec![
            "request_id",
            "idempotency_key",
            "kind",
            "payload_json",
            "terminal_result_json",
            "terminal_generation_name",
            "terminal_log_sequence",
            "completed_at",
        ]
    );

    insert_receipt(&connection, "request-a", "key-a", 41).unwrap();
    assert!(insert_receipt(&connection, "request-b", "key-a", 42).is_err());
    assert!(insert_receipt(&connection, "request-a", "key-b", 43).is_err());
    assert!(insert_receipt(&connection, "request-c", "key-c", 41).is_err());
    assert!(
        connection
            .execute(
                "UPDATE request_receipts SET terminal_result_json = '{\"state\":\"other\"}'
                 WHERE request_id = 'request-a'",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "DELETE FROM request_receipts WHERE request_id = 'request-a'",
                [],
            )
            .is_err()
    );
}

#[test]
fn consumer_cursors_and_allocator_marks_never_regress() {
    let connection = open_coordinator();

    connection
        .execute(
            "INSERT INTO consumer_cursors
             (consumer_id, generation_name, store_log_sequence, updated_at)
             VALUES ('search', 'gen-002', 50, 1000)",
            [],
        )
        .unwrap();
    assert!(
        connection
            .execute(
                "UPDATE consumer_cursors SET store_log_sequence = 49, updated_at = 1001
                 WHERE consumer_id = 'search'",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE consumer_cursors SET store_log_sequence = 51, updated_at = 999
                 WHERE consumer_id = 'search'",
                [],
            )
            .is_err()
    );
    connection
        .execute(
            "UPDATE consumer_cursors
             SET generation_name = 'gen-003', store_log_sequence = 51, updated_at = 1001
             WHERE consumer_id = 'search'",
            [],
        )
        .unwrap();

    insert_allocator(&connection, "file_version", "", 80, 1000).unwrap();
    insert_allocator(&connection, "manifest_generation", "view-a", 9, 1000).unwrap();
    assert!(insert_allocator(&connection, "file_version", "view-a", 81, 1001).is_err());
    assert!(insert_allocator(&connection, "manifest_generation", "", 10, 1001).is_err());
    assert!(
        connection
            .execute(
                "UPDATE family_allocator_marks SET high_water = 79, updated_at = 1001
                 WHERE allocator_kind = 'file_version' AND scope_id = ''",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE family_allocator_marks SET high_water = 81, updated_at = 999
                 WHERE allocator_kind = 'file_version' AND scope_id = ''",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE family_allocator_marks
                 SET allocator_kind = 'store_log', high_water = 81, updated_at = 1001
                 WHERE allocator_kind = 'file_version' AND scope_id = ''",
                [],
            )
            .is_err()
    );
}

#[test]
fn maintenance_intent_is_a_singleton_with_a_coherent_lease_window() {
    let connection = open_coordinator();

    connection
        .execute(
            "INSERT INTO maintenance_intent
             (resource, run_id, action, source_generation_name, owner_id, owner_pid,
              fencing_token, heartbeat_at, expires_at, started_at, plan_fingerprint,
              source_min_writer_version)
             VALUES ('store-maintenance', 'run-a', 'gc', 'gen-002', 'holder-a', 42,
                     7, 1000, 2000, 900, 'plan-a', '2.30.0')",
            [],
        )
        .unwrap();
    assert!(
        connection
            .execute(
                "INSERT INTO maintenance_intent
                 (resource, run_id, action, source_generation_name, owner_id, owner_pid,
                  fencing_token, heartbeat_at, expires_at, started_at, plan_fingerprint,
                  source_min_writer_version)
                 VALUES ('store-maintenance', 'run-b', 'gc', 'gen-002', 'holder-b', 43,
                         8, 1001, 2001, 901, 'plan-b', '2.30.0')",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE maintenance_intent SET heartbeat_at = 999, expires_at = 2000",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE maintenance_intent SET plan_fingerprint = 'other-plan'",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE maintenance_intent SET expires_at = heartbeat_at",
                [],
            )
            .is_err()
    );
}

#[test]
fn store_generation_state_is_seeded_and_rejects_unknown_values() {
    let connection = Connection::open_in_memory().unwrap();
    create_store_schema(&connection).unwrap();

    assert_eq!(
        connection
            .query_row(
                "SELECT value FROM store_meta WHERE key = 'generation_state'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "serving"
    );
    assert!(
        connection
            .execute(
                "UPDATE store_meta SET value = 'unknown' WHERE key = 'generation_state'",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM store_meta WHERE key = 'generation_state'", [],)
            .is_err()
    );
}

#[test]
fn lifecycle_catalog_enums_share_the_storage_vocabulary() {
    assert_eq!(GenerationState::Serving.as_str(), "serving");
    assert_eq!(GenerationState::Retired.as_str(), "retired");
    assert_eq!(MaintenanceAction::Gc.as_str(), "gc");
    assert_eq!(MaintenanceAction::Repair.as_str(), "repair");
    assert_eq!(MaintenanceAction::Promote.as_str(), "promote");
    assert_eq!(MaintenanceAction::Rollback.as_str(), "rollback");
    assert_eq!(FamilyAllocatorKind::FileVersion.as_str(), "file_version");
    assert_eq!(FamilyAllocatorKind::StoreLog.as_str(), "store_log");
    assert_eq!(
        FamilyAllocatorKind::ManifestGeneration.as_str(),
        "manifest_generation"
    );
    assert_eq!(
        FamilyAllocatorKind::ResolutionDeltaGeneration.as_str(),
        "resolution_delta_generation"
    );
}

fn open_coordinator() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    create_coordinator_schema(&connection).unwrap();
    connection
}

fn insert_receipt(
    connection: &Connection,
    request_id: &str,
    idempotency_key: &str,
    terminal_log_sequence: i64,
) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO request_receipts
         (request_id, idempotency_key, kind, payload_json, terminal_result_json,
          terminal_generation_name, terminal_log_sequence, completed_at)
         VALUES (?1, ?2, 'import', '{\"root\":\"repo\"}', '{\"state\":\"committed\"}',
                 'gen-002', ?3, 1000)",
        (request_id, idempotency_key, terminal_log_sequence),
    )
}

fn insert_allocator(
    connection: &Connection,
    allocator_kind: &str,
    scope_id: &str,
    high_water: i64,
    updated_at: i64,
) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO family_allocator_marks
         (allocator_kind, scope_id, high_water, updated_at)
         VALUES (?1, ?2, ?3, ?4)",
        (allocator_kind, scope_id, high_water, updated_at),
    )
}

fn table_columns(connection: &Connection, table: &str) -> Vec<String> {
    connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn index_columns(connection: &Connection, index: &str) -> Vec<String> {
    connection
        .prepare(&format!("PRAGMA index_info({index})"))
        .unwrap()
        .query_map([], |row| row.get(2))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
