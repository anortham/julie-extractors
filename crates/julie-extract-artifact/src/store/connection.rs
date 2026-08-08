use std::cmp::Ordering;
use std::error::Error;
use std::fmt;

use rusqlite::{Connection, OpenFlags, TransactionBehavior};

use super::pragmas::{PragmaError, WriterPragmaProfile, configure_writer_pragmas};
use super::{STORE_SQLITE_SCHEMA_VERSION, StoreLayout, StoreSchemaError};

/// Opens store connections under the family and binary compatibility contract.
#[derive(Debug, Clone)]
pub struct StoreConnectionFactory {
    layout: StoreLayout,
    expected_family_id: String,
    binary_version: String,
}

impl StoreConnectionFactory {
    pub fn new(
        layout: StoreLayout,
        expected_family_id: impl Into<String>,
        binary_version: impl Into<String>,
    ) -> Self {
        Self {
            layout,
            expected_family_id: expected_family_id.into(),
            binary_version: binary_version.into(),
        }
    }

    pub(crate) fn layout(&self) -> &StoreLayout {
        &self.layout
    }

    /// Opens a query-only connection after enforcing the reader floor.
    pub fn open_reader(&self) -> Result<Connection, StoreConnectionError> {
        let connection = Connection::open_with_flags(
            self.layout.store_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        validate_store_schema(&connection)?;
        self.validate_identity_and_floor(&connection, AccessMode::Reader)?;
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA query_only = ON;")?;
        verify_integer_pragma(&connection, "foreign_keys", 1)?;
        verify_integer_pragma(&connection, "query_only", 1)?;
        Ok(connection)
    }

    /// Opens a writable connection after enforcing the writer floor and durability pragmas.
    pub fn open_writer(&self) -> Result<Connection, StoreConnectionError> {
        let mut connection = Connection::open_with_flags(
            self.layout.store_db(),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        validate_store_schema(&connection)?;
        self.validate_identity_and_floor(&connection, AccessMode::Writer)?;
        configure_writer_pragmas(&connection, WriterPragmaProfile::Routine)?;
        self.advance_binary_version(&mut connection)?;
        Ok(connection)
    }

    fn advance_binary_version(
        &self,
        connection: &mut Connection,
    ) -> Result<(), StoreConnectionError> {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let recorded = metadata_value(&transaction, "binary_version")?;
        let running_version = ParsedVersion::parse("binary_version", &self.binary_version)?;
        let recorded_version = ParsedVersion::parse("binary_version", &recorded)?;
        if running_version > recorded_version {
            transaction.execute(
                "UPDATE store_meta SET value = ?1 WHERE key = 'binary_version'",
                [&self.binary_version],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn validate_identity_and_floor(
        &self,
        connection: &Connection,
        access_mode: AccessMode,
    ) -> Result<(), StoreConnectionError> {
        let found_family = metadata_value(connection, "family_id")?;
        if found_family != self.expected_family_id {
            return Err(StoreConnectionError::FamilyMismatch {
                expected: self.expected_family_id.clone(),
                found: found_family,
            });
        }

        let (floor_key, required) = match access_mode {
            AccessMode::Reader => (
                "min_reader_version",
                metadata_value(connection, "min_reader_version")?,
            ),
            AccessMode::Writer => {
                let minimum_reader = metadata_value(connection, "min_reader_version")?;
                let minimum_writer = metadata_value(connection, "min_writer_version")?;
                let recorded = metadata_value(connection, "binary_version")?;
                (
                    "min_writer_version",
                    required_writer_version(
                        &minimum_reader,
                        &minimum_writer,
                        &recorded,
                        extractor_downgrade_allowed(),
                    )?
                    .to_string(),
                )
            }
        };
        let running_version = ParsedVersion::parse("binary_version", &self.binary_version)?;
        let required_version = ParsedVersion::parse(floor_key, &required)?;
        if running_version < required_version {
            return Err(match access_mode {
                AccessMode::Reader => StoreConnectionError::ReaderVersionTooOld {
                    running: self.binary_version.clone(),
                    required,
                },
                AccessMode::Writer => StoreConnectionError::WriterVersionTooOld {
                    running: self.binary_version.clone(),
                    required,
                },
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum AccessMode {
    Reader,
    Writer,
}

/// A typed compatibility, schema, pragma, or SQLite connection failure.
#[derive(Debug)]
pub enum StoreConnectionError {
    FamilyMismatch {
        expected: String,
        found: String,
    },
    ReaderVersionTooOld {
        running: String,
        required: String,
    },
    WriterVersionTooOld {
        running: String,
        required: String,
    },
    MissingMetadata {
        key: &'static str,
    },
    InvalidVersion {
        field: &'static str,
        value: String,
    },
    PragmaMismatch {
        pragma: &'static str,
        expected: i64,
        found: i64,
    },
    TextPragmaMismatch {
        pragma: &'static str,
        expected: &'static str,
        found: String,
    },
    Schema(StoreSchemaError),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for StoreConnectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FamilyMismatch { expected, found } => {
                write!(
                    formatter,
                    "store family {found:?} does not match {expected:?}"
                )
            }
            Self::ReaderVersionTooOld { running, required } => write!(
                formatter,
                "binary {running} cannot read this store; version {required} is required"
            ),
            Self::WriterVersionTooOld { running, required } => write!(
                formatter,
                "binary {running} cannot write this store; version {required} is required"
            ),
            Self::MissingMetadata { key } => write!(formatter, "store metadata is missing {key}"),
            Self::InvalidVersion { field, value } => {
                write!(
                    formatter,
                    "store metadata {field} has invalid version {value:?}"
                )
            }
            Self::PragmaMismatch {
                pragma,
                expected,
                found,
            } => write!(
                formatter,
                "SQLite pragma {pragma} is {found}, expected {expected}"
            ),
            Self::TextPragmaMismatch {
                pragma,
                expected,
                found,
            } => write!(
                formatter,
                "SQLite pragma {pragma} is {found:?}, expected {expected:?}"
            ),
            Self::Schema(error) => error.fmt(formatter),
            Self::Sqlite(error) => error.fmt(formatter),
        }
    }
}

impl Error for StoreConnectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Schema(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::FamilyMismatch { .. }
            | Self::ReaderVersionTooOld { .. }
            | Self::WriterVersionTooOld { .. }
            | Self::MissingMetadata { .. }
            | Self::InvalidVersion { .. }
            | Self::PragmaMismatch { .. }
            | Self::TextPragmaMismatch { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for StoreConnectionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<PragmaError> for StoreConnectionError {
    fn from(error: PragmaError) -> Self {
        match error {
            PragmaError::Sqlite(error) => Self::Sqlite(error),
            PragmaError::IntegerMismatch {
                pragma,
                expected,
                found,
            } => Self::PragmaMismatch {
                pragma,
                expected,
                found,
            },
            PragmaError::TextMismatch {
                pragma,
                expected,
                found,
            } => Self::TextPragmaMismatch {
                pragma,
                expected,
                found,
            },
        }
    }
}

impl From<StoreSchemaError> for StoreConnectionError {
    fn from(error: StoreSchemaError) -> Self {
        Self::Schema(error)
    }
}

fn validate_store_schema(connection: &Connection) -> Result<(), StoreConnectionError> {
    let found = connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?;
    if found > STORE_SQLITE_SCHEMA_VERSION {
        return Err(StoreSchemaError::NewerSchema {
            database: "store.db",
            found,
            supported: STORE_SQLITE_SCHEMA_VERSION,
        }
        .into());
    }
    if found < STORE_SQLITE_SCHEMA_VERSION {
        return Err(StoreSchemaError::OlderSchema {
            database: "store.db",
            found,
            supported: STORE_SQLITE_SCHEMA_VERSION,
        }
        .into());
    }
    Ok(())
}

fn metadata_value(
    connection: &Connection,
    key: &'static str,
) -> Result<String, StoreConnectionError> {
    match connection.query_row(
        "SELECT value FROM store_meta WHERE key = ?1",
        [key],
        |row| row.get(0),
    ) {
        Ok(value) => Ok(value),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            Err(StoreConnectionError::MissingMetadata { key })
        }
        Err(error) => Err(error.into()),
    }
}

fn verify_integer_pragma(
    connection: &Connection,
    pragma: &'static str,
    expected: i64,
) -> Result<(), StoreConnectionError> {
    let found = connection.query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))?;
    if found == expected {
        Ok(())
    } else {
        Err(StoreConnectionError::PragmaMismatch {
            pragma,
            expected,
            found,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedVersion {
    core: Vec<u64>,
    pre_release: Option<Vec<VersionIdentifier>>,
}

pub(crate) fn compare_versions(left: &str, right: &str) -> Result<Ordering, StoreConnectionError> {
    Ok(ParsedVersion::parse("version", left)?.cmp(&ParsedVersion::parse("version", right)?))
}

pub(crate) fn required_writer_version<'a>(
    minimum_reader: &'a str,
    minimum_writer: &'a str,
    recorded_binary: &'a str,
    allow_downgrade: bool,
) -> Result<&'a str, StoreConnectionError> {
    let reader = ParsedVersion::parse("min_reader_version", minimum_reader)?;
    let writer = ParsedVersion::parse("min_writer_version", minimum_writer)?;
    let recorded = ParsedVersion::parse("binary_version", recorded_binary)?;
    let (minimum, minimum_version) = if reader >= writer {
        (minimum_reader, reader)
    } else {
        (minimum_writer, writer)
    };
    if allow_downgrade || minimum_version >= recorded {
        Ok(minimum)
    } else {
        Ok(recorded_binary)
    }
}

pub(crate) fn extractor_downgrade_allowed() -> bool {
    std::env::var_os("MILLER_ALLOW_EXTRACTOR_DOWNGRADE").is_some_and(|value| value == "1")
}

impl ParsedVersion {
    fn parse(field: &'static str, value: &str) -> Result<Self, StoreConnectionError> {
        let value_without_prefix = value.strip_prefix('v').unwrap_or(value);
        let version_without_build = value_without_prefix
            .split_once('+')
            .map_or(value_without_prefix, |(version, _)| version);
        let (core, pre_release) = version_without_build
            .split_once('-')
            .map_or((version_without_build, None), |(core, pre)| {
                (core, Some(pre))
            });
        let core = core
            .split('.')
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| StoreConnectionError::InvalidVersion {
                field,
                value: value.to_string(),
            })?;
        if core.is_empty() {
            return Err(StoreConnectionError::InvalidVersion {
                field,
                value: value.to_string(),
            });
        }
        let pre_release = pre_release
            .map(|pre_release| {
                if pre_release.is_empty() {
                    return Err(StoreConnectionError::InvalidVersion {
                        field,
                        value: value.to_string(),
                    });
                }
                pre_release
                    .split('.')
                    .map(|identifier| {
                        if identifier.is_empty()
                            || !identifier
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                        {
                            Err(StoreConnectionError::InvalidVersion {
                                field,
                                value: value.to_string(),
                            })
                        } else if let Ok(number) = identifier.parse::<u64>() {
                            Ok(VersionIdentifier::Numeric(number))
                        } else {
                            Ok(VersionIdentifier::Text(identifier.to_string()))
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        Ok(Self { core, pre_release })
    }
}

impl PartialOrd for ParsedVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ParsedVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let width = self.core.len().max(other.core.len());
        for index in 0..width {
            let ordering = self
                .core
                .get(index)
                .copied()
                .unwrap_or_default()
                .cmp(&other.core.get(index).copied().unwrap_or_default());
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        match (&self.pre_release, &other.pre_release) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(left), Some(right)) => left.cmp(right),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum VersionIdentifier {
    Numeric(u64),
    Text(String),
}
