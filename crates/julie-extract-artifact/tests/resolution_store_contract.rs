//! Contract tests for the resolution overlay storage primitives.
//!
//! These primitives are the ONLY sanctioned write path to resolution state
//! (`pending_resolutions`, `identifier_resolutions`, and the denormalized
//! `identifiers.target_symbol_id`). Every invariant asserted here is a
//! contract other tasks (3/4/5) rely on.

use julie_extract_artifact::resolution_store::{
    self, Outcome, ResolutionMetadata, ResolutionStatus,
};
use julie_extract_artifact::schema::create_schema;
use rusqlite::{Connection, params};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

fn open() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    create_schema(&conn).unwrap();
    conn
}

/// Seed a minimal but FK-complete graph:
/// revision 1 -> file f1 -> symbols s_from / s_target -> pending p1 + identifier i1.
fn seed(conn: &Connection) {
    conn.execute(
        "INSERT INTO extraction_revisions \
         (revision_id, operation, started_at, completed_at, binary_version, \
          extract_contract_version, sqlite_schema_version, counts_json) \
         VALUES (1, 'scan', 't', 't', 'v', 3, 4, '{}')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO files \
         (file_id, path, language, content_hash, content_bytes, indexed_at, \
          last_revision_id, status) \
         VALUES ('f1', 'src/a.rs', 'rust', 'h', 10, 't', 1, 'active')",
        [],
    )
    .unwrap();
    insert_symbol(conn, "s_from", "caller", "function");
    insert_symbol(conn, "s_target", "Target", "function");
    conn.execute(
        "INSERT INTO pending_relationships \
         (pending_relationship_id, from_symbol_id, file_id, path, kind, \
          target_display_name, target_terminal_name, target_receiver, \
          target_namespace_json, start_line, confidence) \
         VALUES ('p1', 's_from', 'f1', 'src/a.rs', 'calls', 'Target', 'Target', \
                 'obj', '[]', 5, 0.5)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO identifiers \
         (identifier_id, file_id, path, language, name, kind, start_line, \
          start_column, end_line, end_column, start_byte, end_byte, confidence) \
         VALUES ('i1', 'f1', 'src/a.rs', 'rust', 'Target', 'call', 5, 1, 5, 7, 40, 46, 0.5)",
        [],
    )
    .unwrap();
}

fn insert_symbol(conn: &Connection, id: &str, name: &str, kind: &str) {
    conn.execute(
        "INSERT INTO symbols \
         (symbol_id, file_id, path, language, name, kind, start_line, start_column, \
          end_line, end_column, start_byte, end_byte, is_test, test_container, test_lifecycle) \
         VALUES (?1, 'f1', 'src/a.rs', 'rust', ?2, ?3, 1, 1, 2, 1, 0, 20, 0, 0, 0)",
        params![id, name, kind],
    )
    .unwrap();
}

fn pending_resolution_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM pending_resolutions", [], |r| r.get(0))
        .unwrap()
}

fn identifier_resolution_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM identifier_resolutions", [], |r| {
        r.get(0)
    })
    .unwrap()
}

fn identifier_target(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT target_symbol_id FROM identifiers WHERE identifier_id = ?1",
        [id],
        |r| r.get::<_, Option<String>>(0),
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// pending_resolutions overlay
// ---------------------------------------------------------------------------

#[test]
fn record_pending_resolution_writes_overlay_row() {
    // INVARIANT: a resolved pending row is represented by exactly one
    // pending_resolutions row carrying tier/confidence/method/revision.
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_pending_resolution(&tx, "p1", "s_target", 2, 0.85, "tier2_import", 1)
        .unwrap();
    tx.commit().unwrap();

    let (tier, conf, method, rev): (i64, f64, String, i64) = conn
        .query_row(
            "SELECT tier, confidence, method, resolved_at_revision \
             FROM pending_resolutions WHERE pending_relationship_id = 'p1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(tier, 2);
    assert_eq!(conf, 0.85);
    assert_eq!(method, "tier2_import");
    assert_eq!(rev, 1);
}

#[test]
fn record_pending_resolution_is_idempotent_upsert() {
    // INVARIANT: re-recording the same pending id replaces (does not duplicate)
    // the overlay row, so two identical scans stay byte-identical.
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_pending_resolution(&tx, "p1", "s_target", 4, 0.55, "tier4", 1)
        .unwrap();
    resolution_store::record_pending_resolution(&tx, "p1", "s_target", 1, 0.95, "tier1", 2)
        .unwrap();
    tx.commit().unwrap();

    assert_eq!(pending_resolution_count(&conn), 1);
    let (tier, rev): (i64, i64) = conn
        .query_row(
            "SELECT tier, resolved_at_revision FROM pending_resolutions WHERE pending_relationship_id = 'p1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(tier, 1);
    assert_eq!(rev, 2);
}

#[test]
fn pending_resolution_cascades_on_target_death_pending_context_survives() {
    // INVARIANT: killing the target symbol CASCADE-removes the resolution but
    // leaves the pending row (its full context) intact -> reverts to unresolved.
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_pending_resolution(&tx, "p1", "s_target", 2, 0.85, "tier2", 1)
        .unwrap();
    tx.commit().unwrap();
    assert_eq!(pending_resolution_count(&conn), 1);

    conn.execute("DELETE FROM symbols WHERE symbol_id = 's_target'", [])
        .unwrap();

    assert_eq!(
        pending_resolution_count(&conn),
        0,
        "resolution must cascade"
    );
    let surviving: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pending_relationships WHERE pending_relationship_id = 'p1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(surviving, 1, "pending context must survive target death");
}

#[test]
fn pending_resolution_cascades_on_source_file_delete() {
    // INVARIANT: rewriting/deleting the source file cascades the pending row
    // away, which cascades its resolution away too.
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_pending_resolution(&tx, "p1", "s_target", 2, 0.85, "tier2", 1)
        .unwrap();
    tx.commit().unwrap();

    conn.execute("DELETE FROM files WHERE file_id = 'f1'", [])
        .unwrap();
    assert_eq!(pending_resolution_count(&conn), 0);
}

#[test]
fn demote_pending_clears_overlay_row() {
    // INVARIANT: demotion deletes the overlay row (pending reverts to unresolved).
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_pending_resolution(&tx, "p1", "s_target", 2, 0.85, "tier2", 1)
        .unwrap();
    resolution_store::demote_pending(&tx, "p1").unwrap();
    tx.commit().unwrap();
    assert_eq!(pending_resolution_count(&conn), 0);
}

// ---------------------------------------------------------------------------
// identifier_resolutions overlay + denormalized column
// ---------------------------------------------------------------------------

#[test]
fn record_identifier_resolved_writes_overlay_and_denormalized_column() {
    // INVARIANT: a resolved identifier writes BOTH the overlay row and the
    // denormalized identifiers.target_symbol_id in one atomic call.
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_identifier_outcome(
        &tx,
        "i1",
        Outcome::Resolved,
        Some("s_target"),
        Some(2),
        Some(0.85),
        Some("tier2_import"),
        None,
        1,
    )
    .unwrap();
    tx.commit().unwrap();

    assert_eq!(identifier_resolution_count(&conn), 1);
    assert_eq!(identifier_target(&conn, "i1"), Some("s_target".to_string()));
    let outcome: String = conn
        .query_row(
            "SELECT outcome FROM identifier_resolutions WHERE identifier_id = 'i1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(outcome, "resolved");
}

#[test]
fn record_identifier_ambiguous_has_null_target_and_null_denormalized_column() {
    // INVARIANT: a non-resolved outcome writes a NULL-target overlay row and
    // leaves the denormalized column NULL.
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_identifier_outcome(
        &tx,
        "i1",
        Outcome::Ambiguous,
        None,
        None,
        None,
        None,
        Some(3),
        1,
    )
    .unwrap();
    tx.commit().unwrap();

    assert_eq!(identifier_target(&conn, "i1"), None);
    let (outcome, cand): (String, Option<i64>) = conn
        .query_row(
            "SELECT outcome, candidates FROM identifier_resolutions WHERE identifier_id = 'i1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(outcome, "ambiguous");
    assert_eq!(cand, Some(3));
}

#[test]
fn resolved_identifier_cascades_on_target_death_row_survives() {
    // INVARIANT: killing the target CASCADE-removes the identifier overlay row
    // (identifier reverts to never-attempted) AND SET NULLs the denormalized
    // column; the identifier itself survives.
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_identifier_outcome(
        &tx,
        "i1",
        Outcome::Resolved,
        Some("s_target"),
        Some(2),
        Some(0.85),
        Some("tier2"),
        None,
        1,
    )
    .unwrap();
    tx.commit().unwrap();

    conn.execute("DELETE FROM symbols WHERE symbol_id = 's_target'", [])
        .unwrap();

    assert_eq!(
        identifier_resolution_count(&conn),
        0,
        "overlay must cascade"
    );
    assert_eq!(
        identifier_target(&conn, "i1"),
        None,
        "denormalized column SET NULL on target death"
    );
    let survives: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM identifiers WHERE identifier_id = 'i1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(survives, 1, "identifier survives target death");
}

#[test]
fn ambiguous_identifier_row_unaffected_by_target_deletions() {
    // INVARIANT: NULL-target (ambiguous/missing) overlay rows reference no
    // target, so no target deletion can remove them.
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_identifier_outcome(
        &tx,
        "i1",
        Outcome::Missing,
        None,
        None,
        None,
        None,
        Some(0),
        1,
    )
    .unwrap();
    tx.commit().unwrap();

    conn.execute("DELETE FROM symbols WHERE symbol_id = 's_target'", [])
        .unwrap();
    assert_eq!(identifier_resolution_count(&conn), 1);
}

#[test]
fn check_rejects_resolved_with_null_target() {
    // INVARIANT: the CHECK enforces outcome='resolved' <=> target NOT NULL.
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    let err = tx.execute(
        "INSERT INTO identifier_resolutions \
         (identifier_id, target_symbol_id, outcome, resolved_at_revision) \
         VALUES ('i1', NULL, 'resolved', 1)",
        [],
    );
    assert!(err.is_err(), "resolved with NULL target must be rejected");
}

#[test]
fn check_rejects_non_resolved_with_non_null_target() {
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    let err = tx.execute(
        "INSERT INTO identifier_resolutions \
         (identifier_id, target_symbol_id, outcome, resolved_at_revision) \
         VALUES ('i1', 's_target', 'ambiguous', 1)",
        [],
    );
    assert!(
        err.is_err(),
        "ambiguous with non-NULL target must be rejected"
    );
}

#[test]
fn demote_identifier_clears_overlay_and_denormalized_column() {
    // INVARIANT (round-3 finding 1): demotion deletes the overlay row AND clears
    // the denormalized column in the same batch (FK SET NULL never fires here).
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_identifier_outcome(
        &tx,
        "i1",
        Outcome::Resolved,
        Some("s_target"),
        Some(1),
        Some(0.95),
        Some("tier1"),
        None,
        1,
    )
    .unwrap();
    resolution_store::demote_identifier(&tx, "i1").unwrap();
    tx.commit().unwrap();

    assert_eq!(identifier_resolution_count(&conn), 0);
    assert_eq!(
        identifier_target(&conn, "i1"),
        None,
        "demote must clear denormalized column, not rely on FK"
    );
}

#[test]
fn record_identifier_outcome_is_idempotent_upsert() {
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_identifier_outcome(
        &tx,
        "i1",
        Outcome::Ambiguous,
        None,
        None,
        None,
        None,
        Some(2),
        1,
    )
    .unwrap();
    // Re-resolve the same identifier to a concrete target.
    resolution_store::record_identifier_outcome(
        &tx,
        "i1",
        Outcome::Resolved,
        Some("s_target"),
        Some(1),
        Some(0.95),
        Some("tier1"),
        None,
        2,
    )
    .unwrap();
    tx.commit().unwrap();

    assert_eq!(identifier_resolution_count(&conn), 1);
    assert_eq!(identifier_target(&conn, "i1"), Some("s_target".to_string()));
}

// ---------------------------------------------------------------------------
// worklist queries
// ---------------------------------------------------------------------------

#[test]
fn worklist_unresolved_pending_by_names_finds_only_unresolved() {
    let mut conn = open();
    seed(&conn);
    // Before resolution: p1 is on the worklist by terminal name.
    let items = resolution_store::worklist_unresolved_pending_by_names(&conn, &["Target"]).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].pending_relationship_id, "p1");
    assert_eq!(items[0].language, "rust");
    assert_eq!(items[0].target_terminal_name, "Target");

    // After resolution: p1 drops off the unresolved worklist.
    let tx = conn.transaction().unwrap();
    resolution_store::record_pending_resolution(&tx, "p1", "s_target", 2, 0.85, "tier2", 1)
        .unwrap();
    tx.commit().unwrap();
    let items = resolution_store::worklist_unresolved_pending_by_names(&conn, &["Target"]).unwrap();
    assert!(items.is_empty());
}

#[test]
fn worklist_unresolved_pending_matches_receiver_name() {
    let conn = open();
    seed(&conn);
    // "obj" is the receiver, not the terminal name.
    let items = resolution_store::worklist_unresolved_pending_by_names(&conn, &["obj"]).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].target_receiver.as_deref(), Some("obj"));
}

#[test]
fn worklist_empty_name_set_returns_nothing() {
    let conn = open();
    seed(&conn);
    assert!(
        resolution_store::worklist_unresolved_pending_by_names(&conn, &[])
            .unwrap()
            .is_empty()
    );
    assert!(
        resolution_store::worklist_never_attempted_identifiers_by_names(&conn, &[])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn worklist_resolved_pending_by_names_returns_resolved_rows() {
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_pending_resolution(&tx, "p1", "s_target", 2, 0.85, "tier2", 1)
        .unwrap();
    tx.commit().unwrap();

    let rows = resolution_store::worklist_resolved_pending_by_names(&conn, &["Target"]).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].pending.pending_relationship_id, "p1");
    assert_eq!(rows[0].target_symbol_id, "s_target");
    assert_eq!(rows[0].tier, 2);
}

#[test]
fn worklist_never_attempted_identifiers_by_names_and_files() {
    let mut conn = open();
    seed(&conn);
    // i1 is never-attempted, matched by name and by file.
    assert_eq!(
        resolution_store::worklist_never_attempted_identifiers_by_names(&conn, &["Target"])
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        resolution_store::worklist_never_attempted_identifiers_by_files(&conn, &["f1"])
            .unwrap()
            .len(),
        1
    );

    // Once resolved, it is no longer never-attempted.
    let tx = conn.transaction().unwrap();
    resolution_store::record_identifier_outcome(
        &tx,
        "i1",
        Outcome::Resolved,
        Some("s_target"),
        Some(1),
        Some(0.95),
        Some("tier1"),
        None,
        1,
    )
    .unwrap();
    tx.commit().unwrap();
    assert!(
        resolution_store::worklist_never_attempted_identifiers_by_names(&conn, &["Target"])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn worklist_resolved_identifiers_by_names_returns_overlay() {
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_identifier_outcome(
        &tx,
        "i1",
        Outcome::Resolved,
        Some("s_target"),
        Some(1),
        Some(0.95),
        Some("tier1"),
        None,
        1,
    )
    .unwrap();
    tx.commit().unwrap();

    let rows =
        resolution_store::worklist_resolved_identifiers_by_names(&conn, &["Target"]).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, Outcome::Resolved);
    assert_eq!(rows[0].target_symbol_id.as_deref(), Some("s_target"));
}

#[test]
fn worklist_full_pending_returns_every_unresolved_row() {
    let mut conn = open();
    seed(&conn);
    assert_eq!(
        resolution_store::worklist_full_pending(&conn)
            .unwrap()
            .len(),
        1
    );
    let tx = conn.transaction().unwrap();
    resolution_store::record_pending_resolution(&tx, "p1", "s_target", 2, 0.85, "tier2", 1)
        .unwrap();
    tx.commit().unwrap();
    assert!(
        resolution_store::worklist_full_pending(&conn)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn worklist_full_identifiers_covers_never_attempted_and_null_target() {
    let mut conn = open();
    seed(&conn);
    // Never-attempted -> included.
    assert_eq!(
        resolution_store::worklist_full_identifiers(&conn)
            .unwrap()
            .len(),
        1
    );

    // Ambiguous (NULL target) -> still included (needs re-resolution).
    let tx = conn.transaction().unwrap();
    resolution_store::record_identifier_outcome(
        &tx,
        "i1",
        Outcome::Ambiguous,
        None,
        None,
        None,
        None,
        Some(2),
        1,
    )
    .unwrap();
    tx.commit().unwrap();
    assert_eq!(
        resolution_store::worklist_full_identifiers(&conn)
            .unwrap()
            .len(),
        1
    );

    // Resolved (non-NULL target) -> excluded.
    let tx = conn.transaction().unwrap();
    resolution_store::record_identifier_outcome(
        &tx,
        "i1",
        Outcome::Resolved,
        Some("s_target"),
        Some(1),
        Some(0.95),
        Some("tier1"),
        None,
        2,
    )
    .unwrap();
    tx.commit().unwrap();
    assert!(
        resolution_store::worklist_full_identifiers(&conn)
            .unwrap()
            .is_empty()
    );
}

// ---------------------------------------------------------------------------
// resolution report
// ---------------------------------------------------------------------------

#[test]
fn resolution_report_aggregates_by_language_tier_outcome() {
    let mut conn = open();
    seed(&conn);
    let tx = conn.transaction().unwrap();
    resolution_store::record_pending_resolution(&tx, "p1", "s_target", 2, 0.85, "tier2", 1)
        .unwrap();
    resolution_store::record_identifier_outcome(
        &tx,
        "i1",
        Outcome::Resolved,
        Some("s_target"),
        Some(2),
        Some(0.85),
        Some("tier2"),
        None,
        1,
    )
    .unwrap();
    tx.commit().unwrap();

    let rows = resolution_store::resolution_report(&conn).unwrap();
    assert_eq!(
        rows.iter()
            .filter(|r| r.language == "rust" && r.tier == Some(2) && r.outcome == "resolved")
            .count(),
        1,
        "pending and identifier overlays in the same bucket must collapse to one report row"
    );
    assert!(rows.iter().any(|r| r.language == "rust"
        && r.tier == Some(2)
        && r.outcome == "resolved"
        && r.count == 2));
}

// ---------------------------------------------------------------------------
// resolution metadata
// ---------------------------------------------------------------------------

#[test]
fn resolution_metadata_round_trips_without_touching_artifact_rows() {
    // INVARIANT: the three durable status keys round-trip via a SEPARATE upsert
    // and never require widening ArtifactMetadata::rows().
    let conn = open();
    assert!(
        resolution_store::read_resolution_metadata(&conn)
            .unwrap()
            .is_none(),
        "absent before any write"
    );

    resolution_store::write_resolution_metadata(&conn, ResolutionStatus::Partial, 4, 7).unwrap();
    let meta = resolution_store::read_resolution_metadata(&conn)
        .unwrap()
        .expect("metadata present after write");
    assert_eq!(
        meta,
        ResolutionMetadata {
            status: ResolutionStatus::Partial,
            version: 4,
            last_full_revision: 7,
        }
    );

    // Upsert overwrites in place.
    resolution_store::write_resolution_metadata(&conn, ResolutionStatus::Complete, 4, 9).unwrap();
    let meta = resolution_store::read_resolution_metadata(&conn)
        .unwrap()
        .unwrap();
    assert_eq!(meta.status, ResolutionStatus::Complete);
    assert_eq!(meta.last_full_revision, 9);

    // Raw keys are present in artifact_metadata.
    let keys: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT key FROM artifact_metadata \
                 WHERE key LIKE 'reference_resolution_%' ORDER BY key",
            )
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(
        keys,
        vec![
            "reference_resolution_last_full_revision".to_string(),
            "reference_resolution_status".to_string(),
            "reference_resolution_version".to_string(),
        ]
    );
}

#[test]
fn resolution_status_string_round_trip() {
    for status in [
        ResolutionStatus::Complete,
        ResolutionStatus::Partial,
        ResolutionStatus::Failed,
        ResolutionStatus::Absent,
    ] {
        assert_eq!(ResolutionStatus::parse(status.as_str()), Some(status));
    }
}

#[test]
fn outcome_string_round_trip() {
    for outcome in [
        Outcome::Resolved,
        Outcome::Ambiguous,
        Outcome::Missing,
        Outcome::NoContext,
    ] {
        assert_eq!(Outcome::parse(outcome.as_str()), Some(outcome));
    }
}
