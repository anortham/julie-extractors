use std::collections::{BTreeMap, BTreeSet};

use julie_extract_artifact::store::{
    STORE_FORMAT_EPOCH, STORE_SQLITE_SCHEMA_VERSION, StoreSchemaError, create_coordinator_schema,
    create_store_schema,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

const AUTHORITY: &str = include_str!("../../../docs/contracts/sqlite-store-schema-v1.md");

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
fn schemas_are_independent_strict_idempotent_version_one_catalogs() {
    assert_eq!(STORE_SQLITE_SCHEMA_VERSION, 1);
    assert_eq!(STORE_FORMAT_EPOCH, 1);

    let store = open_store();
    let coordinator = open_coordinator();

    create_store_schema(&store).unwrap();
    create_coordinator_schema(&coordinator).unwrap();

    assert_eq!(user_version(&store), 1);
    assert_eq!(user_version(&coordinator), 1);
    assert_eq!(ordinary_tables(&store), expected_store_tables());
    assert_eq!(ordinary_tables(&coordinator), expected_coordinator_tables());
    assert_all_tables_strict(&store);
    assert_all_tables_strict(&coordinator);
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
            ("retention_byte_ceiling".to_string(), "1.25".to_string()),
            ("retention_byte_target".to_string(), "1.20".to_string()),
            ("retention_path_cap".to_string(), "24".to_string()),
            ("retention_window_days".to_string(), "7".to_string()),
            (
                "store_format_epoch".to_string(),
                STORE_FORMAT_EPOCH.to_string(),
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
                || name.starts_with("uidx_read_"),
            "unclassified explicit index {name}"
        );
    }
}

#[test]
fn ph2b_excludes_resolution_and_reader_pin_state() {
    let store = open_store();
    let coordinator = open_coordinator();

    for forbidden in [
        "pending_resolutions",
        "identifier_resolutions",
        "resolution_bases",
        "resolution_deltas",
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
    assert!(!views_sql.contains("'ready'"));
    assert!(!views_sql.contains("'exact'"));
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
         (view_id, generation, path, version_id, status, observed_content_hash, indexed_at)
         VALUES ('view-a', 1, 'src/a.rs', 1, 'indexed', 'blake3:a', '2026-08-07T12:00:00Z')",
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
             (view_id, generation, path, status, observed_content_hash, indexed_at)
             VALUES ('view-a', 1, 'bad.rs', 'indexed', 'h', '2026-08-07T12:00:00Z')",
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
    store.pragma_update(None, "user_version", 2).unwrap();
    assert!(matches!(
        create_store_schema(&store),
        Err(StoreSchemaError::NewerSchema {
            database: "store.db",
            found: 2,
            supported: 1,
        })
    ));

    let coordinator = Connection::open_in_memory().unwrap();
    coordinator.pragma_update(None, "user_version", 2).unwrap();
    assert!(matches!(
        create_coordinator_schema(&coordinator),
        Err(StoreSchemaError::NewerSchema {
            database: "coord.db",
            found: 2,
            supported: 1,
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
    ["requests", "writer_lease"]
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
