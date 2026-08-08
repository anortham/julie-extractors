use std::error::Error;
use std::fmt;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::StoreLevel;

/// Coordinates and payload for one durable store-log entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLogEntry {
    pub request_id: String,
    pub event_kind: String,
    pub view_id: Option<String>,
    pub generation: Option<u64>,
    pub version_id: Option<i64>,
    pub level: Option<StoreLevel>,
    pub payload_json: String,
    pub created_at: String,
}

impl StoreLogEntry {
    pub fn new(
        request_id: impl Into<String>,
        event_kind: impl Into<String>,
        payload_json: impl Into<String>,
        created_at: impl Into<String>,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            event_kind: event_kind.into(),
            view_id: None,
            generation: None,
            version_id: None,
            level: None,
            payload_json: payload_json.into(),
            created_at: created_at.into(),
        }
    }

    pub fn with_view(mut self, view_id: impl Into<String>) -> Self {
        self.view_id = Some(view_id.into());
        self
    }

    pub fn with_generation(mut self, generation: u64) -> Self {
        self.generation = Some(generation);
        self
    }

    pub fn with_version(mut self, version_id: i64) -> Self {
        self.version_id = Some(version_id);
        self
    }

    pub fn with_level(mut self, level: StoreLevel) -> Self {
        self.level = Some(level);
        self
    }
}

/// A committed store-log row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLogRecord {
    pub sequence: i64,
    pub request_id: String,
    pub event_kind: String,
    pub view_id: Option<String>,
    pub generation: Option<u64>,
    pub version_id: Option<i64>,
    pub level: Option<StoreLevel>,
    pub terminal: bool,
    pub payload_json: String,
    pub created_at: String,
}

/// Typed store-log protocol or SQLite failure.
#[derive(Debug)]
pub enum StoreLogError {
    InvalidEntry,
    InvalidChunkIndex {
        chunk_index: u64,
    },
    ChunkOutOfOrder {
        request_id: String,
        expected: u64,
        actual: u64,
    },
    RequestMismatch {
        sequence: i64,
        expected: String,
        found: String,
    },
    LogEntryNotFound {
        sequence: i64,
    },
    TerminalSequence {
        sequence: i64,
    },
    RequestAlreadyTerminal {
        request_id: String,
    },
    TerminalAlreadyExists {
        request_id: String,
    },
    InvalidLevel {
        value: i64,
    },
    Sqlite(rusqlite::Error),
}

impl StoreLogError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidEntry => "invalid_store_log_entry",
            Self::InvalidChunkIndex { .. } => "invalid_request_chunk_index",
            Self::ChunkOutOfOrder { .. } => "request_chunk_out_of_order",
            Self::RequestMismatch { .. } => "request_chunk_log_mismatch",
            Self::LogEntryNotFound { .. } => "store_log_entry_not_found",
            Self::TerminalSequence { .. } => "request_chunk_terminal_sequence",
            Self::RequestAlreadyTerminal { .. } => "request_already_terminal",
            Self::TerminalAlreadyExists { .. } => "terminal_already_exists",
            Self::InvalidLevel { .. } => "invalid_store_level",
            Self::Sqlite(_) => "store_sqlite_error",
        }
    }
}

impl fmt::Display for StoreLogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntry => write!(formatter, "store-log entry is invalid"),
            Self::InvalidChunkIndex { chunk_index } => {
                write!(
                    formatter,
                    "request chunk index {chunk_index} does not fit SQLite"
                )
            }
            Self::ChunkOutOfOrder {
                request_id,
                expected,
                actual,
            } => write!(
                formatter,
                "request {request_id:?} chunk {actual} is out of order; expected {expected}"
            ),
            Self::RequestMismatch {
                sequence,
                expected,
                found,
            } => write!(
                formatter,
                "store-log sequence {sequence} belongs to {found:?}, not {expected:?}"
            ),
            Self::LogEntryNotFound { sequence } => {
                write!(formatter, "store-log sequence {sequence} was not found")
            }
            Self::TerminalSequence { sequence } => {
                write!(
                    formatter,
                    "terminal store-log sequence {sequence} cannot be a chunk"
                )
            }
            Self::RequestAlreadyTerminal { request_id } => {
                write!(formatter, "request {request_id:?} is already terminal")
            }
            Self::TerminalAlreadyExists { request_id } => {
                write!(
                    formatter,
                    "request {request_id:?} already has a terminal entry"
                )
            }
            Self::InvalidLevel { value } => write!(formatter, "store-log level {value} is invalid"),
            Self::Sqlite(error) => error.fmt(formatter),
        }
    }
}

impl Error for StoreLogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for StoreLogError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

/// Appends transaction-bound effects and reads the terminal idempotency anchor.
pub struct StoreLog;

impl StoreLog {
    pub fn append_effect(
        transaction: &Transaction<'_>,
        entry: &StoreLogEntry,
    ) -> Result<i64, StoreLogError> {
        validate_entry(entry)?;
        ensure_request_open(transaction, &entry.request_id)?;
        insert_entry(transaction, entry, false)
    }

    pub fn append_progress(
        transaction: &Transaction<'_>,
        entry: &StoreLogEntry,
        chunk_index: u64,
    ) -> Result<i64, StoreLogError> {
        validate_entry(entry)?;
        ensure_request_open(transaction, &entry.request_id)?;
        validate_next_chunk(transaction, &entry.request_id, chunk_index)?;
        let sequence = insert_entry(transaction, entry, false)?;
        Self::record_progress(
            transaction,
            &entry.request_id,
            chunk_index,
            sequence,
            entry.level,
            &entry.payload_json,
            &entry.created_at,
        )?;
        Ok(sequence)
    }

    pub fn record_progress(
        transaction: &Transaction<'_>,
        request_id: &str,
        chunk_index: u64,
        store_log_sequence: i64,
        level: Option<StoreLevel>,
        payload_json: &str,
        created_at: &str,
    ) -> Result<(), StoreLogError> {
        if request_id.is_empty()
            || created_at.is_empty()
            || serde_json::from_str::<serde_json::Value>(payload_json).is_err()
        {
            return Err(StoreLogError::InvalidEntry);
        }
        ensure_request_open(transaction, request_id)?;
        validate_next_chunk(transaction, request_id, chunk_index)?;
        let sequence_owner = transaction
            .query_row(
                "SELECT request_id, terminal FROM store_log WHERE sequence = ?1",
                [store_log_sequence],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
            )
            .optional()?
            .ok_or(StoreLogError::LogEntryNotFound {
                sequence: store_log_sequence,
            })?;
        if sequence_owner.0 != request_id {
            return Err(StoreLogError::RequestMismatch {
                sequence: store_log_sequence,
                expected: request_id.to_string(),
                found: sequence_owner.0,
            });
        }
        if sequence_owner.1 {
            return Err(StoreLogError::TerminalSequence {
                sequence: store_log_sequence,
            });
        }
        let chunk_index_sql = i64::try_from(chunk_index)
            .map_err(|_| StoreLogError::InvalidChunkIndex { chunk_index })?;
        transaction.execute(
            "INSERT INTO request_chunks
             (request_id, chunk_index, store_log_sequence, level, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                request_id,
                chunk_index_sql,
                store_log_sequence,
                level.map(StoreLevel::as_i64),
                payload_json,
                created_at,
            ],
        )?;
        Ok(())
    }

    pub fn append_terminal(
        transaction: &Transaction<'_>,
        entry: &StoreLogEntry,
    ) -> Result<i64, StoreLogError> {
        validate_entry(entry)?;
        if terminal_record(transaction, &entry.request_id)?.is_some() {
            return Err(StoreLogError::TerminalAlreadyExists {
                request_id: entry.request_id.clone(),
            });
        }
        insert_entry(transaction, entry, true).map_err(|error| match error {
            StoreLogError::Sqlite(error)
                if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) =>
            {
                StoreLogError::TerminalAlreadyExists {
                    request_id: entry.request_id.clone(),
                }
            }
            other => other,
        })
    }

    pub fn committed_in_fact(
        connection: &Connection,
        request_id: &str,
    ) -> Result<Option<StoreLogRecord>, StoreLogError> {
        terminal_record(connection, request_id)
    }
}

fn insert_entry(
    transaction: &Transaction<'_>,
    entry: &StoreLogEntry,
    terminal: bool,
) -> Result<i64, StoreLogError> {
    let generation = entry
        .generation
        .map(i64::try_from)
        .transpose()
        .map_err(|_| StoreLogError::InvalidEntry)?;
    transaction.execute(
        "INSERT INTO store_log
         (request_id, event_kind, view_id, generation, version_id, level,
          terminal, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            entry.request_id,
            entry.event_kind,
            entry.view_id,
            generation,
            entry.version_id,
            entry.level.map(StoreLevel::as_i64),
            terminal,
            entry.payload_json,
            entry.created_at,
        ],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn validate_entry(entry: &StoreLogEntry) -> Result<(), StoreLogError> {
    if entry.request_id.is_empty()
        || entry.event_kind.is_empty()
        || entry.created_at.is_empty()
        || serde_json::from_str::<serde_json::Value>(&entry.payload_json).is_err()
    {
        Err(StoreLogError::InvalidEntry)
    } else {
        Ok(())
    }
}

fn ensure_request_open(connection: &Connection, request_id: &str) -> Result<(), StoreLogError> {
    if terminal_record(connection, request_id)?.is_some() {
        Err(StoreLogError::RequestAlreadyTerminal {
            request_id: request_id.to_string(),
        })
    } else {
        Ok(())
    }
}

fn validate_next_chunk(
    connection: &Connection,
    request_id: &str,
    chunk_index: u64,
) -> Result<(), StoreLogError> {
    let next = connection.query_row(
        "SELECT COALESCE(MAX(chunk_index), -1) + 1
         FROM request_chunks WHERE request_id = ?1",
        [request_id],
        |row| row.get::<_, i64>(0),
    )?;
    let expected = u64::try_from(next).expect("request chunk indexes are non-negative");
    if expected == chunk_index {
        Ok(())
    } else {
        Err(StoreLogError::ChunkOutOfOrder {
            request_id: request_id.to_string(),
            expected,
            actual: chunk_index,
        })
    }
}

fn terminal_record(
    connection: &Connection,
    request_id: &str,
) -> Result<Option<StoreLogRecord>, StoreLogError> {
    let record = connection
        .query_row(
            "SELECT sequence, request_id, event_kind, view_id, generation,
                    version_id, level, terminal, payload_json, created_at
             FROM store_log
             WHERE request_id = ?1 AND terminal = 1",
            [request_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, bool>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            },
        )
        .optional()?;
    record
        .map(
            |(
                sequence,
                request_id,
                event_kind,
                view_id,
                generation,
                version_id,
                level,
                terminal,
                payload_json,
                created_at,
            )| {
                Ok(StoreLogRecord {
                    sequence,
                    request_id,
                    event_kind,
                    view_id,
                    generation: generation
                        .map(u64::try_from)
                        .transpose()
                        .map_err(|_| StoreLogError::InvalidEntry)?,
                    version_id,
                    level: level.map(parse_level).transpose()?,
                    terminal,
                    payload_json,
                    created_at,
                })
            },
        )
        .transpose()
}

fn parse_level(value: i64) -> Result<StoreLevel, StoreLogError> {
    match value {
        1 => Ok(StoreLevel::L1),
        2 => Ok(StoreLevel::L2),
        3 => Ok(StoreLevel::L3),
        _ => Err(StoreLogError::InvalidLevel { value }),
    }
}
