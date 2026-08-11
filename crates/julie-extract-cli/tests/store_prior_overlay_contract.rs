#[path = "../src/store/prior_overlay.rs"]
mod prior_overlay;

use std::fs;

use julie_extract_artifact::store::{
    ResolutionBaseWriter, ResolutionIdentifierRow, ResolutionPendingRow, ResolutionScopeState,
    StoreLayout, ensure_resolution_scope_feature,
};
use prior_overlay::{
    PriorOverlayAccess, PriorOverlayFallback, PriorOverlayKey, PriorOverlayReader,
};
use rusqlite::{Connection, params};
use tempfile::TempDir;

const NOW: &str = "2026-08-11T18:00:00Z";
const FAMILY_ID: &str = "family-prior-overlay";
const BINARY_VERSION: &str = "2.30.0";
const VIEW_ID: &str = "view-a";
const BASE_ID: &str = "base-a";
const PREDECESSOR_HASH: &str = "manifest-predecessor";
const BASE_MANIFEST_HASH: &str = "manifest-base";
const RESOLVER_EPOCH: i64 = 7;

#[test]
fn replacement_and_tombstone_precedence_preserves_version_qualified_keys() {
    let fixture = Fixture::new();
    let reader = fixture.reader();

    let v1_collision = ready(reader.identifier(fixture.v1, "identifier-shared").unwrap()).unwrap();
    let v2_collision = ready(reader.identifier(fixture.v2, "identifier-shared").unwrap()).unwrap();
    let base_only = ready(reader.identifier(fixture.v1, "identifier-base").unwrap()).unwrap();
    let replacement = ready(reader.pending(fixture.v1, "pending-replaced").unwrap()).unwrap();
    let tombstone = ready(reader.pending(fixture.v1, "pending-removed").unwrap());
    let delta_only = ready(reader.pending(fixture.v2, "pending-new").unwrap()).unwrap();

    assert_eq!(v1_collision.method.as_deref(), Some("delta-v1"));
    assert_eq!(v2_collision.method.as_deref(), Some("delta-v2"));
    assert_eq!(base_only.method.as_deref(), Some("base"));
    assert_eq!(replacement.method, "delta-pending");
    assert_eq!(tombstone, None);
    assert_eq!(delta_only.method, "delta-new");
}

#[test]
fn by_name_and_by_file_reads_are_bounded_and_deterministic() {
    let fixture = Fixture::new();
    let reader = fixture.reader();

    let first = ready(
        reader
            .identifiers_by_names(&["Collision"], None, 1)
            .unwrap(),
    );
    assert_eq!(first.rows.len(), 1);
    assert_eq!(first.rows[0].version_id, fixture.v1);
    assert_eq!(first.rows[0].identifier_id, "identifier-shared");
    assert_eq!(
        first.next,
        Some(PriorOverlayKey::new(fixture.v1, "identifier-shared"))
    );

    let second = ready(
        reader
            .identifiers_by_names(&["Collision"], first.next.as_ref(), 1)
            .unwrap(),
    );
    assert_eq!(second.rows.len(), 1);
    assert_eq!(second.rows[0].version_id, fixture.v2);
    assert_eq!(second.next, None);

    let by_file = ready(reader.identifiers_by_files(&[fixture.v2], None, 8).unwrap());
    assert_eq!(
        by_file
            .rows
            .iter()
            .map(|row| row.identifier_id.as_str())
            .collect::<Vec<_>>(),
        vec!["identifier-new", "identifier-shared"]
    );

    let pending = ready(reader.pending_by_names(&["Replaced"], None, 8).unwrap());
    assert_eq!(pending.rows.len(), 1);
    assert_eq!(pending.rows[0].pending_relationship_id, "pending-replaced");
    assert_eq!(pending.rows[0].method, "delta-pending");

    let pending_file = ready(reader.pending_by_files(&[fixture.v1], None, 8).unwrap());
    assert_eq!(
        pending_file
            .rows
            .iter()
            .map(|row| row.pending_relationship_id.as_str())
            .collect::<Vec<_>>(),
        vec!["pending-base", "pending-replaced"]
    );
}

#[test]
fn pending_cursor_walk_emits_visible_rows_once_across_tombstones() {
    let fixture = Fixture::new();
    let reader = fixture.reader();
    let mut after = None;
    let mut ids = Vec::new();

    loop {
        let page = ready(
            reader
                .pending_by_files(&[fixture.v1], after.as_ref(), 1)
                .unwrap(),
        );
        ids.extend(
            page.rows
                .iter()
                .map(|row| row.pending_relationship_id.clone()),
        );
        let Some(next) = page.next else {
            break;
        };
        after = Some(next);
    }

    assert_eq!(ids, vec!["pending-base", "pending-replaced"]);
}

#[test]
fn missing_base_file_returns_typed_full_fallback() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.layout.bases_dir().join("base-a.db")).unwrap();

    assert!(matches!(
        PriorOverlayReader::open(&fixture.layout, &fixture.state).unwrap(),
        PriorOverlayAccess::FullFallback(PriorOverlayFallback::BaseFileMissing { .. })
    ));
}

#[test]
fn incoherent_delta_counts_return_typed_full_fallback() {
    let fixture = Fixture::new();
    Connection::open(fixture.layout.store_db())
        .unwrap()
        .execute(
            "UPDATE resolution_deltas SET identifier_replacements=99
             WHERE view_id=?1 AND delta_generation=1",
            [VIEW_ID],
        )
        .unwrap();

    assert!(matches!(
        PriorOverlayReader::open(&fixture.layout, &fixture.state).unwrap(),
        PriorOverlayAccess::FullFallback(PriorOverlayFallback::DeltaRowCountMismatch {
            table: "resolution_identifier_deltas",
            expected: 99,
            found: 3,
        })
    ));
}

#[test]
fn mismatched_base_root_sets_return_typed_full_fallback() {
    let fixture = Fixture::new();
    Connection::open(fixture.layout.store_db())
        .unwrap()
        .execute(
            "INSERT INTO resolution_base_versions(base_id,version_id) VALUES (?1,?2)",
            params![BASE_ID, fixture.v2],
        )
        .unwrap();

    assert!(matches!(
        PriorOverlayReader::open(&fixture.layout, &fixture.state).unwrap(),
        PriorOverlayAccess::FullFallback(PriorOverlayFallback::BaseCatalogIncoherent {
            base_id,
            detail,
        }) if base_id == BASE_ID && detail.contains("root set")
    ));
}

#[test]
fn missing_source_and_overlay_rows_return_typed_full_fallback() {
    let missing_source = Fixture::new();
    Connection::open(missing_source.layout.store_db())
        .unwrap()
        .execute(
            "DELETE FROM identifiers WHERE version_id=?1 AND identifier_id='identifier-new'",
            [missing_source.v2],
        )
        .unwrap();
    assert!(matches!(
        PriorOverlayReader::open(&missing_source.layout, &missing_source.state).unwrap(),
        PriorOverlayAccess::FullFallback(PriorOverlayFallback::SourceRowMissing {
            table: "identifiers",
            local_id,
            ..
        }) if local_id == "identifier-new"
    ));

    let missing_overlay = Fixture::new();
    insert_identifier(
        &Connection::open(missing_overlay.layout.store_db()).unwrap(),
        missing_overlay.v1,
        "identifier-uncovered",
        "Uncovered",
        "src/a.rs",
    );
    assert!(matches!(
        PriorOverlayReader::open(&missing_overlay.layout, &missing_overlay.state).unwrap(),
        PriorOverlayAccess::FullFallback(PriorOverlayFallback::OverlayRowMissing {
            table: "identifier_resolutions",
            local_id,
            ..
        }) if local_id == "identifier-uncovered"
    ));
}

fn ready<T>(access: PriorOverlayAccess<T>) -> T {
    match access {
        PriorOverlayAccess::Ready(value) => value,
        PriorOverlayAccess::FullFallback(fallback) => panic!("unexpected fallback: {fallback:?}"),
    }
}

struct Fixture {
    _temp: TempDir,
    layout: StoreLayout,
    state: ResolutionScopeState,
    v1: i64,
    v2: i64,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let layout =
            StoreLayout::create(temp.path().join("family"), FAMILY_ID, BINARY_VERSION).unwrap();
        let connection = Connection::open(layout.store_db()).unwrap();
        connection.execute("PRAGMA foreign_keys=ON", []).unwrap();
        ensure_resolution_scope_feature(&connection).unwrap();
        connection
            .execute(
                "INSERT INTO views(view_id,root,created_at,updated_at)
                 VALUES (?1,'/repo',?2,?2)",
                params![VIEW_ID, NOW],
            )
            .unwrap();
        let v1 = insert_version(&connection, "src/a.rs", "hash-a");
        let v2 = insert_version(&connection, "src/b.rs", "hash-b");
        for (generation, hash) in [(1, PREDECESSOR_HASH), (2, "manifest-current")] {
            connection
                .execute(
                    "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
                     VALUES (?1,?2,?3,?4,?5)",
                    params![
                        VIEW_ID,
                        generation,
                        hash,
                        format!("request-{generation}"),
                        NOW
                    ],
                )
                .unwrap();
        }
        for (generation, path, version, hash) in [
            (1, "src/a.rs", v1, "hash-a"),
            (1, "src/b.rs", v2, "hash-b"),
            (2, "src/a.rs", v1, "hash-a"),
            (2, "src/b.rs", v2, "hash-b"),
        ] {
            connection
                .execute(
                    "INSERT INTO manifest_entries
                     (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
                     VALUES (?1,?2,?3,'rust',?4,'indexed',?5,?6)",
                    params![VIEW_ID, generation, path, version, hash, NOW],
                )
                .unwrap();
        }
        insert_symbol(&connection, v1, "target-base", "src/a.rs");
        insert_symbol(&connection, v1, "target-delta", "src/a.rs");
        insert_symbol(&connection, v2, "target-base", "src/b.rs");
        insert_symbol(&connection, v2, "target-v2", "src/b.rs");
        insert_identifier(&connection, v1, "identifier-base", "Alpha", "src/a.rs");
        insert_identifier(
            &connection,
            v1,
            "identifier-shared",
            "Collision",
            "src/a.rs",
        );
        insert_identifier(&connection, v2, "identifier-new", "New", "src/b.rs");
        insert_identifier(
            &connection,
            v2,
            "identifier-shared",
            "Collision",
            "src/b.rs",
        );
        insert_pending(&connection, v1, "pending-base", "Base", "src/a.rs");
        insert_pending(&connection, v1, "pending-removed", "Removed", "src/a.rs");
        insert_pending(&connection, v1, "pending-replaced", "Replaced", "src/a.rs");
        insert_pending(&connection, v2, "pending-new", "New", "src/b.rs");

        let base_path = layout.bases_dir().join("base-a.db");
        let mut base =
            ResolutionBaseWriter::new(&base_path, BASE_MANIFEST_HASH, RESOLVER_EPOCH).unwrap();
        base.push_source_version(v1).unwrap();
        base.push_identifier_resolution(identifier_row(v1, "identifier-base", "base"))
            .unwrap();
        base.push_identifier_resolution(identifier_row(v1, "identifier-shared", "base-shared"))
            .unwrap();
        for id in ["pending-base", "pending-removed", "pending-replaced"] {
            base.push_pending_resolution(pending_row(v1, id, "base"))
                .unwrap();
        }
        let base_identity = base.finish_with_target_lookup(|_, _| Ok(true)).unwrap();
        connection
            .execute(
                "INSERT INTO resolution_bases
                 (base_id,manifest_hash,resolver_output_epoch,state,relative_path,
                  identifier_count,pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
                 VALUES (?1,?2,?3,'ready','bases/base-a.db',2,3,?4,?5,'request-base',?6,?6)",
                params![
                    BASE_ID,
                    BASE_MANIFEST_HASH,
                    RESOLVER_EPOCH,
                    i64::try_from(base_identity.file_bytes).unwrap(),
                    base_identity.file_sha256,
                    NOW
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO resolution_base_versions(base_id,version_id) VALUES (?1,?2)",
                params![BASE_ID, v1],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO resolution_deltas
                 (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
                  resolver_output_epoch,identifier_replacements,pending_replacements,
                  pending_tombstones,exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
                 VALUES (?1,1,?2,1,?3,?4,3,2,1,0,0,'[]','request-delta',?5)",
                params![VIEW_ID, BASE_ID, PREDECESSOR_HASH, RESOLVER_EPOCH, NOW],
            )
            .unwrap();
        for (version, id, method) in [
            (v1, "identifier-shared", "delta-v1"),
            (v2, "identifier-new", "delta-new"),
            (v2, "identifier-shared", "delta-v2"),
        ] {
            connection
                .execute(
                    "INSERT INTO resolution_identifier_deltas
                     (view_id,delta_generation,version_id,identifier_id,target_version_id,
                      target_symbol_id,tier,confidence,method,outcome,candidates)
                     VALUES (?1,1,?2,?3,?2,?4,2,0.9,?5,'resolved',1)",
                    params![
                        VIEW_ID,
                        version,
                        id,
                        if version == v1 {
                            "target-delta"
                        } else {
                            "target-v2"
                        },
                        method
                    ],
                )
                .unwrap();
        }
        for (version, id, operation, target, method) in [
            (v1, "pending-removed", "tombstone", None, None),
            (
                v1,
                "pending-replaced",
                "replace",
                Some("target-delta"),
                Some("delta-pending"),
            ),
            (
                v2,
                "pending-new",
                "replace",
                Some("target-v2"),
                Some("delta-new"),
            ),
        ] {
            connection
                .execute(
                    "INSERT INTO resolution_pending_deltas
                     (view_id,delta_generation,version_id,pending_relationship_id,operation,
                      target_version_id,target_symbol_id,tier,confidence,method)
                     VALUES (?1,1,?2,?3,?4,
                       CASE WHEN ?4='replace' THEN ?2 END,?5,
                       CASE WHEN ?4='replace' THEN 2 END,
                       CASE WHEN ?4='replace' THEN 0.9 END,?6)",
                    params![VIEW_ID, version, id, operation, target, method],
                )
                .unwrap();
        }
        connection
            .execute(
                "INSERT INTO resolution_scope_batches
                 (transition_id,view_id,previous_transition_id,from_manifest_generation,
                  from_manifest_hash,to_manifest_generation,to_manifest_hash,scope_usable,
                  predecessor_manifest_generation,predecessor_manifest_hash,base_id,
                  delta_generation,resolver_output_epoch,change_count,change_hash,request_id,completed_at)
                 VALUES (1,?1,NULL,1,?2,2,'manifest-current',1,1,?2,?3,1,?4,0,
                         'sha256:empty','request-scope',?5)",
                params![VIEW_ID, PREDECESSOR_HASH, BASE_ID, RESOLVER_EPOCH, NOW],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO resolution_scope_state
                 (view_id,predecessor_manifest_generation,predecessor_manifest_hash,base_id,
                  delta_generation,resolver_output_epoch,current_manifest_generation,
                  current_manifest_hash,journal_through_transition_id)
                 VALUES (?1,1,?2,?3,1,?4,2,'manifest-current',1)",
                params![VIEW_ID, PREDECESSOR_HASH, BASE_ID, RESOLVER_EPOCH],
            )
            .unwrap();
        let state = ResolutionScopeState {
            view_id: VIEW_ID.to_string(),
            predecessor_manifest_generation: 1,
            predecessor_manifest_hash: PREDECESSOR_HASH.to_string(),
            base_id: BASE_ID.to_string(),
            delta_generation: 1,
            resolver_output_epoch: RESOLVER_EPOCH,
            current_manifest_generation: 2,
            current_manifest_hash: "manifest-current".to_string(),
            journal_through_transition_id: 1,
        };
        Self {
            _temp: temp,
            layout,
            state,
            v1,
            v2,
        }
    }

    fn reader(&self) -> PriorOverlayReader {
        ready(PriorOverlayReader::open(&self.layout, &self.state).unwrap())
    }
}

fn insert_version(connection: &Connection, path: &str, hash: &str) -> i64 {
    connection
        .execute(
            "INSERT INTO file_versions
             (path,content_hash,extraction_epoch,language,content_bytes,complete_l1,complete_l2)
             VALUES (?1,?2,1,'rust',1,1,2)",
            params![path, hash],
        )
        .unwrap();
    connection.last_insert_rowid()
}

fn insert_symbol(connection: &Connection, version: i64, id: &str, path: &str) {
    connection
        .execute(
            "INSERT INTO symbols
             (version_id,symbol_id,path,language,name,kind,start_line,start_column,
              end_line,end_column,start_byte,end_byte)
             VALUES (?1,?2,?3,'rust',?2,'function',1,0,1,1,0,1)",
            params![version, id, path],
        )
        .unwrap();
}

fn insert_identifier(connection: &Connection, version: i64, id: &str, name: &str, path: &str) {
    let site = format!("site-identifier-{id}");
    insert_site(connection, version, &site, path);
    connection
        .execute(
            "INSERT INTO identifiers
             (version_id,identifier_id,reference_site_id,path,language,name,kind,
              start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,?2,?3,?4,'rust',?5,'call',1,0,1,1,0,1,1.0)",
            params![version, id, site, path, name],
        )
        .unwrap();
}

fn insert_pending(connection: &Connection, version: i64, id: &str, name: &str, path: &str) {
    let site = format!("site-pending-{id}");
    insert_site(connection, version, &site, path);
    connection
        .execute(
            "INSERT INTO pending_relationships
             (version_id,pending_relationship_id,reference_site_id,from_symbol_id,path,kind,
              target_display_name,target_terminal_name,target_namespace_json,start_line,
              start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,?2,?3,'target-base',?4,'calls',?5,?5,'[]',1,0,1,1,0,1,1.0)",
            params![version, id, site, path, name],
        )
        .unwrap();
}

fn insert_site(connection: &Connection, version: i64, id: &str, path: &str) {
    connection
        .execute(
            "INSERT INTO reference_sites
             (version_id,reference_site_id,path,language,start_line,start_column,end_line,
              end_column,start_byte,end_byte,is_exact,provenance,level)
             VALUES (?1,?2,?3,'rust',1,0,1,1,0,1,1,'target_token',2)",
            params![version, id, path],
        )
        .unwrap();
}

fn identifier_row(version: i64, id: &str, method: &str) -> ResolutionIdentifierRow {
    ResolutionIdentifierRow {
        version_id: version,
        identifier_id: id.to_string(),
        target_version_id: Some(version),
        target_symbol_id: Some("target-base".to_string()),
        tier: Some(2),
        confidence: Some(0.9),
        method: Some(method.to_string()),
        outcome: "resolved".to_string(),
        candidates: Some(1),
    }
}

fn pending_row(version: i64, id: &str, method: &str) -> ResolutionPendingRow {
    ResolutionPendingRow {
        version_id: version,
        pending_relationship_id: id.to_string(),
        target_version_id: version,
        target_symbol_id: "target-base".to_string(),
        tier: 2,
        confidence: 0.9,
        method: method.to_string(),
    }
}
