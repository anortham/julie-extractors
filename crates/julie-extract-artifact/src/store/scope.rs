use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};

/// Durable scope-journal feature version stored in `store_meta`.
pub const RESOLUTION_SCOPE_JOURNAL_VERSION: i64 = 1;
/// Maximum touched paths retained in one incremental-resolution batch.
pub const RESOLUTION_SCOPE_MAX_CHANGES: usize = 512;

const RESOLUTION_SCOPE_CHANGES_HASH_DOMAIN: &[u8] = b"julie-resolution-scope-changes-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
/// Preserved exact predecessor and current journal head for one view.
pub struct ResolutionScopeState {
    pub view_id: String,
    pub predecessor_manifest_generation: i64,
    pub predecessor_manifest_hash: String,
    pub base_id: String,
    pub delta_generation: i64,
    pub resolver_output_epoch: i64,
    pub current_manifest_generation: i64,
    pub current_manifest_hash: String,
    pub latest_transition_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One deterministic touched-path payload in a scope transition.
pub struct ResolutionScopeChange {
    pub ordinal: i64,
    pub path: String,
    pub old_version_id: Option<i64>,
    pub new_version_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One immutable manifest-transition header and its ordered child payload.
pub struct ResolutionScopeBatch {
    pub transition_id: i64,
    pub view_id: String,
    pub previous_transition_id: Option<i64>,
    pub from_manifest_generation: Option<i64>,
    pub from_manifest_hash: Option<String>,
    pub to_manifest_generation: i64,
    pub to_manifest_hash: String,
    pub scope_usable: bool,
    pub predecessor_manifest_generation: Option<i64>,
    pub predecessor_manifest_hash: Option<String>,
    pub base_id: Option<String>,
    pub delta_generation: Option<i64>,
    pub resolver_output_epoch: Option<i64>,
    pub change_count: i64,
    pub changes_hash: String,
    pub request_id: String,
    pub created_at: String,
    pub changes: Vec<ResolutionScopeChange>,
}

#[derive(Debug)]
/// Typed feature-upgrade or scope-batch validation failure.
pub enum ResolutionScopeError {
    InvalidBatch { transition_id: i64, detail: String },
    UnsupportedJournalVersion { found: String, supported: i64 },
    Sqlite(rusqlite::Error),
}

impl fmt::Display for ResolutionScopeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBatch {
                transition_id,
                detail,
            } => write!(
                formatter,
                "resolution scope transition {transition_id} is invalid: {detail}"
            ),
            Self::UnsupportedJournalVersion { found, supported } => write!(
                formatter,
                "resolution scope journal version {found:?} is not supported; expected {supported}"
            ),
            Self::Sqlite(error) => error.fmt(formatter),
        }
    }
}

impl Error for ResolutionScopeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::InvalidBatch { .. } | Self::UnsupportedJournalVersion { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for ResolutionScopeError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

/// Atomically installs the additive scope-journal feature when it is absent.
pub fn ensure_resolution_scope_feature(
    connection: &Connection,
) -> Result<(), ResolutionScopeError> {
    if let Some(found) = resolution_scope_journal_version(connection)?
        && found != RESOLUTION_SCOPE_JOURNAL_VERSION
    {
        return Err(ResolutionScopeError::UnsupportedJournalVersion {
            found: found.to_string(),
            supported: RESOLUTION_SCOPE_JOURNAL_VERSION,
        });
    }

    if connection.is_autocommit() {
        connection.execute_batch("BEGIN IMMEDIATE;")?;
        if let Err(error) = connection.execute_batch(RESOLUTION_SCOPE_FEATURE_SQL) {
            let _ = connection.execute_batch("ROLLBACK;");
            return Err(error.into());
        }
        connection.execute_batch("COMMIT;")?;
    } else {
        connection.execute_batch(RESOLUTION_SCOPE_FEATURE_SQL)?;
    }
    Ok(())
}

/// Reads the optional scope-journal feature version without mutating the store.
pub fn resolution_scope_journal_version(
    connection: &Connection,
) -> Result<Option<i64>, ResolutionScopeError> {
    let value = connection
        .query_row(
            "SELECT value FROM store_meta WHERE key='resolution_scope_journal_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    value
        .map(|value| {
            value
                .parse::<i64>()
                .map_err(|_| ResolutionScopeError::UnsupportedJournalVersion {
                    found: value,
                    supported: RESOLUTION_SCOPE_JOURNAL_VERSION,
                })
        })
        .transpose()
}

/// Reads the preserved scope state for a view when the feature is installed.
pub fn resolution_scope_state(
    connection: &Connection,
    view_id: &str,
) -> Result<Option<ResolutionScopeState>, ResolutionScopeError> {
    if resolution_scope_journal_version(connection)?.is_none() {
        return Ok(None);
    }
    connection
        .query_row(
            "SELECT view_id,predecessor_manifest_generation,predecessor_manifest_hash,
                    base_id,delta_generation,resolver_output_epoch,current_manifest_generation,
                    current_manifest_hash,latest_transition_id
             FROM resolution_scope_states WHERE view_id=?1",
            [view_id],
            scope_state_from_row,
        )
        .optional()
        .map_err(Into::into)
}

/// Reads a scope batch and its children without validating their identities.
pub fn resolution_scope_batch(
    connection: &Connection,
    transition_id: i64,
) -> Result<Option<ResolutionScopeBatch>, ResolutionScopeError> {
    if resolution_scope_journal_version(connection)?.is_none() {
        return Ok(None);
    }
    let Some(mut batch) = connection
        .query_row(
            "SELECT transition_id,view_id,previous_transition_id,from_manifest_generation,
                    from_manifest_hash,to_manifest_generation,to_manifest_hash,scope_usable,
                    predecessor_manifest_generation,predecessor_manifest_hash,base_id,
                    delta_generation,resolver_output_epoch,change_count,changes_hash,request_id,
                    created_at
             FROM resolution_scope_batches WHERE transition_id=?1",
            [transition_id],
            scope_batch_from_row,
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut statement = connection.prepare(
        "SELECT ordinal,path,old_version_id,new_version_id
         FROM resolution_scope_changes WHERE transition_id=?1 ORDER BY ordinal",
    )?;
    batch.changes = statement
        .query_map([transition_id], |row| {
            Ok(ResolutionScopeChange {
                ordinal: row.get(0)?,
                path: row.get(1)?,
                old_version_id: row.get(2)?,
                new_version_id: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(batch))
}

/// Loads and validates one scope batch against its manifests and predecessor.
pub fn validate_resolution_scope_batch(
    connection: &Connection,
    transition_id: i64,
) -> Result<Option<ResolutionScopeBatch>, ResolutionScopeError> {
    let Some(batch) = resolution_scope_batch(connection, transition_id)? else {
        return Ok(None);
    };
    if batch.change_count != batch.changes.len() as i64 {
        return Err(invalid_batch(
            transition_id,
            "stored change count does not match child rows",
        ));
    }
    if batch
        .changes
        .iter()
        .enumerate()
        .any(|(ordinal, change)| change.ordinal != ordinal as i64)
    {
        return Err(invalid_batch(
            transition_id,
            "change ordinals are not contiguous",
        ));
    }
    if batch.changes_hash != changes_hash(&batch.changes) {
        return Err(invalid_batch(
            transition_id,
            "stored change hash does not match child rows",
        ));
    }
    if !batch.scope_usable && !batch.changes.is_empty() {
        return Err(invalid_batch(
            transition_id,
            "scope-unusable batch contains child rows",
        ));
    }
    validate_manifest_identity(
        connection,
        transition_id,
        &batch.view_id,
        batch.to_manifest_generation,
        &batch.to_manifest_hash,
        "target",
    )?;
    if let (Some(generation), Some(hash)) = (
        batch.from_manifest_generation,
        batch.from_manifest_hash.as_deref(),
    ) {
        validate_manifest_identity(
            connection,
            transition_id,
            &batch.view_id,
            generation,
            hash,
            "source",
        )?;
    }
    if let Some(previous_transition_id) = batch.previous_transition_id {
        if previous_transition_id >= transition_id {
            return Err(invalid_batch(
                transition_id,
                "previous transition is not earlier than this transition",
            ));
        }
        let previous = resolution_scope_batch(connection, previous_transition_id)?
            .ok_or_else(|| invalid_batch(transition_id, "previous transition is missing"))?;
        let discontinuous = previous.view_id != batch.view_id
            || Some(previous.to_manifest_generation) != batch.from_manifest_generation
            || Some(previous.to_manifest_hash.as_str()) != batch.from_manifest_hash.as_deref();
        if batch.scope_usable && discontinuous {
            return Err(invalid_batch(
                transition_id,
                "previous transition target does not match this transition source",
            ));
        }
    }
    if batch.scope_usable {
        validate_usable_batch(connection, &batch)?;
    }
    Ok(Some(batch))
}

fn validate_manifest_identity(
    connection: &Connection,
    transition_id: i64,
    view_id: &str,
    generation: i64,
    expected_hash: &str,
    role: &str,
) -> Result<(), ResolutionScopeError> {
    let found = connection
        .query_row(
            "SELECT manifest_hash FROM manifests WHERE view_id=?1 AND generation=?2",
            params![view_id, generation],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if found.as_deref() != Some(expected_hash) {
        return Err(invalid_batch(
            transition_id,
            &format!("{role} manifest identity does not match the immutable manifest"),
        ));
    }
    Ok(())
}

fn validate_usable_batch(
    connection: &Connection,
    batch: &ResolutionScopeBatch,
) -> Result<(), ResolutionScopeError> {
    let Some(from_generation) = batch.from_manifest_generation else {
        return Err(invalid_batch(
            batch.transition_id,
            "usable batch has no source manifest",
        ));
    };
    let old_entries = manifest_versions(connection, &batch.view_id, Some(from_generation))?;
    let new_entries = manifest_versions(
        connection,
        &batch.view_id,
        Some(batch.to_manifest_generation),
    )?;
    if scope_changes(&old_entries, &new_entries) != batch.changes {
        return Err(invalid_batch(
            batch.transition_id,
            "child rows do not match the source and target manifests",
        ));
    }
    let predecessor = connection
        .query_row(
            "SELECT manifest_generation,manifest_hash,base_id,resolver_output_epoch
             FROM resolution_deltas WHERE view_id=?1 AND delta_generation=?2",
            params![batch.view_id, batch.delta_generation],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?;
    let expected = batch
        .predecessor_manifest_generation
        .zip(batch.predecessor_manifest_hash.as_deref())
        .zip(batch.base_id.as_deref())
        .zip(batch.resolver_output_epoch)
        .map(|(((generation, hash), base_id), epoch)| (generation, hash, base_id, epoch));
    if predecessor
        .as_ref()
        .map(|(generation, hash, base_id, epoch)| {
            (*generation, hash.as_str(), base_id.as_str(), *epoch)
        })
        != expected
    {
        return Err(invalid_batch(
            batch.transition_id,
            "predecessor tuple does not match the immutable resolution delta",
        ));
    }
    Ok(())
}

pub(crate) fn capture_resolution_scope_transition(
    transaction: &Transaction<'_>,
    view_id: &str,
    from_manifest_generation: Option<i64>,
    to_manifest_generation: i64,
    to_manifest_hash: &str,
    new_entries: impl IntoIterator<Item = (String, Option<i64>)>,
    request_id: &str,
) -> Result<i64, ResolutionScopeError> {
    ensure_resolution_scope_feature(transaction)?;
    let previous_transition_id = transaction.query_row(
        "SELECT MAX(transition_id) FROM resolution_scope_batches WHERE view_id=?1",
        [view_id],
        |row| row.get::<_, Option<i64>>(0),
    )?;
    let from_manifest_hash = from_manifest_generation
        .map(|generation| {
            transaction.query_row(
                "SELECT manifest_hash FROM manifests WHERE view_id=?1 AND generation=?2",
                params![view_id, generation],
                |row| row.get::<_, String>(0),
            )
        })
        .transpose()?;
    let old_entries = manifest_versions(transaction, view_id, from_manifest_generation)?;
    let new_entries = new_entries.into_iter().collect::<BTreeMap<_, _>>();
    let changes = scope_changes(&old_entries, &new_entries);
    let state = if from_manifest_generation.is_some() && previous_transition_id.is_none() {
        None
    } else if let Some(state) = exact_scope_state(transaction, view_id)? {
        Some(state)
    } else {
        resolution_scope_state(transaction, view_id)?.filter(|state| {
            Some(state.current_manifest_generation) == from_manifest_generation
                && Some(state.current_manifest_hash.as_str()) == from_manifest_hash.as_deref()
        })
    };
    let usable = state.is_some() && changes.len() <= RESOLUTION_SCOPE_MAX_CHANGES;
    let stored_changes = if usable { changes.as_slice() } else { &[] };
    let hash = changes_hash(stored_changes);
    let state_columns = state.as_ref().filter(|_| usable);
    transaction.execute(
        "INSERT INTO resolution_scope_batches
         (view_id,previous_transition_id,from_manifest_generation,from_manifest_hash,
          to_manifest_generation,to_manifest_hash,scope_usable,
          predecessor_manifest_generation,predecessor_manifest_hash,base_id,
          delta_generation,resolver_output_epoch,change_count,changes_hash,request_id,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,
                 strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
        params![
            view_id,
            previous_transition_id,
            from_manifest_generation,
            from_manifest_hash,
            to_manifest_generation,
            to_manifest_hash,
            if usable { 1 } else { 0 },
            state_columns.map(|state| state.predecessor_manifest_generation),
            state_columns.map(|state| state.predecessor_manifest_hash.as_str()),
            state_columns.map(|state| state.base_id.as_str()),
            state_columns.map(|state| state.delta_generation),
            state_columns.map(|state| state.resolver_output_epoch),
            stored_changes.len() as i64,
            hash,
            request_id,
        ],
    )?;
    let transition_id = transaction.last_insert_rowid();
    for change in stored_changes {
        transaction.execute(
            "INSERT INTO resolution_scope_changes
             (transition_id,ordinal,path,old_version_id,new_version_id)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                transition_id,
                change.ordinal,
                change.path,
                change.old_version_id,
                change.new_version_id,
            ],
        )?;
    }
    if let Some(state) = state_columns {
        transaction.execute(
            "INSERT INTO resolution_scope_states
             (view_id,predecessor_manifest_generation,predecessor_manifest_hash,base_id,
              delta_generation,resolver_output_epoch,current_manifest_generation,
              current_manifest_hash,latest_transition_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(view_id) DO UPDATE SET
               predecessor_manifest_generation=excluded.predecessor_manifest_generation,
               predecessor_manifest_hash=excluded.predecessor_manifest_hash,
               base_id=excluded.base_id,
               delta_generation=excluded.delta_generation,
               resolver_output_epoch=excluded.resolver_output_epoch,
               current_manifest_generation=excluded.current_manifest_generation,
               current_manifest_hash=excluded.current_manifest_hash,
               latest_transition_id=excluded.latest_transition_id",
            params![
                view_id,
                state.predecessor_manifest_generation,
                state.predecessor_manifest_hash,
                state.base_id,
                state.delta_generation,
                state.resolver_output_epoch,
                to_manifest_generation,
                to_manifest_hash,
                transition_id,
            ],
        )?;
    } else {
        transaction.execute(
            "DELETE FROM resolution_scope_states WHERE view_id=?1",
            [view_id],
        )?;
    }
    Ok(transition_id)
}

fn exact_scope_state(
    connection: &Connection,
    view_id: &str,
) -> Result<Option<ResolutionScopeState>, ResolutionScopeError> {
    connection
        .query_row(
            "SELECT view.view_id,view.current_generation,manifest.manifest_hash,
                    view.resolution_base_id,view.resolution_delta_generation,
                    delta.resolver_output_epoch,view.current_generation,manifest.manifest_hash,0
             FROM views AS view
             JOIN manifests AS manifest
               ON manifest.view_id=view.view_id AND manifest.generation=view.current_generation
             JOIN resolution_deltas AS delta
               ON delta.view_id=view.view_id
              AND delta.delta_generation=view.resolution_delta_generation
              AND delta.manifest_generation=view.current_generation
              AND delta.manifest_hash=manifest.manifest_hash
             WHERE view.view_id=?1 AND view.resolution_state='exact'
               AND view.resolution_exact_at=view.current_generation",
            [view_id],
            scope_state_from_row,
        )
        .optional()
        .map_err(Into::into)
}

fn manifest_versions(
    connection: &Connection,
    view_id: &str,
    generation: Option<i64>,
) -> Result<BTreeMap<String, Option<i64>>, ResolutionScopeError> {
    let Some(generation) = generation else {
        return Ok(BTreeMap::new());
    };
    let mut statement = connection.prepare(
        "SELECT path,version_id FROM manifest_entries
         WHERE view_id=?1 AND generation=?2 ORDER BY path COLLATE BINARY",
    )?;
    Ok(statement
        .query_map(params![view_id, generation], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
        })?
        .collect::<Result<BTreeMap<_, _>, _>>()?)
}

fn scope_changes(
    old_entries: &BTreeMap<String, Option<i64>>,
    new_entries: &BTreeMap<String, Option<i64>>,
) -> Vec<ResolutionScopeChange> {
    let mut paths = old_entries
        .keys()
        .chain(new_entries.keys())
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    paths.dedup();
    paths
        .into_iter()
        .filter(|path| old_entries.get(*path) != new_entries.get(*path))
        .enumerate()
        .map(|(ordinal, path)| ResolutionScopeChange {
            ordinal: ordinal as i64,
            path: path.clone(),
            old_version_id: old_entries.get(path).copied().flatten(),
            new_version_id: new_entries.get(path).copied().flatten(),
        })
        .collect()
}

fn changes_hash(changes: &[ResolutionScopeChange]) -> String {
    let mut canonical = Vec::new();
    canonical.extend_from_slice(RESOLUTION_SCOPE_CHANGES_HASH_DOMAIN);
    canonical.extend_from_slice(&(changes.len() as u64).to_be_bytes());
    for change in changes {
        canonical.extend_from_slice(&(change.ordinal as u64).to_be_bytes());
        canonical.extend_from_slice(&(change.path.len() as u64).to_be_bytes());
        canonical.extend_from_slice(change.path.as_bytes());
        encode_optional_i64(&mut canonical, change.old_version_id);
        encode_optional_i64(&mut canonical, change.new_version_id);
    }
    format!("sha256:{:x}", Sha256::digest(canonical))
}

fn encode_optional_i64(target: &mut Vec<u8>, value: Option<i64>) {
    match value {
        Some(value) => {
            target.push(1);
            target.extend_from_slice(&value.to_be_bytes());
        }
        None => target.push(0),
    }
}

fn scope_state_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResolutionScopeState> {
    Ok(ResolutionScopeState {
        view_id: row.get(0)?,
        predecessor_manifest_generation: row.get(1)?,
        predecessor_manifest_hash: row.get(2)?,
        base_id: row.get(3)?,
        delta_generation: row.get(4)?,
        resolver_output_epoch: row.get(5)?,
        current_manifest_generation: row.get(6)?,
        current_manifest_hash: row.get(7)?,
        latest_transition_id: row.get(8)?,
    })
}

fn scope_batch_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResolutionScopeBatch> {
    Ok(ResolutionScopeBatch {
        transition_id: row.get(0)?,
        view_id: row.get(1)?,
        previous_transition_id: row.get(2)?,
        from_manifest_generation: row.get(3)?,
        from_manifest_hash: row.get(4)?,
        to_manifest_generation: row.get(5)?,
        to_manifest_hash: row.get(6)?,
        scope_usable: row.get::<_, i64>(7)? == 1,
        predecessor_manifest_generation: row.get(8)?,
        predecessor_manifest_hash: row.get(9)?,
        base_id: row.get(10)?,
        delta_generation: row.get(11)?,
        resolver_output_epoch: row.get(12)?,
        change_count: row.get(13)?,
        changes_hash: row.get(14)?,
        request_id: row.get(15)?,
        created_at: row.get(16)?,
        changes: Vec::new(),
    })
}

fn invalid_batch(transition_id: i64, detail: &str) -> ResolutionScopeError {
    ResolutionScopeError::InvalidBatch {
        transition_id,
        detail: detail.to_string(),
    }
}

const RESOLUTION_SCOPE_FEATURE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS resolution_scope_batches (
  transition_id INTEGER PRIMARY KEY AUTOINCREMENT,
  view_id TEXT NOT NULL,
  previous_transition_id INTEGER,
  from_manifest_generation INTEGER,
  from_manifest_hash TEXT,
  to_manifest_generation INTEGER NOT NULL CHECK (to_manifest_generation > 0),
  to_manifest_hash TEXT NOT NULL CHECK (length(to_manifest_hash) > 0),
  scope_usable INTEGER NOT NULL CHECK (scope_usable IN (0, 1)),
  predecessor_manifest_generation INTEGER,
  predecessor_manifest_hash TEXT,
  base_id TEXT,
  delta_generation INTEGER,
  resolver_output_epoch INTEGER,
  change_count INTEGER NOT NULL CHECK (change_count >= 0),
  changes_hash TEXT NOT NULL CHECK (length(changes_hash) > 0),
  request_id TEXT NOT NULL CHECK (length(request_id) > 0),
  created_at TEXT NOT NULL CHECK (
    length(created_at) BETWEEN 20 AND 30
      AND substr(created_at, 5, 1) = '-'
      AND substr(created_at, 8, 1) = '-'
      AND substr(created_at, 11, 1) = 'T'
      AND substr(created_at, 14, 1) = ':'
      AND substr(created_at, 17, 1) = ':'
      AND substr(created_at, -1, 1) = 'Z'
      AND strftime('%Y-%m-%dT%H:%M:%S', created_at) = substr(created_at, 1, 19)
      AND (
        length(created_at) = 20
        OR (
          substr(created_at, 20, 1) = '.'
          AND length(created_at) >= 22
          AND substr(created_at, 21, length(created_at) - 21) NOT GLOB '*[^0-9]*'
        )
      )
  ),
  UNIQUE (view_id, transition_id),
  FOREIGN KEY (view_id) REFERENCES views(view_id) ON DELETE NO ACTION,
  FOREIGN KEY (view_id, previous_transition_id)
    REFERENCES resolution_scope_batches(view_id, transition_id) ON DELETE NO ACTION,
  FOREIGN KEY (view_id, from_manifest_generation)
    REFERENCES manifests(view_id, generation) ON DELETE NO ACTION,
  FOREIGN KEY (view_id, to_manifest_generation)
    REFERENCES manifests(view_id, generation) ON DELETE NO ACTION,
  FOREIGN KEY (base_id) REFERENCES resolution_bases(base_id) ON DELETE NO ACTION,
  FOREIGN KEY (view_id, delta_generation)
    REFERENCES resolution_deltas(view_id, delta_generation) ON DELETE NO ACTION,
  CHECK (
    (from_manifest_generation IS NULL AND from_manifest_hash IS NULL)
    OR
    (from_manifest_generation > 0 AND from_manifest_hash IS NOT NULL
      AND length(from_manifest_hash) > 0)
  ),
  CHECK (
    (scope_usable = 0
      AND predecessor_manifest_generation IS NULL
      AND predecessor_manifest_hash IS NULL
      AND base_id IS NULL
      AND delta_generation IS NULL
      AND resolver_output_epoch IS NULL
      AND change_count = 0)
    OR
    (scope_usable = 1
      AND predecessor_manifest_generation > 0
      AND predecessor_manifest_hash IS NOT NULL
      AND length(predecessor_manifest_hash) > 0
      AND base_id IS NOT NULL
      AND length(base_id) > 0
      AND delta_generation > 0
      AND resolver_output_epoch > 0)
  )
) STRICT;

CREATE TABLE IF NOT EXISTS resolution_scope_changes (
  transition_id INTEGER NOT NULL,
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  path TEXT NOT NULL CHECK (length(path) > 0),
  old_version_id INTEGER,
  new_version_id INTEGER,
  PRIMARY KEY (transition_id, ordinal),
  UNIQUE (transition_id, path),
  FOREIGN KEY (transition_id) REFERENCES resolution_scope_batches(transition_id) ON DELETE CASCADE,
  FOREIGN KEY (old_version_id) REFERENCES file_versions(version_id) ON DELETE RESTRICT,
  FOREIGN KEY (new_version_id) REFERENCES file_versions(version_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE IF NOT EXISTS resolution_scope_states (
  view_id TEXT PRIMARY KEY,
  predecessor_manifest_generation INTEGER NOT NULL CHECK (predecessor_manifest_generation > 0),
  predecessor_manifest_hash TEXT NOT NULL CHECK (length(predecessor_manifest_hash) > 0),
  base_id TEXT NOT NULL CHECK (length(base_id) > 0),
  delta_generation INTEGER NOT NULL CHECK (delta_generation > 0),
  resolver_output_epoch INTEGER NOT NULL CHECK (resolver_output_epoch > 0),
  current_manifest_generation INTEGER NOT NULL CHECK (current_manifest_generation > 0),
  current_manifest_hash TEXT NOT NULL CHECK (length(current_manifest_hash) > 0),
  latest_transition_id INTEGER NOT NULL,
  FOREIGN KEY (view_id) REFERENCES views(view_id) ON DELETE CASCADE,
  FOREIGN KEY (view_id, predecessor_manifest_generation)
    REFERENCES manifests(view_id, generation) ON DELETE NO ACTION,
  FOREIGN KEY (base_id) REFERENCES resolution_bases(base_id) ON DELETE NO ACTION,
  FOREIGN KEY (view_id, delta_generation)
    REFERENCES resolution_deltas(view_id, delta_generation) ON DELETE NO ACTION,
  FOREIGN KEY (view_id, current_manifest_generation)
    REFERENCES manifests(view_id, generation) ON DELETE NO ACTION,
  FOREIGN KEY (view_id, latest_transition_id)
    REFERENCES resolution_scope_batches(view_id, transition_id) ON DELETE NO ACTION
) STRICT;

CREATE INDEX IF NOT EXISTS idx_read_resolution_scope_batches_view
ON resolution_scope_batches(view_id, transition_id);
CREATE INDEX IF NOT EXISTS idx_read_resolution_scope_changes_versions
ON resolution_scope_changes(old_version_id, new_version_id, transition_id);

CREATE TRIGGER IF NOT EXISTS trg_resolution_scope_batch_immutable_update
BEFORE UPDATE ON resolution_scope_batches
BEGIN
  SELECT RAISE(ABORT, 'resolution scope batches are immutable');
END;

CREATE TRIGGER IF NOT EXISTS trg_resolution_scope_change_immutable_update
BEFORE UPDATE ON resolution_scope_changes
BEGIN
  SELECT RAISE(ABORT, 'resolution scope changes are immutable');
END;

INSERT INTO store_meta(key, value)
VALUES ('resolution_scope_journal_version', '1')
ON CONFLICT(key) DO NOTHING;
"#;
