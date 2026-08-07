//! The workspace-wide `resolution_report` aggregate is an O(workspace) query and
//! must only run on passes that re-derive the whole workspace. A scoped delta
//! returns no aggregate rows — recomputing them per single-file update is what
//! regressed the delta gate from 82 ms to 180 ms (see
//! docs/findings/2026-08-05-single-file-delta-172ms-attribution.md) and costs
//! O(workspace) inside the write transaction on every Miller converge save.
//!
//! Durable metadata (`status`, `version`, `last_full_revision`) is derived from
//! pass bookkeeping, not from the aggregate rows, so it stays correct either way.

use std::collections::HashSet;

use julie_extract_artifact::resolution_store::{self, ResolutionStatus};
use julie_extract_artifact::schema::create_schema;
use julie_extract_artifact::writer::ResolutionScopeInput;
use julie_extract_cli::resolution::resolve_workspace;
use rusqlite::Connection;

const FILE_IDS: [&str; 4] = ["f1", "f2", "f3", "f4"];

fn seeded_connection() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory artifact");
    create_schema(&conn).expect("schema");
    conn.execute(
        "INSERT INTO extraction_revisions \
         (revision_id, parent_revision_id, operation, mode, started_at, completed_at, \
          binary_version, extract_contract_version, sqlite_schema_version, input_root, counts_json) \
         VALUES (1, NULL, 'scan', 'full', '2026-08-05T00:00:00Z', '2026-08-05T00:00:01Z', \
                 'julie-extract test', 3, 4, '/repo', '{}')",
        [],
    )
    .expect("revision row");
    for file_id in FILE_IDS {
        conn.execute(
            "INSERT INTO files \
             (file_id, path, language, content_hash, content_bytes, line_count, \
              indexed_at, last_revision_id, status) \
             VALUES (?1, ?2, 'rust', 'hash', 64, 8, '2026-08-05T00:00:00Z', 1, 'indexed')",
            rusqlite::params![file_id, format!("src/{file_id}.rs")],
        )
        .expect("file row");
    }
    conn
}

fn full_scope() -> ResolutionScopeInput {
    ResolutionScopeInput {
        changed_file_ids: Vec::new(),
        touched_symbol_names: HashSet::new(),
        is_full_scan: true,
        whole_corpus: true,
    }
}

fn delta_scope(changed: &[&str]) -> ResolutionScopeInput {
    ResolutionScopeInput {
        changed_file_ids: changed.iter().map(ToString::to_string).collect(),
        touched_symbol_names: HashSet::new(),
        is_full_scan: false,
        whole_corpus: false,
    }
}

fn run_warm_full(conn: &mut Connection) {
    let tx = conn.transaction().expect("tx");
    let (_counts, report) = resolve_workspace(&tx, &full_scope()).expect("full resolve");
    tx.commit().expect("commit");
    resolution_store::write_resolution_metadata(
        conn,
        report.status,
        report.version,
        report.last_full_revision,
    )
    .expect("metadata write");
}

#[test]
fn full_pass_carries_aggregate_rows() {
    let mut conn = seeded_connection();
    let tx = conn.transaction().expect("tx");
    let (_counts, report) = resolve_workspace(&tx, &full_scope()).expect("full resolve");
    assert!(
        report.rows.is_some(),
        "a full pass re-derives the workspace and must carry the aggregate rows"
    );
    assert_eq!(report.status, ResolutionStatus::Complete);
    assert_eq!(report.last_full_revision, 1);
}

#[test]
fn scoped_delta_skips_the_workspace_aggregate() {
    let mut conn = seeded_connection();
    run_warm_full(&mut conn);
    let tx = conn.transaction().expect("tx");
    let (_counts, report) = resolve_workspace(&tx, &delta_scope(&["f1"])).expect("delta resolve");
    assert_eq!(
        report.rows, None,
        "a scoped delta must not recompute the O(workspace) aggregate"
    );
    assert_eq!(report.status, ResolutionStatus::Partial);
    assert_eq!(
        report.last_full_revision, 1,
        "delta preserves the prior full revision"
    );
}

#[test]
fn crossover_promoted_delta_carries_aggregate_rows() {
    let mut conn = seeded_connection();
    run_warm_full(&mut conn);
    let tx = conn.transaction().expect("tx");
    let (_counts, report) =
        resolve_workspace(&tx, &delta_scope(&["f1", "f2", "f3", "f4"])).expect("promoted resolve");
    assert!(
        report.rows.is_some(),
        "a delta promoted to Full past the crossover re-derives the workspace and \
         must carry the aggregate rows"
    );
}

fn seed_identifiers(conn: &Connection, file_id: &str, count: usize) {
    for i in 0..count {
        let id = format!("{file_id}-ident-{i}");
        conn.execute(
            "INSERT INTO reference_sites \
             (reference_site_id, file_id, path, language, containing_symbol_id, \
              start_line, start_column, end_line, end_column, start_byte, end_byte, \
              is_exact, provenance) \
             VALUES (?1, ?2, ?3, 'rust', NULL, 1, 0, 1, 8, 0, 8, 1, 'target_token')",
            rusqlite::params![id, file_id, format!("src/{file_id}.rs")],
        )
        .expect("reference site row");
        conn.execute(
            "INSERT INTO identifiers \
             (identifier_id, reference_site_id, file_id, path, language, name, kind, \
              containing_symbol_id, start_line, start_column, end_line, \
              end_column, start_byte, end_byte, confidence) \
             VALUES (?1, ?1, ?2, ?3, 'rust', ?4, 'call', NULL, 1, 0, 1, 8, 0, 8, 1.0)",
            rusqlite::params![
                id,
                file_id,
                format!("src/{file_id}.rs"),
                format!("name_{i}")
            ],
        )
        .expect("identifier row");
    }
}

#[test]
fn dense_multi_file_delta_promotes_past_the_identifier_crossover() {
    let mut conn = seeded_connection();
    seed_identifiers(&conn, "f1", 60);
    seed_identifiers(&conn, "f2", 30);
    for quiet in ["f3", "f4"] {
        seed_identifiers(&conn, quiet, 3);
    }
    run_warm_full(&mut conn);
    let tx = conn.transaction().expect("tx");
    let (_counts, report) =
        resolve_workspace(&tx, &delta_scope(&["f1", "f2"])).expect("dense resolve");
    assert!(
        report.rows.is_some(),
        "two changed files holding 93.75% of the workspace's identifiers are past the \
         crossover: the cost is denominated in identifier rows, not files (50% of files), \
         so the pass must promote to Full"
    );
}

#[test]
fn recheck_names_matching_rows_outside_the_changed_files_count_toward_the_crossover() {
    let mut conn = seeded_connection();
    for sparse in ["f1", "f2"] {
        seed_identifiers(&conn, sparse, 2);
    }
    for dense in ["f3", "f4"] {
        seed_identifiers(&conn, dense, 45);
    }
    run_warm_full(&mut conn);
    let mut scope = delta_scope(&["f1", "f2"]);
    scope.touched_symbol_names = (0..41).map(|i| format!("name_{i}")).collect();
    let tx = conn.transaction().expect("tx");
    let (_counts, report) = resolve_workspace(&tx, &scope).expect("named delta resolve");
    assert!(
        report.rows.is_some(),
        "the changed files hold 4% of the workspace's identifiers, but the recheck names \
         select 91% of them: a row-scoped pass pays for the rows it re-derives, not for the \
         files it sweeps, so both arms count toward the crossover and the pass must promote"
    );
}

#[test]
fn dense_single_file_delta_never_promotes() {
    let mut conn = seeded_connection();
    seed_identifiers(&conn, "f1", 90);
    for quiet in ["f2", "f3", "f4"] {
        seed_identifiers(&conn, quiet, 2);
    }
    run_warm_full(&mut conn);
    let tx = conn.transaction().expect("tx");
    let (_counts, report) = resolve_workspace(&tx, &delta_scope(&["f1"])).expect("dense resolve");
    assert_eq!(
        report.rows, None,
        "a single-changed-file scope never promotes: measured A/B (2026-08-07) shows a \
         dense save pays the same or less on the scoped path, and promotion would only \
         re-derive the workspace aggregate for nothing"
    );
    assert_eq!(
        report.last_full_revision, 1,
        "a single-file save must not claim whole-corpus coverage"
    );
}
