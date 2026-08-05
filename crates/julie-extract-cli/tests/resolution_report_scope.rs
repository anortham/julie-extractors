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
