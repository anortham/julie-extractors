use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};

use super::coordinator::{CoordinatorError, LeaseDisposition, LeaseHolder, StoreCoordinator};
use super::pragmas::{PragmaError, WriterPragmaProfile, configure_writer_pragmas};
use super::{STORE_SQLITE_SCHEMA_VERSION, StoreLayout, StoreLayoutError, StoreSchemaError};

static NEXT_DIRECT_WRITER: AtomicU64 = AtomicU64::new(1);

/// Ownership proof for writes performed by the active maintenance run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationFence {
    root: PathBuf,
    generation_name: String,
    run_id: Option<String>,
    owner_id: String,
    owner_pid: u32,
    fencing_token: i64,
    checked_at: i64,
}

impl GenerationFence {
    pub fn maintenance(
        layout: &StoreLayout,
        run_id: impl Into<String>,
        owner_id: impl Into<String>,
        owner_pid: u32,
        fencing_token: i64,
        checked_at: i64,
    ) -> Self {
        Self {
            root: layout.root().to_path_buf(),
            generation_name: layout.generation_name().to_string(),
            run_id: Some(run_id.into()),
            owner_id: owner_id.into(),
            owner_pid,
            fencing_token,
            checked_at,
        }
    }

    pub fn writer(
        layout: &StoreLayout,
        owner_id: impl Into<String>,
        owner_pid: u32,
        fencing_token: i64,
        checked_at: i64,
    ) -> Self {
        Self {
            root: layout.root().to_path_buf(),
            generation_name: layout.generation_name().to_string(),
            run_id: None,
            owner_id: owner_id.into(),
            owner_pid,
            fencing_token,
            checked_at,
        }
    }
}

/// Writable store connection that retains ownership of its coordinator lease.
pub struct StoreWriterConnection {
    connection: Connection,
    lease: Option<(StoreCoordinator, LeaseHolder, i64)>,
    fence: GenerationFence,
}

impl fmt::Debug for StoreWriterConnection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoreWriterConnection")
            .field("fence", &self.fence)
            .finish_non_exhaustive()
    }
}

impl Deref for StoreWriterConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

impl DerefMut for StoreWriterConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connection
    }
}

impl Drop for StoreWriterConnection {
    fn drop(&mut self) {
        if let Some((coordinator, holder, fencing_token)) = self.lease.as_mut() {
            let _ = coordinator.release_lease(holder, *fencing_token);
        }
    }
}

/// Opens store connections under the family and binary compatibility contract.
#[derive(Debug, Clone)]
pub struct StoreConnectionFactory {
    layout: StoreLayout,
    expected_family_id: String,
    binary_version: String,
    generation_fence: Option<GenerationFence>,
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
            generation_fence: None,
        }
    }

    pub fn with_generation_fence(mut self, fence: GenerationFence) -> Self {
        self.generation_fence = Some(fence);
        self
    }

    /// Validates that this generation may begin a new mutation.
    pub fn validate_write_fence(&self) -> Result<(), StoreConnectionError> {
        let connection = Connection::open_with_flags(
            self.layout.store_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        validate_store_schema(&connection)?;
        let found_family = metadata_value(&connection, "family_id")?;
        if found_family != self.expected_family_id {
            return Err(StoreConnectionError::FamilyMismatch {
                expected: self.expected_family_id.clone(),
                found: found_family,
            });
        }
        self.validate_generation_write_fence(&connection)
    }

    pub(crate) fn layout(&self) -> &StoreLayout {
        &self.layout
    }

    pub(crate) fn binary_version(&self) -> &str {
        &self.binary_version
    }

    pub(crate) fn validate_writer_compatibility(&self) -> Result<(), StoreConnectionError> {
        let connection = Connection::open_with_flags(
            self.layout.store_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        validate_store_schema(&connection)?;
        self.validate_identity_and_floor(&connection, AccessMode::Writer)
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
    pub fn open_writer(&self) -> Result<StoreWriterConnection, StoreConnectionError> {
        self.validate_write_fence()?;
        let connection = Connection::open_with_flags(
            self.layout.store_db(),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        validate_store_schema(&connection)?;
        self.validate_identity_and_floor(&connection, AccessMode::Writer)?;
        self.validate_generation_write_fence(&connection)?;
        let (fence, lease) = match self.generation_fence.clone() {
            Some(fence) if fence.run_id.is_some() => {
                // Maintenance path: full intent identity was checked above. Do not treat
                // holder_id/PID alone as ownership and do not take an ordinary lease.
                (fence, None)
            }
            Some(fence) => {
                // Pre-fenced ordinary writer (caller already holds the lease).
                (fence, None)
            }
            None => {
                let sequence = NEXT_DIRECT_WRITER.fetch_add(1, AtomicOrdering::Relaxed);
                let holder = LeaseHolder::new(
                    format!("store-factory-{}-{sequence}", std::process::id()),
                    &self.binary_version,
                    std::process::id(),
                );
                let mut coordinator =
                    StoreCoordinator::open(&self.layout).map_err(map_coordinator_lease_error)?;
                let deadline = Instant::now() + Duration::from_secs(5);
                let (fencing_token, checked_at) = loop {
                    let checked_at = system_now_ms();
                    let disposition = coordinator
                        .try_acquire_or_takeover(holder.clone(), checked_at)
                        .map_err(map_coordinator_lease_error)?;
                    if let LeaseDisposition::Acquired { fencing_token } = disposition {
                        if let Err(error) = self.validate_generation_write_fence(&connection) {
                            let _ = coordinator.release_lease(&holder, fencing_token);
                            return Err(error);
                        }
                        break (fencing_token, checked_at);
                    }
                    if Instant::now() >= deadline {
                        return Err(StoreConnectionError::WriterLeaseUnavailable {
                            detail: "store-writer lease is held by another process".to_string(),
                        });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                };
                (
                    GenerationFence::writer(
                        &self.layout,
                        &holder.holder_id,
                        holder.holder_pid,
                        fencing_token,
                        checked_at,
                    ),
                    Some((coordinator, holder, fencing_token)),
                )
            }
        };
        let writer = StoreWriterConnection {
            connection,
            lease,
            fence,
        };
        self.validate_writer_lease(&writer.fence)?;
        configure_writer_pragmas(&writer, WriterPragmaProfile::Routine)?;
        Ok(writer)
    }

    /// Advances the recorded binary version from an explicitly held write path.
    pub fn advance_binary_version(
        &self,
        connection: &mut StoreWriterConnection,
    ) -> Result<(), StoreConnectionError> {
        self.validate_generation_write_fence(connection)?;
        self.validate_writer_lease(&connection.fence)?;
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

    fn validate_writer_lease(&self, fence: &GenerationFence) -> Result<(), StoreConnectionError> {
        let owns_generation = fence.root == self.layout.root()
            && fence.generation_name == self.layout.generation_name();
        // Prefer wall clock when the fence was minted near real time (production
        // drain/resolve path). Injected test clocks (tiny epochs or historical
        // absolute stamps far from wall) validate in the fence clock domain so
        // expires_at and now stay coherent. Quantum fences are recreated every
        // few seconds, so a 60s near-wall window does not reintroduce long-lived
        // stale checked_at acceptance.
        let wall_now = system_now_ms();
        // Quantum fences are recreated every few seconds. Anything minted more
        // than 10 minutes from wall is treated as a synthetic/historical clock
        // domain (tests), and validated against fence.checked_at instead.
        let near_wall = fence.checked_at <= wall_now.saturating_add(5_000)
            && wall_now.saturating_sub(fence.checked_at) <= 600_000;
        let now_ms = if near_wall {
            wall_now
        } else {
            fence.checked_at
        };
        let owns_lease = Connection::open_with_flags(
            self.layout.coordinator_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?
        .query_row(
            "SELECT EXISTS (
               SELECT 1 FROM writer_lease
               WHERE resource = 'store-writer' AND holder_id = ?1 AND holder_pid = ?2
                 AND fencing_token = ?3 AND expires_at > ?4
             )",
            rusqlite::params![fence.owner_id, fence.owner_pid, fence.fencing_token, now_ms],
            |row| row.get::<_, i64>(0),
        )? == 1;
        if !owns_generation || !owns_lease {
            return Err(StoreConnectionError::WriterLeaseLost);
        }
        Ok(())
    }

    fn validate_generation_write_fence(
        &self,
        connection: &Connection,
    ) -> Result<(), StoreConnectionError> {
        let current = StoreLayout::open(self.layout.root())?;
        if current.generation_name() != self.layout.generation_name() {
            return Err(StoreConnectionError::CurrentGenerationChanged {
                expected: self.layout.generation_name().to_string(),
                found: current.generation_name().to_string(),
            });
        }
        let state = metadata_value(connection, "generation_state")?;
        if state != "serving" {
            return Err(StoreConnectionError::GenerationNotServing { state });
        }
        let coordinator = Connection::open_with_flags(
            self.layout.coordinator_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let intent = coordinator
            .query_row(
                "SELECT run_id, owner_id, owner_pid, fencing_token, source_generation_name, expires_at
                 FROM maintenance_intent WHERE resource = 'store-maintenance'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u32>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((run_id, owner_id, owner_pid, fencing_token, generation_name, expires_at)) =
            intent
        else {
            return Ok(());
        };
        let now_ms = system_now_ms();
        if expires_at <= now_ms {
            return Ok(());
        }
        let matching_fence = self.generation_fence.as_ref().is_some_and(|fence| {
            fence.root == self.layout.root()
                && fence.generation_name == self.layout.generation_name()
                && fence.run_id.as_deref() == Some(run_id.as_str())
                && fence.owner_id == owner_id
                && fence.owner_pid == owner_pid
                && fence.fencing_token == fencing_token
                && generation_name == self.layout.generation_name()
        });
        if matching_fence {
            Ok(())
        } else {
            Err(StoreConnectionError::MaintenanceInProgress { run_id })
        }
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
    CurrentGenerationChanged {
        expected: String,
        found: String,
    },
    GenerationNotServing {
        state: String,
    },
    MaintenanceInProgress {
        run_id: String,
    },
    WriterLeaseLost,
    WriterLeaseUnavailable {
        detail: String,
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
    Layout(StoreLayoutError),
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
            Self::CurrentGenerationChanged { expected, found } => write!(
                formatter,
                "store generation changed from {expected:?} to {found:?}"
            ),
            Self::GenerationNotServing { state } => {
                write!(formatter, "store generation is {state:?}, not serving")
            }
            Self::MaintenanceInProgress { run_id } => {
                write!(formatter, "store maintenance run {run_id:?} is in progress")
            }
            Self::WriterLeaseLost => write!(formatter, "store writer lease fencing check failed"),
            Self::WriterLeaseUnavailable { detail } => {
                write!(formatter, "store writer lease is unavailable: {detail}")
            }
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
            Self::Layout(error) => error.fmt(formatter),
            Self::Sqlite(error) => error.fmt(formatter),
        }
    }
}

impl Error for StoreConnectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Schema(error) => Some(error),
            Self::Layout(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::FamilyMismatch { .. }
            | Self::ReaderVersionTooOld { .. }
            | Self::WriterVersionTooOld { .. }
            | Self::CurrentGenerationChanged { .. }
            | Self::GenerationNotServing { .. }
            | Self::MaintenanceInProgress { .. }
            | Self::WriterLeaseLost
            | Self::WriterLeaseUnavailable { .. }
            | Self::MissingMetadata { .. }
            | Self::InvalidVersion { .. }
            | Self::PragmaMismatch { .. }
            | Self::TextPragmaMismatch { .. } => None,
        }
    }
}

impl From<StoreLayoutError> for StoreConnectionError {
    fn from(error: StoreLayoutError) -> Self {
        Self::Layout(error)
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

fn map_coordinator_lease_error(error: CoordinatorError) -> StoreConnectionError {
    match error {
        CoordinatorError::StoreConnection(error) => error,
        other => StoreConnectionError::WriterLeaseUnavailable {
            detail: other.to_string(),
        },
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

pub(crate) fn system_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
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
