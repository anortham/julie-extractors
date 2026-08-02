//! Storage primitives for the workspace reference-resolution overlay.
//!
//! Resolution state lives in two FK-governed overlay tables — `pending_resolutions`
//! and `identifier_resolutions` — plus the denormalized `identifiers.target_symbol_id`
//! convenience column. This module is the **only** sanctioned write path to that
//! state: the `record_*` / `demote_*` primitives keep the overlay row and the
//! denormalized column consistent in a single statement batch, and no caller writes
//! either surface directly (design §"Resolution state model", round-3 finding 1).
//!
//! This crate stays pure storage: it holds no language semantics and no tier policy.
//! Resolver policy lives in `julie-extract-cli`; it only calls these primitives.

use std::collections::BTreeMap;

use rusqlite::types::Value;
use rusqlite::{Connection, Transaction, params, params_from_iter};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Outcome recorded for an identifier resolution attempt.
///
/// `resolved` is the only outcome that carries a target symbol; the schema CHECK
/// enforces `outcome='resolved' <=> target_symbol_id IS NOT NULL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Resolved,
    Ambiguous,
    Missing,
    NoContext,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Resolved => "resolved",
            Outcome::Ambiguous => "ambiguous",
            Outcome::Missing => "missing",
            Outcome::NoContext => "no_context",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "resolved" => Some(Outcome::Resolved),
            "ambiguous" => Some(Outcome::Ambiguous),
            "missing" => Some(Outcome::Missing),
            "no_context" => Some(Outcome::NoContext),
            _ => None,
        }
    }

    pub fn is_resolved(self) -> bool {
        matches!(self, Outcome::Resolved)
    }
}

/// Durable resolution-availability status published to `artifact_metadata`.
///
/// Miller gates on this key, never on the schema version or table probing
/// (design §"Contract & rollout" item 2, round-3 finding 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionStatus {
    Complete,
    Partial,
    Failed,
    Absent,
}

impl ResolutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ResolutionStatus::Complete => "complete",
            ResolutionStatus::Partial => "partial",
            ResolutionStatus::Failed => "failed",
            ResolutionStatus::Absent => "absent",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "complete" => Some(ResolutionStatus::Complete),
            "partial" => Some(ResolutionStatus::Partial),
            "failed" => Some(ResolutionStatus::Failed),
            "absent" => Some(ResolutionStatus::Absent),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Value types
// ---------------------------------------------------------------------------

/// Rows written per overlay table during a resolution pass, for revision accounting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResolutionCounts {
    pub pending_resolutions: u64,
    pub identifier_resolutions: u64,
}

impl ResolutionCounts {
    pub fn total(self) -> u64 {
        self.pending_resolutions + self.identifier_resolutions
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionReportRow {
    pub language: String,
    pub origin: String,
    pub raw_kind: String,
    pub canonical_kind: String,
    pub tier: Option<u8>,
    pub method: Option<String>,
    pub outcome: String,
    pub span_present: bool,
    pub count: i64,
}

pub fn canonical_reference_kind<'a>(origin: &str, raw_kind: &'a str) -> Option<&'a str> {
    match (origin, raw_kind) {
        ("identifier", "call") => Some("calls"),
        ("identifier", "type_usage") => Some("uses"),
        ("identifier", "member_access" | "variable_ref") => Some("references"),
        (
            "relationship" | "pending_relationship",
            "calls" | "extends" | "implements" | "imports" | "instantiates" | "references" | "uses",
        ) => Some(raw_kind),
        _ => None,
    }
}

/// Durable resolution-status metadata, mirrored from the three `reference_resolution_*`
/// keys in `artifact_metadata`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionMetadata {
    pub status: ResolutionStatus,
    pub version: i64,
    pub last_full_revision: i64,
}

/// A pending relationship the resolver may act on. `language` is joined from
/// `files` (pending rows carry no language column of their own).
#[derive(Debug, Clone, PartialEq)]
pub struct PendingWorkItem {
    pub pending_relationship_id: String,
    pub from_symbol_id: String,
    pub caller_scope_symbol_id: Option<String>,
    pub file_id: String,
    pub path: String,
    pub language: String,
    pub kind: String,
    pub target_display_name: String,
    pub target_terminal_name: String,
    pub target_receiver: Option<String>,
    pub target_namespace_json: String,
    pub target_import_context: Option<String>,
    pub start_line: i64,
    pub start_byte: Option<i64>,
    pub end_byte: Option<i64>,
    pub confidence: f64,
}

/// A pending row plus its current resolution overlay (used by the re-resolution
/// / demotion pass).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedPendingWorkItem {
    pub pending: PendingWorkItem,
    pub target_symbol_id: String,
    pub tier: i64,
    pub confidence: f64,
    pub method: String,
}

/// An identifier the resolver may act on.
#[derive(Debug, Clone, PartialEq)]
pub struct IdentifierWorkItem {
    pub identifier_id: String,
    pub file_id: String,
    pub path: String,
    pub language: String,
    pub name: String,
    pub kind: String,
    pub containing_symbol_id: Option<String>,
    pub start_line: i64,
    pub start_byte: i64,
    pub end_byte: i64,
    pub receiver: Option<String>,
    pub receiver_qualifier: Option<String>,
    pub import_context: Option<String>,
    pub confidence: f64,
}

/// An identifier plus its current resolution overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedIdentifierWorkItem {
    pub identifier: IdentifierWorkItem,
    pub target_symbol_id: Option<String>,
    pub tier: Option<i64>,
    pub confidence: Option<f64>,
    pub method: Option<String>,
    pub outcome: Outcome,
    pub candidates: Option<i64>,
}

// ---------------------------------------------------------------------------
// Metadata keys
// ---------------------------------------------------------------------------

pub const KEY_RESOLUTION_STATUS: &str = "reference_resolution_status";
pub const KEY_RESOLUTION_VERSION: &str = "reference_resolution_version";
pub const KEY_RESOLUTION_LAST_FULL_REVISION: &str = "reference_resolution_last_full_revision";

// ---------------------------------------------------------------------------
// Write primitives (the ONLY sanctioned resolution write path)
// ---------------------------------------------------------------------------

/// Record (or replace) the resolution of a pending relationship. Idempotent
/// upsert so identical scans produce byte-identical tables.
pub fn record_pending_resolution(
    tx: &Transaction<'_>,
    pending_relationship_id: &str,
    target_symbol_id: &str,
    tier: u8,
    confidence: f64,
    method: &str,
    revision: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO pending_resolutions \
         (pending_relationship_id, target_symbol_id, tier, confidence, method, resolved_at_revision) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(pending_relationship_id) DO UPDATE SET \
           target_symbol_id = excluded.target_symbol_id, \
           tier = excluded.tier, \
           confidence = excluded.confidence, \
           method = excluded.method, \
           resolved_at_revision = excluded.resolved_at_revision",
        params![
            pending_relationship_id,
            target_symbol_id,
            tier as i64,
            confidence,
            method,
            revision
        ],
    )?;
    Ok(())
}

/// Record (or replace) an identifier resolution outcome, writing the overlay row
/// AND the denormalized `identifiers.target_symbol_id` in one statement batch.
///
/// `target` must be `Some` iff `outcome` is `Resolved` — the schema CHECK enforces
/// this and a violating call returns an error rather than corrupting state.
#[allow(clippy::too_many_arguments)]
pub fn record_identifier_outcome(
    tx: &Transaction<'_>,
    identifier_id: &str,
    outcome: Outcome,
    target: Option<&str>,
    tier: Option<u8>,
    confidence: Option<f64>,
    method: Option<&str>,
    candidates: Option<i64>,
    revision: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO identifier_resolutions \
         (identifier_id, target_symbol_id, tier, confidence, method, outcome, candidates, resolved_at_revision) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(identifier_id) DO UPDATE SET \
           target_symbol_id = excluded.target_symbol_id, \
           tier = excluded.tier, \
           confidence = excluded.confidence, \
           method = excluded.method, \
           outcome = excluded.outcome, \
           candidates = excluded.candidates, \
           resolved_at_revision = excluded.resolved_at_revision",
        params![
            identifier_id,
            target,
            tier.map(|t| t as i64),
            confidence,
            method,
            outcome.as_str(),
            candidates,
            revision
        ],
    )?;
    // Denormalized convenience column: kept in lockstep with the overlay. Non-resolved
    // outcomes carry no target, so this clears the column.
    tx.execute(
        "UPDATE identifiers SET target_symbol_id = ?2 WHERE identifier_id = ?1",
        params![identifier_id, target],
    )?;
    Ok(())
}

/// Delete a pending resolution (demotion): the pending row reverts to unresolved.
/// Pending relationships carry no denormalized column, so only the overlay row
/// is removed.
pub fn demote_pending(tx: &Transaction<'_>, pending_relationship_id: &str) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM pending_resolutions WHERE pending_relationship_id = ?1",
        params![pending_relationship_id],
    )?;
    Ok(())
}

/// Delete an identifier resolution (demotion): removes the overlay row AND clears
/// the denormalized `identifiers.target_symbol_id` in the same batch. FK SET NULL
/// does not fire for demotion (the target still exists), so the clear is explicit
/// (round-3 finding 1).
pub fn demote_identifier(tx: &Transaction<'_>, identifier_id: &str) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM identifier_resolutions WHERE identifier_id = ?1",
        params![identifier_id],
    )?;
    tx.execute(
        "UPDATE identifiers SET target_symbol_id = NULL WHERE identifier_id = ?1",
        params![identifier_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Batched write buffer (collect-then-flush — the ONLY perf-safe write path for a
// whole-pass resolve; the per-row `record_*`/`demote_*` primitives above stay for
// single-shot callers and the pinned contract tests)
// ---------------------------------------------------------------------------

/// Max rows bound into one batched statement. Kept well under SQLite's compiled
/// `SQLITE_MAX_VARIABLE_NUMBER` (32766): the widest batch is the 8-column
/// `identifier_resolutions` upsert (8 × 500 = 4000 binds); every other batch binds
/// fewer.
const FLUSH_CHUNK: usize = 500;

/// A buffered `pending_resolutions` op. Collapsed per key (last-op-wins) at flush.
enum PendingOp {
    Upsert {
        target_symbol_id: String,
        tier: u8,
        confidence: f64,
        method: String,
        revision: i64,
    },
    Demote,
}

/// A buffered `identifier_resolutions` op (plus the denormalized column). Collapsed
/// per key (last-op-wins) at flush.
enum IdentifierOp {
    Upsert {
        target: Option<String>,
        tier: Option<u8>,
        confidence: Option<f64>,
        method: Option<String>,
        outcome: Outcome,
        candidates: Option<i64>,
        revision: i64,
    },
    Demote,
}

/// In-memory accumulator for a resolution pass's overlay writes.
///
/// The resolver records outcomes here instead of issuing one `INSERT`/`UPDATE` per
/// row; [`ResolutionWriteBuffer::flush`] then emits a SMALL number of chunked,
/// multi-row statements. This keeps the count of statement-ends inside the writer's
/// open `SAVEPOINT resolution_hook` in the low hundreds rather than ~125k — the
/// v2.9.0 quadratic: each statement end truncates the in-memory savepoint
/// sub-journal (`temp_store = MEMORY`) by walking its ever-growing chunk list, so
/// ~125k statements over a growing journal is O(n²) `memjrnlTruncate` CPU.
///
/// **Ordering contract.** Ops are collapsed per key to the LAST op recorded, which
/// is exactly how a sequence of `record_*`/`demote_*` calls resolves (idempotent
/// upsert / delete, last-write-wins). A key recorded then demoted flushes as a
/// delete; demoted then recorded flushes as an upsert. Because the collapse is
/// per-key, the relative order of DIFFERENT keys never matters. Callers MUST
/// [`ResolutionWriteBuffer::flush`] at every point where a later same-pass `SELECT`
/// could observe these writes — the resolver flushes at each phase boundary so
/// cross-phase read-after-write dependencies see the same state they did when every
/// write was immediate.
#[derive(Default)]
pub struct ResolutionWriteBuffer {
    // Append-ordered so the per-key collapse keeps the last op.
    pending: Vec<(String, PendingOp)>,
    identifiers: Vec<(String, IdentifierOp)>,
}

impl ResolutionWriteBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty() && self.identifiers.is_empty()
    }

    /// Buffer a pending-relationship resolution (mirrors [`record_pending_resolution`]).
    pub fn record_pending_resolution(
        &mut self,
        pending_relationship_id: &str,
        target_symbol_id: &str,
        tier: u8,
        confidence: f64,
        method: &str,
        revision: i64,
    ) {
        self.pending.push((
            pending_relationship_id.to_string(),
            PendingOp::Upsert {
                target_symbol_id: target_symbol_id.to_string(),
                tier,
                confidence,
                method: method.to_string(),
                revision,
            },
        ));
    }

    /// Buffer a pending demotion (mirrors [`demote_pending`]).
    pub fn demote_pending(&mut self, pending_relationship_id: &str) {
        self.pending
            .push((pending_relationship_id.to_string(), PendingOp::Demote));
    }

    /// Buffer an identifier resolution outcome plus its denormalized column
    /// (mirrors [`record_identifier_outcome`]).
    #[allow(clippy::too_many_arguments)]
    pub fn record_identifier_outcome(
        &mut self,
        identifier_id: &str,
        outcome: Outcome,
        target: Option<&str>,
        tier: Option<u8>,
        confidence: Option<f64>,
        method: Option<&str>,
        candidates: Option<i64>,
        revision: i64,
    ) {
        self.identifiers.push((
            identifier_id.to_string(),
            IdentifierOp::Upsert {
                target: target.map(str::to_string),
                tier,
                confidence,
                method: method.map(str::to_string),
                outcome,
                candidates,
                revision,
            },
        ));
    }

    /// Buffer an identifier demotion (mirrors [`demote_identifier`]).
    pub fn demote_identifier(&mut self, identifier_id: &str) {
        self.identifiers
            .push((identifier_id.to_string(), IdentifierOp::Demote));
    }

    /// Flush every buffered op to `tx` as a small number of chunked, multi-row
    /// statements, then clear the buffer. Byte-for-byte equivalent to having called
    /// the per-row primitives in order (last-write-wins per key), but with hundreds
    /// of statement-ends instead of tens of thousands.
    pub fn flush(&mut self, tx: &Transaction<'_>) -> rusqlite::Result<()> {
        self.flush_pending(tx)?;
        self.flush_identifiers(tx)?;
        Ok(())
    }

    fn flush_pending(&mut self, tx: &Transaction<'_>) -> rusqlite::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        // Collapse to last-op-per-key (BTreeMap → deterministic, id-sorted order).
        let mut collapsed: BTreeMap<String, PendingOp> = BTreeMap::new();
        for (id, op) in self.pending.drain(..) {
            collapsed.insert(id, op);
        }
        let mut upserts: Vec<(String, String, u8, f64, String, i64)> = Vec::new();
        let mut demotes: Vec<String> = Vec::new();
        for (id, op) in collapsed {
            match op {
                PendingOp::Upsert {
                    target_symbol_id,
                    tier,
                    confidence,
                    method,
                    revision,
                } => upserts.push((id, target_symbol_id, tier, confidence, method, revision)),
                PendingOp::Demote => demotes.push(id),
            }
        }

        for chunk in upserts.chunks(FLUSH_CHUNK) {
            let values = repeat_row_placeholders(chunk.len(), 6);
            let sql = format!(
                "INSERT INTO pending_resolutions \
                 (pending_relationship_id, target_symbol_id, tier, confidence, method, resolved_at_revision) \
                 VALUES {values} \
                 ON CONFLICT(pending_relationship_id) DO UPDATE SET \
                   target_symbol_id = excluded.target_symbol_id, \
                   tier = excluded.tier, \
                   confidence = excluded.confidence, \
                   method = excluded.method, \
                   resolved_at_revision = excluded.resolved_at_revision"
            );
            let mut binds: Vec<Value> = Vec::with_capacity(chunk.len() * 6);
            for (id, target, tier, confidence, method, revision) in chunk {
                binds.push(Value::Text(id.clone()));
                binds.push(Value::Text(target.clone()));
                binds.push(Value::Integer(i64::from(*tier)));
                binds.push(Value::Real(*confidence));
                binds.push(Value::Text(method.clone()));
                binds.push(Value::Integer(*revision));
            }
            tx.execute(&sql, params_from_iter(binds.iter()))?;
        }

        for chunk in demotes.chunks(FLUSH_CHUNK) {
            let sql = format!(
                "DELETE FROM pending_resolutions WHERE pending_relationship_id IN ({})",
                placeholders(chunk.len())
            );
            tx.execute(&sql, params_from_iter(chunk.iter()))?;
        }
        Ok(())
    }

    fn flush_identifiers(&mut self, tx: &Transaction<'_>) -> rusqlite::Result<()> {
        if self.identifiers.is_empty() {
            return Ok(());
        }
        let mut collapsed: BTreeMap<String, IdentifierOp> = BTreeMap::new();
        for (id, op) in self.identifiers.drain(..) {
            collapsed.insert(id, op);
        }
        #[allow(clippy::type_complexity)]
        let mut upserts: Vec<(
            String,
            Option<String>,
            Option<u8>,
            Option<f64>,
            Option<String>,
            Outcome,
            Option<i64>,
            i64,
        )> = Vec::new();
        let mut demotes: Vec<String> = Vec::new();
        for (id, op) in collapsed {
            match op {
                IdentifierOp::Upsert {
                    target,
                    tier,
                    confidence,
                    method,
                    outcome,
                    candidates,
                    revision,
                } => upserts.push((
                    id, target, tier, confidence, method, outcome, candidates, revision,
                )),
                IdentifierOp::Demote => demotes.push(id),
            }
        }

        // Demotions first: delete the overlay row and clear the denormalized column
        // (equivalent to `demote_identifier`). Upsert and demote keys are disjoint
        // after the collapse, so order between the two groups is immaterial.
        for chunk in demotes.chunks(FLUSH_CHUNK) {
            let ph = placeholders(chunk.len());
            tx.execute(
                &format!("DELETE FROM identifier_resolutions WHERE identifier_id IN ({ph})"),
                params_from_iter(chunk.iter()),
            )?;
            tx.execute(
                &format!(
                    "UPDATE identifiers SET target_symbol_id = NULL WHERE identifier_id IN ({ph})"
                ),
                params_from_iter(chunk.iter()),
            )?;
        }

        for chunk in upserts.chunks(FLUSH_CHUNK) {
            let values = repeat_row_placeholders(chunk.len(), 8);
            let sql = format!(
                "INSERT INTO identifier_resolutions \
                 (identifier_id, target_symbol_id, tier, confidence, method, outcome, candidates, resolved_at_revision) \
                 VALUES {values} \
                 ON CONFLICT(identifier_id) DO UPDATE SET \
                   target_symbol_id = excluded.target_symbol_id, \
                   tier = excluded.tier, \
                   confidence = excluded.confidence, \
                   method = excluded.method, \
                   outcome = excluded.outcome, \
                   candidates = excluded.candidates, \
                   resolved_at_revision = excluded.resolved_at_revision"
            );
            let mut binds: Vec<Value> = Vec::with_capacity(chunk.len() * 8);
            let mut ids: Vec<Value> = Vec::with_capacity(chunk.len());
            for (id, target, tier, confidence, method, outcome, candidates, revision) in chunk {
                binds.push(Value::Text(id.clone()));
                binds.push(opt_text(target.clone()));
                binds.push(opt_int(tier.map(i64::from)));
                binds.push(opt_real(*confidence));
                binds.push(opt_text(method.clone()));
                binds.push(Value::Text(outcome.as_str().to_string()));
                binds.push(opt_int(*candidates));
                binds.push(Value::Integer(*revision));
                ids.push(Value::Text(id.clone()));
            }
            tx.execute(&sql, params_from_iter(binds.iter()))?;

            // Denormalized `identifiers.target_symbol_id`, kept in lockstep with the
            // overlay exactly as `record_identifier_outcome` does. The correlated
            // subquery reads the row we just wrote above, so a resolved outcome sets
            // the target and every non-resolved outcome clears it (overlay target is
            // NULL) — one statement per chunk instead of one UPDATE per row.
            let update_sql = format!(
                "UPDATE identifiers SET target_symbol_id = ( \
                     SELECT ir.target_symbol_id FROM identifier_resolutions ir \
                     WHERE ir.identifier_id = identifiers.identifier_id \
                 ) WHERE identifier_id IN ({})",
                placeholders(ids.len())
            );
            tx.execute(&update_sql, params_from_iter(ids.iter()))?;
        }
        Ok(())
    }
}

fn opt_text(value: Option<String>) -> Value {
    value.map(Value::Text).unwrap_or(Value::Null)
}

fn opt_int(value: Option<i64>) -> Value {
    value.map(Value::Integer).unwrap_or(Value::Null)
}

fn opt_real(value: Option<f64>) -> Value {
    value.map(Value::Real).unwrap_or(Value::Null)
}

/// `"(?, ?, ...), (?, ?, ...)"` — `rows` groups of `cols` placeholders each, for a
/// multi-row `VALUES` clause.
fn repeat_row_placeholders(rows: usize, cols: usize) -> String {
    let one = format!("({})", placeholders(cols));
    vec![one; rows].join(", ")
}

// ---------------------------------------------------------------------------
// Worklist queries
// ---------------------------------------------------------------------------

const PENDING_COLUMNS: &str = "pr.pending_relationship_id, pr.from_symbol_id, \
     pr.caller_scope_symbol_id, pr.file_id, pr.path, f.language, pr.kind, \
     pr.target_display_name, pr.target_terminal_name, pr.target_receiver, \
     pr.target_namespace_json, pr.target_import_context, pr.start_line, \
     pr.start_byte, pr.end_byte, pr.confidence";

const IDENTIFIER_COLUMNS: &str = "i.identifier_id, i.file_id, i.path, i.language, \
     i.name, i.kind, i.containing_symbol_id, i.start_line, i.start_byte, \
     i.end_byte, json_extract(i.metadata_json, '$.receiver'), \
     json_extract(i.metadata_json, '$.receiver_qualifier'), \
     json_extract(i.metadata_json, '$.import_context'), i.confidence";

fn map_pending(row: &rusqlite::Row<'_>) -> rusqlite::Result<PendingWorkItem> {
    Ok(PendingWorkItem {
        pending_relationship_id: row.get(0)?,
        from_symbol_id: row.get(1)?,
        caller_scope_symbol_id: row.get(2)?,
        file_id: row.get(3)?,
        path: row.get(4)?,
        language: row.get(5)?,
        kind: row.get(6)?,
        target_display_name: row.get(7)?,
        target_terminal_name: row.get(8)?,
        target_receiver: row.get(9)?,
        target_namespace_json: row.get(10)?,
        target_import_context: row.get(11)?,
        start_line: row.get(12)?,
        start_byte: row.get(13)?,
        end_byte: row.get(14)?,
        confidence: row.get(15)?,
    })
}

fn map_identifier(row: &rusqlite::Row<'_>) -> rusqlite::Result<IdentifierWorkItem> {
    Ok(IdentifierWorkItem {
        identifier_id: row.get(0)?,
        file_id: row.get(1)?,
        path: row.get(2)?,
        language: row.get(3)?,
        name: row.get(4)?,
        kind: row.get(5)?,
        containing_symbol_id: row.get(6)?,
        start_line: row.get(7)?,
        start_byte: row.get(8)?,
        end_byte: row.get(9)?,
        receiver: row.get(10)?,
        receiver_qualifier: row.get(11)?,
        import_context: row.get(12)?,
        confidence: row.get(13)?,
    })
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
}

/// Max distinct names/ids bound into one `IN (...)` clause. Kept well under
/// SQLite's compiled `SQLITE_MAX_VARIABLE_NUMBER` (default 32766) so that a
/// by-names worklist which binds `2 * N` variables (terminal + receiver) cannot
/// overflow it on a large delta. A delta touching more distinct names is split
/// into chunks whose results are unioned; see [`chunked_by`].
const WORKLIST_QUERY_CHUNK: usize = 8000;

/// Run `run` once per `WORKLIST_QUERY_CHUNK`-sized chunk of `items` and union the
/// rows. A single query never yields intra-chunk duplicates, but a `terminal IN
/// (..) OR receiver IN (..)` row can match one chunk on its terminal name and
/// another on its receiver name, so results are de-duplicated by `key` and then
/// re-sorted by `key` to preserve the deterministic per-query `ORDER BY <id>`.
fn chunked_by<T>(
    items: &[&str],
    key: impl Fn(&T) -> String,
    mut run: impl FnMut(&[&str]) -> rusqlite::Result<Vec<T>>,
) -> rusqlite::Result<Vec<T>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    if items.len() <= WORKLIST_QUERY_CHUNK {
        return run(items);
    }
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<T> = Vec::new();
    for chunk in items.chunks(WORKLIST_QUERY_CHUNK) {
        for item in run(chunk)? {
            if seen.insert(key(&item)) {
                out.push(item);
            }
        }
    }
    out.sort_by_key(|item| key(item));
    Ok(out)
}

/// Unresolved pending rows whose terminal OR receiver name is in `names`.
pub fn worklist_unresolved_pending_by_names(
    conn: &Connection,
    names: &[&str],
) -> rusqlite::Result<Vec<PendingWorkItem>> {
    chunked_by(
        names,
        |item: &PendingWorkItem| item.pending_relationship_id.clone(),
        |chunk| {
            let ph = placeholders(chunk.len());
            let sql = format!(
                "SELECT {PENDING_COLUMNS} \
                 FROM pending_relationships pr \
                 JOIN files f ON f.file_id = pr.file_id \
                 WHERE pr.pending_relationship_id NOT IN \
                       (SELECT pending_relationship_id FROM pending_resolutions) \
                   AND (pr.target_terminal_name IN ({ph}) OR pr.target_receiver IN ({ph})) \
                 ORDER BY pr.pending_relationship_id"
            );
            let bind = chunk.iter().chain(chunk.iter());
            let mut stmt = conn.prepare(&sql)?;
            stmt.query_map(params_from_iter(bind), map_pending)?
                .collect::<Result<Vec<_>, _>>()
        },
    )
}

/// Every unresolved pending row (Full scope / v3 backfill).
pub fn worklist_full_pending(conn: &Connection) -> rusqlite::Result<Vec<PendingWorkItem>> {
    let sql = format!(
        "SELECT {PENDING_COLUMNS} \
         FROM pending_relationships pr \
         JOIN files f ON f.file_id = pr.file_id \
         WHERE pr.pending_relationship_id NOT IN \
               (SELECT pending_relationship_id FROM pending_resolutions) \
         ORDER BY pr.pending_relationship_id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], map_pending)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Resolved pending rows (overlay present) whose terminal OR receiver name is in
/// `names` — used to re-check uniqueness and demote on regression.
pub fn worklist_resolved_pending_by_names(
    conn: &Connection,
    names: &[&str],
) -> rusqlite::Result<Vec<ResolvedPendingWorkItem>> {
    chunked_by(
        names,
        |item: &ResolvedPendingWorkItem| item.pending.pending_relationship_id.clone(),
        |chunk| {
            let ph = placeholders(chunk.len());
            let sql = format!(
                "SELECT {PENDING_COLUMNS}, res.target_symbol_id, res.tier, res.confidence, res.method \
                 FROM pending_resolutions res \
                 JOIN pending_relationships pr ON pr.pending_relationship_id = res.pending_relationship_id \
                 JOIN files f ON f.file_id = pr.file_id \
                 WHERE pr.target_terminal_name IN ({ph}) OR pr.target_receiver IN ({ph}) \
                 ORDER BY pr.pending_relationship_id"
            );
            let bind = chunk.iter().chain(chunk.iter());
            let mut stmt = conn.prepare(&sql)?;
            stmt.query_map(params_from_iter(bind), |row| {
                Ok(ResolvedPendingWorkItem {
                    pending: map_pending(row)?,
                    target_symbol_id: row.get(16)?,
                    tier: row.get(17)?,
                    confidence: row.get(18)?,
                    method: row.get(19)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
        },
    )
}

/// Every resolved pending row (overlay present). Used by Full scope to re-check
/// uniqueness and demote stale overlay rows before filling unresolved rows.
pub fn worklist_resolved_pending(
    conn: &Connection,
) -> rusqlite::Result<Vec<ResolvedPendingWorkItem>> {
    let sql = format!(
        "SELECT {PENDING_COLUMNS}, res.target_symbol_id, res.tier, res.confidence, res.method \
         FROM pending_resolutions res \
         JOIN pending_relationships pr ON pr.pending_relationship_id = res.pending_relationship_id \
         JOIN files f ON f.file_id = pr.file_id \
         ORDER BY pr.pending_relationship_id"
    );
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_map([], |row| {
        Ok(ResolvedPendingWorkItem {
            pending: map_pending(row)?,
            target_symbol_id: row.get(16)?,
            tier: row.get(17)?,
            confidence: row.get(18)?,
            method: row.get(19)?,
        })
    })?
    .collect::<Result<Vec<_>, _>>()
}

/// Never-attempted identifiers (no overlay row) whose name is in `names`.
pub fn worklist_never_attempted_identifiers_by_names(
    conn: &Connection,
    names: &[&str],
) -> rusqlite::Result<Vec<IdentifierWorkItem>> {
    chunked_by(
        names,
        |item: &IdentifierWorkItem| item.identifier_id.clone(),
        |chunk| {
            let ph = placeholders(chunk.len());
            let sql = format!(
                "SELECT {IDENTIFIER_COLUMNS} \
                 FROM identifiers i \
                 WHERE i.identifier_id NOT IN (SELECT identifier_id FROM identifier_resolutions) \
                   AND i.name IN ({ph}) \
                 ORDER BY i.identifier_id"
            );
            let mut stmt = conn.prepare(&sql)?;
            stmt.query_map(params_from_iter(chunk.iter()), map_identifier)?
                .collect::<Result<Vec<_>, _>>()
        },
    )
}

/// Never-attempted identifiers (no overlay row) in any of `file_ids`.
pub fn worklist_never_attempted_identifiers_by_files(
    conn: &Connection,
    file_ids: &[&str],
) -> rusqlite::Result<Vec<IdentifierWorkItem>> {
    chunked_by(
        file_ids,
        |item: &IdentifierWorkItem| item.identifier_id.clone(),
        |chunk| {
            let ph = placeholders(chunk.len());
            let sql = format!(
                "SELECT {IDENTIFIER_COLUMNS} \
                 FROM identifiers i \
                 WHERE i.identifier_id NOT IN (SELECT identifier_id FROM identifier_resolutions) \
                   AND i.file_id IN ({ph}) \
                 ORDER BY i.identifier_id"
            );
            let mut stmt = conn.prepare(&sql)?;
            stmt.query_map(params_from_iter(chunk.iter()), map_identifier)?
                .collect::<Result<Vec<_>, _>>()
        },
    )
}

/// Resolved identifier rows (overlay present) whose name is in `names`.
pub fn worklist_resolved_identifiers_by_names(
    conn: &Connection,
    names: &[&str],
) -> rusqlite::Result<Vec<ResolvedIdentifierWorkItem>> {
    chunked_by(
        names,
        |item: &ResolvedIdentifierWorkItem| item.identifier.identifier_id.clone(),
        |chunk| {
            let ph = placeholders(chunk.len());
            // Matches the pending worklist: a resolution can hang off the receiver
            // as much as the member, so touching only the receiver's type name has
            // to sweep the row. Keying on `i.name` alone left `Color.Red` claiming
            // an exact target after a second `Color` appeared.
            let sql = format!(
                "SELECT {IDENTIFIER_COLUMNS}, r.target_symbol_id, r.tier, r.confidence, r.method, \
                        r.outcome, r.candidates \
                 FROM identifier_resolutions r \
                 JOIN identifiers i ON i.identifier_id = r.identifier_id \
                 WHERE i.name IN ({ph}) \
                    OR json_extract(i.metadata_json, '$.receiver') IN ({ph}) \
                 ORDER BY i.identifier_id"
            );
            let bind = chunk.iter().chain(chunk.iter());
            let mut stmt = conn.prepare(&sql)?;
            stmt.query_map(params_from_iter(bind), map_resolved_identifier)?
                .collect::<Result<Vec<_>, _>>()
        },
    )
}

/// Resolved identifier rows whose overlay currently carries a target. Used by
/// Full scope to re-run and demote stale generic identifier resolutions.
pub fn worklist_resolved_identifiers(
    conn: &Connection,
) -> rusqlite::Result<Vec<ResolvedIdentifierWorkItem>> {
    let sql = format!(
        "SELECT {IDENTIFIER_COLUMNS}, r.target_symbol_id, r.tier, r.confidence, r.method, \
                r.outcome, r.candidates \
         FROM identifier_resolutions r \
         JOIN identifiers i ON i.identifier_id = r.identifier_id \
         WHERE r.target_symbol_id IS NOT NULL \
         ORDER BY i.identifier_id"
    );
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_map([], map_resolved_identifier)?
        .collect::<Result<Vec<_>, _>>()
}

/// Every identifier that still needs resolution work: never-attempted OR
/// attempted-with-NULL-target (ambiguous/missing/no_context). Used for Full scope
/// and v3-artifact backfill.
pub fn worklist_full_identifiers(conn: &Connection) -> rusqlite::Result<Vec<IdentifierWorkItem>> {
    let sql = format!(
        "SELECT {IDENTIFIER_COLUMNS} \
         FROM identifiers i \
         LEFT JOIN identifier_resolutions r ON r.identifier_id = i.identifier_id \
         WHERE r.identifier_id IS NULL OR r.target_symbol_id IS NULL \
         ORDER BY i.identifier_id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], map_identifier)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn map_resolved_identifier(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ResolvedIdentifierWorkItem> {
    let outcome_str: String = row.get(18)?;
    let outcome = Outcome::parse(&outcome_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            18,
            rusqlite::types::Type::Text,
            format!("unknown identifier resolution outcome: {outcome_str}").into(),
        )
    })?;
    Ok(ResolvedIdentifierWorkItem {
        identifier: map_identifier(row)?,
        target_symbol_id: row.get(14)?,
        tier: row.get(15)?,
        confidence: row.get(16)?,
        method: row.get(17)?,
        outcome,
        candidates: row.get(19)?,
    })
}

// ---------------------------------------------------------------------------
// Report aggregation
// ---------------------------------------------------------------------------

pub fn resolution_report(conn: &Connection) -> rusqlite::Result<Vec<ResolutionReportRow>> {
    let mut stmt = conn.prepare(
        "SELECT language, origin, raw_kind, tier, method, outcome, span_present, COUNT(*) \
         FROM ( \
           SELECT f.language AS language, 'relationship' AS origin, r.kind AS raw_kind, \
                  1 AS tier, 'extraction_direct' AS method, 'resolved' AS outcome, \
                  CASE WHEN r.start_column IS NOT NULL AND r.end_line IS NOT NULL \
                              AND r.end_column IS NOT NULL AND r.start_byte IS NOT NULL \
                              AND r.end_byte IS NOT NULL THEN 1 ELSE 0 END AS span_present \
           FROM relationships r JOIN files f ON f.file_id = r.file_id \
           WHERE r.kind IN ('calls', 'extends', 'implements', 'imports', \
                            'instantiates', 'references', 'uses') \
           UNION ALL \
           SELECT f.language AS language, 'pending_relationship' AS origin, pr.kind AS raw_kind, \
                  p.tier AS tier, p.method AS method, \
                  CASE WHEN p.pending_relationship_id IS NOT NULL THEN 'resolved' \
                       WHEN pr.kind IN ('imports', 'references') THEN 'unattempted' \
                       ELSE 'unresolved_pending' END AS outcome, \
                  CASE WHEN pr.start_column IS NOT NULL AND pr.end_line IS NOT NULL \
                              AND pr.end_column IS NOT NULL AND pr.start_byte IS NOT NULL \
                              AND pr.end_byte IS NOT NULL THEN 1 ELSE 0 END AS span_present \
           FROM pending_relationships pr \
           JOIN files f ON f.file_id = pr.file_id \
           LEFT JOIN pending_resolutions p \
             ON p.pending_relationship_id = pr.pending_relationship_id \
           WHERE pr.kind IN ('calls', 'extends', 'implements', 'imports', \
                             'instantiates', 'references', 'uses') \
           UNION ALL \
           SELECT i.language AS language, 'identifier' AS origin, i.kind AS raw_kind, \
                  ir.tier AS tier, ir.method AS method, \
                  COALESCE(ir.outcome, 'unattempted') AS outcome, \
                  CASE WHEN i.start_column IS NOT NULL AND i.end_line IS NOT NULL \
                              AND i.end_column IS NOT NULL AND i.start_byte IS NOT NULL \
                              AND i.end_byte IS NOT NULL THEN 1 ELSE 0 END AS span_present \
           FROM identifiers i \
           LEFT JOIN identifier_resolutions ir ON ir.identifier_id = i.identifier_id \
         ) \
         GROUP BY language, origin, raw_kind, tier, method, outcome, span_present \
         ORDER BY language, origin, raw_kind, tier, method, outcome, span_present",
    )?;
    let rows = stmt
        .query_map([], |row| {
            let origin: String = row.get(1)?;
            let raw_kind: String = row.get(2)?;
            let tier: Option<i64> = row.get(3)?;
            Ok(ResolutionReportRow {
                language: row.get(0)?,
                canonical_kind: canonical_reference_kind(&origin, &raw_kind)
                    .unwrap_or("unmapped")
                    .to_string(),
                origin,
                raw_kind,
                tier: tier.map(|t| t as u8),
                method: row.get(4)?,
                outcome: row.get(5)?,
                span_present: row.get::<_, i64>(6)? != 0,
                count: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Resolution-status metadata (separate upsert; never widens ArtifactMetadata::rows)
// ---------------------------------------------------------------------------

/// Upsert the three durable `reference_resolution_*` keys into `artifact_metadata`.
/// This is a SEPARATE upsert from `initialize_metadata` and does not touch
/// `ArtifactMetadata::rows()`.
pub fn write_resolution_metadata(
    conn: &Connection,
    status: ResolutionStatus,
    version: i64,
    last_full_revision: i64,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO artifact_metadata (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )?;
    stmt.execute(params![KEY_RESOLUTION_STATUS, status.as_str()])?;
    stmt.execute(params![KEY_RESOLUTION_VERSION, version.to_string()])?;
    stmt.execute(params![
        KEY_RESOLUTION_LAST_FULL_REVISION,
        last_full_revision.to_string()
    ])?;
    Ok(())
}

/// Read the durable resolution-status metadata, or `None` when the keys have never
/// been written (an artifact with no resolution pass yet).
pub fn read_resolution_metadata(conn: &Connection) -> rusqlite::Result<Option<ResolutionMetadata>> {
    let mut stmt =
        conn.prepare("SELECT key, value FROM artifact_metadata WHERE key IN (?1, ?2, ?3)")?;
    let entries: BTreeMap<String, String> = stmt
        .query_map(
            params![
                KEY_RESOLUTION_STATUS,
                KEY_RESOLUTION_VERSION,
                KEY_RESOLUTION_LAST_FULL_REVISION
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?
        .collect::<Result<_, _>>()?;

    let Some(status_raw) = entries.get(KEY_RESOLUTION_STATUS) else {
        return Ok(None);
    };
    let status = ResolutionStatus::parse(status_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown resolution status: {status_raw}").into(),
        )
    })?;
    let version = parse_i64_metadata(entries.get(KEY_RESOLUTION_VERSION), KEY_RESOLUTION_VERSION)?;
    let last_full_revision = parse_i64_metadata(
        entries.get(KEY_RESOLUTION_LAST_FULL_REVISION),
        KEY_RESOLUTION_LAST_FULL_REVISION,
    )?;
    Ok(Some(ResolutionMetadata {
        status,
        version,
        last_full_revision,
    }))
}

fn parse_i64_metadata(value: Option<&String>, key: &str) -> rusqlite::Result<i64> {
    value
        .ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("missing resolution metadata key: {key}").into(),
            )
        })?
        .parse::<i64>()
        .map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("invalid integer for {key}: {err}").into(),
            )
        })
}
