use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use super::pragmas::{WriterPragmaProfile, configure_writer_pragmas};
use super::resolution::{
    ResolutionFileIdentity, ResolutionIdentifierRow, ResolutionPendingRow,
    ResolutionSemanticCounts, ResolutionValidationError, catalog_hash, ensure_contained,
    ensure_parent, file_identity, reject_existing_file, sync_path, validate_existing_path,
    validate_output_path,
};

pub const RESOLUTION_SCRATCH_USER_VERSION: i64 = 1;
pub const RESOLUTION_SCRATCH_FORMAT_VERSION: &str = "1";
pub const RESOLUTION_SCRATCH_SQL: &str = r#"
PRAGMA user_version = 1;

CREATE TABLE IF NOT EXISTS delta_meta (
  key TEXT PRIMARY KEY CHECK (length(key) > 0),
  value TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS identifier_replacements (
  version_id INTEGER NOT NULL CHECK (version_id > 0),
  identifier_id TEXT NOT NULL CHECK (length(identifier_id) > 0),
  target_version_id INTEGER,
  target_symbol_id TEXT,
  tier INTEGER,
  confidence REAL,
  method TEXT,
  outcome TEXT NOT NULL CHECK (outcome IN ('resolved', 'ambiguous', 'missing', 'no_context')),
  candidates INTEGER,
  PRIMARY KEY (version_id, identifier_id),
  CHECK ((outcome = 'resolved' AND target_version_id IS NOT NULL AND target_symbol_id IS NOT NULL)
      OR (outcome <> 'resolved' AND target_version_id IS NULL AND target_symbol_id IS NULL)),
  CHECK (target_version_id IS NULL OR target_version_id > 0),
  CHECK (target_symbol_id IS NULL OR length(target_symbol_id) > 0),
  CHECK (tier IS NULL OR tier > 0),
  CHECK (confidence IS NULL OR (confidence >= 0.0 AND confidence <= 1.0)),
  CHECK (method IS NULL OR length(method) > 0),
  CHECK (candidates IS NULL OR candidates >= 0)
) STRICT;

CREATE TABLE IF NOT EXISTS pending_replacements (
  version_id INTEGER NOT NULL CHECK (version_id > 0),
  pending_relationship_id TEXT NOT NULL CHECK (length(pending_relationship_id) > 0),
  target_version_id INTEGER NOT NULL CHECK (target_version_id > 0),
  target_symbol_id TEXT NOT NULL CHECK (length(target_symbol_id) > 0),
  tier INTEGER NOT NULL CHECK (tier > 0),
  confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
  method TEXT NOT NULL CHECK (length(method) > 0),
  PRIMARY KEY (version_id, pending_relationship_id)
) STRICT;

CREATE TABLE IF NOT EXISTS pending_tombstones (
  version_id INTEGER NOT NULL CHECK (version_id > 0),
  pending_relationship_id TEXT NOT NULL CHECK (length(pending_relationship_id) > 0),
  PRIMARY KEY (version_id, pending_relationship_id)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_read_resolution_identifier_replacements_target
  ON identifier_replacements(target_version_id, target_symbol_id, version_id, identifier_id);
CREATE INDEX IF NOT EXISTS idx_export_resolution_identifier_replacements_order
  ON identifier_replacements(version_id, identifier_id);
CREATE INDEX IF NOT EXISTS idx_read_resolution_pending_replacements_target
  ON pending_replacements(target_version_id, target_symbol_id, version_id, pending_relationship_id);
CREATE INDEX IF NOT EXISTS idx_export_resolution_pending_replacements_order
  ON pending_replacements(version_id, pending_relationship_id);
CREATE INDEX IF NOT EXISTS idx_export_resolution_pending_tombstones_order
  ON pending_tombstones(version_id, pending_relationship_id);
"#;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResolutionScratchCounts {
    pub identifier_replacements: u64,
    pub pending_replacements: u64,
    pub pending_tombstones: u64,
}

impl ResolutionScratchCounts {
    pub fn total(self) -> u64 {
        self.identifier_replacements + self.pending_replacements + self.pending_tombstones
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResolutionPendingTombstone {
    pub version_id: i64,
    pub pending_relationship_id: String,
}

#[derive(Debug)]
pub struct ResolutionScratchDelta {
    path: PathBuf,
    manifest_hash: String,
    resolver_output_epoch: i64,
    identifiers: Vec<ResolutionIdentifierRow>,
    pending: Vec<ResolutionPendingRow>,
    tombstones: Vec<ResolutionPendingTombstone>,
}

impl ResolutionScratchDelta {
    pub fn new(
        path: impl AsRef<Path>,
        manifest_hash: impl Into<String>,
        resolver_output_epoch: i64,
    ) -> Result<Self, ResolutionValidationError> {
        let path = path.as_ref().to_path_buf();
        validate_output_path(&path)?;
        if path.exists() {
            return Err(ResolutionValidationError::UnexpectedPathType { path });
        }
        let manifest_hash = manifest_hash.into();
        if manifest_hash.is_empty() || resolver_output_epoch <= 0 {
            return Err(ResolutionValidationError::InvalidArgument("identity"));
        }
        Ok(Self {
            path,
            manifest_hash,
            resolver_output_epoch,
            identifiers: Vec::new(),
            pending: Vec::new(),
            tombstones: Vec::new(),
        })
    }

    pub fn new_contained(
        root: impl AsRef<Path>,
        path: impl AsRef<Path>,
        manifest_hash: impl Into<String>,
        resolver_output_epoch: i64,
    ) -> Result<Self, ResolutionValidationError> {
        ensure_contained(root.as_ref(), path.as_ref())?;
        Self::new(path, manifest_hash, resolver_output_epoch)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn push_identifier_replacement(&mut self, row: ResolutionIdentifierRow) {
        self.identifiers.push(row);
    }

    pub fn push_identifier_batch(
        &mut self,
        rows: impl IntoIterator<Item = ResolutionIdentifierRow>,
    ) {
        self.identifiers.extend(rows);
    }

    pub fn push_pending_replacement(&mut self, row: ResolutionPendingRow) {
        self.pending.push(row);
    }

    pub fn push_pending_batch(&mut self, rows: impl IntoIterator<Item = ResolutionPendingRow>) {
        self.pending.extend(rows);
    }

    pub fn push_pending_tombstone(
        &mut self,
        version_id: i64,
        pending_relationship_id: impl Into<String>,
    ) {
        self.tombstones.push(ResolutionPendingTombstone {
            version_id,
            pending_relationship_id: pending_relationship_id.into(),
        });
    }

    pub fn push_tombstone(&mut self, tombstone: ResolutionPendingTombstone) {
        self.tombstones.push(tombstone);
    }

    pub fn finish(mut self) -> Result<ResolutionFileIdentity, ResolutionValidationError> {
        validate_scratch_rows(&self.identifiers, &self.pending, &self.tombstones)?;
        self.identifiers.sort_by(|left, right| {
            (left.version_id, &left.identifier_id).cmp(&(right.version_id, &right.identifier_id))
        });
        self.pending.sort_by(|left, right| {
            (left.version_id, &left.pending_relationship_id)
                .cmp(&(right.version_id, &right.pending_relationship_id))
        });
        self.tombstones.sort();
        let path = self.path.clone();
        ensure_parent(&path)?;
        reject_existing_file(&path)?;
        let mut connection = Connection::open(&path)?;
        configure_writer_pragmas(&connection, WriterPragmaProfile::Bulk).map_err(|error| {
            ResolutionValidationError::InvalidMetadata {
                key: "pragma".to_string(),
                value: format!("{error:?}"),
            }
        })?;
        connection.execute_batch(RESOLUTION_SCRATCH_SQL)?;
        let catalog_hash = resolution_scratch_catalog_hash(&connection)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let counts = ResolutionScratchCounts {
            identifier_replacements: self.identifiers.len() as u64,
            pending_replacements: self.pending.len() as u64,
            pending_tombstones: self.tombstones.len() as u64,
        };
        insert_scratch_meta(
            &transaction,
            &self.manifest_hash,
            self.resolver_output_epoch,
            counts,
            &catalog_hash,
            false,
        )?;
        {
            let mut statement = transaction.prepare("INSERT INTO identifier_replacements (version_id,identifier_id,target_version_id,target_symbol_id,tier,confidence,method,outcome,candidates) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)")?;
            for row in &self.identifiers {
                statement.execute(params![
                    row.version_id,
                    row.identifier_id,
                    row.target_version_id,
                    row.target_symbol_id,
                    row.tier,
                    row.confidence,
                    row.method,
                    row.outcome,
                    row.candidates
                ])?;
            }
        }
        {
            let mut statement = transaction.prepare("INSERT INTO pending_replacements (version_id,pending_relationship_id,target_version_id,target_symbol_id,tier,confidence,method) VALUES (?1,?2,?3,?4,?5,?6,?7)")?;
            for row in &self.pending {
                statement.execute(params![
                    row.version_id,
                    row.pending_relationship_id,
                    row.target_version_id,
                    row.target_symbol_id,
                    row.tier,
                    row.confidence,
                    row.method
                ])?;
            }
        }
        {
            let mut statement = transaction.prepare("INSERT INTO pending_tombstones (version_id,pending_relationship_id) VALUES (?1,?2)")?;
            for row in &self.tombstones {
                statement.execute(params![row.version_id, row.pending_relationship_id])?;
            }
        }
        let integrity: String =
            transaction.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(ResolutionValidationError::InvalidMetadata {
                key: "integrity_check".to_string(),
                value: integrity,
            });
        }
        insert_scratch_meta(
            &transaction,
            &self.manifest_hash,
            self.resolver_output_epoch,
            counts,
            &catalog_hash,
            false,
        )?;
        transaction.commit()?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        drop(connection);
        sync_path(&path)?;
        let mut connection = Connection::open(&path)?;
        configure_writer_pragmas(&connection, WriterPragmaProfile::Bulk).map_err(|error| {
            ResolutionValidationError::InvalidMetadata {
                key: "pragma".to_string(),
                value: format!("{error:?}"),
            }
        })?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("UPDATE delta_meta SET value='1' WHERE key='completed'", [])?;
        transaction.commit()?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        drop(connection);
        sync_path(&path)?;
        file_identity(
            &path,
            self.manifest_hash,
            self.resolver_output_epoch,
            catalog_hash,
            ResolutionSemanticCounts {
                identifiers: counts.identifier_replacements,
                pending: counts.pending_replacements + counts.pending_tombstones,
            },
        )
    }

    pub fn abort(&self) -> Result<(), ResolutionValidationError> {
        for suffix in ["", "-wal", "-shm"] {
            let path = if suffix.is_empty() {
                self.path.clone()
            } else {
                PathBuf::from(format!("{}{}", self.path.display(), suffix))
            };
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    pub fn catalog_hash(&self) -> String {
        resolution_scratch_catalog_hash_for_sql()
    }
}

pub struct ResolutionScratchWriter {
    path: PathBuf,
    connection: Connection,
    manifest_hash: String,
    resolver_output_epoch: i64,
    catalog_hash: String,
    counts: ResolutionScratchCounts,
    last_identifier: Option<(i64, String)>,
    last_pending: Option<(i64, String)>,
    last_tombstone: Option<(i64, String)>,
    completed: bool,
}

impl ResolutionScratchWriter {
    pub fn new(
        path: impl AsRef<Path>,
        manifest_hash: impl Into<String>,
        resolver_output_epoch: i64,
    ) -> Result<Self, ResolutionValidationError> {
        let path = path.as_ref().to_path_buf();
        validate_output_path(&path)?;
        reject_existing_file(&path)?;
        ensure_parent(&path)?;
        let manifest_hash = manifest_hash.into();
        if manifest_hash.is_empty() || resolver_output_epoch <= 0 {
            return Err(ResolutionValidationError::InvalidArgument("identity"));
        }
        let connection = Connection::open(&path)?;
        configure_writer_pragmas(&connection, WriterPragmaProfile::Bulk).map_err(|error| {
            ResolutionValidationError::InvalidMetadata {
                key: "pragma".to_string(),
                value: format!("{error:?}"),
            }
        })?;
        connection.execute_batch(RESOLUTION_SCRATCH_SQL)?;
        let catalog_hash = resolution_scratch_catalog_hash(&connection)?;
        connection.execute_batch("BEGIN IMMEDIATE")?;
        Ok(Self {
            path,
            connection,
            manifest_hash,
            resolver_output_epoch,
            catalog_hash,
            counts: ResolutionScratchCounts::default(),
            last_identifier: None,
            last_pending: None,
            last_tombstone: None,
            completed: false,
        })
    }

    pub fn push_identifier_replacement(
        &mut self,
        row: ResolutionIdentifierRow,
    ) -> Result<(), ResolutionValidationError> {
        let key = (row.version_id, row.identifier_id.clone());
        if self
            .last_identifier
            .as_ref()
            .is_some_and(|last| key <= *last)
        {
            return Err(ResolutionValidationError::InvalidArgument(
                "identifier replacement order",
            ));
        }
        self.connection.execute(
            "INSERT INTO identifier_replacements
             (version_id,identifier_id,target_version_id,target_symbol_id,tier,confidence,method,outcome,candidates)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![row.version_id,row.identifier_id,row.target_version_id,row.target_symbol_id,
                row.tier,row.confidence,row.method,row.outcome,row.candidates],
        )?;
        self.last_identifier = Some(key);
        self.counts.identifier_replacements += 1;
        Ok(())
    }

    pub fn push_pending_replacement(
        &mut self,
        row: ResolutionPendingRow,
    ) -> Result<(), ResolutionValidationError> {
        let key = (row.version_id, row.pending_relationship_id.clone());
        if self.last_pending.as_ref().is_some_and(|last| key <= *last) {
            return Err(ResolutionValidationError::InvalidArgument(
                "pending replacement order",
            ));
        }
        self.connection.execute(
            "INSERT INTO pending_replacements
             (version_id,pending_relationship_id,target_version_id,target_symbol_id,tier,confidence,method)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![row.version_id,row.pending_relationship_id,row.target_version_id,
                row.target_symbol_id,row.tier,row.confidence,row.method],
        )?;
        self.last_pending = Some(key);
        self.counts.pending_replacements += 1;
        Ok(())
    }

    pub fn push_pending_tombstone(
        &mut self,
        row: ResolutionPendingTombstone,
    ) -> Result<(), ResolutionValidationError> {
        let key = (row.version_id, row.pending_relationship_id.clone());
        if self
            .last_tombstone
            .as_ref()
            .is_some_and(|last| key <= *last)
        {
            return Err(ResolutionValidationError::InvalidArgument(
                "pending tombstone order",
            ));
        }
        self.connection.execute(
            "INSERT INTO pending_tombstones(version_id,pending_relationship_id) VALUES (?1,?2)",
            params![row.version_id, row.pending_relationship_id],
        )?;
        self.last_tombstone = Some(key);
        self.counts.pending_tombstones += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<ResolutionFileIdentity, ResolutionValidationError> {
        insert_scratch_meta(
            &self.connection,
            &self.manifest_hash,
            self.resolver_output_epoch,
            self.counts,
            &self.catalog_hash,
            false,
        )?;
        validate_scratch_row_checks(&self.connection)?;
        let integrity: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(ResolutionValidationError::InvalidMetadata {
                key: "integrity_check".to_string(),
                value: integrity,
            });
        }
        self.connection
            .execute_batch("COMMIT; PRAGMA wal_checkpoint(TRUNCATE);")?;
        let placeholder = Connection::open_in_memory()?;
        drop(std::mem::replace(&mut self.connection, placeholder));
        sync_path(&self.path)?;
        self.connection = Connection::open(&self.path)?;
        configure_writer_pragmas(&self.connection, WriterPragmaProfile::Bulk).map_err(|error| {
            ResolutionValidationError::InvalidMetadata {
                key: "pragma".to_string(),
                value: format!("{error:?}"),
            }
        })?;
        self.connection.execute_batch("BEGIN IMMEDIATE")?;
        self.connection
            .execute("UPDATE delta_meta SET value='1' WHERE key='completed'", [])?;
        self.connection
            .execute_batch("COMMIT; PRAGMA wal_checkpoint(TRUNCATE);")?;
        let placeholder = Connection::open_in_memory()?;
        drop(std::mem::replace(&mut self.connection, placeholder));
        sync_path(&self.path)?;
        self.completed = true;
        file_identity(
            &self.path,
            self.manifest_hash.clone(),
            self.resolver_output_epoch,
            self.catalog_hash.clone(),
            ResolutionSemanticCounts {
                identifiers: self.counts.identifier_replacements,
                pending: self.counts.pending_replacements + self.counts.pending_tombstones,
            },
        )
    }
}

impl Drop for ResolutionScratchWriter {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let _ = self.connection.execute_batch("ROLLBACK");
        for suffix in ["", "-wal", "-shm"] {
            let path = PathBuf::from(format!("{}{}", self.path.display(), suffix));
            if path.exists() {
                let _ = fs::remove_file(path);
            }
        }
    }
}

#[derive(Debug)]
pub struct ResolutionScratchReader {
    path: PathBuf,
    connection: Connection,
    identity: super::resolution::ResolutionFileIdentity,
    counts: ResolutionScratchCounts,
}

pub type ResolutionScratchDeltaReader = ResolutionScratchReader;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionGapTable {
    Identifier,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionGapKind {
    Added,
    Replaced,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionGapFact {
    pub table: ResolutionGapTable,
    pub version_id: i64,
    pub local_id: String,
    pub kind: ResolutionGapKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionDiffResult {
    pub delta: ResolutionFileIdentity,
    pub gaps: u64,
    pub max_window_rows: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResolutionApplyCounts {
    pub identifiers: u64,
    pub pending: u64,
    pub max_window_rows: usize,
}

pub fn stream_resolution_diff<F>(
    base: &super::resolution::ResolutionBaseReader,
    exact: &super::resolution::ResolutionBaseReader,
    scratch_path: impl AsRef<Path>,
    window_size: usize,
    mut emit_gap: F,
) -> Result<ResolutionDiffResult, ResolutionValidationError>
where
    F: FnMut(ResolutionGapFact) -> Result<(), ResolutionValidationError>,
{
    if window_size == 0 {
        return Err(ResolutionValidationError::InvalidArgument("window size"));
    }
    validate_resolver_output_epoch(
        base.file_identity().resolver_output_epoch,
        exact.file_identity().resolver_output_epoch,
    )?;
    let identity = exact.file_identity();
    let mut writer = ResolutionScratchWriter::new(
        scratch_path,
        identity.manifest_hash.clone(),
        identity.resolver_output_epoch,
    )?;
    let mut gaps = 0u64;
    let mut base_identifiers = BaseIdentifierCursor::new(base, window_size);
    let mut exact_identifiers = BaseIdentifierCursor::new(exact, window_size);
    let mut exact_version_cache: Option<(i64, bool)> = None;
    let mut left = base_identifiers.next()?;
    let mut right = exact_identifiers.next()?;
    while left.is_some() || right.is_some() {
        match (&left, &right) {
            (Some(base_row), Some(exact_row)) => {
                match identifier_key(base_row).cmp(&identifier_key(exact_row)) {
                    std::cmp::Ordering::Less => {
                        enforce_identifier_totality(exact, &mut exact_version_cache, base_row)?;
                        emit_gap_fact(
                            &mut emit_gap,
                            &mut gaps,
                            ResolutionGapTable::Identifier,
                            base_row.version_id,
                            &base_row.identifier_id,
                            ResolutionGapKind::Removed,
                        )?;
                        left = base_identifiers.next()?;
                    }
                    std::cmp::Ordering::Greater => {
                        writer.push_identifier_replacement(exact_row.clone())?;
                        emit_gap_fact(
                            &mut emit_gap,
                            &mut gaps,
                            ResolutionGapTable::Identifier,
                            exact_row.version_id,
                            &exact_row.identifier_id,
                            ResolutionGapKind::Added,
                        )?;
                        right = exact_identifiers.next()?;
                    }
                    std::cmp::Ordering::Equal => {
                        if base_row != exact_row {
                            writer.push_identifier_replacement(exact_row.clone())?;
                            emit_gap_fact(
                                &mut emit_gap,
                                &mut gaps,
                                ResolutionGapTable::Identifier,
                                exact_row.version_id,
                                &exact_row.identifier_id,
                                ResolutionGapKind::Replaced,
                            )?;
                        }
                        left = base_identifiers.next()?;
                        right = exact_identifiers.next()?;
                    }
                }
            }
            (Some(base_row), None) => {
                enforce_identifier_totality(exact, &mut exact_version_cache, base_row)?;
                emit_gap_fact(
                    &mut emit_gap,
                    &mut gaps,
                    ResolutionGapTable::Identifier,
                    base_row.version_id,
                    &base_row.identifier_id,
                    ResolutionGapKind::Removed,
                )?;
                left = base_identifiers.next()?;
            }
            (None, Some(exact_row)) => {
                writer.push_identifier_replacement(exact_row.clone())?;
                emit_gap_fact(
                    &mut emit_gap,
                    &mut gaps,
                    ResolutionGapTable::Identifier,
                    exact_row.version_id,
                    &exact_row.identifier_id,
                    ResolutionGapKind::Added,
                )?;
                right = exact_identifiers.next()?;
            }
            (None, None) => break,
        }
    }
    let mut base_pending = BasePendingCursor::new(base, window_size);
    let mut exact_pending = BasePendingCursor::new(exact, window_size);
    let mut left = base_pending.next()?;
    let mut right = exact_pending.next()?;
    while left.is_some() || right.is_some() {
        match (&left, &right) {
            (Some(base_row), Some(exact_row)) => {
                match pending_key(base_row).cmp(&pending_key(exact_row)) {
                    std::cmp::Ordering::Less => {
                        writer.push_pending_tombstone(ResolutionPendingTombstone {
                            version_id: base_row.version_id,
                            pending_relationship_id: base_row.pending_relationship_id.clone(),
                        })?;
                        emit_gap_fact(
                            &mut emit_gap,
                            &mut gaps,
                            ResolutionGapTable::Pending,
                            base_row.version_id,
                            &base_row.pending_relationship_id,
                            ResolutionGapKind::Removed,
                        )?;
                        left = base_pending.next()?;
                    }
                    std::cmp::Ordering::Greater => {
                        writer.push_pending_replacement(exact_row.clone())?;
                        emit_gap_fact(
                            &mut emit_gap,
                            &mut gaps,
                            ResolutionGapTable::Pending,
                            exact_row.version_id,
                            &exact_row.pending_relationship_id,
                            ResolutionGapKind::Added,
                        )?;
                        right = exact_pending.next()?;
                    }
                    std::cmp::Ordering::Equal => {
                        if base_row != exact_row {
                            writer.push_pending_replacement(exact_row.clone())?;
                            emit_gap_fact(
                                &mut emit_gap,
                                &mut gaps,
                                ResolutionGapTable::Pending,
                                exact_row.version_id,
                                &exact_row.pending_relationship_id,
                                ResolutionGapKind::Replaced,
                            )?;
                        }
                        left = base_pending.next()?;
                        right = exact_pending.next()?;
                    }
                }
            }
            (Some(base_row), None) => {
                writer.push_pending_tombstone(ResolutionPendingTombstone {
                    version_id: base_row.version_id,
                    pending_relationship_id: base_row.pending_relationship_id.clone(),
                })?;
                emit_gap_fact(
                    &mut emit_gap,
                    &mut gaps,
                    ResolutionGapTable::Pending,
                    base_row.version_id,
                    &base_row.pending_relationship_id,
                    ResolutionGapKind::Removed,
                )?;
                left = base_pending.next()?;
            }
            (None, Some(exact_row)) => {
                writer.push_pending_replacement(exact_row.clone())?;
                emit_gap_fact(
                    &mut emit_gap,
                    &mut gaps,
                    ResolutionGapTable::Pending,
                    exact_row.version_id,
                    &exact_row.pending_relationship_id,
                    ResolutionGapKind::Added,
                )?;
                right = exact_pending.next()?;
            }
            (None, None) => break,
        }
    }
    let max_window_rows = base_identifiers
        .max_page
        .max(exact_identifiers.max_page)
        .max(base_pending.max_page)
        .max(exact_pending.max_page);
    Ok(ResolutionDiffResult {
        delta: writer.finish()?,
        gaps,
        max_window_rows,
    })
}

fn enforce_identifier_totality(
    exact: &super::resolution::ResolutionBaseReader,
    cache: &mut Option<(i64, bool)>,
    base_row: &ResolutionIdentifierRow,
) -> Result<(), ResolutionValidationError> {
    if cache.is_none_or(|(version_id, _)| version_id != base_row.version_id) {
        *cache = Some((
            base_row.version_id,
            exact.contains_source_version(base_row.version_id)?,
        ));
    }
    if cache.is_some_and(|(_, visible)| visible) {
        return Err(ResolutionValidationError::IdentifierTotalityViolation {
            version_id: base_row.version_id,
            identifier_id: base_row.identifier_id.clone(),
        });
    }
    Ok(())
}

fn emit_gap_fact<F>(
    emit: &mut F,
    count: &mut u64,
    table: ResolutionGapTable,
    version_id: i64,
    local_id: &str,
    kind: ResolutionGapKind,
) -> Result<(), ResolutionValidationError>
where
    F: FnMut(ResolutionGapFact) -> Result<(), ResolutionValidationError>,
{
    emit(ResolutionGapFact {
        table,
        version_id,
        local_id: local_id.to_string(),
        kind,
    })?;
    *count += 1;
    Ok(())
}

struct BaseIdentifierCursor<'a> {
    reader: &'a super::resolution::ResolutionBaseReader,
    window_size: usize,
    rows: std::collections::VecDeque<ResolutionIdentifierRow>,
    after: Option<(i64, String)>,
    max_page: usize,
}

impl<'a> BaseIdentifierCursor<'a> {
    fn new(reader: &'a super::resolution::ResolutionBaseReader, window_size: usize) -> Self {
        Self {
            reader,
            window_size,
            rows: Default::default(),
            after: None,
            max_page: 0,
        }
    }
    fn next(&mut self) -> Result<Option<ResolutionIdentifierRow>, ResolutionValidationError> {
        if self.rows.is_empty() {
            let page = self.reader.identifier_window(
                self.after
                    .as_ref()
                    .map(|(version, id)| (*version, id.as_str())),
                self.window_size,
            )?;
            self.max_page = self.max_page.max(page.len());
            self.rows = page.into();
        }
        let row = self.rows.pop_front();
        if let Some(row) = &row {
            self.after = Some((row.version_id, row.identifier_id.clone()));
        }
        Ok(row)
    }
}

struct BasePendingCursor<'a> {
    reader: &'a super::resolution::ResolutionBaseReader,
    window_size: usize,
    rows: std::collections::VecDeque<ResolutionPendingRow>,
    after: Option<(i64, String)>,
    max_page: usize,
}

impl<'a> BasePendingCursor<'a> {
    fn new(reader: &'a super::resolution::ResolutionBaseReader, window_size: usize) -> Self {
        Self {
            reader,
            window_size,
            rows: Default::default(),
            after: None,
            max_page: 0,
        }
    }
    fn next(&mut self) -> Result<Option<ResolutionPendingRow>, ResolutionValidationError> {
        if self.rows.is_empty() {
            let page = self.reader.pending_window(
                self.after
                    .as_ref()
                    .map(|(version, id)| (*version, id.as_str())),
                self.window_size,
            )?;
            self.max_page = self.max_page.max(page.len());
            self.rows = page.into();
        }
        let row = self.rows.pop_front();
        if let Some(row) = &row {
            self.after = Some((row.version_id, row.pending_relationship_id.clone()));
        }
        Ok(row)
    }
}

fn identifier_key(row: &ResolutionIdentifierRow) -> (i64, &str) {
    (row.version_id, row.identifier_id.as_str())
}

fn pending_key(row: &ResolutionPendingRow) -> (i64, &str) {
    (row.version_id, row.pending_relationship_id.as_str())
}

pub fn apply_base_delta<FV, FI, FP>(
    base: &super::resolution::ResolutionBaseReader,
    delta: &ResolutionScratchReader,
    window_size: usize,
    mut version_visible: FV,
    mut emit_identifier: FI,
    mut emit_pending: FP,
) -> Result<ResolutionApplyCounts, ResolutionValidationError>
where
    FV: FnMut(i64) -> Result<bool, ResolutionValidationError>,
    FI: FnMut(ResolutionIdentifierRow) -> Result<(), ResolutionValidationError>,
    FP: FnMut(ResolutionPendingRow) -> Result<(), ResolutionValidationError>,
{
    if window_size == 0 {
        return Err(ResolutionValidationError::InvalidArgument("window size"));
    }
    validate_resolver_output_epoch(
        base.file_identity().resolver_output_epoch,
        delta.file_identity().resolver_output_epoch,
    )?;
    let mut counts = ResolutionApplyCounts::default();
    let mut visibility = |version_id: i64, cache: &mut Option<(i64, bool)>| {
        if cache.is_none_or(|(cached, _)| cached != version_id) {
            *cache = Some((version_id, version_visible(version_id)?));
        }
        Ok::<bool, ResolutionValidationError>(cache.is_some_and(|(_, visible)| visible))
    };
    let mut visible_cache = None;
    let mut base_identifiers = BaseIdentifierCursor::new(base, window_size);
    let mut replacements = ScratchIdentifierCursor::new(delta, window_size);
    let mut left = base_identifiers.next()?;
    let mut right = replacements.next()?;
    while left.is_some() || right.is_some() {
        let row = match (&left, &right) {
            (Some(base_row), Some(replacement)) => {
                match identifier_key(base_row).cmp(&identifier_key(replacement)) {
                    std::cmp::Ordering::Less => {
                        let row = left.take();
                        left = base_identifiers.next()?;
                        row
                    }
                    std::cmp::Ordering::Greater => {
                        let row = right.take();
                        right = replacements.next()?;
                        row
                    }
                    std::cmp::Ordering::Equal => {
                        let row = right.take();
                        left = base_identifiers.next()?;
                        right = replacements.next()?;
                        row
                    }
                }
            }
            (Some(_), None) => {
                let row = left.take();
                left = base_identifiers.next()?;
                row
            }
            (None, Some(_)) => {
                let row = right.take();
                right = replacements.next()?;
                row
            }
            (None, None) => None,
        };
        if let Some(row) = row
            && visibility(row.version_id, &mut visible_cache)?
        {
            emit_identifier(row)?;
            counts.identifiers += 1;
        }
    }

    visible_cache = None;
    let mut base_pending = BasePendingCursor::new(base, window_size);
    let mut changes = ScratchPendingChangeCursor::new(delta, window_size);
    let mut left = base_pending.next()?;
    let mut right = changes.next()?;
    while left.is_some() || right.is_some() {
        let row = match (&left, &right) {
            (Some(base_row), Some(change)) => match pending_key(base_row).cmp(&change.key()) {
                std::cmp::Ordering::Less => {
                    let row = left.take();
                    left = base_pending.next()?;
                    row
                }
                std::cmp::Ordering::Greater => {
                    let change = right.take().expect("pending change exists");
                    right = changes.next()?;
                    change.into_replacement()
                }
                std::cmp::Ordering::Equal => {
                    let change = right.take().expect("pending change exists");
                    left = base_pending.next()?;
                    right = changes.next()?;
                    change.into_replacement()
                }
            },
            (Some(_), None) => {
                let row = left.take();
                left = base_pending.next()?;
                row
            }
            (None, Some(_)) => {
                let change = right.take().expect("pending change exists");
                right = changes.next()?;
                change.into_replacement()
            }
            (None, None) => None,
        };
        if let Some(row) = row
            && visibility(row.version_id, &mut visible_cache)?
        {
            emit_pending(row)?;
            counts.pending += 1;
        }
    }
    counts.max_window_rows = base_identifiers
        .max_page
        .max(replacements.max_page)
        .max(base_pending.max_page)
        .max(changes.max_page());
    Ok(counts)
}

fn validate_resolver_output_epoch(
    expected: i64,
    found: i64,
) -> Result<(), ResolutionValidationError> {
    if expected != found {
        return Err(ResolutionValidationError::ResolverOutputEpochMismatch { expected, found });
    }
    Ok(())
}

struct ScratchIdentifierCursor<'a> {
    reader: &'a ResolutionScratchReader,
    window_size: usize,
    rows: std::collections::VecDeque<ResolutionIdentifierRow>,
    after: Option<(i64, String)>,
    max_page: usize,
}

impl<'a> ScratchIdentifierCursor<'a> {
    fn new(reader: &'a ResolutionScratchReader, window_size: usize) -> Self {
        Self {
            reader,
            window_size,
            rows: Default::default(),
            after: None,
            max_page: 0,
        }
    }
    fn next(&mut self) -> Result<Option<ResolutionIdentifierRow>, ResolutionValidationError> {
        if self.rows.is_empty() {
            let page = self.reader.identifier_replacement_window(
                self.after
                    .as_ref()
                    .map(|(version, id)| (*version, id.as_str())),
                self.window_size,
            )?;
            self.max_page = self.max_page.max(page.len());
            self.rows = page.into();
        }
        let row = self.rows.pop_front();
        if let Some(row) = &row {
            self.after = Some((row.version_id, row.identifier_id.clone()));
        }
        Ok(row)
    }
}

enum ScratchPendingChange {
    Replacement(ResolutionPendingRow),
    Tombstone(ResolutionPendingTombstone),
}

impl ScratchPendingChange {
    fn key(&self) -> (i64, &str) {
        match self {
            Self::Replacement(row) => pending_key(row),
            Self::Tombstone(row) => (row.version_id, row.pending_relationship_id.as_str()),
        }
    }
    fn into_replacement(self) -> Option<ResolutionPendingRow> {
        match self {
            Self::Replacement(row) => Some(row),
            Self::Tombstone(_) => None,
        }
    }
}

struct ScratchPendingChangeCursor<'a> {
    reader: &'a ResolutionScratchReader,
    window_size: usize,
    replacements: std::collections::VecDeque<ResolutionPendingRow>,
    tombstones: std::collections::VecDeque<ResolutionPendingTombstone>,
    replacement_after: Option<(i64, String)>,
    tombstone_after: Option<(i64, String)>,
    replacement_done: bool,
    tombstone_done: bool,
    max_replacement_page: usize,
    max_tombstone_page: usize,
}

impl<'a> ScratchPendingChangeCursor<'a> {
    fn new(reader: &'a ResolutionScratchReader, window_size: usize) -> Self {
        Self {
            reader,
            window_size,
            replacements: Default::default(),
            tombstones: Default::default(),
            replacement_after: None,
            tombstone_after: None,
            replacement_done: false,
            tombstone_done: false,
            max_replacement_page: 0,
            max_tombstone_page: 0,
        }
    }
    fn refill(&mut self) -> Result<(), ResolutionValidationError> {
        if self.replacements.is_empty() && !self.replacement_done {
            let page = self.reader.pending_replacement_window(
                self.replacement_after
                    .as_ref()
                    .map(|(version, id)| (*version, id.as_str())),
                self.window_size,
            )?;
            self.max_replacement_page = self.max_replacement_page.max(page.len());
            self.replacement_done = page.is_empty();
            self.replacements = page.into();
        }
        if self.tombstones.is_empty() && !self.tombstone_done {
            let page = self.reader.pending_tombstone_window(
                self.tombstone_after
                    .as_ref()
                    .map(|(version, id)| (*version, id.as_str())),
                self.window_size,
            )?;
            self.max_tombstone_page = self.max_tombstone_page.max(page.len());
            self.tombstone_done = page.is_empty();
            self.tombstones = page.into();
        }
        Ok(())
    }
    fn next(&mut self) -> Result<Option<ScratchPendingChange>, ResolutionValidationError> {
        self.refill()?;
        let replacement_key = self.replacements.front().map(pending_key);
        let tombstone_key = self
            .tombstones
            .front()
            .map(|row| (row.version_id, row.pending_relationship_id.as_str()));
        let change = match (replacement_key, tombstone_key) {
            (Some(left), Some(right)) if left < right => self
                .replacements
                .pop_front()
                .map(ScratchPendingChange::Replacement),
            (Some(_), Some(_)) => self
                .tombstones
                .pop_front()
                .map(ScratchPendingChange::Tombstone),
            (Some(_), None) => self
                .replacements
                .pop_front()
                .map(ScratchPendingChange::Replacement),
            (None, Some(_)) => self
                .tombstones
                .pop_front()
                .map(ScratchPendingChange::Tombstone),
            (None, None) => None,
        };
        match &change {
            Some(ScratchPendingChange::Replacement(row)) => {
                self.replacement_after = Some((row.version_id, row.pending_relationship_id.clone()))
            }
            Some(ScratchPendingChange::Tombstone(row)) => {
                self.tombstone_after = Some((row.version_id, row.pending_relationship_id.clone()))
            }
            None => {}
        }
        Ok(change)
    }
    fn max_page(&self) -> usize {
        self.max_replacement_page.max(self.max_tombstone_page)
    }
}

impl ResolutionScratchReader {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ResolutionValidationError> {
        let path = path.as_ref().to_path_buf();
        validate_existing_path(&path)?;
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.execute_batch("PRAGMA query_only = ON;")?;
        validate_scratch_integrity(&connection)?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != RESOLUTION_SCRATCH_USER_VERSION {
            return Err(ResolutionValidationError::InvalidMetadata {
                key: "user_version".to_string(),
                value: version.to_string(),
            });
        }
        let found = resolution_scratch_catalog_hash(&connection)?;
        let expected = scratch_metadata(&connection, "catalog_sha256")?;
        if found != expected {
            return Err(ResolutionValidationError::CatalogHashMismatch { expected, found });
        }
        if scratch_metadata(&connection, "format_version")? != RESOLUTION_SCRATCH_FORMAT_VERSION {
            return Err(ResolutionValidationError::InvalidMetadata {
                key: "format_version".to_string(),
                value: scratch_metadata(&connection, "format_version")?,
            });
        }
        if scratch_metadata(&connection, "completed")? != "1" {
            return Err(ResolutionValidationError::IncompleteFile);
        }
        let counts = ResolutionScratchCounts {
            identifier_replacements: parse_count(
                &scratch_metadata(&connection, "identifier_replacement_count")?,
                "identifier_replacements",
            )?,
            pending_replacements: parse_count(
                &scratch_metadata(&connection, "pending_replacement_count")?,
                "pending_replacements",
            )?,
            pending_tombstones: parse_count(
                &scratch_metadata(&connection, "pending_tombstone_count")?,
                "pending_tombstones",
            )?,
        };
        validate_scratch_row_checks(&connection)?;
        for (table, expected_count) in [
            ("identifier_replacements", counts.identifier_replacements),
            ("pending_replacements", counts.pending_replacements),
            ("pending_tombstones", counts.pending_tombstones),
        ] {
            let found_count = count_rows(&connection, table)?;
            if expected_count != found_count {
                return Err(ResolutionValidationError::RowCountMismatch {
                    table,
                    expected: expected_count,
                    found: found_count,
                });
            }
        }
        let manifest_hash = scratch_metadata(&connection, "manifest_hash")?;
        if manifest_hash.is_empty() {
            return Err(ResolutionValidationError::InvalidMetadata {
                key: "manifest_hash".to_string(),
                value: manifest_hash,
            });
        }
        let identity = file_identity(
            &path,
            manifest_hash,
            parse_positive_i64(
                &scratch_metadata(&connection, "resolver_output_epoch")?,
                "resolver_output_epoch",
            )?,
            found,
            ResolutionSemanticCounts {
                identifiers: counts.identifier_replacements,
                pending: counts.pending_replacements + counts.pending_tombstones,
            },
        )?;
        Ok(Self {
            path,
            connection,
            identity,
            counts,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn file_identity(&self) -> &super::resolution::ResolutionFileIdentity {
        &self.identity
    }
    pub fn semantic_counts(&self) -> ResolutionScratchCounts {
        self.counts
    }
    pub fn identifier_replacements(
        &self,
    ) -> Result<Vec<ResolutionIdentifierRow>, ResolutionValidationError> {
        let mut statement = self.connection.prepare("SELECT version_id,identifier_id,target_version_id,target_symbol_id,tier,confidence,method,outcome,candidates FROM identifier_replacements ORDER BY version_id,identifier_id")?;
        Ok(statement
            .query_map([], |row| {
                Ok(ResolutionIdentifierRow {
                    version_id: row.get(0)?,
                    identifier_id: row.get(1)?,
                    target_version_id: row.get(2)?,
                    target_symbol_id: row.get(3)?,
                    tier: row.get(4)?,
                    confidence: row.get(5)?,
                    method: row.get(6)?,
                    outcome: row.get(7)?,
                    candidates: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }
    pub fn identifier_replacement_window(
        &self,
        after: Option<(i64, &str)>,
        limit: usize,
    ) -> Result<Vec<ResolutionIdentifierRow>, ResolutionValidationError> {
        if limit == 0 {
            return Err(ResolutionValidationError::InvalidArgument("window size"));
        }
        let limit = i64::try_from(limit)
            .map_err(|_| ResolutionValidationError::InvalidArgument("window size"))?;
        let (version_id, identifier_id) = after.unwrap_or((0, ""));
        let mut statement = self.connection.prepare(
            "SELECT version_id,identifier_id,target_version_id,target_symbol_id,tier,confidence,method,outcome,candidates
             FROM identifier_replacements
             WHERE version_id>?1 OR (version_id=?1 AND identifier_id>?2)
             ORDER BY version_id,identifier_id LIMIT ?3",
        )?;
        Ok(statement
            .query_map(params![version_id, identifier_id, limit], |row| {
                Ok(ResolutionIdentifierRow {
                    version_id: row.get(0)?,
                    identifier_id: row.get(1)?,
                    target_version_id: row.get(2)?,
                    target_symbol_id: row.get(3)?,
                    tier: row.get(4)?,
                    confidence: row.get(5)?,
                    method: row.get(6)?,
                    outcome: row.get(7)?,
                    candidates: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }
    pub fn pending_replacements(
        &self,
    ) -> Result<Vec<ResolutionPendingRow>, ResolutionValidationError> {
        let mut statement = self.connection.prepare("SELECT version_id,pending_relationship_id,target_version_id,target_symbol_id,tier,confidence,method FROM pending_replacements ORDER BY version_id,pending_relationship_id")?;
        Ok(statement
            .query_map([], |row| {
                Ok(ResolutionPendingRow {
                    version_id: row.get(0)?,
                    pending_relationship_id: row.get(1)?,
                    target_version_id: row.get(2)?,
                    target_symbol_id: row.get(3)?,
                    tier: row.get(4)?,
                    confidence: row.get(5)?,
                    method: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }
    pub fn pending_replacement_window(
        &self,
        after: Option<(i64, &str)>,
        limit: usize,
    ) -> Result<Vec<ResolutionPendingRow>, ResolutionValidationError> {
        if limit == 0 {
            return Err(ResolutionValidationError::InvalidArgument("window size"));
        }
        let limit = i64::try_from(limit)
            .map_err(|_| ResolutionValidationError::InvalidArgument("window size"))?;
        let (version_id, pending_id) = after.unwrap_or((0, ""));
        let mut statement = self.connection.prepare(
            "SELECT version_id,pending_relationship_id,target_version_id,target_symbol_id,tier,confidence,method
             FROM pending_replacements
             WHERE version_id>?1 OR (version_id=?1 AND pending_relationship_id>?2)
             ORDER BY version_id,pending_relationship_id LIMIT ?3",
        )?;
        Ok(statement
            .query_map(params![version_id, pending_id, limit], |row| {
                Ok(ResolutionPendingRow {
                    version_id: row.get(0)?,
                    pending_relationship_id: row.get(1)?,
                    target_version_id: row.get(2)?,
                    target_symbol_id: row.get(3)?,
                    tier: row.get(4)?,
                    confidence: row.get(5)?,
                    method: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }
    pub fn pending_tombstones(
        &self,
    ) -> Result<Vec<ResolutionPendingTombstone>, ResolutionValidationError> {
        let mut statement = self.connection.prepare("SELECT version_id,pending_relationship_id FROM pending_tombstones ORDER BY version_id,pending_relationship_id")?;
        Ok(statement
            .query_map([], |row| {
                Ok(ResolutionPendingTombstone {
                    version_id: row.get(0)?,
                    pending_relationship_id: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }
    pub fn pending_tombstone_window(
        &self,
        after: Option<(i64, &str)>,
        limit: usize,
    ) -> Result<Vec<ResolutionPendingTombstone>, ResolutionValidationError> {
        if limit == 0 {
            return Err(ResolutionValidationError::InvalidArgument("window size"));
        }
        let limit = i64::try_from(limit)
            .map_err(|_| ResolutionValidationError::InvalidArgument("window size"))?;
        let (version_id, pending_id) = after.unwrap_or((0, ""));
        let mut statement = self.connection.prepare(
            "SELECT version_id,pending_relationship_id FROM pending_tombstones
             WHERE version_id>?1 OR (version_id=?1 AND pending_relationship_id>?2)
             ORDER BY version_id,pending_relationship_id LIMIT ?3",
        )?;
        Ok(statement
            .query_map(params![version_id, pending_id, limit], |row| {
                Ok(ResolutionPendingTombstone {
                    version_id: row.get(0)?,
                    pending_relationship_id: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }
}

fn validate_scratch_integrity(connection: &Connection) -> Result<(), ResolutionValidationError> {
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(ResolutionValidationError::InvalidMetadata {
            key: "integrity_check".to_string(),
            value: integrity,
        });
    }
    Ok(())
}

fn validate_scratch_row_checks(connection: &Connection) -> Result<(), ResolutionValidationError> {
    let identifier_violation: i64 = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM identifier_replacements
           WHERE version_id <= 0 OR length(identifier_id) = 0
             OR outcome NOT IN ('resolved', 'ambiguous', 'missing', 'no_context')
             OR (outcome = 'resolved' AND (target_version_id IS NULL OR target_symbol_id IS NULL))
             OR (outcome <> 'resolved' AND (target_version_id IS NOT NULL OR target_symbol_id IS NOT NULL))
             OR (target_version_id IS NOT NULL AND target_version_id <= 0)
             OR (target_symbol_id IS NOT NULL AND length(target_symbol_id) = 0)
             OR (tier IS NOT NULL AND tier <= 0)
             OR (confidence IS NOT NULL AND (confidence < 0.0 OR confidence > 1.0))
             OR (method IS NOT NULL AND length(method) = 0)
             OR (candidates IS NOT NULL AND candidates < 0)
         )",
        [],
        |row| row.get(0),
    )?;
    if identifier_violation != 0 {
        return Err(ResolutionValidationError::InvalidMetadata {
            key: "row_check".to_string(),
            value: "identifier_replacements".to_string(),
        });
    }
    let pending_violation: i64 = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM pending_replacements
           WHERE version_id <= 0 OR length(pending_relationship_id) = 0
             OR target_version_id <= 0 OR length(target_symbol_id) = 0
             OR tier <= 0 OR confidence < 0.0 OR confidence > 1.0
             OR length(method) = 0
         )",
        [],
        |row| row.get(0),
    )?;
    if pending_violation != 0 {
        return Err(ResolutionValidationError::InvalidMetadata {
            key: "row_check".to_string(),
            value: "pending_replacements".to_string(),
        });
    }
    let tombstone_violation: i64 = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM pending_tombstones
           WHERE version_id <= 0 OR length(pending_relationship_id) = 0
         )",
        [],
        |row| row.get(0),
    )?;
    if tombstone_violation != 0 {
        return Err(ResolutionValidationError::InvalidMetadata {
            key: "row_check".to_string(),
            value: "pending_tombstones".to_string(),
        });
    }
    let collision: i64 = connection.query_row(
        "SELECT EXISTS(
           SELECT 1
           FROM pending_replacements AS replacements
           INNER JOIN pending_tombstones AS tombstones
             ON tombstones.version_id = replacements.version_id
            AND tombstones.pending_relationship_id = replacements.pending_relationship_id
         )",
        [],
        |row| row.get(0),
    )?;
    if collision != 0 {
        return Err(ResolutionValidationError::InvalidMetadata {
            key: "row_check".to_string(),
            value: "pending replacement/tombstone collision".to_string(),
        });
    }
    Ok(())
}

fn validate_scratch_rows(
    identifiers: &[ResolutionIdentifierRow],
    pending: &[ResolutionPendingRow],
    tombstones: &[ResolutionPendingTombstone],
) -> Result<(), ResolutionValidationError> {
    let mut identifiers_seen = BTreeSet::new();
    for row in identifiers {
        if row.version_id <= 0
            || row.identifier_id.is_empty()
            || row.target_version_id.is_some_and(|version| version <= 0)
            || row.tier.is_some_and(|tier| tier <= 0)
            || row
                .confidence
                .is_some_and(|confidence| !(0.0..=1.0).contains(&confidence))
            || row.target_symbol_id.as_ref().is_some_and(String::is_empty)
            || row.method.as_ref().is_some_and(String::is_empty)
            || row.candidates.is_some_and(|candidates| candidates < 0)
            || !identifiers_seen.insert((row.version_id, row.identifier_id.clone()))
        {
            return Err(ResolutionValidationError::InvalidArgument(
                "identifier replacement",
            ));
        }
        if !matches!(
            row.outcome.as_str(),
            "resolved" | "ambiguous" | "missing" | "no_context"
        ) {
            return Err(ResolutionValidationError::InvalidArgument(
                "identifier replacement outcome",
            ));
        }
        if row.outcome == "resolved" {
            if row.target_version_id.is_none()
                || row.target_symbol_id.as_ref().is_none_or(String::is_empty)
            {
                return Err(ResolutionValidationError::InvalidArgument(
                    "identifier replacement target",
                ));
            }
        } else if row.target_version_id.is_some() || row.target_symbol_id.is_some() {
            return Err(ResolutionValidationError::InvalidArgument(
                "identifier replacement target",
            ));
        }
    }
    let mut pending_seen = BTreeSet::new();
    for row in pending {
        if row.version_id <= 0
            || row.pending_relationship_id.is_empty()
            || row.target_version_id <= 0
            || row.target_symbol_id.is_empty()
            || row.tier <= 0
            || !(0.0..=1.0).contains(&row.confidence)
            || row.method.is_empty()
            || !pending_seen.insert((row.version_id, row.pending_relationship_id.clone()))
        {
            return Err(ResolutionValidationError::InvalidArgument(
                "pending replacement",
            ));
        }
    }
    let mut tombstone_seen = BTreeSet::new();
    for row in tombstones {
        if row.version_id <= 0
            || row.pending_relationship_id.is_empty()
            || !tombstone_seen.insert((row.version_id, row.pending_relationship_id.clone()))
            || pending_seen.contains(&(row.version_id, row.pending_relationship_id.clone()))
        {
            return Err(ResolutionValidationError::InvalidArgument(
                "pending tombstone",
            ));
        }
    }
    Ok(())
}

fn insert_scratch_meta(
    transaction: &Connection,
    manifest_hash: &str,
    epoch: i64,
    counts: ResolutionScratchCounts,
    catalog_hash: &str,
    completed: bool,
) -> Result<(), rusqlite::Error> {
    for (key, value) in [
        (
            "format_version",
            RESOLUTION_SCRATCH_FORMAT_VERSION.to_string(),
        ),
        ("catalog_sha256", catalog_hash.to_string()),
        ("manifest_hash", manifest_hash.to_string()),
        ("resolver_output_epoch", epoch.to_string()),
        (
            "identifier_replacement_count",
            counts.identifier_replacements.to_string(),
        ),
        (
            "pending_replacement_count",
            counts.pending_replacements.to_string(),
        ),
        (
            "pending_tombstone_count",
            counts.pending_tombstones.to_string(),
        ),
        ("completed", if completed { "1" } else { "0" }.to_string()),
    ] {
        transaction.execute("INSERT INTO delta_meta(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
    }
    Ok(())
}

fn parse_positive_i64(value: &str, key: &'static str) -> Result<i64, ResolutionValidationError> {
    let parsed = value
        .parse::<i64>()
        .map_err(|_| ResolutionValidationError::InvalidMetadata {
            key: key.to_string(),
            value: value.to_string(),
        })?;
    if parsed > 0 {
        Ok(parsed)
    } else {
        Err(ResolutionValidationError::InvalidMetadata {
            key: key.to_string(),
            value: value.to_string(),
        })
    }
}

fn parse_count(value: &str, table: &'static str) -> Result<u64, ResolutionValidationError> {
    value
        .parse::<u64>()
        .map_err(|_| ResolutionValidationError::InvalidMetadata {
            key: table.to_string(),
            value: value.to_string(),
        })
}

fn count_rows(
    connection: &Connection,
    table: &'static str,
) -> Result<u64, ResolutionValidationError> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    Ok(connection.query_row(&sql, [], |row| row.get::<_, i64>(0))? as u64)
}

fn scratch_metadata(
    connection: &Connection,
    key: &str,
) -> Result<String, ResolutionValidationError> {
    connection
        .query_row(
            "SELECT value FROM delta_meta WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

pub fn resolution_scratch_catalog_hash(
    connection: &Connection,
) -> Result<String, ResolutionValidationError> {
    catalog_hash(connection)
}

pub fn resolution_scratch_catalog_hash_for_sql() -> String {
    let connection = Connection::open_in_memory().expect("in-memory SQLite");
    connection
        .execute_batch(RESOLUTION_SCRATCH_SQL)
        .expect("scratch DDL");
    catalog_hash(&connection).expect("scratch catalog hash")
}

pub fn scratch_identifier_target_set(rows: &[ResolutionIdentifierRow]) -> BTreeSet<(i64, String)> {
    rows.iter()
        .filter_map(|row| row.target_version_id.zip(row.target_symbol_id.clone()))
        .collect()
}

pub fn scratch_semantic_counts(
    rows: &[ResolutionIdentifierRow],
    pending: &[ResolutionPendingRow],
    tombstones: &[ResolutionPendingTombstone],
) -> ResolutionScratchCounts {
    ResolutionScratchCounts {
        identifier_replacements: rows.len() as u64,
        pending_replacements: pending.len() as u64,
        pending_tombstones: tombstones.len() as u64,
    }
}

pub fn scratch_resolution_counts(
    rows: &[ResolutionIdentifierRow],
    pending: &[ResolutionPendingRow],
    tombstones: &[ResolutionPendingTombstone],
) -> ResolutionSemanticCounts {
    ResolutionSemanticCounts {
        identifiers: rows.len() as u64,
        pending: pending.len() as u64 + tombstones.len() as u64,
    }
}

fn _sha256(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}
