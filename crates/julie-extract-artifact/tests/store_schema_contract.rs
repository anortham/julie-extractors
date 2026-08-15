use std::collections::{BTreeMap, BTreeSet};

use julie_extract_artifact::store::{
    ResolutionBaseRecord, ResolutionBaseState, ResolutionPendingOperation, ResolutionPinOwnerKind,
    STORE_FORMAT_EPOCH, STORE_SQLITE_SCHEMA_VERSION, StoreSchemaError, ViewResolutionState,
    create_coordinator_schema, create_store_schema,
};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

const AUTHORITY: &str = include_str!("../../../docs/contracts/sqlite-store-schema-v2.md");

#[test]
fn store_and_coordinator_catalogs_match_the_checked_in_authority() {
    let store = open_store();
    let coordinator = open_coordinator();

    assert_eq!(catalog_hash(&store), authority_hash("store-catalog-sha256"));
    assert_eq!(
        catalog_hash(&coordinator),
        authority_hash("coordinator-catalog-sha256")
    );
}

#[test]
fn schemas_are_independent_strict_idempotent_version_two_catalogs() {
    assert_eq!(STORE_SQLITE_SCHEMA_VERSION, 2);
    assert_eq!(STORE_FORMAT_EPOCH, 1);

    let store = open_store();
    let coordinator = open_coordinator();

    create_store_schema(&store).unwrap();
    create_coordinator_schema(&coordinator).unwrap();

    assert_eq!(user_version(&store), 2);
    assert_eq!(user_version(&coordinator), 2);
    assert_eq!(ordinary_tables(&store), expected_store_tables());
    assert_eq!(ordinary_tables(&coordinator), expected_coordinator_tables());
    assert_all_tables_strict(&store);
    assert_all_tables_strict(&coordinator);
}

#[test]
fn resolution_scope_journal_is_an_additive_schema_v2_feature() {
    let store = open_store();

    assert_eq!(user_version(&store), 2);
    assert_eq!(
        store
            .query_row(
                "SELECT value FROM store_meta WHERE key='resolution_scope_journal_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "1"
    );
    assert_eq!(
        table_columns(&store, "resolution_scope_state"),
        vec![
            "view_id TEXT",
            "predecessor_manifest_generation INTEGER",
            "predecessor_manifest_hash TEXT",
            "base_id TEXT",
            "delta_generation INTEGER",
            "resolver_output_epoch INTEGER",
            "current_manifest_generation INTEGER",
            "current_manifest_hash TEXT",
            "journal_through_transition_id INTEGER",
        ]
    );
    assert_eq!(
        table_columns(&store, "resolution_scope_batches"),
        vec![
            "transition_id INTEGER",
            "view_id TEXT",
            "previous_transition_id INTEGER",
            "from_manifest_generation INTEGER",
            "from_manifest_hash TEXT",
            "to_manifest_generation INTEGER",
            "to_manifest_hash TEXT",
            "scope_usable INTEGER",
            "predecessor_manifest_generation INTEGER",
            "predecessor_manifest_hash TEXT",
            "base_id TEXT",
            "delta_generation INTEGER",
            "resolver_output_epoch INTEGER",
            "change_count INTEGER",
            "change_hash TEXT",
            "request_id TEXT",
            "completed_at TEXT",
        ]
    );
    assert_eq!(
        table_columns(&store, "resolution_scope_journal"),
        vec![
            "transition_id INTEGER",
            "path TEXT",
            "change_kind TEXT",
            "old_version_id INTEGER",
            "new_version_id INTEGER",
            "touched_names_json TEXT",
        ]
    );
}

#[test]
fn resolution_scope_batches_reject_noncanonical_timestamps() {
    let store = open_store();
    store
        .execute(
            "INSERT INTO views(view_id,root,created_at,updated_at)
             VALUES ('view-a','/repo','2026-08-11T12:00:00Z','2026-08-11T12:00:00Z')",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'hash-a','request-a','2026-08-11T12:00:00Z')",
            [],
        )
        .unwrap();

    assert!(
        store
            .execute(
                "INSERT INTO resolution_scope_batches
                 (view_id,to_manifest_generation,to_manifest_hash,scope_usable,change_count,
                  change_hash,request_id,completed_at)
                 VALUES ('view-a',1,'hash-a',0,0,'sha256:empty','request-a','not-a-time')",
                [],
            )
            .is_err()
    );
}

#[test]
fn resolution_scope_journal_change_kinds_constrain_absent_sides() {
    let store = open_store();
    store
        .execute_batch(
            "INSERT INTO file_versions
             (version_id,path,content_hash,extraction_epoch,language,content_bytes,complete_l1)
             VALUES (1,'src/a.rs','blake3:a',1,'rust',1,1);
             INSERT INTO views(view_id,root,created_at,updated_at)
             VALUES ('view-a','/repo','2026-08-11T12:00:00Z','2026-08-11T12:00:00Z');
             INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'hash-a','request-a','2026-08-11T12:00:00Z');
             INSERT INTO resolution_scope_batches
             (transition_id,view_id,to_manifest_generation,to_manifest_hash,scope_usable,
              change_count,change_hash,request_id,completed_at)
             VALUES (1,'view-a',1,'hash-a',0,0,'sha256:empty','request-a',
                     '2026-08-11T12:00:00Z');",
        )
        .unwrap();

    assert!(
        store
            .execute(
                "INSERT INTO resolution_scope_journal
                 (transition_id,path,change_kind,old_version_id,touched_names_json)
                 VALUES (1,'src/a.rs','path_added',1,'[]')",
                [],
            )
            .is_err()
    );
    assert!(
        store
            .execute(
                "INSERT INTO resolution_scope_journal
                 (transition_id,path,change_kind,new_version_id,touched_names_json)
                 VALUES (1,'src/b.rs','path_deleted',1,'[]')",
                [],
            )
            .is_err()
    );
}

#[test]
fn resolution_catalog_columns_are_frozen() {
    let store = open_store();

    assert_eq!(
        table_columns(&store, "manifest_entries"),
        vec![
            "view_id TEXT",
            "generation INTEGER",
            "path TEXT",
            "language TEXT",
            "version_id INTEGER",
            "status TEXT",
            "observed_content_hash TEXT",
            "indexed_at TEXT",
            "error_class TEXT",
            "error_json TEXT",
        ]
    );
    assert_eq!(
        table_columns(&store, "resolution_bases"),
        vec![
            "base_id TEXT",
            "manifest_hash TEXT",
            "resolver_output_epoch INTEGER",
            "state TEXT",
            "relative_path TEXT",
            "identifier_count INTEGER",
            "pending_count INTEGER",
            "file_bytes INTEGER",
            "file_sha256 TEXT",
            "request_id TEXT",
            "created_at TEXT",
            "updated_at TEXT",
        ]
    );
    assert_eq!(
        table_columns(&store, "resolution_base_versions"),
        vec!["base_id TEXT", "version_id INTEGER"]
    );
    assert_eq!(
        table_columns(&store, "resolution_deltas"),
        vec![
            "view_id TEXT",
            "delta_generation INTEGER",
            "base_id TEXT",
            "manifest_generation INTEGER",
            "manifest_hash TEXT",
            "resolver_output_epoch INTEGER",
            "identifier_replacements INTEGER",
            "pending_replacements INTEGER",
            "pending_tombstones INTEGER",
            "exact_gap_rows INTEGER",
            "exact_gap_files INTEGER",
            "exact_gap_json TEXT",
            "request_id TEXT",
            "created_at TEXT",
        ]
    );
    assert_eq!(
        table_columns(&store, "resolution_identifier_deltas"),
        vec![
            "view_id TEXT",
            "delta_generation INTEGER",
            "version_id INTEGER",
            "identifier_id TEXT",
            "target_version_id INTEGER",
            "target_symbol_id TEXT",
            "tier INTEGER",
            "confidence REAL",
            "method TEXT",
            "outcome TEXT",
            "candidates INTEGER",
        ]
    );
    assert_eq!(
        table_columns(&store, "resolution_pending_deltas"),
        vec![
            "view_id TEXT",
            "delta_generation INTEGER",
            "version_id INTEGER",
            "pending_relationship_id TEXT",
            "operation TEXT",
            "target_version_id INTEGER",
            "target_symbol_id TEXT",
            "tier INTEGER",
            "confidence REAL",
            "method TEXT",
        ]
    );
    assert_eq!(
        table_columns(&store, "resolution_pins"),
        vec![
            "pin_id TEXT",
            "owner_kind TEXT",
            "owner_id TEXT",
            "view_id TEXT",
            "manifest_generation INTEGER",
            "base_id TEXT",
            "delta_generation INTEGER",
            "expires_at TEXT",
            "created_at TEXT",
        ]
    );
}

#[test]
fn resolution_catalog_models_have_stable_storage_values() {
    assert_eq!(ResolutionBaseState::Building.as_str(), "building");
    assert_eq!(ResolutionBaseState::Ready.as_str(), "ready");
    assert_eq!(ResolutionPendingOperation::Replace.as_str(), "replace");
    assert_eq!(ResolutionPendingOperation::Tombstone.as_str(), "tombstone");
    assert_eq!(ResolutionPinOwnerKind::Reader.as_str(), "reader");
    assert_eq!(ResolutionPinOwnerKind::Resolve.as_str(), "resolve");
    assert_eq!(ViewResolutionState::Unbound.as_str(), "unbound");
    assert_eq!(ViewResolutionState::Converging.as_str(), "converging");
    assert_eq!(ViewResolutionState::Exact.as_str(), "exact");

    let row = ResolutionBaseRecord {
        base_id: "base-a".to_string(),
        manifest_hash: "hash-a".to_string(),
        resolver_output_epoch: 1,
        state: ResolutionBaseState::Building,
        relative_path: "bases/base-a.db".to_string(),
        identifier_count: 0,
        pending_count: 0,
        file_bytes: None,
        file_sha256: None,
        request_id: "request-a".to_string(),
        created_at: "2026-08-08T12:00:00Z".to_string(),
        updated_at: "2026-08-08T12:00:00Z".to_string(),
    };
    assert_eq!(row.state, ResolutionBaseState::Building);
}

#[test]
fn resolution_catalog_state_and_binding_coherence_are_enforced() {
    let store = open_store();
    let timestamp = "2026-08-08T12:00:00Z";
    store
        .execute(
            "INSERT INTO file_versions
             (path,content_hash,extraction_epoch,language,content_bytes,complete_l1,complete_l2)
             VALUES ('src/a.rs','blake3:a',1,'rust',1,1,2)",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO views(view_id,root,created_at,updated_at)
             VALUES ('view-a','/repo',?1,?1)",
            [timestamp],
        )
        .unwrap();
    for generation in [1, 2] {
        store
            .execute(
                "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
                 VALUES ('view-a',?1,?2,?3,?4)",
                rusqlite::params![
                    generation,
                    format!("hash-{generation}"),
                    format!("manifest-{generation}"),
                    timestamp
                ],
            )
            .unwrap();
    }
    store
        .execute(
            "INSERT INTO resolution_bases
             (base_id,manifest_hash,resolver_output_epoch,state,relative_path,
              identifier_count,pending_count,request_id,created_at,updated_at)
             VALUES ('base-a','hash-1',1,'building','bases/base-a.db',0,0,'request-a',?1,?1)",
            [timestamp],
        )
        .unwrap();
    assert!(
        store
            .execute(
                "INSERT INTO resolution_deltas
                 (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
                  resolver_output_epoch,identifier_replacements,pending_replacements,
                  pending_tombstones,exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
                 VALUES ('view-a',1,'base-a',1,'hash-1',1,0,0,0,0,0,'[]','request-a',?1)",
                [timestamp],
            )
            .is_err()
    );
    store
        .execute(
            "UPDATE resolution_bases
             SET state='ready',file_bytes=1,file_sha256='sha256:a',updated_at=?1
             WHERE base_id='base-a'",
            [timestamp],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO resolution_base_versions(base_id,version_id) VALUES ('base-a',1)",
            [],
        )
        .unwrap();
    assert!(
        store
            .execute(
                "INSERT INTO resolution_deltas
                 (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
                  resolver_output_epoch,identifier_replacements,pending_replacements,
                  pending_tombstones,exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
                 VALUES ('view-a',2,'base-a',2,'wrong-hash',1,0,0,0,0,0,'[]','request-b',?1)",
                [timestamp],
            )
            .is_err()
    );
    store
        .execute(
            "INSERT INTO resolution_deltas
             (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
              resolver_output_epoch,identifier_replacements,pending_replacements,
              pending_tombstones,exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
             VALUES ('view-a',1,'base-a',1,'hash-1',1,0,0,0,0,0,'[]','request-a',?1)",
            [timestamp],
        )
        .unwrap();
    assert!(
        store
            .execute(
                "UPDATE views SET current_generation=2,resolution_state='converging',
                        resolution_base_id='base-a',resolution_delta_generation=1
                 WHERE view_id='view-a'",
                [],
            )
            .is_err()
    );
    assert!(
        store
            .execute(
                "INSERT INTO resolution_pins
                 (pin_id,owner_kind,owner_id,view_id,manifest_generation,base_id,
                  delta_generation,expires_at,created_at)
                 VALUES ('pin-bad','reader','reader-a','view-a',2,'base-a',1,?1,?1)",
                [timestamp],
            )
            .is_err()
    );
    store
        .execute(
            "INSERT INTO resolution_pins
             (pin_id,owner_kind,owner_id,view_id,manifest_generation,base_id,
              delta_generation,expires_at,created_at)
             VALUES ('pin-a','reader','reader-a','view-a',1,'base-a',1,?1,?1)",
            [timestamp],
        )
        .unwrap();
    store
        .execute(
            "UPDATE views SET current_generation=1,resolution_state='converging',
                    resolution_base_id='base-a',resolution_delta_generation=1
             WHERE view_id='view-a'",
            [],
        )
        .unwrap();
    assert!(
        store
            .execute(
                "UPDATE views SET resolution_state='exact',resolution_exact_at=2
                 WHERE view_id='view-a'",
                [],
            )
            .is_err()
    );
    store
        .execute(
            "UPDATE views SET resolution_state='exact',resolution_exact_at=1
             WHERE view_id='view-a'",
            [],
        )
        .unwrap();
    assert!(
        store
            .execute(
                "UPDATE resolution_bases
                 SET state='building',file_bytes=NULL,file_sha256=NULL
                 WHERE base_id='base-a'",
                [],
            )
            .is_err()
    );
    assert!(store.execute_batch("PRAGMA foreign_key_check").is_ok());
}

#[test]
fn store_meta_seeds_only_schema_and_retention_defaults() {
    let conn = open_store();
    let rows = conn
        .prepare("SELECT key, value FROM store_meta ORDER BY key")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<BTreeMap<_, _>, _>>()
        .unwrap();

    assert_eq!(
        rows,
        BTreeMap::from([
            ("generation_state".to_string(), "serving".to_string()),
            ("retention_byte_ceiling".to_string(), "1.25".to_string()),
            ("retention_byte_target".to_string(), "1.20".to_string()),
            (
                "retention_physical_breach_limit".to_string(),
                "3".to_string(),
            ),
            ("retention_path_cap".to_string(), "24".to_string()),
            ("retention_window_days".to_string(), "7".to_string()),
            (
                "store_format_epoch".to_string(),
                STORE_FORMAT_EPOCH.to_string(),
            ),
            (
                "resolution_scope_journal_version".to_string(),
                "1".to_string(),
            ),
            (
                "store_sqlite_schema_version".to_string(),
                STORE_SQLITE_SCHEMA_VERSION.to_string(),
            ),
        ])
    );
}

#[test]
fn file_versions_use_non_reused_identity_and_ordered_completeness_stamps() {
    let conn = open_store();

    assert_eq!(
        table_columns(&conn, "file_versions"),
        vec![
            "version_id INTEGER",
            "path TEXT",
            "content_hash TEXT",
            "extraction_epoch INTEGER",
            "language TEXT",
            "content_bytes INTEGER",
            "line_count INTEGER",
            "metadata_json TEXT",
            "complete_l1 INTEGER",
            "complete_l2 INTEGER",
            "complete_l3 INTEGER",
        ]
    );

    conn.execute(
        "INSERT INTO file_versions
         (path, content_hash, extraction_epoch, language, content_bytes)
         VALUES ('src/a.rs', 'blake3:a', 1, 'rust', 10)",
        [],
    )
    .unwrap();
    conn.execute("DELETE FROM file_versions WHERE version_id = 1", [])
        .unwrap();
    conn.execute(
        "INSERT INTO file_versions
         (path, content_hash, extraction_epoch, language, content_bytes, complete_l1)
         VALUES ('src/b.rs', 'blake3:b', 1, 'rust', 20, 1)",
        [],
    )
    .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT version_id FROM file_versions WHERE path = 'src/b.rs'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        2
    );

    for statement in [
        "INSERT INTO file_versions (path, content_hash, extraction_epoch, language, content_bytes) VALUES ('bad-bytes', 'h', 1, 'rust', -1)",
        "INSERT INTO file_versions (path, content_hash, extraction_epoch, language, content_bytes, line_count) VALUES ('bad-lines', 'h', 1, 'rust', 1, -1)",
        "INSERT INTO file_versions (path, content_hash, extraction_epoch, language, content_bytes, complete_l1) VALUES ('bad-stamp', 'h', 1, 'rust', 1, 0)",
        "INSERT INTO file_versions (path, content_hash, extraction_epoch, language, content_bytes, complete_l2) VALUES ('bad-l2', 'h', 1, 'rust', 1, 2)",
        "INSERT INTO file_versions (path, content_hash, extraction_epoch, language, content_bytes, complete_l1, complete_l3) VALUES ('bad-l3', 'h', 1, 'rust', 1, 1, 3)",
    ] {
        assert!(conn.execute(statement, []).is_err(), "accepted {statement}");
    }
}

#[test]
fn every_per_version_local_reference_is_composite_and_every_child_roots_at_its_version() {
    let conn = open_store();
    let children = expected_child_tables();

    for child in &children {
        let foreign_keys = foreign_keys(&conn, child);
        assert!(
            foreign_keys.iter().any(|foreign_key| {
                foreign_key.target_table == "file_versions"
                    && foreign_key.from == ["version_id"]
                    && foreign_key.to == ["version_id"]
                    && foreign_key.on_delete == "CASCADE"
            }),
            "{child} has no direct cascading version root: {foreign_keys:?}"
        );

        for foreign_key in foreign_keys
            .iter()
            .filter(|foreign_key| children.contains(foreign_key.target_table.as_str()))
        {
            assert_eq!(
                foreign_key.from.first().map(String::as_str),
                Some("version_id"),
                "{child} has an unqualified local reference: {foreign_key:?}"
            );
            assert_eq!(
                foreign_key.to.first().map(String::as_str),
                Some("version_id"),
                "{child} targets an unqualified local identity: {foreign_key:?}"
            );
            assert_eq!(foreign_key.from.len(), 2, "{foreign_key:?}");
            assert_eq!(foreign_key.to.len(), 2, "{foreign_key:?}");
            assert_eq!(foreign_key.on_delete, "NO ACTION", "{foreign_key:?}");
        }
    }
}

#[test]
fn reference_site_identity_guard_is_version_qualified_and_compares_level() {
    let conn = open_store();
    for (path, hash) in [("src/a.rs", "blake3:a"), ("src/b.rs", "blake3:b")] {
        conn.execute(
            "INSERT INTO file_versions
             (path, content_hash, extraction_epoch, language, content_bytes)
             VALUES (?1, ?2, 1, 'rust', 10)",
            [path, hash],
        )
        .unwrap();
    }

    conn.execute(
        "INSERT INTO reference_sites
         (version_id, reference_site_id, path, language, is_exact, provenance, level)
         VALUES (1, 'site-a', 'src/a.rs', 'rust', 0, 'spanless', 1)",
        [],
    )
    .unwrap();
    assert_eq!(
        conn.execute(
            "INSERT INTO reference_sites
             (version_id, reference_site_id, path, language, is_exact, provenance, level)
             VALUES (1, 'site-a', 'src/a.rs', 'rust', 0, 'spanless', 2)",
            [],
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT level FROM reference_sites
             WHERE version_id = 1 AND reference_site_id = 'site-a'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );

    conn.execute(
        "INSERT INTO reference_sites
         (version_id, reference_site_id, path, language, is_exact, provenance, level)
         VALUES (2, 'site-a', 'src/b.rs', 'rust', 0, 'spanless', 2)",
        [],
    )
    .unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM reference_sites WHERE reference_site_id = 'site-a'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        2
    );
}

#[test]
fn explicit_secondary_indexes_are_exhaustive_and_classified() {
    let store = open_store();
    let coordinator = open_coordinator();

    assert_eq!(explicit_indexes(&store), expected_store_indexes());
    assert_eq!(
        explicit_indexes(&coordinator),
        expected_coordinator_indexes()
    );

    for name in explicit_indexes(&store)
        .keys()
        .chain(explicit_indexes(&coordinator).keys())
    {
        assert!(
            name.starts_with("idx_gc_")
                || name.starts_with("idx_read_")
                || name.starts_with("uidx_read_")
                || name.starts_with("uidx_coord_"),
            "unclassified explicit index {name}"
        );
    }
}

#[test]
fn existing_store_schema_ensure_repairs_symbol_id_index_without_changing_rows_or_identity() {
    let store = open_store();
    store
        .execute(
            "INSERT INTO file_versions
             (path, content_hash, extraction_epoch, language, content_bytes)
             VALUES ('src/a.rs', 'blake3:a', 1, 'rust', 1)",
            [],
        )
        .unwrap();
    store
        .execute(
            "INSERT INTO symbols
             (version_id, symbol_id, path, language, name, kind,
              start_line, start_column, end_line, end_column, start_byte, end_byte)
             VALUES (1, 'symbol-a', 'src/a.rs', 'rust', 'a', 'function',
                     1, 1, 1, 2, 0, 1)",
            [],
        )
        .unwrap();

    let before_version = user_version(&store);
    let before_meta = store
        .prepare("SELECT key, value FROM store_meta ORDER BY key")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let before_symbol: (i64, String, String, String, String, String) = store
        .query_row(
            "SELECT version_id, symbol_id, path, language, name, kind
             FROM symbols",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap();

    store
        .execute("DROP INDEX IF EXISTS idx_read_symbols_symbol", [])
        .unwrap();
    assert!(!explicit_indexes(&store).contains_key("idx_read_symbols_symbol"));

    create_store_schema(&store).unwrap();

    assert_eq!(
        explicit_indexes(&store)
            .get("idx_read_symbols_symbol")
            .cloned(),
        Some(vec!["symbol_id".to_string(), "version_id".to_string()])
    );
    assert_eq!(user_version(&store), before_version);
    assert_eq!(
        store
            .prepare("SELECT key, value FROM store_meta ORDER BY key")
            .unwrap()
            .query_map([], |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?
            )))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
        before_meta
    );
    assert_eq!(
        store
            .query_row(
                "SELECT version_id, symbol_id, path, language, name, kind
                 FROM symbols",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .unwrap(),
        before_symbol
    );

    create_store_schema(&store).unwrap();
    assert_eq!(
        explicit_indexes(&store)
            .get("idx_read_symbols_symbol")
            .cloned(),
        Some(vec!["symbol_id".to_string(), "version_id".to_string()])
    );
}

#[test]
fn symbol_id_read_index_supports_candidate_batches_and_symbol_exists() {
    let mut store = open_store();
    seed_symbol_lookup_fixture(&mut store);
    store
        .execute_batch(
            "CREATE TEMP TABLE _miller_visible_entries (
                 version_id INTEGER PRIMARY KEY
             );",
        )
        .unwrap();
    for version_id in 1..=128_i64 {
        store
            .execute(
                "INSERT INTO _miller_visible_entries(version_id) VALUES (?1)",
                [version_id],
            )
            .unwrap();
    }

    for candidate_count in [63_usize, 16] {
        let plan = candidate_lookup_plan(&store, candidate_count);
        assert_symbol_lookup_plan(&plan, &format!("candidate batch {candidate_count}"));
    }

    let symbol_exists_plan = store
        .prepare(
            "EXPLAIN QUERY PLAN
             SELECT 1 FROM symbols WHERE symbol_id = ?1 LIMIT 1",
        )
        .unwrap()
        .query_map(["symbol-0"], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    assert_symbol_lookup_plan(&symbol_exists_plan, "SymbolExists");
}

#[test]
fn ph2c_keeps_semantic_rows_in_immutable_base_and_delta_artifacts() {
    let store = open_store();
    let coordinator = open_coordinator();

    for forbidden in [
        "pending_resolutions",
        "identifier_resolutions",
        "reader_pins",
        "pins",
    ] {
        assert!(!ordinary_tables(&store).contains(forbidden));
        assert!(!ordinary_tables(&coordinator).contains(forbidden));
    }

    let views_sql: String = store
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'views'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(views_sql.contains("'converging'"));
    assert!(views_sql.contains("'exact'"));
}

#[test]
fn manifest_statuses_and_gc_roots_are_enforced() {
    let conn = open_store();
    conn.execute(
        "INSERT INTO file_versions
         (path, content_hash, extraction_epoch, language, content_bytes)
         VALUES ('src/a.rs', 'blake3:a', 1, 'rust', 10)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO views (view_id, root, created_at, updated_at)
         VALUES ('view-a', '/repo', '2026-08-07T12:00:00Z', '2026-08-07T12:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO manifests (view_id, generation, manifest_hash, request_id, created_at)
         VALUES ('view-a', 1, 'hash-a', 'request-a', '2026-08-07T12:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO manifest_entries
         (view_id, generation, path, language, version_id, status, observed_content_hash, indexed_at)
         VALUES ('view-a', 1, 'src/a.rs', 'rust', 1, 'indexed', 'blake3:a', '2026-08-07T12:00:00Z')",
        [],
    )
    .unwrap();

    assert!(
        conn.execute("DELETE FROM file_versions WHERE version_id = 1", [])
            .is_err()
    );
    assert!(
        conn.execute(
            "INSERT INTO manifest_entries
             (view_id, generation, path, language, status, observed_content_hash, indexed_at)
             VALUES ('view-a', 1, 'bad.rs', 'rust', 'indexed', 'h', '2026-08-07T12:00:00Z')",
            [],
        )
        .is_err()
    );
}

#[test]
fn store_timestamps_accept_utc_rfc3339_fractions_and_reject_offsets() {
    let conn = open_store();
    conn.execute(
        "INSERT INTO views (view_id, root, created_at, updated_at)
         VALUES ('fractional', '/repo',
                 '2026-08-07T12:00:00.123456789Z',
                 '2026-08-07T12:00:00.1Z')",
        [],
    )
    .unwrap();

    assert!(
        conn.execute(
            "INSERT INTO views (view_id, root, created_at, updated_at)
             VALUES ('offset', '/repo',
                     '2026-08-07T12:00:00+00:00',
                     '2026-08-07T12:00:00Z')",
            [],
        )
        .is_err()
    );
}

#[test]
fn terminal_effect_and_chunk_log_ownership_are_unique() {
    let conn = open_store();
    let timestamp = "2026-08-07T12:00:00Z";

    conn.execute(
        "INSERT INTO store_log (request_id, event_kind, terminal, payload_json, created_at)
         VALUES ('request-a', 'complete', 1, '{}', ?1)",
        [timestamp],
    )
    .unwrap();
    assert!(
        conn.execute(
            "INSERT INTO store_log (request_id, event_kind, terminal, payload_json, created_at)
             VALUES ('request-a', 'complete-again', 1, '{}', ?1)",
            [timestamp],
        )
        .is_err()
    );

    conn.execute(
        "INSERT INTO request_chunks
         (request_id, chunk_index, store_log_sequence, payload_json, created_at)
         VALUES ('request-b', 0, 1, '{}', ?1)",
        [timestamp],
    )
    .unwrap();
    assert!(
        conn.execute(
            "INSERT INTO request_chunks
             (request_id, chunk_index, store_log_sequence, payload_json, created_at)
             VALUES ('request-c', 0, 1, '{}', ?1)",
            [timestamp],
        )
        .is_err()
    );
}

#[test]
fn coordinator_request_state_and_writer_lease_coherence_are_enforced() {
    let conn = open_coordinator();
    conn.execute(
        "INSERT INTO requests
         (request_id, idempotency_key, kind, payload_json, state, requester_id,
          created_at, updated_at)
         VALUES ('request-a', 'key-a', 'update', '{}', 'queued', 'caller', 1, 1)",
        [],
    )
    .unwrap();

    assert!(
        conn.execute(
            "INSERT INTO requests
             (request_id, idempotency_key, kind, payload_json, state, requester_id,
              created_at, updated_at)
             VALUES ('request-b', 'key-a', 'update', '{}', 'queued', 'caller', 1, 1)",
            [],
        )
        .is_err()
    );
    assert!(
        conn.execute(
            "INSERT INTO requests
             (request_id, idempotency_key, kind, payload_json, state, requester_id,
              claim_owner, created_at, updated_at)
             VALUES ('bad-claim', 'key-b', 'update', '{}', 'queued', 'caller', 'owner', 1, 1)",
            [],
        )
        .is_err()
    );
    assert!(
        conn.execute(
            "INSERT INTO requests
             (request_id, idempotency_key, kind, payload_json, state, requester_id,
              created_at, updated_at)
             VALUES ('bad-commit', 'key-c', 'update', '{}', 'committed', 'caller', 1, 1)",
            [],
        )
        .is_err()
    );

    conn.execute(
        "INSERT INTO writer_lease
         (resource, holder_id, holder_version, holder_pid, heartbeat_at, expires_at, fencing_token)
         VALUES ('store-writer', 'holder', '2.30.0', 42, 1, 2, 1)",
        [],
    )
    .unwrap();
    assert!(
        conn.execute(
            "INSERT INTO writer_lease
             (resource, holder_id, holder_version, holder_pid, heartbeat_at, expires_at, fencing_token)
             VALUES ('other', 'holder', '2.30.0', 42, 1, 2, 1)",
            [],
        )
        .is_err()
    );
}

#[test]
fn unknown_newer_schema_versions_are_typed_refusals() {
    let store = Connection::open_in_memory().unwrap();
    store.pragma_update(None, "user_version", 3).unwrap();
    assert!(matches!(
        create_store_schema(&store),
        Err(StoreSchemaError::NewerSchema {
            database: "store.db",
            found: 3,
            supported: 2,
        })
    ));

    let coordinator = Connection::open_in_memory().unwrap();
    coordinator.pragma_update(None, "user_version", 3).unwrap();
    assert!(matches!(
        create_coordinator_schema(&coordinator),
        Err(StoreSchemaError::NewerSchema {
            database: "coord.db",
            found: 3,
            supported: 2,
        })
    ));
}

fn open_store() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    create_store_schema(&conn).unwrap();
    conn
}

fn open_coordinator() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    create_coordinator_schema(&conn).unwrap();
    conn
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

fn ordinary_tables(conn: &Connection) -> BTreeSet<String> {
    conn.prepare(
        "SELECT name FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
    )
    .unwrap()
    .query_map([], |row| row.get(0))
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn expected_store_tables() -> BTreeSet<String> {
    [
        "complexity_metrics",
        "file_versions",
        "identifiers",
        "language_capabilities",
        "language_capability_fixtures",
        "language_capability_gaps",
        "literals",
        "manifest_entries",
        "manifests",
        "parse_diagnostics",
        "parser_inventory",
        "pending_relationships",
        "reference_sites",
        "relationships",
        "request_chunks",
        "resolution_base_versions",
        "resolution_bases",
        "resolution_deltas",
        "resolution_identifier_deltas",
        "resolution_pending_deltas",
        "resolution_pins",
        "resolution_scope_batches",
        "resolution_scope_journal",
        "resolution_scope_state",
        "source_regions",
        "store_log",
        "store_meta",
        "structural_facts",
        "symbol_annotations",
        "symbols",
        "type_argument_usages",
        "type_arguments",
        "type_facts",
        "views",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn expected_coordinator_tables() -> BTreeSet<String> {
    [
        "consumer_cursors",
        "family_allocator_marks",
        "maintenance_intent",
        "request_receipts",
        "requests",
        "writer_lease",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn expected_child_tables() -> BTreeSet<String> {
    [
        "complexity_metrics",
        "identifiers",
        "literals",
        "parse_diagnostics",
        "pending_relationships",
        "reference_sites",
        "relationships",
        "source_regions",
        "structural_facts",
        "symbol_annotations",
        "symbols",
        "type_argument_usages",
        "type_arguments",
        "type_facts",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn assert_all_tables_strict(conn: &Connection) {
    let strict = conn
        .prepare(
            "SELECT name, strict FROM pragma_table_list
             WHERE schema = 'main' AND type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .unwrap()
        .collect::<Result<BTreeMap<_, _>, _>>()
        .unwrap();
    assert!(strict.values().all(|value| *value == 1), "{strict:?}");
}

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    conn.prepare(&format!("PRAGMA table_info({table})"))
        .unwrap()
        .query_map([], |row| {
            Ok(format!(
                "{} {}",
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn seed_symbol_lookup_fixture(store: &mut Connection) {
    let transaction = store.transaction().unwrap();
    for version_id in 1..=256_i64 {
        transaction
            .execute(
                "INSERT INTO file_versions
                 (path, content_hash, extraction_epoch, language, content_bytes)
                 VALUES (?1, ?2, 1, 'rust', 1)",
                params![
                    format!("src/file-{version_id}.rs"),
                    format!("blake3:{version_id}")
                ],
            )
            .unwrap();
        for symbol_index in 0..128_i64 {
            transaction
                .execute(
                    "INSERT INTO symbols
                     (version_id, symbol_id, path, language, name, kind,
                      start_line, start_column, end_line, end_column, start_byte, end_byte)
                     VALUES (?1, ?2, ?3, 'rust', ?2, 'function', 1, 1, 1, 2, 0, 1)",
                    params![
                        version_id,
                        format!("symbol-{symbol_index}"),
                        format!("src/file-{version_id}.rs")
                    ],
                )
                .unwrap();
        }
    }
    transaction.commit().unwrap();
}

fn candidate_lookup_plan(store: &Connection, candidate_count: usize) -> String {
    let values = (1..=candidate_count)
        .map(|index| format!("(?{index})"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "EXPLAIN QUERY PLAN
         WITH candidate_ids(id) AS (VALUES {values})
         SELECT candidate_ids.id, s.version_id
         FROM candidate_ids
         JOIN _miller_visible_entries AS visible
         JOIN main.symbols AS s
           ON s.version_id = visible.version_id
          AND s.symbol_id = candidate_ids.id"
    );
    let ids = (0..candidate_count)
        .map(|index| format!("symbol-{index}"))
        .collect::<Vec<_>>();
    store
        .prepare(&sql)
        .unwrap()
        .query_map(rusqlite::params_from_iter(ids.iter()), |row| {
            row.get::<_, String>(3)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n")
}

fn assert_symbol_lookup_plan(plan: &str, label: &str) {
    assert!(
        plan.contains("idx_read_symbols_symbol"),
        "{label} did not use idx_read_symbols_symbol. Plan:\n{plan}"
    );
    assert!(
        !plan.contains("AUTOMATIC COVERING INDEX"),
        "{label} used an automatic index. Plan:\n{plan}"
    );
    assert!(
        !plan
            .lines()
            .any(|line| line.contains("SCAN s") || line.contains("SCAN symbols")),
        "{label} scanned symbols. Plan:\n{plan}"
    );
}

#[derive(Debug)]
struct ForeignKey {
    target_table: String,
    from: Vec<String>,
    to: Vec<String>,
    on_delete: String,
}

fn foreign_keys(conn: &Connection, table: &str) -> Vec<ForeignKey> {
    let rows = conn
        .prepare(&format!("PRAGMA foreign_key_list({table})"))
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(6)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let mut grouped = BTreeMap::<i64, ForeignKey>::new();
    for (id, sequence, target_table, from, to, on_delete) in rows {
        let entry = grouped.entry(id).or_insert_with(|| ForeignKey {
            target_table,
            from: Vec::new(),
            to: Vec::new(),
            on_delete,
        });
        assert_eq!(entry.from.len(), sequence as usize);
        entry.from.push(from);
        entry.to.push(to);
    }
    grouped.into_values().collect()
}

fn explicit_indexes(conn: &Connection) -> BTreeMap<String, Vec<String>> {
    let names = conn
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND sql IS NOT NULL
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    names
        .into_iter()
        .map(|name| {
            let columns = conn
                .prepare(&format!("PRAGMA index_info({name})"))
                .unwrap()
                .query_map([], |row| row.get::<_, String>(2))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            (name, columns)
        })
        .collect()
}

fn expected_store_indexes() -> BTreeMap<String, Vec<String>> {
    [
        (
            "idx_gc_complexity_metrics_export_order",
            "version_id,path,start_byte,end_byte,scope,symbol_id,complexity_metric_id",
        ),
        (
            "idx_gc_complexity_metrics_file_scope",
            "version_id,scope,start_byte",
        ),
        ("idx_gc_diagnostics_path", "version_id,path"),
        (
            "idx_gc_resolution_identifier_deltas_version",
            "version_id,view_id,delta_generation,identifier_id",
        ),
        (
            "idx_gc_resolution_pending_deltas_version",
            "version_id,view_id,delta_generation,pending_relationship_id",
        ),
        (
            "idx_gc_source_regions_export_order",
            "version_id,path,start_byte,end_byte,kind,source_region_id",
        ),
        (
            "idx_gc_source_regions_file_span",
            "version_id,start_byte,end_byte",
        ),
        (
            "idx_gc_structural_facts_export_order",
            "version_id,path,start_byte,end_byte,pattern_id,capture_name,structural_fact_id",
        ),
        (
            "idx_gc_structural_facts_file_span",
            "version_id,start_byte,end_byte",
        ),
        ("idx_gc_symbol_annotations_symbol", "version_id,symbol_id"),
        ("idx_gc_symbols_is_test", "version_id,is_test"),
        ("idx_gc_symbols_path", "version_id,path"),
        ("idx_gc_symbols_test_container", "version_id,test_container"),
        ("idx_gc_symbols_test_lifecycle", "version_id,test_lifecycle"),
        (
            "idx_gc_type_arguments_parent",
            "version_id,parent_type_argument_id",
        ),
        ("idx_gc_type_arguments_usage", "version_id,usage_id"),
        (
            "idx_read_complexity_metrics_scope_language",
            "scope,language,path,version_id",
        ),
        ("idx_read_complexity_metrics_symbol", "symbol_id,version_id"),
        (
            "idx_read_identifiers_containing",
            "containing_symbol_id,version_id",
        ),
        (
            "idx_read_identifiers_locator_line",
            "version_id,name,start_line,identifier_id",
        ),
        (
            "idx_read_identifiers_locator_span",
            "version_id,name,start_byte,end_byte,identifier_id",
        ),
        ("idx_read_identifiers_name_kind", "name,kind,version_id"),
        (
            "idx_read_identifiers_reference_site",
            "reference_site_id,version_id",
        ),
        (
            "idx_read_language_capability_gaps_language",
            "extraction_epoch,language",
        ),
        (
            "idx_read_literals_containing_symbol",
            "containing_symbol_id,version_id",
        ),
        (
            "idx_read_manifest_entries_version",
            "version_id,view_id,generation",
        ),
        (
            "idx_read_resolution_base_versions_version",
            "version_id,base_id",
        ),
        (
            "idx_read_resolution_deltas_base",
            "base_id,view_id,delta_generation",
        ),
        (
            "idx_read_resolution_identifier_deltas_target",
            "target_version_id,target_symbol_id,view_id,delta_generation",
        ),
        (
            "idx_read_resolution_pending_deltas_target",
            "target_version_id,target_symbol_id,view_id,delta_generation",
        ),
        (
            "idx_read_resolution_pins_bound",
            "view_id,manifest_generation,base_id,delta_generation",
        ),
        (
            "idx_read_resolution_pins_owner_expiry",
            "owner_kind,owner_id,expires_at,pin_id",
        ),
        (
            "idx_read_resolution_scope_batches_view",
            "view_id,transition_id",
        ),
        (
            "idx_read_resolution_scope_journal_versions",
            "old_version_id,new_version_id,transition_id",
        ),
        (
            "idx_read_resolution_scope_journal_kind",
            "change_kind,transition_id,path",
        ),
        (
            "idx_read_pending_caller_scope",
            "caller_scope_symbol_id,version_id",
        ),
        ("idx_read_pending_from", "from_symbol_id,version_id"),
        (
            "idx_read_pending_reference_site",
            "reference_site_id,version_id",
        ),
        (
            "idx_read_pending_terminal",
            "target_terminal_name,version_id",
        ),
        (
            "idx_read_reference_sites_containing_symbol",
            "containing_symbol_id,version_id",
        ),
        ("idx_read_relationships_from", "from_symbol_id,version_id"),
        ("idx_read_relationships_kind", "kind,version_id"),
        (
            "idx_read_relationships_reference_site",
            "reference_site_id,version_id",
        ),
        ("idx_read_relationships_to", "to_symbol_id,version_id"),
        ("idx_read_source_regions_kind", "kind,version_id,start_byte"),
        (
            "idx_read_source_regions_symbol",
            "containing_symbol_id,version_id",
        ),
        ("idx_read_store_log_request", "request_id,sequence"),
        (
            "idx_read_structural_facts_pattern_language_path",
            "pattern_id,language,path,version_id",
        ),
        (
            "idx_read_structural_facts_symbol",
            "containing_symbol_id,version_id",
        ),
        ("idx_read_symbols_name_kind", "name,kind,version_id"),
        ("idx_read_symbols_parent", "parent_symbol_id,version_id"),
        ("idx_read_symbols_symbol", "symbol_id,version_id"),
        (
            "idx_read_type_argument_usages_identifier",
            "identifier_id,version_id",
        ),
        (
            "uidx_read_file_versions_identity",
            "path,content_hash,extraction_epoch",
        ),
        ("uidx_read_manifests_hash", "view_id,manifest_hash"),
        (
            "uidx_read_resolution_bases_identity",
            "manifest_hash,resolver_output_epoch",
        ),
        (
            "uidx_read_request_chunks_log_sequence",
            "store_log_sequence",
        ),
        ("uidx_read_store_log_terminal_request", "request_id"),
    ]
    .into_iter()
    .map(|(name, columns)| {
        (
            name.to_string(),
            columns.split(',').map(str::to_string).collect(),
        )
    })
    .collect()
}

fn expected_coordinator_indexes() -> BTreeMap<String, Vec<String>> {
    [
        ("idx_read_requests_queue", "state,created_at,request_id"),
        (
            "idx_read_requests_stale",
            "state,claim_heartbeat_at,request_id",
        ),
        ("uidx_read_requests_idempotency_key", "idempotency_key"),
        ("uidx_coord_one_claimed_resolve", "kind"),
    ]
    .into_iter()
    .map(|(name, columns)| {
        (
            name.to_string(),
            columns.split(',').map(str::to_string).collect(),
        )
    })
    .collect()
}

fn catalog_hash(conn: &Connection) -> String {
    let catalog = conn
        .prepare(
            "SELECT type, name, tbl_name, sql
             FROM sqlite_master
             WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%'
             ORDER BY type, name",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(format!(
                "{}|{}|{}|{}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                compact_whitespace(&row.get::<_, String>(3)?),
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    format!("{:x}", Sha256::digest(catalog.as_bytes()))
}

fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn authority_hash(key: &str) -> &str {
    AUTHORITY
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}: ")))
        .unwrap_or_else(|| panic!("missing {key} in sqlite-store-schema-v1.md"))
}
