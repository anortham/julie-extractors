use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use super::pragmas::{WriterPragmaProfile, configure_writer_pragmas};

pub const RESOLUTION_BASE_USER_VERSION: i64 = 1;
pub const RESOLUTION_BASE_FORMAT_VERSION: &str = "1";

fn configure_resolution_scratch_connection(
    connection: &Connection,
) -> Result<(), ResolutionValidationError> {
    configure_writer_pragmas(connection, WriterPragmaProfile::Bulk).map_err(|error| {
        ResolutionValidationError::InvalidMetadata {
            key: "pragma".to_string(),
            value: format!("{error:?}"),
        }
    })
}

pub fn create_resolution_scratch_connection(
    path: impl AsRef<Path>,
) -> Result<Connection, ResolutionValidationError> {
    let path = path.as_ref();
    validate_output_path(path)?;
    ensure_parent(path)?;
    reject_existing_file(path)?;
    let connection = Connection::open(path)?;
    configure_resolution_scratch_connection(&connection)?;
    Ok(connection)
}
pub const RESOLUTION_BASE_SQL: &str = r#"
PRAGMA user_version = 1;

CREATE TABLE IF NOT EXISTS base_meta (
  key TEXT PRIMARY KEY CHECK (length(key) > 0),
  value TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS resolution_base_versions (
  version_id INTEGER PRIMARY KEY CHECK (version_id > 0)
) STRICT;

CREATE TABLE IF NOT EXISTS identifier_resolutions (
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
  CHECK (candidates IS NULL OR candidates >= 0),
  FOREIGN KEY (version_id) REFERENCES resolution_base_versions(version_id)
    ON DELETE NO ACTION ON UPDATE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (target_version_id) REFERENCES resolution_base_versions(version_id)
    ON DELETE NO ACTION ON UPDATE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS pending_resolutions (
  version_id INTEGER NOT NULL CHECK (version_id > 0),
  pending_relationship_id TEXT NOT NULL CHECK (length(pending_relationship_id) > 0),
  target_version_id INTEGER NOT NULL CHECK (target_version_id > 0),
  target_symbol_id TEXT NOT NULL CHECK (length(target_symbol_id) > 0),
  tier INTEGER NOT NULL CHECK (tier > 0),
  confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
  method TEXT NOT NULL CHECK (length(method) > 0),
  PRIMARY KEY (version_id, pending_relationship_id),
  FOREIGN KEY (version_id) REFERENCES resolution_base_versions(version_id)
    ON DELETE NO ACTION ON UPDATE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (target_version_id) REFERENCES resolution_base_versions(version_id)
    ON DELETE NO ACTION ON UPDATE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE INDEX IF NOT EXISTS idx_read_resolution_identifiers_target
  ON identifier_resolutions(target_version_id, target_symbol_id, version_id, identifier_id);
CREATE INDEX IF NOT EXISTS idx_export_resolution_identifiers_order
  ON identifier_resolutions(version_id, identifier_id);
CREATE INDEX IF NOT EXISTS idx_read_resolution_pending_target
  ON pending_resolutions(target_version_id, target_symbol_id, version_id, pending_relationship_id);
CREATE INDEX IF NOT EXISTS idx_export_resolution_pending_order
  ON pending_resolutions(version_id, pending_relationship_id);
"#;

#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionIdentifierRow {
    pub version_id: i64,
    pub identifier_id: String,
    pub target_version_id: Option<i64>,
    pub target_symbol_id: Option<String>,
    pub tier: Option<i64>,
    pub confidence: Option<f64>,
    pub method: Option<String>,
    pub outcome: String,
    pub candidates: Option<i64>,
}

pub type IdentifierResolutionRow = ResolutionIdentifierRow;

#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionPendingRow {
    pub version_id: i64,
    pub pending_relationship_id: String,
    pub target_version_id: i64,
    pub target_symbol_id: String,
    pub tier: i64,
    pub confidence: f64,
    pub method: String,
}

pub type PendingResolutionRow = ResolutionPendingRow;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResolutionSemanticCounts {
    pub identifiers: u64,
    pub pending: u64,
}

impl ResolutionSemanticCounts {
    pub fn total(self) -> u64 {
        self.identifiers + self.pending
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionFileIdentity {
    pub path: PathBuf,
    pub manifest_hash: String,
    pub resolver_output_epoch: i64,
    pub catalog_hash: String,
    pub file_bytes: u64,
    pub file_sha256: String,
    pub counts: ResolutionSemanticCounts,
}

#[derive(Debug)]
pub enum ResolutionValidationError {
    Io(io::Error),
    Sqlite(rusqlite::Error),
    InvalidArgument(&'static str),
    InvalidMetadata {
        key: String,
        value: String,
    },
    CatalogHashMismatch {
        expected: String,
        found: String,
    },
    RowCountMismatch {
        table: &'static str,
        expected: u64,
        found: u64,
    },
    IncompleteFile,
    TargetMissing {
        version_id: i64,
        symbol_id: String,
    },
    VersionRootMissing {
        version_id: i64,
    },
    IdentifierTotalityViolation {
        version_id: i64,
        identifier_id: String,
    },
    PathEscapesRoot {
        path: PathBuf,
        root: PathBuf,
    },
    SymlinkPath {
        path: PathBuf,
    },
    UnexpectedPathType {
        path: PathBuf,
    },
}

impl ResolutionValidationError {
    pub fn is_path_error(&self) -> bool {
        matches!(
            self,
            Self::PathEscapesRoot { .. }
                | Self::SymlinkPath { .. }
                | Self::UnexpectedPathType { .. }
        )
    }
}

impl fmt::Display for ResolutionValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Sqlite(error) => error.fmt(formatter),
            Self::InvalidArgument(argument) => write!(formatter, "invalid resolution {argument}"),
            Self::InvalidMetadata { key, value } => {
                write!(formatter, "invalid resolution metadata {key}={value:?}")
            }
            Self::CatalogHashMismatch { expected, found } => write!(
                formatter,
                "resolution catalog hash {found} does not match {expected}"
            ),
            Self::RowCountMismatch {
                table,
                expected,
                found,
            } => write!(
                formatter,
                "resolution {table} row count {found} does not match {expected}"
            ),
            Self::IncompleteFile => formatter.write_str("resolution file is incomplete"),
            Self::TargetMissing {
                version_id,
                symbol_id,
            } => write!(
                formatter,
                "resolution target ({version_id}, {symbol_id}) is not visible"
            ),
            Self::VersionRootMissing { version_id } => {
                write!(formatter, "resolution version root {version_id} is missing")
            }
            Self::IdentifierTotalityViolation {
                version_id,
                identifier_id,
            } => write!(
                formatter,
                "exact resolution omitted identifier ({version_id}, {identifier_id}) from a visible version"
            ),
            Self::PathEscapesRoot { path, root } => {
                write!(formatter, "resolution path {path:?} escapes {root:?}")
            }
            Self::SymlinkPath { path } => {
                write!(formatter, "resolution path {path:?} is a symlink")
            }
            Self::UnexpectedPathType { path } => {
                write!(formatter, "resolution path {path:?} is not a regular file")
            }
        }
    }
}

impl Error for ResolutionValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ResolutionValidationError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for ResolutionValidationError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

pub struct ResolutionBaseBuilder {
    path: PathBuf,
    manifest_hash: String,
    resolver_output_epoch: i64,
    source_versions: Vec<i64>,
    identifiers: Vec<ResolutionIdentifierRow>,
    pending: Vec<ResolutionPendingRow>,
}

impl ResolutionBaseBuilder {
    pub fn new(
        path: impl AsRef<Path>,
        manifest_hash: impl Into<String>,
        resolver_output_epoch: i64,
        source_versions: impl IntoIterator<Item = i64>,
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
        let mut source_versions = source_versions.into_iter().collect::<Vec<_>>();
        source_versions.sort_unstable();
        source_versions.dedup();
        if source_versions.iter().any(|version| *version <= 0) {
            return Err(ResolutionValidationError::InvalidArgument(
                "source versions",
            ));
        }
        Ok(Self {
            path,
            manifest_hash,
            resolver_output_epoch,
            source_versions,
            identifiers: Vec::new(),
            pending: Vec::new(),
        })
    }

    pub fn new_contained(
        root: impl AsRef<Path>,
        path: impl AsRef<Path>,
        manifest_hash: impl Into<String>,
        resolver_output_epoch: i64,
        source_versions: impl IntoIterator<Item = i64>,
    ) -> Result<Self, ResolutionValidationError> {
        ensure_contained(root.as_ref(), path.as_ref())?;
        Self::new(path, manifest_hash, resolver_output_epoch, source_versions)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn push_identifier_resolution(&mut self, row: ResolutionIdentifierRow) {
        self.identifiers.push(row);
    }

    pub fn push_identifier_batch(
        &mut self,
        rows: impl IntoIterator<Item = ResolutionIdentifierRow>,
    ) {
        self.identifiers.extend(rows);
    }

    pub fn push_pending_resolution(&mut self, row: ResolutionPendingRow) {
        self.pending.push(row);
    }

    pub fn push_pending_batch(&mut self, rows: impl IntoIterator<Item = ResolutionPendingRow>) {
        self.pending.extend(rows);
    }

    pub fn finish(
        mut self,
        visible_symbols: &BTreeSet<(i64, String)>,
    ) -> Result<ResolutionFileIdentity, ResolutionValidationError> {
        let source_versions = self
            .source_versions
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        validate_rows(
            &self.identifiers,
            &self.pending,
            &source_versions,
            Some(visible_symbols),
        )?;
        self.identifiers.sort_by(|left, right| {
            (left.version_id, &left.identifier_id).cmp(&(right.version_id, &right.identifier_id))
        });
        self.pending.sort_by(|left, right| {
            (left.version_id, &left.pending_relationship_id)
                .cmp(&(right.version_id, &right.pending_relationship_id))
        });
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
        connection.execute_batch(RESOLUTION_BASE_SQL)?;
        let catalog_hash = resolution_base_catalog_hash(&connection)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        insert_meta(
            &transaction,
            &self.manifest_hash,
            self.resolver_output_epoch,
            &self.source_versions,
            self.identifiers.len() as u64,
            self.pending.len() as u64,
            &catalog_hash,
            false,
        )?;
        {
            let mut statement = transaction
                .prepare("INSERT INTO resolution_base_versions(version_id) VALUES (?1)")?;
            for version in &self.source_versions {
                statement.execute([version])?;
            }
        }
        {
            let mut statement = transaction.prepare(
                "INSERT INTO identifier_resolutions
                 (version_id,identifier_id,target_version_id,target_symbol_id,tier,confidence,method,outcome,candidates)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            )?;
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
                    row.candidates,
                ])?;
            }
        }
        {
            let mut statement = transaction.prepare(
                "INSERT INTO pending_resolutions
                 (version_id,pending_relationship_id,target_version_id,target_symbol_id,tier,confidence,method)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
            )?;
            for row in &self.pending {
                statement.execute(params![
                    row.version_id,
                    row.pending_relationship_id,
                    row.target_version_id,
                    row.target_symbol_id,
                    row.tier,
                    row.confidence,
                    row.method,
                ])?;
            }
        }
        let foreign_keys: i64 =
            transaction.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })?;
        if foreign_keys != 0 {
            return Err(ResolutionValidationError::InvalidMetadata {
                key: "foreign_key_check".to_string(),
                value: foreign_keys.to_string(),
            });
        }
        let integrity: String =
            transaction.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Err(ResolutionValidationError::InvalidMetadata {
                key: "integrity_check".to_string(),
                value: integrity,
            });
        }
        insert_meta(
            &transaction,
            &self.manifest_hash,
            self.resolver_output_epoch,
            &self.source_versions,
            self.identifiers.len() as u64,
            self.pending.len() as u64,
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
        transaction.execute(
            "UPDATE base_meta SET value = '1' WHERE key = 'completed'",
            [],
        )?;
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
                identifiers: self.identifiers.len() as u64,
                pending: self.pending.len() as u64,
            },
        )
    }

    pub fn catalog_hash(&self) -> String {
        resolution_base_catalog_hash_for_sql()
    }
}

#[derive(Debug)]
pub struct ResolutionBaseWriter {
    path: PathBuf,
    connection: Connection,
    manifest_hash: String,
    resolver_output_epoch: i64,
    catalog_hash: String,
    counts: ResolutionSemanticCounts,
    last_source_version: Option<i64>,
    last_identifier_key: Option<(i64, String)>,
    last_pending_key: Option<(i64, String)>,
    completed: bool,
}

impl ResolutionBaseWriter {
    pub fn new(
        path: impl AsRef<Path>,
        manifest_hash: impl Into<String>,
        resolver_output_epoch: i64,
    ) -> Result<Self, ResolutionValidationError> {
        let path = path.as_ref().to_path_buf();
        validate_output_path(&path)?;
        let manifest_hash = manifest_hash.into();
        if manifest_hash.is_empty() || resolver_output_epoch <= 0 {
            return Err(ResolutionValidationError::InvalidArgument("identity"));
        }
        ensure_parent(&path)?;
        reject_existing_file(&path)?;
        let connection = Connection::open(&path)?;
        configure_writer_pragmas(&connection, WriterPragmaProfile::Bulk).map_err(|error| {
            ResolutionValidationError::InvalidMetadata {
                key: "pragma".to_string(),
                value: format!("{error:?}"),
            }
        })?;
        connection.execute_batch(RESOLUTION_BASE_SQL)?;
        let catalog_hash = resolution_base_catalog_hash(&connection)?;
        connection.execute_batch("BEGIN IMMEDIATE")?;
        Ok(Self {
            path,
            connection,
            manifest_hash,
            resolver_output_epoch,
            catalog_hash,
            counts: ResolutionSemanticCounts::default(),
            last_source_version: None,
            last_identifier_key: None,
            last_pending_key: None,
            completed: false,
        })
    }

    pub fn push_source_version(
        &mut self,
        version_id: i64,
    ) -> Result<(), ResolutionValidationError> {
        if version_id <= 0
            || self
                .last_source_version
                .is_some_and(|last| version_id <= last)
        {
            return Err(ResolutionValidationError::InvalidArgument(
                "source version order",
            ));
        }
        self.connection.execute(
            "INSERT INTO resolution_base_versions(version_id) VALUES (?1)",
            [version_id],
        )?;
        self.last_source_version = Some(version_id);
        Ok(())
    }

    pub fn push_identifier_resolution(
        &mut self,
        row: ResolutionIdentifierRow,
    ) -> Result<(), ResolutionValidationError> {
        let key = (row.version_id, row.identifier_id.clone());
        if row.identifier_id.is_empty()
            || self
                .last_identifier_key
                .as_ref()
                .is_some_and(|last| key <= *last)
        {
            return Err(ResolutionValidationError::InvalidArgument(
                "identifier order",
            ));
        }
        self.connection.execute(
            "INSERT INTO identifier_resolutions
             (version_id,identifier_id,target_version_id,target_symbol_id,tier,confidence,method,outcome,candidates)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                row.version_id,
                row.identifier_id,
                row.target_version_id,
                row.target_symbol_id,
                row.tier,
                row.confidence,
                row.method,
                row.outcome,
                row.candidates,
            ],
        )?;
        self.last_identifier_key = Some(key);
        self.counts.identifiers += 1;
        Ok(())
    }

    pub fn push_pending_resolution(
        &mut self,
        row: ResolutionPendingRow,
    ) -> Result<(), ResolutionValidationError> {
        let key = (row.version_id, row.pending_relationship_id.clone());
        if row.pending_relationship_id.is_empty()
            || self
                .last_pending_key
                .as_ref()
                .is_some_and(|last| key <= *last)
        {
            return Err(ResolutionValidationError::InvalidArgument("pending order"));
        }
        self.connection.execute(
            "INSERT INTO pending_resolutions
             (version_id,pending_relationship_id,target_version_id,target_symbol_id,tier,confidence,method)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                row.version_id,
                row.pending_relationship_id,
                row.target_version_id,
                row.target_symbol_id,
                row.tier,
                row.confidence,
                row.method,
            ],
        )?;
        self.last_pending_key = Some(key);
        self.counts.pending += 1;
        Ok(())
    }

    pub fn finish_with_target_lookup<F>(
        mut self,
        mut target_exists: F,
    ) -> Result<ResolutionFileIdentity, ResolutionValidationError>
    where
        F: FnMut(i64, &str) -> Result<bool, ResolutionValidationError>,
    {
        {
            let mut statement = self.connection.prepare(
                "SELECT target_version_id,target_symbol_id
                 FROM identifier_resolutions
                 WHERE target_version_id IS NOT NULL
                 ORDER BY version_id,identifier_id",
            )?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let version_id = row.get::<_, i64>(0)?;
                let symbol_id = row.get::<_, String>(1)?;
                if !target_exists(version_id, &symbol_id)? {
                    return Err(ResolutionValidationError::TargetMissing {
                        version_id,
                        symbol_id,
                    });
                }
            }
        }
        {
            let mut statement = self.connection.prepare(
                "SELECT target_version_id,target_symbol_id
                 FROM pending_resolutions
                 ORDER BY version_id,pending_relationship_id",
            )?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let version_id = row.get::<_, i64>(0)?;
                let symbol_id = row.get::<_, String>(1)?;
                if !target_exists(version_id, &symbol_id)? {
                    return Err(ResolutionValidationError::TargetMissing {
                        version_id,
                        symbol_id,
                    });
                }
            }
        }
        insert_streaming_meta(
            &self.connection,
            &self.manifest_hash,
            self.resolver_output_epoch,
            self.counts,
            &self.catalog_hash,
            false,
        )?;
        let foreign_keys: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM pragma_foreign_key_check",
            [],
            |row| row.get(0),
        )?;
        if foreign_keys != 0 {
            return Err(ResolutionValidationError::InvalidMetadata {
                key: "foreign_key_check".to_string(),
                value: foreign_keys.to_string(),
            });
        }
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
            .execute("UPDATE base_meta SET value='1' WHERE key='completed'", [])?;
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
            self.counts,
        )
    }
}

impl Drop for ResolutionBaseWriter {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let _ = self.connection.execute_batch("ROLLBACK");
        for suffix in ["", "-wal", "-shm"] {
            let path = if suffix.is_empty() {
                self.path.clone()
            } else {
                PathBuf::from(format!("{}{}", self.path.display(), suffix))
            };
            let _ = fs::remove_file(path);
        }
    }
}

fn insert_streaming_meta(
    connection: &Connection,
    manifest_hash: &str,
    epoch: i64,
    counts: ResolutionSemanticCounts,
    catalog_hash: &str,
    completed: bool,
) -> Result<(), rusqlite::Error> {
    for (key, value) in [
        ("format_version", RESOLUTION_BASE_FORMAT_VERSION.to_string()),
        ("catalog_sha256", catalog_hash.to_string()),
        ("manifest_hash", manifest_hash.to_string()),
        ("resolver_output_epoch", epoch.to_string()),
        ("identifier_count", counts.identifiers.to_string()),
        ("pending_count", counts.pending.to_string()),
        ("completed", if completed { "1" } else { "0" }.to_string()),
    ] {
        connection.execute(
            "INSERT INTO base_meta(key,value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
    }
    connection.execute(
        "INSERT INTO base_meta(key,value)
         SELECT 'source_versions', json_group_array(version_id)
         FROM (SELECT version_id FROM resolution_base_versions ORDER BY version_id)
         WHERE 1
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [],
    )?;
    Ok(())
}

#[derive(Debug)]
pub struct ResolutionBaseReader {
    path: PathBuf,
    connection: Connection,
    identity: ResolutionFileIdentity,
}

impl ResolutionBaseReader {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ResolutionValidationError> {
        let path = path.as_ref().to_path_buf();
        validate_existing_path(&path)?;
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA query_only = ON;")?;
        validate_base_integrity(&connection)?;
        let user_version: i64 =
            connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if user_version != RESOLUTION_BASE_USER_VERSION {
            return Err(ResolutionValidationError::InvalidMetadata {
                key: "user_version".to_string(),
                value: user_version.to_string(),
            });
        }
        let found_catalog = resolution_base_catalog_hash(&connection)?;
        let expected_catalog = metadata(&connection, "catalog_sha256")?;
        if found_catalog != expected_catalog {
            return Err(ResolutionValidationError::CatalogHashMismatch {
                expected: expected_catalog,
                found: found_catalog,
            });
        }
        if metadata(&connection, "format_version")? != RESOLUTION_BASE_FORMAT_VERSION {
            return Err(ResolutionValidationError::InvalidMetadata {
                key: "format_version".to_string(),
                value: metadata(&connection, "format_version")?,
            });
        }
        if metadata(&connection, "completed")? != "1" {
            return Err(ResolutionValidationError::IncompleteFile);
        }
        let manifest_hash = metadata(&connection, "manifest_hash")?;
        if manifest_hash.is_empty() {
            return Err(ResolutionValidationError::InvalidMetadata {
                key: "manifest_hash".to_string(),
                value: manifest_hash,
            });
        }
        let resolver_output_epoch = parse_positive_i64(
            &metadata(&connection, "resolver_output_epoch")?,
            "resolver_output_epoch",
        )?;
        let counts = ResolutionSemanticCounts {
            identifiers: parse_count(
                &metadata(&connection, "identifier_count")?,
                "identifier_resolutions",
            )?,
            pending: parse_count(
                &metadata(&connection, "pending_count")?,
                "pending_resolutions",
            )?,
        };
        let source_versions = parse_source_versions(&metadata(&connection, "source_versions")?)?;
        let stored_versions = connection
            .prepare("SELECT version_id FROM resolution_base_versions ORDER BY version_id")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if stored_versions != source_versions {
            return Err(ResolutionValidationError::InvalidMetadata {
                key: "source_versions".to_string(),
                value: format!("metadata={source_versions:?} table={stored_versions:?}"),
            });
        }
        validate_base_row_checks(&connection)?;
        let identifier_rows = count_rows(&connection, "identifier_resolutions")?;
        let pending_rows = count_rows(&connection, "pending_resolutions")?;
        if identifier_rows != counts.identifiers {
            return Err(ResolutionValidationError::RowCountMismatch {
                table: "identifier_resolutions",
                expected: counts.identifiers,
                found: identifier_rows,
            });
        }
        if pending_rows != counts.pending {
            return Err(ResolutionValidationError::RowCountMismatch {
                table: "pending_resolutions",
                expected: counts.pending,
                found: pending_rows,
            });
        }
        let identity = file_identity(
            &path,
            manifest_hash,
            resolver_output_epoch,
            found_catalog,
            counts,
        )?;
        Ok(Self {
            path,
            connection,
            identity,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_identity(&self) -> &ResolutionFileIdentity {
        &self.identity
    }

    pub fn catalog_hash(&self) -> &str {
        &self.identity.catalog_hash
    }

    pub fn semantic_counts(&self) -> ResolutionSemanticCounts {
        self.identity.counts
    }

    pub fn source_versions(&self) -> Result<Vec<i64>, ResolutionValidationError> {
        Ok(self
            .connection
            .prepare("SELECT version_id FROM resolution_base_versions ORDER BY version_id")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn contains_source_version(
        &self,
        version_id: i64,
    ) -> Result<bool, ResolutionValidationError> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM resolution_base_versions WHERE version_id=?1)",
            [version_id],
            |row| row.get(0),
        )?)
    }

    pub fn validate_targets(
        &self,
        visible_symbols: &BTreeSet<(i64, String)>,
    ) -> Result<(), ResolutionValidationError> {
        let source_versions = self.source_versions()?.into_iter().collect::<BTreeSet<_>>();
        let mut identifiers = self.connection.prepare("SELECT target_version_id,target_symbol_id FROM identifier_resolutions WHERE target_version_id IS NOT NULL ORDER BY version_id,identifier_id")?;
        let mut rows = identifiers.query([])?;
        while let Some(row) = rows.next()? {
            let target = (row.get::<_, i64>(0)?, row.get::<_, String>(1)?);
            if !visible_symbols.contains(&target) {
                return Err(ResolutionValidationError::TargetMissing {
                    version_id: target.0,
                    symbol_id: target.1,
                });
            }
            if !source_versions.contains(&target.0) {
                return Err(ResolutionValidationError::VersionRootMissing {
                    version_id: target.0,
                });
            }
        }
        let mut pending = self.connection.prepare("SELECT target_version_id,target_symbol_id FROM pending_resolutions ORDER BY version_id,pending_relationship_id")?;
        let mut rows = pending.query([])?;
        while let Some(row) = rows.next()? {
            let target = (row.get::<_, i64>(0)?, row.get::<_, String>(1)?);
            if !visible_symbols.contains(&target) {
                return Err(ResolutionValidationError::TargetMissing {
                    version_id: target.0,
                    symbol_id: target.1,
                });
            }
            if !source_versions.contains(&target.0) {
                return Err(ResolutionValidationError::VersionRootMissing {
                    version_id: target.0,
                });
            }
        }
        Ok(())
    }

    pub fn identifiers(&self) -> Result<Vec<ResolutionIdentifierRow>, ResolutionValidationError> {
        let mut statement = self.connection.prepare("SELECT version_id,identifier_id,target_version_id,target_symbol_id,tier,confidence,method,outcome,candidates FROM identifier_resolutions ORDER BY version_id,identifier_id")?;
        let rows = statement
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
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn identifier_window(
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
             FROM identifier_resolutions
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

    pub fn pending(&self) -> Result<Vec<ResolutionPendingRow>, ResolutionValidationError> {
        let mut statement = self.connection.prepare("SELECT version_id,pending_relationship_id,target_version_id,target_symbol_id,tier,confidence,method FROM pending_resolutions ORDER BY version_id,pending_relationship_id")?;
        let rows = statement
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
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn pending_window(
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
             FROM pending_resolutions
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
}

fn validate_rows(
    identifiers: &[ResolutionIdentifierRow],
    pending: &[ResolutionPendingRow],
    source_versions: &BTreeSet<i64>,
    visible_symbols: Option<&BTreeSet<(i64, String)>>,
) -> Result<(), ResolutionValidationError> {
    let mut identifier_keys = BTreeSet::new();
    for row in identifiers {
        if !matches!(
            row.outcome.as_str(),
            "resolved" | "ambiguous" | "missing" | "no_context"
        ) {
            return Err(ResolutionValidationError::InvalidArgument(
                "identifier outcome",
            ));
        }
        if row.target_version_id.is_some_and(|version| version <= 0)
            || row.tier.is_some_and(|tier| tier <= 0)
            || row
                .confidence
                .is_some_and(|confidence| !(0.0..=1.0).contains(&confidence))
            || row.target_symbol_id.as_ref().is_some_and(String::is_empty)
            || row.method.as_ref().is_some_and(String::is_empty)
            || row.candidates.is_some_and(|candidates| candidates < 0)
        {
            return Err(ResolutionValidationError::InvalidArgument(
                "identifier payload",
            ));
        }
        if row.version_id <= 0
            || row.identifier_id.is_empty()
            || !identifier_keys.insert((row.version_id, row.identifier_id.clone()))
        {
            return Err(ResolutionValidationError::InvalidArgument("identifier row"));
        }
        if !source_versions.contains(&row.version_id) {
            return Err(ResolutionValidationError::VersionRootMissing {
                version_id: row.version_id,
            });
        }
        if row.outcome == "resolved" {
            let target = row
                .target_version_id
                .zip(row.target_symbol_id.clone())
                .ok_or(ResolutionValidationError::InvalidArgument(
                    "resolved target",
                ))?;
            if let Some(visible_symbols) = visible_symbols
                && !visible_symbols.contains(&target)
            {
                return Err(ResolutionValidationError::TargetMissing {
                    version_id: target.0,
                    symbol_id: target.1,
                });
            }
            if !source_versions.contains(&target.0) {
                return Err(ResolutionValidationError::VersionRootMissing {
                    version_id: target.0,
                });
            }
        } else if row.target_version_id.is_some() || row.target_symbol_id.is_some() {
            return Err(ResolutionValidationError::InvalidArgument(
                "unresolved target",
            ));
        }
    }
    let mut pending_keys = BTreeSet::new();
    for row in pending {
        if row.target_version_id <= 0
            || row.tier <= 0
            || !(0.0..=1.0).contains(&row.confidence)
            || row.method.is_empty()
        {
            return Err(ResolutionValidationError::InvalidArgument(
                "pending payload",
            ));
        }
        if row.version_id <= 0
            || row.pending_relationship_id.is_empty()
            || !pending_keys.insert((row.version_id, row.pending_relationship_id.clone()))
        {
            return Err(ResolutionValidationError::InvalidArgument("pending row"));
        }
        if !source_versions.contains(&row.version_id) {
            return Err(ResolutionValidationError::VersionRootMissing {
                version_id: row.version_id,
            });
        }
        if row.target_symbol_id.is_empty() {
            return Err(ResolutionValidationError::InvalidArgument("pending target"));
        }
        if let Some(visible_symbols) = visible_symbols
            && !visible_symbols.contains(&(row.target_version_id, row.target_symbol_id.clone()))
        {
            return Err(ResolutionValidationError::TargetMissing {
                version_id: row.target_version_id,
                symbol_id: row.target_symbol_id.clone(),
            });
        }
        if !source_versions.contains(&row.target_version_id) {
            return Err(ResolutionValidationError::VersionRootMissing {
                version_id: row.target_version_id,
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_meta(
    transaction: &rusqlite::Transaction<'_>,
    manifest_hash: &str,
    resolver_output_epoch: i64,
    source_versions: &[i64],
    identifiers: u64,
    pending: u64,
    catalog_hash: &str,
    completed: bool,
) -> Result<(), rusqlite::Error> {
    let source_versions =
        serde_json::to_string(source_versions).expect("integer vectors serialize");
    for (key, value) in [
        ("format_version", RESOLUTION_BASE_FORMAT_VERSION.to_string()),
        ("catalog_sha256", catalog_hash.to_string()),
        ("manifest_hash", manifest_hash.to_string()),
        ("resolver_output_epoch", resolver_output_epoch.to_string()),
        ("source_versions", source_versions),
        ("identifier_count", identifiers.to_string()),
        ("pending_count", pending.to_string()),
        ("completed", if completed { "1" } else { "0" }.to_string()),
    ] {
        transaction.execute(
            "INSERT INTO base_meta(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
    }
    Ok(())
}

pub(crate) fn validate_output_path(path: &Path) -> Result<(), ResolutionValidationError> {
    if path.as_os_str().is_empty() {
        return Err(ResolutionValidationError::InvalidArgument("path"));
    }
    for ancestor in path.ancestors().skip(1) {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if ancestor == Path::new("/var") || ancestor == Path::new("/tmp") {
                    continue;
                }
                return Err(ResolutionValidationError::SymlinkPath {
                    path: ancestor.to_path_buf(),
                });
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(ResolutionValidationError::UnexpectedPathType {
                    path: ancestor.to_path_buf(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(ResolutionValidationError::SymlinkPath {
                path: path.to_path_buf(),
            });
        }
        if metadata.is_dir() {
            return Err(ResolutionValidationError::UnexpectedPathType {
                path: path.to_path_buf(),
            });
        }
    }
    Ok(())
}

pub(crate) fn validate_existing_path(path: &Path) -> Result<(), ResolutionValidationError> {
    validate_output_path(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(ResolutionValidationError::SymlinkPath {
            path: path.to_path_buf(),
        });
    }
    if !metadata.is_file() {
        return Err(ResolutionValidationError::UnexpectedPathType {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

pub(crate) fn ensure_parent(path: &Path) -> Result<(), ResolutionValidationError> {
    let parent = path
        .parent()
        .ok_or(ResolutionValidationError::InvalidArgument("path"))?;
    validate_output_path(path)?;
    fs::create_dir_all(parent)?;
    validate_output_path(path)
}

pub(crate) fn ensure_contained(root: &Path, path: &Path) -> Result<(), ResolutionValidationError> {
    let lexical_root = lexical_normalize(root, root)?;
    let canonical_root = root.canonicalize()?;
    let logical = lexical_normalize(path, root)?;
    reject_symlink_components(&lexical_root, &logical)?;
    let mut existing = logical.as_path();
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or(ResolutionValidationError::InvalidArgument("path"))?;
    }
    let resolved_existing = existing.canonicalize()?;
    if !resolved_existing.starts_with(&canonical_root) || !logical.starts_with(&lexical_root) {
        return Err(ResolutionValidationError::PathEscapesRoot {
            path: path.to_path_buf(),
            root: canonical_root,
        });
    }
    validate_output_path(path)
}

fn reject_symlink_components(root: &Path, path: &Path) -> Result<(), ResolutionValidationError> {
    let relative =
        path.strip_prefix(root)
            .map_err(|_| ResolutionValidationError::PathEscapesRoot {
                path: path.to_path_buf(),
                root: root.to_path_buf(),
            })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ResolutionValidationError::SymlinkPath { path: current });
            }
            Ok(metadata) if !metadata.is_dir() && current != path => {
                return Err(ResolutionValidationError::UnexpectedPathType { path: current });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn lexical_normalize(path: &Path, root: &Path) -> Result<PathBuf, ResolutionValidationError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(ResolutionValidationError::InvalidArgument("path"));
    }
    Ok(normalized)
}

pub(crate) fn metadata(
    connection: &Connection,
    key: &str,
) -> Result<String, ResolutionValidationError> {
    connection
        .query_row("SELECT value FROM base_meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .map_err(Into::into)
}

fn validate_base_integrity(connection: &Connection) -> Result<(), ResolutionValidationError> {
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(ResolutionValidationError::InvalidMetadata {
            key: "integrity_check".to_string(),
            value: integrity,
        });
    }
    let foreign_keys: i64 =
        connection.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if foreign_keys != 0 {
        return Err(ResolutionValidationError::InvalidMetadata {
            key: "foreign_key_check".to_string(),
            value: foreign_keys.to_string(),
        });
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

fn parse_source_versions(value: &str) -> Result<Vec<i64>, ResolutionValidationError> {
    let versions = serde_json::from_str::<Vec<i64>>(value).map_err(|_| {
        ResolutionValidationError::InvalidMetadata {
            key: "source_versions".to_string(),
            value: value.to_string(),
        }
    })?;
    if versions.windows(2).any(|pair| pair[0] >= pair[1])
        || versions.iter().any(|version| *version <= 0)
    {
        return Err(ResolutionValidationError::InvalidMetadata {
            key: "source_versions".to_string(),
            value: value.to_string(),
        });
    }
    Ok(versions)
}

fn count_rows(
    connection: &Connection,
    table: &'static str,
) -> Result<u64, ResolutionValidationError> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    Ok(connection.query_row(&sql, [], |row| row.get::<_, i64>(0))? as u64)
}

fn validate_base_row_checks(connection: &Connection) -> Result<(), ResolutionValidationError> {
    let identifier_violation: i64 = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM identifier_resolutions
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
            value: "identifier_resolutions".to_string(),
        });
    }
    let pending_violation: i64 = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM pending_resolutions
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
            value: "pending_resolutions".to_string(),
        });
    }
    Ok(())
}

pub(crate) fn file_identity(
    path: &Path,
    manifest_hash: String,
    resolver_output_epoch: i64,
    catalog_hash: String,
    counts: ResolutionSemanticCounts,
) -> Result<ResolutionFileIdentity, ResolutionValidationError> {
    let mut file = File::open(path)?;
    let file_bytes = file.metadata()?.len();
    let mut buffer = [0u8; 64 * 1024];
    let mut digest = Sha256::new();
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(ResolutionFileIdentity {
        path: path.to_path_buf(),
        manifest_hash,
        resolver_output_epoch,
        catalog_hash,
        file_bytes,
        file_sha256: format!("{:x}", digest.finalize()),
        counts,
    })
}

pub fn resolution_base_catalog_hash(
    connection: &Connection,
) -> Result<String, ResolutionValidationError> {
    catalog_hash(connection)
}

pub fn resolution_base_catalog_hash_for_sql() -> String {
    let connection = Connection::open_in_memory().expect("in-memory SQLite");
    connection
        .execute_batch(RESOLUTION_BASE_SQL)
        .expect("base DDL");
    catalog_hash(&connection).expect("base catalog hash")
}

pub(crate) fn catalog_hash(connection: &Connection) -> Result<String, ResolutionValidationError> {
    let mut statement = connection.prepare(
        "SELECT type,name,tbl_name,sql FROM sqlite_master
         WHERE name NOT LIKE 'sqlite_%' AND sql IS NOT NULL ORDER BY type,name",
    )?;
    let mut rows = statement.query([])?;
    let mut normalized = String::new();
    while let Some(row) = rows.next()? {
        let sql: String = row.get(3)?;
        normalized.push_str(&format!(
            "{}|{}|{}|{}\n",
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            sql.split_whitespace().collect::<Vec<_>>().join(" ")
        ));
    }
    let mut digest = Sha256::new();
    digest.update(normalized.as_bytes());
    Ok(format!("{:x}", digest.finalize()))
}

pub(crate) fn reject_existing_file(path: &Path) -> Result<(), ResolutionValidationError> {
    if path.exists() {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(ResolutionValidationError::SymlinkPath {
                path: path.to_path_buf(),
            });
        }
        return Err(ResolutionValidationError::UnexpectedPathType {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

pub(crate) fn sync_path(path: &Path) -> Result<(), ResolutionValidationError> {
    File::open(path)?.sync_all()?;
    Ok(())
}
