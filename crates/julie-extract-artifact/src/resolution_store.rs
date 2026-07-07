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

/// One aggregated resolution-report row: per language, per tier, per outcome.
/// `tier` is `None` for identifier outcomes that never reached a tier (ambiguous,
/// missing, no_context).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionReportRow {
    pub language: String,
    pub tier: Option<u8>,
    pub outcome: String,
    pub count: i64,
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub code_context: Option<String>,
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
// Worklist queries
// ---------------------------------------------------------------------------

const PENDING_COLUMNS: &str = "pr.pending_relationship_id, pr.from_symbol_id, \
     pr.caller_scope_symbol_id, pr.file_id, pr.path, f.language, pr.kind, \
     pr.target_display_name, pr.target_terminal_name, pr.target_receiver, \
     pr.target_namespace_json, pr.target_import_context, pr.start_line, \
     pr.start_byte, pr.end_byte";

const IDENTIFIER_COLUMNS: &str = "i.identifier_id, i.file_id, i.path, i.language, \
     i.name, i.kind, i.containing_symbol_id, i.start_line, i.start_byte, \
     i.end_byte, i.code_context";

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
        code_context: row.get(10)?,
    })
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
}

/// Unresolved pending rows whose terminal OR receiver name is in `names`.
pub fn worklist_unresolved_pending_by_names(
    conn: &Connection,
    names: &[&str],
) -> rusqlite::Result<Vec<PendingWorkItem>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let ph = placeholders(names.len());
    let sql = format!(
        "SELECT {PENDING_COLUMNS} \
         FROM pending_relationships pr \
         JOIN files f ON f.file_id = pr.file_id \
         WHERE pr.pending_relationship_id NOT IN \
               (SELECT pending_relationship_id FROM pending_resolutions) \
           AND (pr.target_terminal_name IN ({ph}) OR pr.target_receiver IN ({ph})) \
         ORDER BY pr.pending_relationship_id"
    );
    let bind = names.iter().chain(names.iter());
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(bind), map_pending)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
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
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let ph = placeholders(names.len());
    let sql = format!(
        "SELECT {PENDING_COLUMNS}, res.target_symbol_id, res.tier, res.confidence, res.method \
         FROM pending_resolutions res \
         JOIN pending_relationships pr ON pr.pending_relationship_id = res.pending_relationship_id \
         JOIN files f ON f.file_id = pr.file_id \
         WHERE pr.target_terminal_name IN ({ph}) OR pr.target_receiver IN ({ph}) \
         ORDER BY pr.pending_relationship_id"
    );
    let bind = names.iter().chain(names.iter());
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(bind), |row| {
            Ok(ResolvedPendingWorkItem {
                pending: map_pending(row)?,
                target_symbol_id: row.get(15)?,
                tier: row.get(16)?,
                confidence: row.get(17)?,
                method: row.get(18)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Never-attempted identifiers (no overlay row) whose name is in `names`.
pub fn worklist_never_attempted_identifiers_by_names(
    conn: &Connection,
    names: &[&str],
) -> rusqlite::Result<Vec<IdentifierWorkItem>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let ph = placeholders(names.len());
    let sql = format!(
        "SELECT {IDENTIFIER_COLUMNS} \
         FROM identifiers i \
         WHERE i.identifier_id NOT IN (SELECT identifier_id FROM identifier_resolutions) \
           AND i.name IN ({ph}) \
         ORDER BY i.identifier_id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(names.iter()), map_identifier)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Never-attempted identifiers (no overlay row) in any of `file_ids`.
pub fn worklist_never_attempted_identifiers_by_files(
    conn: &Connection,
    file_ids: &[&str],
) -> rusqlite::Result<Vec<IdentifierWorkItem>> {
    if file_ids.is_empty() {
        return Ok(Vec::new());
    }
    let ph = placeholders(file_ids.len());
    let sql = format!(
        "SELECT {IDENTIFIER_COLUMNS} \
         FROM identifiers i \
         WHERE i.identifier_id NOT IN (SELECT identifier_id FROM identifier_resolutions) \
           AND i.file_id IN ({ph}) \
         ORDER BY i.identifier_id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(file_ids.iter()), map_identifier)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Resolved identifier rows (overlay present) whose name is in `names`.
pub fn worklist_resolved_identifiers_by_names(
    conn: &Connection,
    names: &[&str],
) -> rusqlite::Result<Vec<ResolvedIdentifierWorkItem>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let ph = placeholders(names.len());
    let sql = format!(
        "SELECT {IDENTIFIER_COLUMNS}, r.target_symbol_id, r.tier, r.confidence, r.method, \
                r.outcome, r.candidates \
         FROM identifier_resolutions r \
         JOIN identifiers i ON i.identifier_id = r.identifier_id \
         WHERE i.name IN ({ph}) \
         ORDER BY i.identifier_id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(names.iter()), map_resolved_identifier)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
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
    let outcome_str: String = row.get(15)?;
    let outcome = Outcome::parse(&outcome_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            15,
            rusqlite::types::Type::Text,
            format!("unknown identifier resolution outcome: {outcome_str}").into(),
        )
    })?;
    Ok(ResolvedIdentifierWorkItem {
        identifier: map_identifier(row)?,
        target_symbol_id: row.get(11)?,
        tier: row.get(12)?,
        confidence: row.get(13)?,
        method: row.get(14)?,
        outcome,
        candidates: row.get(16)?,
    })
}

// ---------------------------------------------------------------------------
// Report aggregation
// ---------------------------------------------------------------------------

/// Aggregate resolution outcomes into per-language, per-tier, per-outcome counts.
/// Resolved pending rows contribute `outcome='resolved'` with their tier; identifier
/// overlay rows contribute their recorded outcome (tier NULL for non-resolved).
pub fn resolution_report(conn: &Connection) -> rusqlite::Result<Vec<ResolutionReportRow>> {
    let mut stmt = conn.prepare(
        "SELECT language, tier, outcome, cnt FROM ( \
           SELECT f.language AS language, p.tier AS tier, 'resolved' AS outcome, \
                  COUNT(*) AS cnt \
           FROM pending_resolutions p \
           JOIN pending_relationships pr ON pr.pending_relationship_id = p.pending_relationship_id \
           JOIN files f ON f.file_id = pr.file_id \
           GROUP BY f.language, p.tier \
           UNION ALL \
           SELECT i.language AS language, r.tier AS tier, r.outcome AS outcome, \
                  COUNT(*) AS cnt \
           FROM identifier_resolutions r \
           JOIN identifiers i ON i.identifier_id = r.identifier_id \
           GROUP BY i.language, r.tier, r.outcome \
         ) ORDER BY language, tier, outcome",
    )?;
    let rows = stmt
        .query_map([], |row| {
            let tier: Option<i64> = row.get(1)?;
            Ok(ResolutionReportRow {
                language: row.get(0)?,
                tier: tier.map(|t| t as u8),
                outcome: row.get(2)?,
                count: row.get(3)?,
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
