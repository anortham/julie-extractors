use std::error::Error;
use std::fmt;

use rusqlite::{Connection, OptionalExtension, params};

use crate::model::ArtifactCapabilitySnapshot;

use super::rows::{
    CapabilityWriteError, delete_level_rows, insert_level_rows, sync_capability_snapshot,
};
use super::{
    StoreConnectionError, StoreConnectionFactory, StoreFileVersion, StoreLevel, StoreRowCounts,
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
    Sqlite(rusqlite::Error),
    PreviousLevelIncomplete {
        requested: StoreLevel,
        required: StoreLevel,
    },
    CapabilitySnapshotConflict {
        extraction_epoch: u32,
    },
}

impl fmt::Display for StoreWriterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(error) => error.fmt(formatter),
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
        }
    }
}

impl Error for StoreWriterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Connection(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::PreviousLevelIncomplete { .. } | Self::CapabilitySnapshotConflict { .. } => None,
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

pub struct StoreWriter {
    connection: Connection,
    capability_snapshot: Option<ArtifactCapabilitySnapshot>,
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

    pub fn stage_capability_snapshot(&mut self, snapshot: ArtifactCapabilitySnapshot) {
        self.capability_snapshot = Some(snapshot);
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

    pub fn write_level(
        &mut self,
        request: &StoreWriteRequest,
        version: &StoreFileVersion,
        level: StoreLevel,
    ) -> Result<StoreWriteResult, StoreWriterError> {
        let autocheckpoint = if request.bulk { 8_000 } else { 1_000 };
        self.connection
            .pragma_update(None, "wal_autocheckpoint", autocheckpoint)?;
        let tx = self.connection.transaction()?;
        let existing = lookup_identity_in_tx(
            &tx,
            version.path(),
            version.content_hash(),
            version.extraction_epoch(),
        )?;
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

        let (capability_counts, capability_preparations) =
            if let Some(snapshot) = self.capability_snapshot.as_ref() {
                match sync_capability_snapshot(&tx, version.extraction_epoch(), snapshot) {
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
            } else {
                (StoreRowCounts::default(), 0)
            };
        if let Some(existing) = &existing
            && let Some(completion_sequence) = completion_stamp(existing, level)
        {
            tx.commit()?;
            return Ok(StoreWriteResult {
                state: StoreVersionState::Reused,
                version_id: existing.version_id,
                level,
                counts: capability_counts,
                completion_sequence,
                statement_preparations: capability_preparations,
            });
        }

        let (state, version_id) = match existing {
            Some(existing) => (StoreVersionState::Incomplete, existing.version_id),
            None => {
                let file = version.artifact_file();
                tx.execute(
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
                (StoreVersionState::Created, tx.last_insert_rowid())
            }
        };

        delete_level_rows(&tx, version_id, level)?;
        let (mut counts, row_preparations) = insert_level_rows(&tx, version_id, version, level)?;
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
        tx.execute(
            "INSERT INTO store_log
             (request_id, event_kind, version_id, level, terminal, payload_json, created_at)
             VALUES (?1, 'version_level_completed', ?2, ?3, 0, ?4, ?5)",
            params![
                request.request_id,
                version_id,
                level.as_i64(),
                payload_json,
                request.created_at
            ],
        )?;
        let completion_sequence = tx.last_insert_rowid();
        let stamp_column = match level {
            StoreLevel::L1 => "complete_l1",
            StoreLevel::L2 => "complete_l2",
            StoreLevel::L3 => "complete_l3",
        };
        tx.execute(
            &format!("UPDATE file_versions SET {stamp_column} = ?1 WHERE version_id = ?2"),
            params![completion_sequence, version_id],
        )?;
        tx.commit()?;
        Ok(StoreWriteResult {
            state,
            version_id,
            level,
            counts,
            completion_sequence,
            statement_preparations: capability_preparations + row_preparations,
        })
    }
}

fn completion_stamp(version: &StoredFileVersion, level: StoreLevel) -> Option<i64> {
    match level {
        StoreLevel::L1 => version.complete_l1,
        StoreLevel::L2 => version.complete_l2,
        StoreLevel::L3 => version.complete_l3,
    }
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
) -> rusqlite::Result<Option<StoredFileVersion>> {
    tx.query_row(
        "SELECT version_id, path, content_hash, extraction_epoch,
                complete_l1, complete_l2, complete_l3
         FROM file_versions
         WHERE path = ?1 AND content_hash = ?2 AND extraction_epoch = ?3",
        params![path, content_hash, extraction_epoch],
        |row| {
            Ok(StoredFileVersion {
                version_id: row.get(0)?,
                path: row.get(1)?,
                content_hash: row.get(2)?,
                extraction_epoch: row.get(3)?,
                complete_l1: row.get(4)?,
                complete_l2: row.get(5)?,
                complete_l3: row.get(6)?,
            })
        },
    )
    .optional()
}
