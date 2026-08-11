use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use sha2::{Digest, Sha256};

use super::pragmas::{WriterPragmaProfile, configure_writer_pragmas};
use super::{
    GenerationFence, ResolutionBaseRecord, ResolutionBaseState, ResolutionDeltaRecord,
    ResolutionGapFact, ResolutionGapKind, ResolutionGapTable, ResolutionIdentifierDeltaRecord,
    ResolutionPendingDeltaRecord, ResolutionPendingOperation, ResolutionPinOwnerKind,
    ResolutionPinRecord, ResolutionScratchReader, StoreConnectionError, StoreConnectionFactory,
    StoreLog, StoreLogEntry, StoreLogError, ViewResolutionState,
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionBaseBuild {
    pub record: ResolutionBaseRecord,
    pub scratch_path: PathBuf,
    pub final_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionBaseBegin {
    Build(ResolutionBaseBuild),
    Building(ResolutionBaseRecord),
    Ready(ResolutionBaseRecord),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionBaseRecovery {
    Ready(ResolutionBaseRecord),
    Rebuild(ResolutionBaseBuild),
    LiveOwner(ResolutionBaseRecord),
}

#[derive(Debug)]
pub enum ResolutionBaseCatalogError {
    InvalidArgument(&'static str),
    ManifestNotFound { manifest_hash: String },
    IncompleteVersion { version_id: i64 },
    BuildOwnerMismatch { expected: String, found: String },
    ReadyCasLost { base_id: String },
    FileIdentityMismatch { detail: String },
    FileProtected { base_id: String },
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Connection(StoreConnectionError),
    Validation(ResolutionValidationError),
}

impl fmt::Display for ResolutionBaseCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgument(value) => write!(formatter, "invalid resolution base {value}"),
            Self::ManifestNotFound { manifest_hash } => {
                write!(
                    formatter,
                    "resolution manifest {manifest_hash:?} was not found"
                )
            }
            Self::IncompleteVersion { version_id } => write!(
                formatter,
                "resolution source version {version_id} is not complete through L2"
            ),
            Self::BuildOwnerMismatch { expected, found } => write!(
                formatter,
                "resolution base build owner {found:?} does not match {expected:?}"
            ),
            Self::ReadyCasLost { base_id } => {
                write!(formatter, "resolution base {base_id:?} ready CAS was lost")
            }
            Self::FileIdentityMismatch { detail } => {
                write!(
                    formatter,
                    "resolution base file identity mismatch: {detail}"
                )
            }
            Self::FileProtected { base_id } => {
                write!(
                    formatter,
                    "resolution base {base_id:?} is protected by a pin"
                )
            }
            Self::Io(error) => error.fmt(formatter),
            Self::Sqlite(error) => error.fmt(formatter),
            Self::Connection(error) => error.fmt(formatter),
            Self::Validation(error) => error.fmt(formatter),
        }
    }
}

impl Error for ResolutionBaseCatalogError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::Connection(error) => Some(error),
            Self::Validation(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ResolutionBaseCatalogError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for ResolutionBaseCatalogError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<StoreConnectionError> for ResolutionBaseCatalogError {
    fn from(error: StoreConnectionError) -> Self {
        Self::Connection(error)
    }
}

impl From<ResolutionValidationError> for ResolutionBaseCatalogError {
    fn from(error: ResolutionValidationError) -> Self {
        Self::Validation(error)
    }
}

#[derive(Debug, Clone)]
pub struct ResolutionBaseCatalog {
    factory: StoreConnectionFactory,
}

impl ResolutionBaseCatalog {
    pub fn new(factory: StoreConnectionFactory) -> Self {
        Self { factory }
    }

    pub fn begin_build(
        &self,
        manifest_hash: &str,
        resolver_output_epoch: i64,
        request_id: &str,
        now: &str,
    ) -> Result<ResolutionBaseBegin, ResolutionBaseCatalogError> {
        validate_catalog_identity(manifest_hash, resolver_output_epoch, request_id)?;
        let mut connection = self.factory.open_writer()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(record) =
            base_record_by_identity(&transaction, manifest_hash, resolver_output_epoch)?
        {
            transaction.commit()?;
            return Ok(match record.state {
                ResolutionBaseState::Ready => ResolutionBaseBegin::Ready(record),
                ResolutionBaseState::Building => ResolutionBaseBegin::Building(record),
            });
        }

        let source_versions = manifest_source_versions(&transaction, manifest_hash)?;
        for version_id in &source_versions {
            let complete_l2: Option<i64> = transaction.query_row(
                "SELECT complete_l2 FROM file_versions WHERE version_id=?1",
                [version_id],
                |row| row.get(0),
            )?;
            if complete_l2.is_none() {
                return Err(ResolutionBaseCatalogError::IncompleteVersion {
                    version_id: *version_id,
                });
            }
        }
        let base_id = base_id(manifest_hash, resolver_output_epoch);
        let relative_path = format!("bases/{base_id}.db");
        transaction.execute(
            "INSERT INTO resolution_bases
             (base_id,manifest_hash,resolver_output_epoch,state,relative_path,
              identifier_count,pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
             VALUES (?1,?2,?3,'building',?4,0,0,NULL,NULL,?5,?6,?6)",
            params![
                base_id,
                manifest_hash,
                resolver_output_epoch,
                relative_path,
                request_id,
                now,
            ],
        )?;
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("resolution_base_after_row_insert");
        {
            let mut insert = transaction.prepare(
                "INSERT INTO resolution_base_versions(base_id,version_id) VALUES (?1,?2)",
            )?;
            for version_id in source_versions {
                insert.execute(params![base_id, version_id])?;
            }
        }
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("resolution_base_after_root_insert");
        let record = base_record_by_id(&transaction, &base_id)?.ok_or_else(|| {
            ResolutionBaseCatalogError::ReadyCasLost {
                base_id: base_id.clone(),
            }
        })?;
        transaction.commit()?;
        Ok(ResolutionBaseBegin::Build(self.build_from_record(record)?))
    }

    pub fn publish_scratch(
        &self,
        build: &ResolutionBaseBuild,
    ) -> Result<ResolutionFileIdentity, ResolutionBaseCatalogError> {
        self.validate_build_paths(build)?;
        let scratch = ResolutionBaseReader::open(&build.scratch_path)?;
        self.validate_reader_for_catalog(&scratch, &build.record)?;
        drop(scratch);

        match fs::hard_link(&build.scratch_path, &build.final_path) {
            Ok(()) => {
                sync_directory_path(
                    build
                        .final_path
                        .parent()
                        .ok_or(ResolutionBaseCatalogError::InvalidArgument("final path"))?,
                )?;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let final_reader = ResolutionBaseReader::open(&build.final_path)?;
                self.validate_reader_for_catalog(&final_reader, &build.record)?;
            }
            Err(error) => return Err(error.into()),
        }
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("resolution_base_after_final_publish");
        remove_resolution_file(&build.scratch_path)?;
        Ok(ResolutionBaseReader::open(&build.final_path)?
            .file_identity()
            .clone())
    }

    pub fn mark_ready(
        &self,
        build: &ResolutionBaseBuild,
        now: &str,
    ) -> Result<ResolutionBaseRecord, ResolutionBaseCatalogError> {
        self.validate_build_paths(build)?;
        let reader = ResolutionBaseReader::open(&build.final_path)?;
        self.validate_reader_for_catalog(&reader, &build.record)?;
        let identity = reader.file_identity().clone();
        let file_bytes = i64::try_from(identity.file_bytes).map_err(|_| {
            ResolutionBaseCatalogError::FileIdentityMismatch {
                detail: "file length exceeds SQLite INTEGER".to_string(),
            }
        })?;

        let mut connection = self.factory.open_writer()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let owner = base_record_by_id(&transaction, &build.record.base_id)?.ok_or_else(|| {
            ResolutionBaseCatalogError::ReadyCasLost {
                base_id: build.record.base_id.clone(),
            }
        })?;
        if owner.request_id != build.record.request_id {
            return Err(ResolutionBaseCatalogError::BuildOwnerMismatch {
                expected: build.record.request_id.clone(),
                found: owner.request_id,
            });
        }
        if owner.state == ResolutionBaseState::Ready {
            transaction.commit()?;
            return self
                .find_ready(&owner.manifest_hash, owner.resolver_output_epoch)?
                .ok_or(ResolutionBaseCatalogError::ReadyCasLost {
                    base_id: owner.base_id,
                });
        }
        let changed = transaction.execute(
            "UPDATE resolution_bases
             SET state='ready',identifier_count=?1,pending_count=?2,file_bytes=?3,
                 file_sha256=?4,updated_at=?5
             WHERE base_id=?6 AND state='building' AND request_id=?7",
            params![
                i64::try_from(identity.counts.identifiers).map_err(|_| {
                    ResolutionBaseCatalogError::FileIdentityMismatch {
                        detail: "identifier count exceeds SQLite INTEGER".to_string(),
                    }
                })?,
                i64::try_from(identity.counts.pending).map_err(|_| {
                    ResolutionBaseCatalogError::FileIdentityMismatch {
                        detail: "pending count exceeds SQLite INTEGER".to_string(),
                    }
                })?,
                file_bytes,
                identity.file_sha256,
                now,
                build.record.base_id,
                build.record.request_id,
            ],
        )?;
        if changed != 1 {
            return Err(ResolutionBaseCatalogError::ReadyCasLost {
                base_id: build.record.base_id.clone(),
            });
        }
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("resolution_base_before_ready_commit");
        let record = base_record_by_id(&transaction, &build.record.base_id)?.ok_or_else(|| {
            ResolutionBaseCatalogError::ReadyCasLost {
                base_id: build.record.base_id.clone(),
            }
        })?;
        transaction.commit()?;
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("resolution_base_after_ready_commit");
        Ok(record)
    }

    pub fn find_ready(
        &self,
        manifest_hash: &str,
        resolver_output_epoch: i64,
    ) -> Result<Option<ResolutionBaseRecord>, ResolutionBaseCatalogError> {
        let connection = self.factory.open_reader()?;
        let Some(record) =
            base_record_by_identity(&connection, manifest_hash, resolver_output_epoch)?
        else {
            return Ok(None);
        };
        if record.state != ResolutionBaseState::Ready {
            return Ok(None);
        }
        let build = self.build_from_record(record.clone())?;
        let reader = ResolutionBaseReader::open(&build.final_path)?;
        self.validate_reader_for_catalog(&reader, &record)?;
        validate_ready_file_identity(&record, reader.file_identity())?;
        Ok(Some(record))
    }

    pub fn recover(
        &self,
        manifest_hash: &str,
        resolver_output_epoch: i64,
        claimant_request_id: &str,
        prior_owner_live: bool,
        now: &str,
    ) -> Result<ResolutionBaseRecovery, ResolutionBaseCatalogError> {
        validate_catalog_identity(manifest_hash, resolver_output_epoch, claimant_request_id)?;
        let connection = self.factory.open_reader()?;
        let Some(record) =
            base_record_by_identity(&connection, manifest_hash, resolver_output_epoch)?
        else {
            drop(connection);
            return match self.begin_build(
                manifest_hash,
                resolver_output_epoch,
                claimant_request_id,
                now,
            )? {
                ResolutionBaseBegin::Build(build) => Ok(ResolutionBaseRecovery::Rebuild(build)),
                ResolutionBaseBegin::Building(record) => {
                    Ok(ResolutionBaseRecovery::LiveOwner(record))
                }
                ResolutionBaseBegin::Ready(record) => Ok(ResolutionBaseRecovery::Ready(record)),
            };
        };
        drop(connection);
        let build = self.build_from_record(record.clone())?;
        let same_owner = record.request_id == claimant_request_id;
        let final_valid = self.valid_final_for_build(&build)?;

        if record.state == ResolutionBaseState::Ready && final_valid {
            if build.scratch_path.exists() {
                remove_resolution_file(&build.scratch_path)?;
            }
            return Ok(ResolutionBaseRecovery::Ready(
                self.find_ready(manifest_hash, resolver_output_epoch)?
                    .ok_or_else(|| ResolutionBaseCatalogError::ReadyCasLost {
                        base_id: record.base_id.clone(),
                    })?,
            ));
        }
        if !same_owner && prior_owner_live {
            return Ok(ResolutionBaseRecovery::LiveOwner(record));
        }
        if record.state == ResolutionBaseState::Ready
            && self.base_is_protected(&record.base_id, now)?
        {
            return Err(ResolutionBaseCatalogError::FileProtected {
                base_id: record.base_id,
            });
        }

        if final_valid {
            if build.scratch_path.exists() {
                remove_resolution_file(&build.scratch_path)?;
            }
            let reassigned = self.reassign_build_owner(&record, claimant_request_id, false, now)?;
            let reassigned_build = self.build_from_record(reassigned)?;
            return Ok(ResolutionBaseRecovery::Ready(
                self.mark_ready(&reassigned_build, now)?,
            ));
        }

        let scratch_valid = if build.scratch_path.exists() {
            ResolutionBaseReader::open(&build.scratch_path)
                .is_ok_and(|reader| self.validate_reader_for_catalog(&reader, &record).is_ok())
        } else {
            false
        };
        if scratch_valid {
            self.publish_scratch(&build)?;
            let reassigned = self.reassign_build_owner(
                &record,
                claimant_request_id,
                record.state == ResolutionBaseState::Ready,
                now,
            )?;
            return Ok(ResolutionBaseRecovery::Ready(
                self.mark_ready(&self.build_from_record(reassigned)?, now)?,
            ));
        }

        if build.final_path.exists() {
            remove_resolution_file(&build.final_path)?;
        }
        if build.scratch_path.exists() {
            let preserve_same_owner_scratch = same_owner
                && ResolutionBaseReader::open(&build.scratch_path)
                    .is_ok_and(|reader| validate_reader_for_record(&reader, &record).is_ok());
            if !preserve_same_owner_scratch {
                remove_resolution_file(&build.scratch_path)?;
            }
        }
        let reassigned = self.reassign_build_owner(
            &record,
            claimant_request_id,
            record.state == ResolutionBaseState::Ready,
            now,
        )?;
        Ok(ResolutionBaseRecovery::Rebuild(
            self.build_from_record(reassigned)?,
        ))
    }

    fn build_from_record(
        &self,
        record: ResolutionBaseRecord,
    ) -> Result<ResolutionBaseBuild, ResolutionBaseCatalogError> {
        validate_catalog_identity(
            &record.manifest_hash,
            record.resolver_output_epoch,
            &record.request_id,
        )?;
        let expected_base_id = base_id(&record.manifest_hash, record.resolver_output_epoch);
        let expected_relative_path = format!("bases/{expected_base_id}.db");
        if record.base_id != expected_base_id || record.relative_path != expected_relative_path {
            return Err(ResolutionBaseCatalogError::FileIdentityMismatch {
                detail: "catalog base ID or relative path is not canonical".to_string(),
            });
        }
        let final_path = self
            .factory
            .layout()
            .generation_dir()
            .join(&record.relative_path);
        let scratch_path = self.factory.layout().scratch_dir().join(format!(
            "resolution-{}-{}.partial.db",
            record.base_id, record.request_id
        ));
        let build = ResolutionBaseBuild {
            record,
            scratch_path,
            final_path,
        };
        self.validate_build_paths(&build)?;
        Ok(build)
    }

    fn validate_build_paths(
        &self,
        build: &ResolutionBaseBuild,
    ) -> Result<(), ResolutionBaseCatalogError> {
        ensure_contained(self.factory.layout().generation_dir(), &build.final_path)?;
        ensure_contained(self.factory.layout().scratch_dir(), &build.scratch_path)?;
        if build.final_path.parent() != Some(self.factory.layout().bases_dir())
            || build.scratch_path.parent() != Some(self.factory.layout().scratch_dir())
        {
            return Err(ResolutionBaseCatalogError::InvalidArgument("catalog path"));
        }
        Ok(())
    }

    fn validate_reader_versions(
        &self,
        reader: &ResolutionBaseReader,
        base_id: &str,
    ) -> Result<(), ResolutionBaseCatalogError> {
        let connection = self.factory.open_reader()?;
        let expected = connection
            .prepare(
                "SELECT version_id FROM resolution_base_versions
                 WHERE base_id=?1 ORDER BY version_id",
            )?
            .query_map([base_id], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let found = reader.source_versions()?;
        if expected != found {
            return Err(ResolutionBaseCatalogError::FileIdentityMismatch {
                detail: format!("source versions {found:?} do not match roots {expected:?}"),
            });
        }
        Ok(())
    }

    fn validate_reader_for_catalog(
        &self,
        reader: &ResolutionBaseReader,
        record: &ResolutionBaseRecord,
    ) -> Result<(), ResolutionBaseCatalogError> {
        validate_reader_for_record(reader, record)?;
        self.validate_reader_versions(reader, &record.base_id)?;
        let store = self.factory.open_reader()?;
        reader.validate_targets_with(|version_id, symbol_id| {
            Ok(store.query_row(
                "SELECT EXISTS(
                   SELECT 1
                   FROM manifests AS manifest
                   JOIN manifest_entries AS entry
                     ON entry.view_id=manifest.view_id
                    AND entry.generation=manifest.generation
                   JOIN symbols AS symbol ON symbol.version_id=entry.version_id
                   WHERE manifest.manifest_hash=?1
                     AND entry.status IN ('indexed','failed_preserved')
                     AND symbol.version_id=?2 AND symbol.symbol_id=?3
                 )",
                params![record.manifest_hash, version_id, symbol_id],
                |row| row.get(0),
            )?)
        })?;
        Ok(())
    }

    fn valid_final_for_build(
        &self,
        build: &ResolutionBaseBuild,
    ) -> Result<bool, ResolutionBaseCatalogError> {
        if !build.final_path.exists() {
            return Ok(false);
        }
        let reader = match ResolutionBaseReader::open(&build.final_path) {
            Ok(reader) => reader,
            Err(_) => return Ok(false),
        };
        if self
            .validate_reader_for_catalog(&reader, &build.record)
            .is_err()
        {
            return Ok(false);
        }
        Ok(true)
    }

    fn base_is_protected(
        &self,
        base_id: &str,
        now: &str,
    ) -> Result<bool, ResolutionBaseCatalogError> {
        let connection = self.factory.open_reader()?;
        Ok(connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM resolution_pins
                 WHERE base_id=?1 AND julianday(expires_at)>julianday(?2)
               )
               OR EXISTS(SELECT 1 FROM resolution_deltas WHERE base_id=?1)",
            params![base_id, now],
            |row| row.get(0),
        )?)
    }

    fn reassign_build_owner(
        &self,
        record: &ResolutionBaseRecord,
        claimant_request_id: &str,
        reset_ready: bool,
        now: &str,
    ) -> Result<ResolutionBaseRecord, ResolutionBaseCatalogError> {
        let mut connection = self.factory.open_writer()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = if reset_ready {
            transaction.execute(
                "UPDATE resolution_bases
                 SET state='building',identifier_count=0,pending_count=0,file_bytes=NULL,
                     file_sha256=NULL,request_id=?1,updated_at=?2
                 WHERE base_id=?3 AND state='ready' AND request_id=?4",
                params![claimant_request_id, now, record.base_id, record.request_id],
            )?
        } else {
            transaction.execute(
                "UPDATE resolution_bases SET request_id=?1,updated_at=?2
                 WHERE base_id=?3 AND state='building' AND request_id=?4",
                params![claimant_request_id, now, record.base_id, record.request_id],
            )?
        };
        if changed != 1 {
            return Err(ResolutionBaseCatalogError::ReadyCasLost {
                base_id: record.base_id.clone(),
            });
        }
        let reassigned = base_record_by_id(&transaction, &record.base_id)?.ok_or_else(|| {
            ResolutionBaseCatalogError::ReadyCasLost {
                base_id: record.base_id.clone(),
            }
        })?;
        transaction.commit()?;
        Ok(reassigned)
    }
}

fn validate_catalog_identity(
    manifest_hash: &str,
    resolver_output_epoch: i64,
    request_id: &str,
) -> Result<(), ResolutionBaseCatalogError> {
    if manifest_hash.len() != 64
        || !manifest_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(ResolutionBaseCatalogError::InvalidArgument("manifest hash"));
    }
    if resolver_output_epoch <= 0 {
        return Err(ResolutionBaseCatalogError::InvalidArgument(
            "resolver output epoch",
        ));
    }
    if request_id.is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(ResolutionBaseCatalogError::InvalidArgument("request id"));
    }
    Ok(())
}

/// Canonical catalog and on-disk base identity shared by import and resolve.
pub fn resolution_base_id(manifest_hash: &str, resolver_output_epoch: i64) -> String {
    format!("base-{manifest_hash}-{resolver_output_epoch}")
}

fn base_id(manifest_hash: &str, resolver_output_epoch: i64) -> String {
    resolution_base_id(manifest_hash, resolver_output_epoch)
}

fn manifest_source_versions(
    transaction: &Transaction<'_>,
    manifest_hash: &str,
) -> Result<Vec<i64>, ResolutionBaseCatalogError> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM manifests WHERE manifest_hash=?1)",
        [manifest_hash],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(ResolutionBaseCatalogError::ManifestNotFound {
            manifest_hash: manifest_hash.to_string(),
        });
    }
    Ok(transaction
        .prepare(
            "SELECT DISTINCT entry.version_id
             FROM manifests AS manifest
             JOIN manifest_entries AS entry
               ON entry.view_id=manifest.view_id AND entry.generation=manifest.generation
             WHERE manifest.manifest_hash=?1 AND entry.version_id IS NOT NULL
               AND entry.status IN ('indexed','failed_preserved')
             ORDER BY entry.version_id",
        )?
        .query_map([manifest_hash], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

fn base_record_by_identity(
    connection: &Connection,
    manifest_hash: &str,
    resolver_output_epoch: i64,
) -> Result<Option<ResolutionBaseRecord>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT base_id,manifest_hash,resolver_output_epoch,state,relative_path,
                    identifier_count,pending_count,file_bytes,file_sha256,request_id,
                    created_at,updated_at
             FROM resolution_bases
             WHERE manifest_hash=?1 AND resolver_output_epoch=?2",
            params![manifest_hash, resolver_output_epoch],
            base_record_from_row,
        )
        .optional()
}

fn base_record_by_id(
    connection: &Connection,
    base_id: &str,
) -> Result<Option<ResolutionBaseRecord>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT base_id,manifest_hash,resolver_output_epoch,state,relative_path,
                    identifier_count,pending_count,file_bytes,file_sha256,request_id,
                    created_at,updated_at
             FROM resolution_bases WHERE base_id=?1",
            [base_id],
            base_record_from_row,
        )
        .optional()
}

fn base_record_from_row(row: &rusqlite::Row<'_>) -> Result<ResolutionBaseRecord, rusqlite::Error> {
    let state = match row.get::<_, String>(3)?.as_str() {
        "building" => ResolutionBaseState::Building,
        "ready" => ResolutionBaseState::Ready,
        value => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                format!("invalid resolution base state {value:?}").into(),
            ));
        }
    };
    Ok(ResolutionBaseRecord {
        base_id: row.get(0)?,
        manifest_hash: row.get(1)?,
        resolver_output_epoch: row.get(2)?,
        state,
        relative_path: row.get(4)?,
        identifier_count: row.get(5)?,
        pending_count: row.get(6)?,
        file_bytes: row.get(7)?,
        file_sha256: row.get(8)?,
        request_id: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn validate_reader_for_record(
    reader: &ResolutionBaseReader,
    record: &ResolutionBaseRecord,
) -> Result<(), ResolutionBaseCatalogError> {
    let identity = reader.file_identity();
    if identity.manifest_hash != record.manifest_hash
        || identity.resolver_output_epoch != record.resolver_output_epoch
    {
        return Err(ResolutionBaseCatalogError::FileIdentityMismatch {
            detail: format!(
                "file ({}, {}) does not match catalog ({}, {})",
                identity.manifest_hash,
                identity.resolver_output_epoch,
                record.manifest_hash,
                record.resolver_output_epoch
            ),
        });
    }
    Ok(())
}

fn validate_ready_file_identity(
    record: &ResolutionBaseRecord,
    identity: &ResolutionFileIdentity,
) -> Result<(), ResolutionBaseCatalogError> {
    if record.identifier_count != i64::try_from(identity.counts.identifiers).unwrap_or(-1)
        || record.pending_count != i64::try_from(identity.counts.pending).unwrap_or(-1)
        || record.file_bytes != i64::try_from(identity.file_bytes).ok()
        || record.file_sha256.as_deref() != Some(identity.file_sha256.as_str())
    {
        return Err(ResolutionBaseCatalogError::FileIdentityMismatch {
            detail: "catalog counts, bytes, or SHA-256 differ from the file".to_string(),
        });
    }
    Ok(())
}

fn sync_directory_path(path: &Path) -> Result<(), ResolutionBaseCatalogError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn remove_resolution_file(path: &Path) -> Result<(), ResolutionBaseCatalogError> {
    for suffix in ["", "-wal", "-shm"] {
        let candidate = if suffix.is_empty() {
            path.to_path_buf()
        } else {
            PathBuf::from(format!("{}{suffix}", path.display()))
        };
        match fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    if let Some(parent) = path.parent() {
        sync_directory_path(parent)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionViewBinding {
    pub view_id: String,
    pub manifest_generation: i64,
    pub manifest_hash: String,
    pub base_id: String,
    pub delta_generation: i64,
    pub state: ViewResolutionState,
    pub exact_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionExactPublish {
    pub view_id: String,
    pub manifest_generation: i64,
    pub manifest_hash: String,
    pub base_id: String,
    pub previous_delta_generation: i64,
    pub resolver_output_epoch: i64,
    pub request_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionConvergenceBegin {
    pub view_id: String,
    pub resolver_output_epoch: i64,
    pub request_id: String,
    pub pin_id: String,
    pub owner_id: String,
    pub expires_at: String,
    pub created_at: String,
}

#[derive(Debug)]
pub enum ResolutionBindingError {
    ViewNotFound { view_id: String },
    ManifestMissing { view_id: String },
    ReadyBaseMissing { resolver_output_epoch: i64 },
    CasLost { view_id: String },
    GenerationOutOfRange { view_id: String },
    ViewUnbound { view_id: String },
    PinNotFound { pin_id: String },
    PinOwnerMismatch { pin_id: String },
    PinExpired { pin_id: String },
    FenceLost { request_id: String },
    InvalidPublication { detail: String },
    Sqlite(rusqlite::Error),
    Connection(StoreConnectionError),
    Catalog(ResolutionBaseCatalogError),
    Log(StoreLogError),
    Scratch(ResolutionValidationError),
}

impl fmt::Display for ResolutionBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ViewNotFound { view_id } => {
                write!(formatter, "resolution view {view_id:?} was not found")
            }
            Self::ManifestMissing { view_id } => {
                write!(formatter, "resolution view {view_id:?} has no manifest")
            }
            Self::ReadyBaseMissing {
                resolver_output_epoch,
            } => write!(
                formatter,
                "no ready resolution base exists for output epoch {resolver_output_epoch}"
            ),
            Self::CasLost { view_id } => write!(
                formatter,
                "resolution binding CAS was lost for view {view_id:?}"
            ),
            Self::GenerationOutOfRange { view_id } => write!(
                formatter,
                "resolution delta generation is out of range for view {view_id:?}"
            ),
            Self::ViewUnbound { view_id } => {
                write!(formatter, "resolution view {view_id:?} is unbound")
            }
            Self::PinNotFound { pin_id } => {
                write!(formatter, "resolution pin {pin_id:?} was not found")
            }
            Self::PinOwnerMismatch { pin_id } => {
                write!(
                    formatter,
                    "resolution pin {pin_id:?} belongs to another owner"
                )
            }
            Self::PinExpired { pin_id } => {
                write!(formatter, "resolution pin {pin_id:?} has expired")
            }
            Self::FenceLost { request_id } => {
                write!(
                    formatter,
                    "resolution publication fence was lost for {request_id:?}"
                )
            }
            Self::InvalidPublication { detail } => {
                write!(formatter, "invalid exact resolution publication: {detail}")
            }
            Self::Sqlite(error) => error.fmt(formatter),
            Self::Connection(error) => error.fmt(formatter),
            Self::Catalog(error) => error.fmt(formatter),
            Self::Log(error) => error.fmt(formatter),
            Self::Scratch(error) => error.fmt(formatter),
        }
    }
}

impl Error for ResolutionBindingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Connection(error) => Some(error),
            Self::Catalog(error) => Some(error),
            Self::Log(error) => Some(error),
            Self::Scratch(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for ResolutionBindingError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<StoreConnectionError> for ResolutionBindingError {
    fn from(error: StoreConnectionError) -> Self {
        Self::Connection(error)
    }
}

impl From<ResolutionBaseCatalogError> for ResolutionBindingError {
    fn from(error: ResolutionBaseCatalogError) -> Self {
        Self::Catalog(error)
    }
}

impl From<StoreLogError> for ResolutionBindingError {
    fn from(error: StoreLogError) -> Self {
        Self::Log(error)
    }
}

impl From<ResolutionValidationError> for ResolutionBindingError {
    fn from(error: ResolutionValidationError) -> Self {
        Self::Scratch(error)
    }
}

#[derive(Debug, Clone)]
pub struct ResolutionBindingStore {
    factory: StoreConnectionFactory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionPublicationFence {
    pub claim_owner: String,
    pub holder_id: String,
    pub holder_pid: u32,
    pub fencing_token: i64,
    pub now_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionPublicationMarker {
    StoreTransactionStart,
    StoreTransactionEnd,
}

pub(crate) fn retire_resolution_scope_chain(
    transaction: &Transaction<'_>,
    view_id: &str,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "DELETE FROM resolution_scope_state WHERE view_id=?1",
        [view_id],
    )?;
    transaction.execute(
        "DELETE FROM resolution_scope_batches WHERE view_id=?1",
        [view_id],
    )?;
    Ok(())
}

impl ResolutionBindingStore {
    pub fn new(factory: StoreConnectionFactory) -> Self {
        Self { factory }
    }

    /// Invalidates a view's resolution binding inside its removal transaction.
    pub fn invalidate_for_view_removal_in_transaction(
        transaction: &Transaction<'_>,
        view_id: &str,
    ) -> Result<(), ResolutionBindingError> {
        retire_resolution_scope_chain(transaction, view_id)?;
        let changed = transaction.execute(
            "UPDATE views SET resolution_state='unbound',resolution_base_id=NULL,
                    resolution_delta_generation=NULL,resolution_exact_at=NULL
             WHERE view_id=?1",
            [view_id],
        )?;
        if changed != 1 {
            return Err(ResolutionBindingError::ViewNotFound {
                view_id: view_id.to_string(),
            });
        }
        Ok(())
    }

    pub fn bind_base(
        &self,
        view_id: &str,
        resolver_output_epoch: i64,
        request_id: &str,
        now: &str,
    ) -> Result<ResolutionViewBinding, ResolutionBindingError> {
        let reader = self.factory.open_reader()?;
        let current = current_view_manifest(&reader, view_id)?;
        let base = select_nearest_ready_base(&reader, view_id, current.0, resolver_output_epoch)?
            .ok_or(ResolutionBindingError::ReadyBaseMissing {
            resolver_output_epoch,
        })?;
        drop(reader);
        ResolutionBaseCatalog::new(self.factory.clone())
            .find_ready(&base.manifest_hash, resolver_output_epoch)?
            .ok_or(ResolutionBindingError::ReadyBaseMissing {
                resolver_output_epoch,
            })?;

        let mut connection = self.factory.open_writer()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let rechecked = current_view_manifest(&transaction, view_id)?;
        if rechecked != current {
            return Err(ResolutionBindingError::CasLost {
                view_id: view_id.to_string(),
            });
        }
        if let Some(binding) =
            current_binding(&transaction, view_id, &current, resolver_output_epoch)?
        {
            transaction.commit()?;
            return Ok(binding);
        }
        let maximum: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(delta_generation),0) FROM resolution_deltas WHERE view_id=?1",
            [view_id],
            |row| row.get(0),
        )?;
        let delta_generation =
            maximum
                .checked_add(1)
                .ok_or_else(|| ResolutionBindingError::GenerationOutOfRange {
                    view_id: view_id.to_string(),
                })?;
        let exact = base.manifest_hash == current.1;
        transaction.execute(
            "INSERT INTO resolution_deltas
             (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
              resolver_output_epoch,identifier_replacements,pending_replacements,
              pending_tombstones,exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,0,0,0,0,0,'{\"files\":[],\"rows\":[]}',?7,?8)",
            params![view_id,delta_generation,base.base_id,current.0,current.1,
                    resolver_output_epoch,request_id,now],
        )?;
        let state = if exact {
            ViewResolutionState::Exact
        } else {
            ViewResolutionState::Converging
        };
        let changed = transaction.execute(
            "UPDATE views SET resolution_state=?1,resolution_base_id=?2,
                    resolution_delta_generation=?3,resolution_exact_at=?4,updated_at=?5
             WHERE view_id=?6 AND current_generation=?7",
            params![
                state.as_str(),
                base.base_id,
                delta_generation,
                exact.then_some(current.0),
                now,
                view_id,
                current.0
            ],
        )?;
        if changed != 1 {
            return Err(ResolutionBindingError::CasLost {
                view_id: view_id.to_string(),
            });
        }
        if exact {
            retire_resolution_scope_chain(&transaction, view_id)?;
        }
        let payload = serde_json::json!({
            "base_id": base.base_id,
            "delta_generation": delta_generation,
            "manifest_generation": current.0,
            "state": state.as_str(),
        })
        .to_string();
        StoreLog::append_effect(
            &transaction,
            &StoreLogEntry::new(request_id, "resolution_bound", payload, now)
                .with_view(view_id)
                .with_generation(u64::try_from(current.0).map_err(|_| {
                    ResolutionBindingError::GenerationOutOfRange {
                        view_id: view_id.to_string(),
                    }
                })?),
        )?;
        transaction.commit()?;
        Ok(ResolutionViewBinding {
            view_id: view_id.to_string(),
            manifest_generation: current.0,
            manifest_hash: current.1,
            base_id: base.base_id,
            delta_generation,
            state,
            exact_at: exact.then_some(current.0),
        })
    }

    pub fn begin_convergence(
        &self,
        request: &ResolutionConvergenceBegin,
    ) -> Result<(ResolutionViewBinding, ResolutionPinRecord), ResolutionBindingError> {
        let binding = self.bind_base(
            &request.view_id,
            request.resolver_output_epoch,
            &request.request_id,
            &request.created_at,
        )?;
        let pin = self.open_pin(
            &request.pin_id,
            ResolutionPinOwnerKind::Resolve,
            &request.owner_id,
            &request.view_id,
            &request.expires_at,
            &request.created_at,
        )?;
        Ok((binding, pin))
    }

    /// Publishes an already-stored exact delta inside the caller's transaction.
    pub fn publish_exact_binding_in_transaction(
        transaction: &Transaction<'_>,
        binding: &ResolutionViewBinding,
        resolver_output_epoch: i64,
        updated_at: &str,
    ) -> Result<(), ResolutionBindingError> {
        if binding.state != ViewResolutionState::Exact
            || binding.exact_at != Some(binding.manifest_generation)
        {
            return Err(ResolutionBindingError::InvalidPublication {
                detail: "exact binding publication must name its manifest generation".to_string(),
            });
        }
        let changed = transaction.execute(
            "UPDATE views SET resolution_state='exact',resolution_base_id=?1,
                    resolution_delta_generation=?2,resolution_exact_at=?3,updated_at=?4
             WHERE view_id=?5 AND current_generation=?3
               AND EXISTS(
                 SELECT 1 FROM manifests
                 WHERE view_id=?5 AND generation=?3 AND manifest_hash=?6
               )
               AND EXISTS(
                 SELECT 1 FROM resolution_deltas
                 WHERE view_id=?5 AND delta_generation=?2 AND base_id=?1
                   AND manifest_generation=?3 AND manifest_hash=?6
                   AND resolver_output_epoch=?7
               )",
            params![
                binding.base_id,
                binding.delta_generation,
                binding.manifest_generation,
                updated_at,
                binding.view_id,
                binding.manifest_hash,
                resolver_output_epoch,
            ],
        )?;
        if changed != 1 {
            return Err(ResolutionBindingError::CasLost {
                view_id: binding.view_id.clone(),
            });
        }
        retire_resolution_scope_chain(transaction, &binding.view_id)?;
        Ok(())
    }

    pub fn publish_exact<H>(
        &self,
        publication: &ResolutionExactPublish,
        fence: &ResolutionPublicationFence,
        scratch: &ResolutionScratchReader,
        gaps: &[ResolutionGapFact],
        window_size: usize,
        heartbeat: H,
    ) -> Result<ResolutionViewBinding, ResolutionBindingError>
    where
        H: FnOnce() -> Result<(), ResolutionBindingError>,
    {
        self.publish_exact_with_markers(
            publication,
            fence,
            scratch,
            gaps,
            window_size,
            heartbeat,
            |_| {},
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn publish_exact_with_markers<H, M>(
        &self,
        publication: &ResolutionExactPublish,
        fence: &ResolutionPublicationFence,
        scratch: &ResolutionScratchReader,
        gaps: &[ResolutionGapFact],
        window_size: usize,
        heartbeat: H,
        mut mark: M,
    ) -> Result<ResolutionViewBinding, ResolutionBindingError>
    where
        H: FnOnce() -> Result<(), ResolutionBindingError>,
        M: FnMut(ResolutionPublicationMarker),
    {
        if window_size == 0 {
            return Err(ResolutionBindingError::InvalidPublication {
                detail: "window size must be positive".to_string(),
            });
        }
        if scratch.file_identity().manifest_hash != publication.manifest_hash
            || scratch.file_identity().resolver_output_epoch != publication.resolver_output_epoch
        {
            return Err(ResolutionBindingError::InvalidPublication {
                detail: "scratch manifest or resolver epoch does not match the publication"
                    .to_string(),
            });
        }
        let counts = scratch.semantic_counts();
        let identifier_replacements = checked_publication_count(
            publication,
            counts.identifier_replacements,
            "identifier replacements",
        )?;
        let pending_replacements = checked_publication_count(
            publication,
            counts.pending_replacements,
            "pending replacements",
        )?;
        let pending_tombstones = checked_publication_count(
            publication,
            counts.pending_tombstones,
            "pending tombstones",
        )?;
        let (gap_json, exact_gap_rows, exact_gap_files) = canonical_gap_payload(publication, gaps)?;

        self.validate_publication_fence(publication, fence)?;

        let mut connection = self
            .factory
            .clone()
            .with_generation_fence(GenerationFence::writer(
                self.factory.layout(),
                &fence.holder_id,
                fence.holder_pid,
                fence.fencing_token,
                fence.now_ms,
            ))
            .open_writer()?;
        mark(ResolutionPublicationMarker::StoreTransactionStart);
        // Mandatory pre-BEGIN heartbeat: renew full lease TTL before IMMEDIATE work.
        // Never heartbeat mid-transaction.
        heartbeat()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = transaction
            .query_row(
                "SELECT view.current_generation,manifest.manifest_hash,
                        view.resolution_base_id,view.resolution_delta_generation,
                        view.resolution_state
                 FROM views AS view
                 LEFT JOIN manifests AS manifest
                   ON manifest.view_id=view.view_id AND manifest.generation=view.current_generation
                 WHERE view.view_id=?1",
                [&publication.view_id],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| ResolutionBindingError::ViewNotFound {
                view_id: publication.view_id.clone(),
            })?;
        if current
            != (
                Some(publication.manifest_generation),
                Some(publication.manifest_hash.clone()),
                Some(publication.base_id.clone()),
                Some(publication.previous_delta_generation),
                "converging".to_string(),
            )
        {
            return Err(ResolutionBindingError::CasLost {
                view_id: publication.view_id.clone(),
            });
        }
        let maximum: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(delta_generation),0) FROM resolution_deltas WHERE view_id=?1",
            [&publication.view_id],
            |row| row.get(0),
        )?;
        let delta_generation =
            maximum
                .checked_add(1)
                .ok_or_else(|| ResolutionBindingError::GenerationOutOfRange {
                    view_id: publication.view_id.clone(),
                })?;
        transaction.execute(
            "INSERT INTO resolution_deltas
             (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
              resolver_output_epoch,identifier_replacements,pending_replacements,
              pending_tombstones,exact_gap_rows,exact_gap_files,exact_gap_json,request_id,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![
                publication.view_id,
                delta_generation,
                publication.base_id,
                publication.manifest_generation,
                publication.manifest_hash,
                publication.resolver_output_epoch,
                identifier_replacements,
                pending_replacements,
                pending_tombstones,
                exact_gap_rows,
                exact_gap_files,
                gap_json,
                publication.request_id,
                publication.created_at
            ],
        )?;
        copy_identifier_delta_rows(
            &transaction,
            &publication.view_id,
            delta_generation,
            scratch,
            window_size,
        )?;
        copy_pending_delta_rows(
            &transaction,
            &publication.view_id,
            delta_generation,
            scratch,
            window_size,
        )?;
        validate_published_delta_visibility(
            &transaction,
            &publication.view_id,
            publication.manifest_generation,
            delta_generation,
        )?;
        // Wall-clock revalidation before view CAS. Failure rolls back the IMMEDIATE txn.
        self.validate_publication_fence(publication, fence)?;
        let changed = transaction.execute(
            "UPDATE views SET resolution_state='exact',resolution_delta_generation=?1,
                    resolution_exact_at=?2,updated_at=?3
             WHERE view_id=?4 AND current_generation=?2
               AND resolution_state='converging' AND resolution_base_id=?5
               AND resolution_delta_generation=?6
               AND EXISTS(
                 SELECT 1 FROM manifests
                 WHERE view_id=?4 AND generation=?2 AND manifest_hash=?7
               )",
            params![
                delta_generation,
                publication.manifest_generation,
                publication.created_at,
                publication.view_id,
                publication.base_id,
                publication.previous_delta_generation,
                publication.manifest_hash,
            ],
        )?;
        if changed != 1 {
            return Err(ResolutionBindingError::CasLost {
                view_id: publication.view_id.clone(),
            });
        }
        retire_resolution_scope_chain(&transaction, &publication.view_id)?;
        let payload = serde_json::json!({
            "base_id": publication.base_id,
            "delta_generation": delta_generation,
            "exact_gap_files": exact_gap_files,
            "exact_gap_rows": exact_gap_rows,
            "manifest_generation": publication.manifest_generation,
            "manifest_hash": publication.manifest_hash,
        })
        .to_string();
        StoreLog::append_effect(
            &transaction,
            &StoreLogEntry::new(
                &publication.request_id,
                "resolution_exact_published",
                payload,
                &publication.created_at,
            )
            .with_view(&publication.view_id)
            .with_generation(u64::try_from(publication.manifest_generation).map_err(|_| {
                ResolutionBindingError::GenerationOutOfRange {
                    view_id: publication.view_id.clone(),
                }
            })?),
        )?;
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("resolution_exact_before_store_commit");
        transaction.commit()?;
        mark(ResolutionPublicationMarker::StoreTransactionEnd);
        Ok(ResolutionViewBinding {
            view_id: publication.view_id.clone(),
            manifest_generation: publication.manifest_generation,
            manifest_hash: publication.manifest_hash.clone(),
            base_id: publication.base_id.clone(),
            delta_generation,
            state: ViewResolutionState::Exact,
            exact_at: Some(publication.manifest_generation),
        })
    }

    fn validate_publication_fence(
        &self,
        publication: &ResolutionExactPublish,
        fence: &ResolutionPublicationFence,
    ) -> Result<(), ResolutionBindingError> {
        if fence.claim_owner != fence.holder_id {
            return Err(ResolutionBindingError::FenceLost {
                request_id: publication.request_id.clone(),
            });
        }
        let connection = Connection::open_with_flags(
            self.factory.layout().coordinator_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.execute_batch("PRAGMA query_only = ON;")?;
        // Compare lease expiry against current wall clock, not fence.now_ms alone.
        let valid: i64 = connection.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM requests AS request
               JOIN writer_lease AS lease ON lease.resource='store-writer'
               WHERE request.request_id=?1 AND request.kind='resolve'
                 AND request.state='claimed' AND request.claim_owner=?2
                 AND lease.holder_id=?3 AND lease.holder_pid=?4
                 AND lease.fencing_token=?5 AND lease.expires_at>?6
             )",
            params![
                publication.request_id,
                fence.claim_owner,
                fence.holder_id,
                i64::from(fence.holder_pid),
                fence.fencing_token,
                super::connection::system_now_ms(),
            ],
            |row| row.get(0),
        )?;
        if valid != 1 {
            return Err(ResolutionBindingError::FenceLost {
                request_id: publication.request_id.clone(),
            });
        }
        Ok(())
    }

    pub fn open_pin(
        &self,
        pin_id: &str,
        owner_kind: ResolutionPinOwnerKind,
        owner_id: &str,
        view_id: &str,
        expires_at: &str,
        now: &str,
    ) -> Result<ResolutionPinRecord, ResolutionBindingError> {
        let mut connection = self.factory.open_writer()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !timestamp_is_later(&transaction, expires_at, now)? {
            return Err(ResolutionBindingError::InvalidPublication {
                detail: "pin expiry must be later than its creation time".to_string(),
            });
        }
        let binding = transaction
            .query_row(
                "SELECT current_generation,resolution_base_id,resolution_delta_generation,
                        resolution_state,resolution_exact_at
                 FROM views WHERE view_id=?1",
                [view_id],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| ResolutionBindingError::ViewNotFound {
                view_id: view_id.to_string(),
            })?;
        let (Some(manifest_generation), Some(base_id), Some(delta_generation), state, exact_at) =
            binding
        else {
            return Err(ResolutionBindingError::ViewUnbound {
                view_id: view_id.to_string(),
            });
        };
        let pinned_delta =
            (state == "exact" && exact_at == Some(manifest_generation)).then_some(delta_generation);
        transaction.execute(
            "INSERT INTO resolution_pins
             (pin_id,owner_kind,owner_id,view_id,manifest_generation,base_id,
              delta_generation,expires_at,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                pin_id,
                owner_kind.as_str(),
                owner_id,
                view_id,
                manifest_generation,
                base_id,
                pinned_delta,
                expires_at,
                now
            ],
        )?;
        let record = pin_record_by_id(&transaction, pin_id)?.ok_or_else(|| {
            ResolutionBindingError::PinNotFound {
                pin_id: pin_id.to_string(),
            }
        })?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn renew_pin(
        &self,
        pin_id: &str,
        owner_kind: ResolutionPinOwnerKind,
        owner_id: &str,
        expires_at: &str,
        now: &str,
    ) -> Result<ResolutionPinRecord, ResolutionBindingError> {
        let mut connection = self.factory.open_writer()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if !timestamp_is_later(&transaction, expires_at, now)? {
            return Err(ResolutionBindingError::InvalidPublication {
                detail: "renewed pin expiry must be in the future".to_string(),
            });
        }
        let changed = transaction.execute(
            "UPDATE resolution_pins SET expires_at=?1
             WHERE pin_id=?2 AND owner_kind=?3 AND owner_id=?4
               AND julianday(expires_at)>julianday(?5)",
            params![expires_at, pin_id, owner_kind.as_str(), owner_id, now],
        )?;
        if changed != 1 {
            return Err(match pin_record_by_id(&transaction, pin_id)? {
                Some(record) if record.owner_kind != owner_kind || record.owner_id != owner_id => {
                    ResolutionBindingError::PinOwnerMismatch {
                        pin_id: pin_id.to_string(),
                    }
                }
                Some(_) => ResolutionBindingError::PinExpired {
                    pin_id: pin_id.to_string(),
                },
                None => ResolutionBindingError::PinNotFound {
                    pin_id: pin_id.to_string(),
                },
            });
        }
        let record = pin_record_by_id(&transaction, pin_id)?.ok_or_else(|| {
            ResolutionBindingError::PinNotFound {
                pin_id: pin_id.to_string(),
            }
        })?;
        transaction.commit()?;
        Ok(record)
    }

    pub fn release_pin(
        &self,
        pin_id: &str,
        owner_kind: ResolutionPinOwnerKind,
        owner_id: &str,
    ) -> Result<bool, ResolutionBindingError> {
        let mut connection = self.factory.open_writer()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute(
            "DELETE FROM resolution_pins WHERE pin_id=?1 AND owner_kind=?2 AND owner_id=?3",
            params![pin_id, owner_kind.as_str(), owner_id],
        )?;
        if changed == 0 && pin_record_by_id(&transaction, pin_id)?.is_some() {
            return Err(ResolutionBindingError::PinOwnerMismatch {
                pin_id: pin_id.to_string(),
            });
        }
        transaction.commit()?;
        Ok(changed == 1)
    }

    pub fn pinned_delta(
        &self,
        pin_id: &str,
        now: &str,
    ) -> Result<Option<ResolutionDeltaRecord>, ResolutionBindingError> {
        let connection = self.factory.open_reader()?;
        Ok(connection
            .query_row(
                "SELECT delta.view_id,delta.delta_generation,delta.base_id,
                        delta.manifest_generation,delta.manifest_hash,delta.resolver_output_epoch,
                        delta.identifier_replacements,delta.pending_replacements,
                        delta.pending_tombstones,delta.exact_gap_rows,delta.exact_gap_files,
                        delta.exact_gap_json,delta.request_id,delta.created_at
                 FROM resolution_pins AS pin
                 JOIN resolution_deltas AS delta
                   ON delta.view_id=pin.view_id AND delta.delta_generation=pin.delta_generation
                  AND delta.base_id=pin.base_id AND delta.manifest_generation=pin.manifest_generation
                 WHERE pin.pin_id=?1 AND julianday(pin.expires_at)>julianday(?2)",
                params![pin_id, now],
                delta_record_from_row,
            )
            .optional()?)
    }

    pub fn pinned_identifier_delta_window(
        &self,
        pin_id: &str,
        now: &str,
        after: Option<(i64, &str)>,
        limit: usize,
    ) -> Result<Vec<ResolutionIdentifierDeltaRecord>, ResolutionBindingError> {
        let limit = checked_window_limit(limit)?;
        let (version_id, identifier_id) = after.unwrap_or((0, ""));
        let connection = self.factory.open_reader()?;
        let mut statement = connection.prepare(
            "SELECT row.view_id,row.delta_generation,row.version_id,row.identifier_id,
                    row.target_version_id,row.target_symbol_id,row.tier,row.confidence,
                    row.method,row.outcome,row.candidates
             FROM resolution_pins AS pin
             JOIN resolution_deltas AS delta
               ON delta.view_id=pin.view_id AND delta.delta_generation=pin.delta_generation
              AND delta.base_id=pin.base_id AND delta.manifest_generation=pin.manifest_generation
             JOIN resolution_identifier_deltas AS row
               ON row.view_id=delta.view_id AND row.delta_generation=delta.delta_generation
             WHERE pin.pin_id=?1 AND julianday(pin.expires_at)>julianday(?2)
               AND (row.version_id,row.identifier_id)>(?3,?4)
             ORDER BY row.version_id,row.identifier_id LIMIT ?5",
        )?;
        Ok(statement
            .query_map(
                params![pin_id, now, version_id, identifier_id, limit],
                |row| {
                    Ok(ResolutionIdentifierDeltaRecord {
                        view_id: row.get(0)?,
                        delta_generation: row.get(1)?,
                        version_id: row.get(2)?,
                        identifier_id: row.get(3)?,
                        target_version_id: row.get(4)?,
                        target_symbol_id: row.get(5)?,
                        tier: row.get(6)?,
                        confidence: row.get(7)?,
                        method: row.get(8)?,
                        outcome: row.get(9)?,
                        candidates: row.get(10)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn pinned_pending_delta_window(
        &self,
        pin_id: &str,
        now: &str,
        after: Option<(i64, &str)>,
        limit: usize,
    ) -> Result<Vec<ResolutionPendingDeltaRecord>, ResolutionBindingError> {
        let limit = checked_window_limit(limit)?;
        let (version_id, pending_id) = after.unwrap_or((0, ""));
        let connection = self.factory.open_reader()?;
        let mut statement = connection.prepare(
            "SELECT row.view_id,row.delta_generation,row.version_id,
                    row.pending_relationship_id,row.operation,row.target_version_id,
                    row.target_symbol_id,row.tier,row.confidence,row.method
             FROM resolution_pins AS pin
             JOIN resolution_deltas AS delta
               ON delta.view_id=pin.view_id AND delta.delta_generation=pin.delta_generation
              AND delta.base_id=pin.base_id AND delta.manifest_generation=pin.manifest_generation
             JOIN resolution_pending_deltas AS row
               ON row.view_id=delta.view_id AND row.delta_generation=delta.delta_generation
             WHERE pin.pin_id=?1 AND julianday(pin.expires_at)>julianday(?2)
               AND (row.version_id,row.pending_relationship_id)>(?3,?4)
             ORDER BY row.version_id,row.pending_relationship_id LIMIT ?5",
        )?;
        Ok(statement
            .query_map(params![pin_id, now, version_id, pending_id, limit], |row| {
                let operation = match row.get::<_, String>(4)?.as_str() {
                    "replace" => ResolutionPendingOperation::Replace,
                    "tombstone" => ResolutionPendingOperation::Tombstone,
                    value => {
                        return Err(rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            format!("invalid pending delta operation {value:?}").into(),
                        ));
                    }
                };
                Ok(ResolutionPendingDeltaRecord {
                    view_id: row.get(0)?,
                    delta_generation: row.get(1)?,
                    version_id: row.get(2)?,
                    pending_relationship_id: row.get(3)?,
                    operation,
                    target_version_id: row.get(5)?,
                    target_symbol_id: row.get(6)?,
                    tier: row.get(7)?,
                    confidence: row.get(8)?,
                    method: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn cleanup_superseded_deltas(
        &self,
        view_id: &str,
        now: &str,
    ) -> Result<usize, ResolutionBindingError> {
        let mut connection = self.factory.open_writer()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM resolution_pins WHERE julianday(expires_at)<=julianday(?1)",
            [now],
        )?;
        let removed = transaction.execute(
            "DELETE FROM resolution_deltas AS delta
             WHERE delta.view_id=?1
               AND delta.delta_generation <> COALESCE(
                 (SELECT resolution_delta_generation FROM views WHERE view_id=?1),-1)
               AND NOT EXISTS(
                 SELECT 1 FROM resolution_pins AS pin
                 WHERE pin.view_id=delta.view_id
                   AND pin.delta_generation=delta.delta_generation
               )
               AND NOT EXISTS(
                 SELECT 1 FROM resolution_scope_state AS scope
                 WHERE scope.view_id=delta.view_id
                   AND scope.delta_generation=delta.delta_generation
               )",
            [view_id],
        )?;
        transaction.commit()?;
        Ok(removed)
    }
}

fn checked_publication_count(
    publication: &ResolutionExactPublish,
    count: u64,
    label: &str,
) -> Result<i64, ResolutionBindingError> {
    i64::try_from(count).map_err(|_| ResolutionBindingError::InvalidPublication {
        detail: format!(
            "{} {label} exceed the store count range",
            publication.view_id
        ),
    })
}

fn timestamp_is_later(
    connection: &Connection,
    later: &str,
    earlier: &str,
) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT julianday(?1) IS NOT NULL AND julianday(?2) IS NOT NULL
                AND julianday(?1)>julianday(?2)",
        params![later, earlier],
        |row| row.get(0),
    )
}

fn checked_window_limit(limit: usize) -> Result<i64, ResolutionBindingError> {
    if limit == 0 {
        return Err(ResolutionBindingError::InvalidPublication {
            detail: "delta read window size must be positive".to_string(),
        });
    }
    i64::try_from(limit).map_err(|_| ResolutionBindingError::InvalidPublication {
        detail: "delta read window size exceeds the SQLite range".to_string(),
    })
}

fn canonical_gap_payload(
    publication: &ResolutionExactPublish,
    gaps: &[ResolutionGapFact],
) -> Result<(String, i64, i64), ResolutionBindingError> {
    let mut rows = gaps
        .iter()
        .map(|gap| {
            (
                gap.version_id,
                gap.local_id.as_str(),
                match gap.table {
                    ResolutionGapTable::Identifier => "identifier",
                    ResolutionGapTable::Pending => "pending",
                },
                match gap.kind {
                    ResolutionGapKind::Added => "added",
                    ResolutionGapKind::Replaced => "replaced",
                    ResolutionGapKind::Removed => "removed",
                },
            )
        })
        .collect::<Vec<_>>();
    rows.sort_unstable();
    let files = rows.iter().map(|row| row.0).collect::<BTreeSet<_>>();
    let payload = serde_json::json!({
        "files": files,
        "rows": rows.into_iter().map(|(version_id,local_id,table,kind)| {
            serde_json::json!({
                "kind": kind,
                "local_id": local_id,
                "table": table,
                "version_id": version_id,
            })
        }).collect::<Vec<_>>(),
    })
    .to_string();
    Ok((
        payload,
        checked_publication_count(publication, gaps.len() as u64, "gap rows")?,
        checked_publication_count(publication, files.len() as u64, "gap files")?,
    ))
}

fn validate_published_delta_visibility(
    transaction: &Transaction<'_>,
    view_id: &str,
    manifest_generation: i64,
    delta_generation: i64,
) -> Result<(), ResolutionBindingError> {
    let invalid = transaction
        .query_row(
            "WITH rows(source_version_id,target_version_id,target_symbol_id) AS (
               SELECT version_id,target_version_id,target_symbol_id
               FROM resolution_identifier_deltas
               WHERE view_id=?1 AND delta_generation=?3
               UNION ALL
               SELECT version_id,target_version_id,target_symbol_id
               FROM resolution_pending_deltas
               WHERE view_id=?1 AND delta_generation=?3 AND operation='replace'
             )
             SELECT source_version_id,target_version_id,target_symbol_id
             FROM rows
             WHERE NOT EXISTS(
               SELECT 1 FROM manifest_entries AS source
               WHERE source.view_id=?1 AND source.generation=?2
                 AND source.version_id=rows.source_version_id
                 AND source.status IN ('indexed','failed_preserved')
             ) OR (
               rows.target_version_id IS NOT NULL AND NOT EXISTS(
                 SELECT 1 FROM manifest_entries AS target
                 JOIN symbols AS symbol ON symbol.version_id=target.version_id
                 WHERE target.view_id=?1 AND target.generation=?2
                   AND target.version_id=rows.target_version_id
                   AND target.status IN ('indexed','failed_preserved')
                   AND symbol.symbol_id=rows.target_symbol_id
               )
             )
             LIMIT 1",
            params![view_id, manifest_generation, delta_generation],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    match invalid {
        None => Ok(()),
        Some((source_version_id, target_version_id, target_symbol_id)) => {
            Err(ResolutionBindingError::InvalidPublication {
                detail: format!(
                    "delta row ({source_version_id},{target_version_id:?},{target_symbol_id:?}) is not manifest-visible"
                ),
            })
        }
    }
}

fn copy_identifier_delta_rows(
    transaction: &Transaction<'_>,
    view_id: &str,
    delta_generation: i64,
    scratch: &ResolutionScratchReader,
    window_size: usize,
) -> Result<(), ResolutionBindingError> {
    let mut after: Option<(i64, String)> = None;
    loop {
        let rows = scratch.identifier_replacement_window(
            after
                .as_ref()
                .map(|(version_id, local_id)| (*version_id, local_id.as_str())),
            window_size,
        )?;
        if rows.is_empty() {
            return Ok(());
        }
        for row in &rows {
            transaction.execute(
                "INSERT INTO resolution_identifier_deltas
                 (view_id,delta_generation,version_id,identifier_id,target_version_id,
                  target_symbol_id,tier,confidence,method,outcome,candidates)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    view_id,
                    delta_generation,
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
        }
        let last = rows.last().expect("nonempty resolution identifier page");
        after = Some((last.version_id, last.identifier_id.clone()));
    }
}

fn copy_pending_delta_rows(
    transaction: &Transaction<'_>,
    view_id: &str,
    delta_generation: i64,
    scratch: &ResolutionScratchReader,
    window_size: usize,
) -> Result<(), ResolutionBindingError> {
    let mut replacement_after: Option<(i64, String)> = None;
    loop {
        let rows = scratch.pending_replacement_window(
            replacement_after
                .as_ref()
                .map(|(version_id, local_id)| (*version_id, local_id.as_str())),
            window_size,
        )?;
        if rows.is_empty() {
            break;
        }
        for row in &rows {
            transaction.execute(
                "INSERT INTO resolution_pending_deltas
                 (view_id,delta_generation,version_id,pending_relationship_id,operation,
                  target_version_id,target_symbol_id,tier,confidence,method)
                 VALUES (?1,?2,?3,?4,'replace',?5,?6,?7,?8,?9)",
                params![
                    view_id,
                    delta_generation,
                    row.version_id,
                    row.pending_relationship_id,
                    row.target_version_id,
                    row.target_symbol_id,
                    row.tier,
                    row.confidence,
                    row.method,
                ],
            )?;
        }
        let last = rows.last().expect("nonempty resolution pending page");
        replacement_after = Some((last.version_id, last.pending_relationship_id.clone()));
    }
    let mut tombstone_after: Option<(i64, String)> = None;
    loop {
        let rows = scratch.pending_tombstone_window(
            tombstone_after
                .as_ref()
                .map(|(version_id, local_id)| (*version_id, local_id.as_str())),
            window_size,
        )?;
        if rows.is_empty() {
            return Ok(());
        }
        for row in &rows {
            transaction.execute(
                "INSERT INTO resolution_pending_deltas
                 (view_id,delta_generation,version_id,pending_relationship_id,operation,
                  target_version_id,target_symbol_id,tier,confidence,method)
                 VALUES (?1,?2,?3,?4,'tombstone',NULL,NULL,NULL,NULL,NULL)",
                params![
                    view_id,
                    delta_generation,
                    row.version_id,
                    row.pending_relationship_id,
                ],
            )?;
        }
        let last = rows.last().expect("nonempty resolution tombstone page");
        tombstone_after = Some((last.version_id, last.pending_relationship_id.clone()));
    }
}

fn pin_record_by_id(
    connection: &Connection,
    pin_id: &str,
) -> Result<Option<ResolutionPinRecord>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT pin_id,owner_kind,owner_id,view_id,manifest_generation,base_id,
                    delta_generation,expires_at,created_at
             FROM resolution_pins WHERE pin_id=?1",
            [pin_id],
            |row| {
                let owner_kind = match row.get::<_, String>(1)?.as_str() {
                    "reader" => ResolutionPinOwnerKind::Reader,
                    "resolve" => ResolutionPinOwnerKind::Resolve,
                    value => {
                        return Err(rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Text,
                            format!("invalid resolution pin owner {value:?}").into(),
                        ));
                    }
                };
                Ok(ResolutionPinRecord {
                    pin_id: row.get(0)?,
                    owner_kind,
                    owner_id: row.get(2)?,
                    view_id: row.get(3)?,
                    manifest_generation: row.get(4)?,
                    base_id: row.get(5)?,
                    delta_generation: row.get(6)?,
                    expires_at: row.get(7)?,
                    created_at: row.get(8)?,
                })
            },
        )
        .optional()
}

fn delta_record_from_row(
    row: &rusqlite::Row<'_>,
) -> Result<ResolutionDeltaRecord, rusqlite::Error> {
    Ok(ResolutionDeltaRecord {
        view_id: row.get(0)?,
        delta_generation: row.get(1)?,
        base_id: row.get(2)?,
        manifest_generation: row.get(3)?,
        manifest_hash: row.get(4)?,
        resolver_output_epoch: row.get(5)?,
        identifier_replacements: row.get(6)?,
        pending_replacements: row.get(7)?,
        pending_tombstones: row.get(8)?,
        exact_gap_rows: row.get(9)?,
        exact_gap_files: row.get(10)?,
        exact_gap_json: row.get(11)?,
        request_id: row.get(12)?,
        created_at: row.get(13)?,
    })
}

fn current_view_manifest(
    connection: &Connection,
    view_id: &str,
) -> Result<(i64, String), ResolutionBindingError> {
    let row = connection
        .query_row(
            "SELECT view.current_generation,manifest.manifest_hash
         FROM views AS view LEFT JOIN manifests AS manifest
           ON manifest.view_id=view.view_id AND manifest.generation=view.current_generation
         WHERE view.view_id=?1",
            [view_id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?;
    match row {
        None => Err(ResolutionBindingError::ViewNotFound {
            view_id: view_id.to_string(),
        }),
        Some((Some(generation), Some(hash))) => Ok((generation, hash)),
        Some(_) => Err(ResolutionBindingError::ManifestMissing {
            view_id: view_id.to_string(),
        }),
    }
}

fn select_nearest_ready_base(
    connection: &Connection,
    view_id: &str,
    generation: i64,
    epoch: i64,
) -> Result<Option<ResolutionBaseRecord>, rusqlite::Error> {
    connection.query_row(
        "SELECT base.base_id,base.manifest_hash,base.resolver_output_epoch,base.state,
                base.relative_path,base.identifier_count,base.pending_count,base.file_bytes,
                base.file_sha256,base.request_id,base.created_at,base.updated_at
         FROM resolution_bases AS base
         LEFT JOIN resolution_base_versions AS root ON root.base_id=base.base_id
         LEFT JOIN manifest_entries AS entry
           ON entry.view_id=?1 AND entry.generation=?2 AND entry.version_id=root.version_id
         WHERE base.state='ready' AND base.resolver_output_epoch=?3
         GROUP BY base.base_id
         ORDER BY (base.manifest_hash=(SELECT manifest_hash FROM manifests WHERE view_id=?1 AND generation=?2)) DESC,
                  COUNT(entry.version_id) DESC,base.base_id
         LIMIT 1",
        params![view_id,generation,epoch],
        base_record_from_row,
    ).optional()
}

fn current_binding(
    connection: &Connection,
    view_id: &str,
    manifest: &(i64, String),
    resolver_output_epoch: i64,
) -> Result<Option<ResolutionViewBinding>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT view.resolution_base_id,view.resolution_delta_generation,
                view.resolution_state,view.resolution_exact_at
         FROM views AS view
         JOIN resolution_deltas AS delta
           ON delta.view_id=view.view_id AND delta.delta_generation=view.resolution_delta_generation
         WHERE view.view_id=?1 AND delta.manifest_generation=?2 AND delta.manifest_hash=?3
           AND delta.resolver_output_epoch=?4",
            params![view_id, manifest.0, manifest.1, resolver_output_epoch],
            |row| {
                let state = match row.get::<_, String>(2)?.as_str() {
                    "converging" => ViewResolutionState::Converging,
                    "exact" => ViewResolutionState::Exact,
                    value => {
                        return Err(rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Text,
                            format!("invalid resolution binding state {value:?}").into(),
                        ));
                    }
                };
                Ok(ResolutionViewBinding {
                    view_id: view_id.to_string(),
                    manifest_generation: manifest.0,
                    manifest_hash: manifest.1.clone(),
                    base_id: row.get(0)?,
                    delta_generation: row.get(1)?,
                    state,
                    exact_at: row.get(3)?,
                })
            },
        )
        .optional()
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
    ResolverOutputEpochMismatch {
        expected: i64,
        found: i64,
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
            Self::ResolverOutputEpochMismatch { expected, found } => write!(
                formatter,
                "resolution output epoch {found} does not match {expected}"
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
                 UNION
                 SELECT target_version_id,target_symbol_id FROM pending_resolutions
                 ORDER BY target_version_id,target_symbol_id COLLATE BINARY",
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
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("resolution_base_after_scratch_close");
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

    pub fn validate_targets_with<F>(
        &self,
        mut target_exists: F,
    ) -> Result<(), ResolutionValidationError>
    where
        F: FnMut(i64, &str) -> Result<bool, ResolutionValidationError>,
    {
        let mut statement = self.connection.prepare(
            "SELECT target_version_id,target_symbol_id
             FROM identifier_resolutions
             WHERE target_version_id IS NOT NULL
             UNION
             SELECT target_version_id,target_symbol_id FROM pending_resolutions
             ORDER BY target_version_id,target_symbol_id COLLATE BINARY",
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
             WHERE (version_id,identifier_id)>(?1,?2)
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
             WHERE (version_id,pending_relationship_id)>(?1,?2)
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

pub(crate) fn resolution_file_bytes(path: &Path) -> Result<u64, io::Error> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "resolution catalog is not a regular file: {}",
                path.display()
            ),
        ));
    }
    Ok(metadata.len())
}

pub(crate) fn resolution_file_sha256(path: &Path) -> Result<String, io::Error> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "resolution catalog is not a regular file: {}",
                path.display()
            ),
        ));
    }
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(crate) fn retire_resolution_delta(
    transaction: &Transaction<'_>,
    view_id: &str,
    generation: i64,
) -> Result<usize, rusqlite::Error> {
    transaction.execute(
        "DELETE FROM resolution_deltas
         WHERE view_id=?1 AND delta_generation=?2
           AND NOT EXISTS(SELECT 1 FROM views
                          WHERE views.view_id=resolution_deltas.view_id
                            AND views.resolution_delta_generation=resolution_deltas.delta_generation)
           AND NOT EXISTS(SELECT 1 FROM resolution_scope_state
                          WHERE resolution_scope_state.view_id=resolution_deltas.view_id
                            AND resolution_scope_state.delta_generation=
                                resolution_deltas.delta_generation)
           AND NOT EXISTS(SELECT 1 FROM resolution_pins
                          WHERE resolution_pins.view_id=resolution_deltas.view_id
                            AND resolution_pins.delta_generation=resolution_deltas.delta_generation)",
        params![view_id, generation],
    )
}

pub(crate) fn retire_resolution_base(
    transaction: &Transaction<'_>,
    base_id: &str,
) -> Result<usize, rusqlite::Error> {
    transaction.execute(
        "DELETE FROM resolution_bases
         WHERE base_id=?1
           AND NOT EXISTS(SELECT 1 FROM views WHERE resolution_base_id=?1)
           AND NOT EXISTS(SELECT 1 FROM resolution_scope_state WHERE base_id=?1)
           AND NOT EXISTS(SELECT 1 FROM resolution_pins WHERE base_id=?1)
           AND NOT EXISTS(SELECT 1 FROM resolution_deltas WHERE base_id=?1)",
        [base_id],
    )
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
