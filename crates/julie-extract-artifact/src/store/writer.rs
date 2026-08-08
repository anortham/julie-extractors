use std::error::Error;
use std::fmt;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::model::ArtifactCapabilitySnapshot;

use super::pragmas::{PragmaError, WriterPragmaProfile, configure_wal_autocheckpoint};
use super::rows::{
    CapabilityWriteError, StatementPreparationCounter, capability_epoch_initialized,
    delete_level_rows, insert_level_rows, sync_capability_snapshot,
};
use super::{
    StoreConnectionError, StoreConnectionFactory, StoreFileVersion, StoreLevel, StoreLog,
    StoreLogEntry, StoreLogError, StoreRowCounts, StoreSchemaError, create_store_schema,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreVersionState {
    Created,
    Reused,
    Incomplete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreWriteRequest {
    pub request_id: String,
    pub created_at: String,
    pub bulk: bool,
}

impl StoreWriteRequest {
    pub fn routine(request_id: impl Into<String>, created_at: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            created_at: created_at.into(),
            bulk: false,
        }
    }

    pub fn bulk(request_id: impl Into<String>, created_at: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            created_at: created_at.into(),
            bulk: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFileVersion {
    pub version_id: i64,
    pub path: String,
    pub content_hash: String,
    pub extraction_epoch: u32,
    pub complete_l1: Option<i64>,
    pub complete_l2: Option<i64>,
    pub complete_l3: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreWriteResult {
    pub state: StoreVersionState,
    pub version_id: i64,
    pub level: StoreLevel,
    pub counts: StoreRowCounts,
    pub completion_sequence: i64,
    pub statement_preparations: usize,
}

#[derive(Debug)]
pub enum StoreWriterError {
    Connection(StoreConnectionError),
    Log(StoreLogError),
    Schema(StoreSchemaError),
    Sqlite(rusqlite::Error),
    PreviousLevelIncomplete {
        requested: StoreLevel,
        required: StoreLevel,
    },
    CapabilitySnapshotConflict {
        extraction_epoch: u32,
    },
    CapabilitySnapshotRequired {
        extraction_epoch: u32,
    },
    CapabilitySnapshotEpochMismatch {
        staged_epoch: u32,
        requested_epoch: u32,
    },
    EmptyCapabilitySnapshot {
        extraction_epoch: u32,
    },
    ImmutableFileConflict {
        version_id: i64,
    },
}

impl fmt::Display for StoreWriterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(error) => error.fmt(formatter),
            Self::Log(error) => error.fmt(formatter),
            Self::Schema(error) => error.fmt(formatter),
            Self::Sqlite(error) => error.fmt(formatter),
            Self::PreviousLevelIncomplete {
                requested,
                required,
            } => {
                write!(
                    formatter,
                    "L{} cannot complete before L{}",
                    requested.as_i64(),
                    required.as_i64()
                )
            }
            Self::CapabilitySnapshotConflict { extraction_epoch } => {
                write!(
                    formatter,
                    "capability snapshot conflict for extraction epoch {extraction_epoch}"
                )
            }
            Self::CapabilitySnapshotRequired { extraction_epoch } => {
                write!(
                    formatter,
                    "capability snapshot required for extraction epoch {extraction_epoch}"
                )
            }
            Self::CapabilitySnapshotEpochMismatch {
                staged_epoch,
                requested_epoch,
            } => {
                write!(
                    formatter,
                    "staged capability snapshot epoch {staged_epoch} does not match requested epoch {requested_epoch}"
                )
            }
            Self::EmptyCapabilitySnapshot { extraction_epoch } => {
                write!(
                    formatter,
                    "capability snapshot for extraction epoch {extraction_epoch} must be non-empty"
                )
            }
            Self::ImmutableFileConflict { version_id } => {
                write!(
                    formatter,
                    "immutable file payload conflict for version {version_id}"
                )
            }
        }
    }
}

impl Error for StoreWriterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Connection(error) => Some(error),
            Self::Log(error) => Some(error),
            Self::Schema(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::PreviousLevelIncomplete { .. }
            | Self::CapabilitySnapshotConflict { .. }
            | Self::CapabilitySnapshotRequired { .. }
            | Self::CapabilitySnapshotEpochMismatch { .. }
            | Self::EmptyCapabilitySnapshot { .. }
            | Self::ImmutableFileConflict { .. } => None,
        }
    }
}

impl From<StoreConnectionError> for StoreWriterError {
    fn from(error: StoreConnectionError) -> Self {
        Self::Connection(error)
    }
}

impl From<rusqlite::Error> for StoreWriterError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<StoreLogError> for StoreWriterError {
    fn from(error: StoreLogError) -> Self {
        Self::Log(error)
    }
}

impl From<StoreSchemaError> for StoreWriterError {
    fn from(error: StoreSchemaError) -> Self {
        Self::Schema(error)
    }
}

pub struct StoreWriter {
    connection: Connection,
    capability_snapshot: Option<StagedCapabilitySnapshot>,
}

struct StagedCapabilitySnapshot {
    extraction_epoch: u32,
    snapshot: ArtifactCapabilitySnapshot,
}

struct ExistingFileVersion {
    version: StoredFileVersion,
    language: String,
    content_bytes: i64,
    line_count: Option<i64>,
    metadata_json: Option<String>,
}

impl std::ops::Deref for ExistingFileVersion {
    type Target = StoredFileVersion;

    fn deref(&self) -> &Self::Target {
        &self.version
    }
}

impl StoreWriter {
    pub fn open(factory: &StoreConnectionFactory) -> Result<Self, StoreWriterError> {
        Ok(Self {
            connection: factory.open_writer()?,
            capability_snapshot: None,
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn stage_capability_snapshot(
        &mut self,
        extraction_epoch: u32,
        snapshot: ArtifactCapabilitySnapshot,
    ) {
        self.capability_snapshot = Some(StagedCapabilitySnapshot {
            extraction_epoch,
            snapshot,
        });
    }

    pub fn lookup_version(
        &self,
        path: &str,
        content_hash: &str,
        extraction_epoch: u32,
        required_level: StoreLevel,
    ) -> Result<Option<StoredFileVersion>, StoreWriterError> {
        let stamp_column = match required_level {
            StoreLevel::L1 => "complete_l1",
            StoreLevel::L2 => "complete_l2",
            StoreLevel::L3 => "complete_l3",
        };
        let sql = format!(
            "SELECT version_id, path, content_hash, extraction_epoch,
                    complete_l1, complete_l2, complete_l3
             FROM file_versions
             WHERE path = ?1 AND content_hash = ?2 AND extraction_epoch = ?3
               AND {stamp_column} IS NOT NULL"
        );
        self.connection
            .query_row(&sql, params![path, content_hash, extraction_epoch], |row| {
                Ok(StoredFileVersion {
                    version_id: row.get(0)?,
                    path: row.get(1)?,
                    content_hash: row.get(2)?,
                    extraction_epoch: row.get(3)?,
                    complete_l1: row.get(4)?,
                    complete_l2: row.get(5)?,
                    complete_l3: row.get(6)?,
                })
            })
            .optional()
            .map_err(StoreWriterError::from)
    }

    pub fn lookup_version_in_transaction(
        transaction: &Transaction<'_>,
        path: &str,
        content_hash: &str,
        extraction_epoch: u32,
        required_level: StoreLevel,
    ) -> Result<Option<StoredFileVersion>, StoreWriterError> {
        let stamp_column = match required_level {
            StoreLevel::L1 => "complete_l1",
            StoreLevel::L2 => "complete_l2",
            StoreLevel::L3 => "complete_l3",
        };
        let sql = format!(
            "SELECT version_id, path, content_hash, extraction_epoch,
                    complete_l1, complete_l2, complete_l3
             FROM file_versions
             WHERE path = ?1 AND content_hash = ?2 AND extraction_epoch = ?3
               AND {stamp_column} IS NOT NULL"
        );
        transaction
            .query_row(&sql, params![path, content_hash, extraction_epoch], |row| {
                Ok(StoredFileVersion {
                    version_id: row.get(0)?,
                    path: row.get(1)?,
                    content_hash: row.get(2)?,
                    extraction_epoch: row.get(3)?,
                    complete_l1: row.get(4)?,
                    complete_l2: row.get(5)?,
                    complete_l3: row.get(6)?,
                })
            })
            .optional()
            .map_err(StoreWriterError::from)
    }

    pub fn l1_projection_matches_in_transaction(
        transaction: &Transaction<'_>,
        stored: &StoredFileVersion,
        candidate: &StoreFileVersion,
    ) -> Result<bool, StoreWriterError> {
        if stored.path != candidate.path()
            || stored.content_hash != candidate.content_hash()
            || stored.extraction_epoch != candidate.extraction_epoch()
        {
            return Ok(false);
        }
        let mut staging = Connection::open_in_memory()?;
        create_store_schema(&staging)?;
        let staged = staging.transaction()?;
        let file = candidate.artifact_file();
        staged.execute(
            "INSERT INTO file_versions
             (version_id, path, content_hash, extraction_epoch, language, content_bytes,
              line_count, metadata_json, complete_l1)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
            params![
                stored.version_id,
                file.path,
                file.content_hash,
                candidate.extraction_epoch(),
                file.language,
                file.content_bytes,
                file.line_count,
                file.metadata_json,
            ],
        )?;
        insert_level_rows(
            &staged,
            stored.version_id,
            candidate,
            StoreLevel::L1,
            &mut StatementPreparationCounter::default(),
        )?;
        let file_sql = "SELECT path, content_hash, extraction_epoch, language, content_bytes,
                               line_count, metadata_json
                        FROM file_versions WHERE version_id = ?1";
        if query_projection_rows(transaction, file_sql, stored.version_id)?
            != query_projection_rows(&staged, file_sql, stored.version_id)?
        {
            return Ok(false);
        }
        for (table, key, predicate) in [
            ("symbols", "symbol_id", ""),
            ("symbol_annotations", "annotation_id", ""),
            ("reference_sites", "reference_site_id", " AND level = 1"),
            ("relationships", "relationship_id", ""),
            ("pending_relationships", "pending_relationship_id", ""),
            ("type_facts", "type_fact_id", ""),
            ("complexity_metrics", "complexity_metric_id", ""),
            ("parse_diagnostics", "diagnostic_id", ""),
        ] {
            let sql =
                format!("SELECT * FROM {table} WHERE version_id = ?1{predicate} ORDER BY {key}");
            if query_projection_rows(transaction, &sql, stored.version_id)?
                != query_projection_rows(&staged, &sql, stored.version_id)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn write_level_in_transaction(
        transaction: &Transaction<'_>,
        request: &StoreWriteRequest,
        capability_snapshot: Option<&ArtifactCapabilitySnapshot>,
        version: &StoreFileVersion,
        level: StoreLevel,
    ) -> Result<StoreWriteResult, StoreWriterError> {
        let mut preparations = StatementPreparationCounter::default();
        let existing = lookup_identity_in_tx(
            transaction,
            version.path(),
            version.content_hash(),
            version.extraction_epoch(),
        )?;
        if let Some(existing) = &existing
            && completion_stamp(existing, level).is_none()
            && !immutable_file_payload_matches(existing, version)
        {
            return Err(StoreWriterError::ImmutableFileConflict {
                version_id: existing.version_id,
            });
        }
        if existing.is_none() && level != StoreLevel::L1 {
            return Err(StoreWriterError::PreviousLevelIncomplete {
                requested: level,
                required: StoreLevel::L1,
            });
        }
        if let Some(existing) = &existing {
            if level == StoreLevel::L2 && existing.complete_l1.is_none() {
                return Err(StoreWriterError::PreviousLevelIncomplete {
                    requested: level,
                    required: StoreLevel::L1,
                });
            }
            if level == StoreLevel::L3 && existing.complete_l2.is_none() {
                return Err(StoreWriterError::PreviousLevelIncomplete {
                    requested: level,
                    required: StoreLevel::L2,
                });
            }
        }
        let initialized = if level == StoreLevel::L1 {
            capability_epoch_initialized(
                transaction,
                version.extraction_epoch(),
                &mut preparations,
            )?
        } else {
            true
        };
        let capability_counts = match (level, capability_snapshot) {
            (StoreLevel::L1, Some(snapshot)) => {
                if snapshot.parser_inventory.is_empty() || snapshot.languages.is_empty() {
                    return Err(StoreWriterError::EmptyCapabilitySnapshot {
                        extraction_epoch: version.extraction_epoch(),
                    });
                }
                match sync_capability_snapshot(
                    transaction,
                    version.extraction_epoch(),
                    snapshot,
                    &mut preparations,
                ) {
                    Ok(result) => result,
                    Err(CapabilityWriteError::Sqlite(error)) => {
                        return Err(StoreWriterError::Sqlite(error));
                    }
                    Err(CapabilityWriteError::Conflict) => {
                        return Err(StoreWriterError::CapabilitySnapshotConflict {
                            extraction_epoch: version.extraction_epoch(),
                        });
                    }
                }
            }
            (StoreLevel::L1, None) if !initialized => {
                return Err(StoreWriterError::CapabilitySnapshotRequired {
                    extraction_epoch: version.extraction_epoch(),
                });
            }
            _ => StoreRowCounts::default(),
        };
        if let Some(existing) = &existing
            && let Some(completion_sequence) = completion_stamp(existing, level)
        {
            return Ok(StoreWriteResult {
                state: StoreVersionState::Reused,
                version_id: existing.version_id,
                level,
                counts: capability_counts,
                completion_sequence,
                statement_preparations: preparations.count(),
            });
        }
        let (state, version_id) = match existing {
            Some(existing) => (StoreVersionState::Incomplete, existing.version_id),
            None => {
                let file = version.artifact_file();
                transaction.execute(
                    "INSERT INTO file_versions
                     (path, content_hash, extraction_epoch, language, content_bytes, line_count,
                      metadata_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        file.path,
                        file.content_hash,
                        version.extraction_epoch(),
                        file.language,
                        file.content_bytes,
                        file.line_count,
                        file.metadata_json,
                    ],
                )?;
                (StoreVersionState::Created, transaction.last_insert_rowid())
            }
        };
        delete_level_rows(transaction, version_id, level)?;
        let mut counts =
            insert_level_rows(transaction, version_id, version, level, &mut preparations)?;
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("child_rows_before_level_stamp");
        if state == StoreVersionState::Created {
            counts.file_versions = 1;
        }
        add_counts(&mut counts, capability_counts);
        let payload_json = serde_json::to_string(&serde_json::json!({
            "content_hash": version.content_hash(),
            "extraction_epoch": version.extraction_epoch(),
            "path": version.path(),
        }))
        .expect("store completion payload is serializable");
        let completion_sequence = StoreLog::append_effect(
            transaction,
            &StoreLogEntry::new(
                &request.request_id,
                "version_level_completed",
                payload_json,
                &request.created_at,
            )
            .with_version(version_id)
            .with_level(level),
        )?;
        let stamp_column = match level {
            StoreLevel::L1 => "complete_l1",
            StoreLevel::L2 => "complete_l2",
            StoreLevel::L3 => "complete_l3",
        };
        transaction.execute(
            &format!("UPDATE file_versions SET {stamp_column} = ?1 WHERE version_id = ?2"),
            params![completion_sequence, version_id],
        )?;
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("level_stamp_before_store_commit");
        Ok(StoreWriteResult {
            state,
            version_id,
            level,
            counts,
            completion_sequence,
            statement_preparations: preparations.count(),
        })
    }

    pub fn write_level(
        &mut self,
        request: &StoreWriteRequest,
        version: &StoreFileVersion,
        level: StoreLevel,
    ) -> Result<StoreWriteResult, StoreWriterError> {
        let pragma_profile = if request.bulk {
            WriterPragmaProfile::Bulk
        } else {
            WriterPragmaProfile::Routine
        };
        configure_wal_autocheckpoint(&self.connection, pragma_profile)
            .map_err(store_writer_pragma_error)?;
        let consumed_capability_snapshot = level == StoreLevel::L1
            && self
                .capability_snapshot
                .as_ref()
                .is_some_and(|staged| staged.extraction_epoch == version.extraction_epoch());
        if level == StoreLevel::L1
            && let Some(staged) = self.capability_snapshot.as_ref()
            && staged.extraction_epoch != version.extraction_epoch()
        {
            return Err(StoreWriterError::CapabilitySnapshotEpochMismatch {
                staged_epoch: staged.extraction_epoch,
                requested_epoch: version.extraction_epoch(),
            });
        }
        let snapshot = self
            .capability_snapshot
            .as_ref()
            .filter(|_| level == StoreLevel::L1)
            .map(|staged| &staged.snapshot);
        let tx = self.connection.transaction()?;
        let result = Self::write_level_in_transaction(&tx, request, snapshot, version, level)?;
        tx.commit()?;
        if consumed_capability_snapshot {
            self.capability_snapshot = None;
        }
        Ok(result)
    }
}

fn query_projection_rows(
    transaction: &Transaction<'_>,
    sql: &str,
    version_id: i64,
) -> rusqlite::Result<Vec<Vec<rusqlite::types::Value>>> {
    let mut statement = transaction.prepare(sql)?;
    let column_count = statement.column_count();
    statement
        .query_map([version_id], |row| {
            (0..column_count)
                .map(|index| row.get(index))
                .collect::<rusqlite::Result<Vec<_>>>()
        })?
        .collect()
}

fn store_writer_pragma_error(error: PragmaError) -> StoreWriterError {
    match error {
        PragmaError::Sqlite(error) => StoreWriterError::Sqlite(error),
        PragmaError::IntegerMismatch {
            pragma,
            expected,
            found,
        } => StoreWriterError::Connection(StoreConnectionError::PragmaMismatch {
            pragma,
            expected,
            found,
        }),
        PragmaError::TextMismatch {
            pragma,
            expected,
            found,
        } => StoreWriterError::Connection(StoreConnectionError::TextPragmaMismatch {
            pragma,
            expected,
            found,
        }),
    }
}

fn completion_stamp(version: &StoredFileVersion, level: StoreLevel) -> Option<i64> {
    match level {
        StoreLevel::L1 => version.complete_l1,
        StoreLevel::L2 => version.complete_l2,
        StoreLevel::L3 => version.complete_l3,
    }
}

fn immutable_file_payload_matches(
    stored: &ExistingFileVersion,
    projected: &StoreFileVersion,
) -> bool {
    let file = projected.artifact_file();
    stored.language == file.language
        && stored.content_bytes == file.content_bytes
        && stored.line_count == file.line_count
        && stored.metadata_json == file.metadata_json
}

fn add_counts(target: &mut StoreRowCounts, added: StoreRowCounts) {
    target.parser_inventory += added.parser_inventory;
    target.language_capabilities += added.language_capabilities;
    target.language_capability_fixtures += added.language_capability_fixtures;
    target.language_capability_gaps += added.language_capability_gaps;
}

fn lookup_identity_in_tx(
    tx: &rusqlite::Transaction<'_>,
    path: &str,
    content_hash: &str,
    extraction_epoch: u32,
) -> rusqlite::Result<Option<ExistingFileVersion>> {
    tx.query_row(
        "SELECT version_id, path, content_hash, extraction_epoch,
                language, content_bytes, line_count, metadata_json,
                complete_l1, complete_l2, complete_l3
         FROM file_versions
         WHERE path = ?1 AND content_hash = ?2 AND extraction_epoch = ?3",
        params![path, content_hash, extraction_epoch],
        |row| {
            Ok(ExistingFileVersion {
                language: row.get(4)?,
                content_bytes: row.get(5)?,
                line_count: row.get(6)?,
                metadata_json: row.get(7)?,
                version: StoredFileVersion {
                    version_id: row.get(0)?,
                    path: row.get(1)?,
                    content_hash: row.get(2)?,
                    extraction_epoch: row.get(3)?,
                    complete_l1: row.get(8)?,
                    complete_l2: row.get(9)?,
                    complete_l3: row.get(10)?,
                },
            })
        },
    )
    .optional()
}
