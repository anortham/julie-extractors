use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

use julie_extract_artifact::resolution_store::{
    IdentifierWorkItem, PendingWorkItem, ResolutionCounts, ResolutionReportRow,
};
use julie_extract_artifact::store::{
    ResolutionBaseWriter, ResolutionFileIdentity, ResolutionIdentifierRow, ResolutionPendingRow,
    ResolutionValidationError, create_resolution_scratch_connection,
};
use julie_extract_artifact::store::{StoreConnectionError, StoreConnectionFactory};
use julie_extractors::SymbolKind;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::resolution::{
    self, CandidateEvidence, CandidateHit, CandidateLookup, CandidateSummary, CandidateSymbol,
    EdgeOrigin, ImportRecord, ReferenceKind, TierOutcome, TypeFact, UnresolvedEdge,
};
use crate::resolution_session::{
    ResolutionCorpusIdentity, ResolutionPassRequest, ResolutionPhase, ResolutionPhaseChunk,
    ResolutionSession, ResolutionWorklists, ResolutionWrite, ResolutionWriteBatch,
    SemanticIdentifierId,
};
use crate::resolution_session::{SemanticSymbolId, SemanticVersionId};

const MAX_STORE_RESOLUTION_WINDOW: usize = 300;
type CandidatePage = (Vec<CandidateHit>, Option<(i64, String)>);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ResolutionLookupKey {
    origin: EdgeOrigin,
    kind: ReferenceKind,
    language: String,
    file_id: String,
    terminal_name: String,
    receiver: Option<String>,
    caller_scope_symbol_id: Option<String>,
    import_context: Option<String>,
    receiver_qualifier: Option<String>,
    source_confidence_bits: u64,
}

impl From<&UnresolvedEdge> for ResolutionLookupKey {
    fn from(edge: &UnresolvedEdge) -> Self {
        Self {
            origin: edge.origin,
            kind: edge.kind,
            language: edge.language.clone(),
            file_id: edge.file_id.clone(),
            terminal_name: edge.terminal_name.clone(),
            receiver: edge.receiver.clone(),
            caller_scope_symbol_id: edge.caller_scope_symbol_id.clone(),
            import_context: edge.import_context.clone(),
            receiver_qualifier: edge.receiver_qualifier.clone(),
            source_confidence_bits: edge.source_confidence.to_bits(),
        }
    }
}

#[cfg(feature = "test-store-resolution-contract")]
pub type ResolutionScratchPragmaValues = (i64, i64, String, i64, i64, i64, i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreManifestIdentity {
    pub family_id: String,
    pub view_id: String,
    pub generation: i64,
    pub manifest_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreManifestEntry {
    pub path: String,
    pub language: Option<String>,
    pub status: String,
    pub version_id: Option<i64>,
}

#[derive(Debug)]
pub enum StoreResolutionError {
    Sqlite(rusqlite::Error),
    Connection(StoreConnectionError),
    Artifact(ResolutionValidationError),
    InvalidIdentity,
    InvalidWindowSize {
        requested: usize,
        maximum: usize,
    },
    PhaseHydrationMismatch {
        phase: &'static str,
        expected: usize,
        actual: usize,
    },
    InputIncomplete {
        path: String,
        version_id: i64,
    },
    UnexpectedOutputPath(PathBuf),
}

impl StoreResolutionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Sqlite(_) => "resolution_sqlite_error",
            Self::Connection(_) => "resolution_store_connection_error",
            Self::Artifact(_) => "resolution_artifact_error",
            Self::InvalidIdentity => "resolution_identity_invalid",
            Self::InvalidWindowSize { .. } => "resolution_window_invalid",
            Self::PhaseHydrationMismatch { .. } => "resolution_phase_corrupt",
            Self::InputIncomplete { .. } => "resolution_input_incomplete",
            Self::UnexpectedOutputPath(_) => "resolution_output_exists",
        }
    }
}

impl fmt::Display for StoreResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "store resolution SQLite error: {error}"),
            Self::Connection(error) => {
                write!(formatter, "store resolution connection error: {error}")
            }
            Self::Artifact(error) => write!(formatter, "resolution artifact error: {error}"),
            Self::InvalidIdentity => write!(formatter, "invalid store resolution identity"),
            Self::InvalidWindowSize { requested, maximum } => write!(
                formatter,
                "store resolution window {requested} is outside 1..={maximum}"
            ),
            Self::PhaseHydrationMismatch {
                phase,
                expected,
                actual,
            } => write!(
                formatter,
                "frozen {phase} phase expected {expected} rows but Store returned {actual}"
            ),
            Self::InputIncomplete { path, version_id } => write!(
                formatter,
                "manifest version {version_id} for {path} has no committed L2 stamp"
            ),
            Self::UnexpectedOutputPath(path) => {
                write!(
                    formatter,
                    "resolution output already exists: {}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for StoreResolutionError {}

impl From<rusqlite::Error> for StoreResolutionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<StoreConnectionError> for StoreResolutionError {
    fn from(error: StoreConnectionError) -> Self {
        Self::Connection(error)
    }
}

impl From<ResolutionValidationError> for StoreResolutionError {
    fn from(error: ResolutionValidationError) -> Self {
        Self::Artifact(error)
    }
}

#[derive(Debug)]
pub struct StoreScratchResolutionSession {
    reader_factory: StoreConnectionFactory,
    identity: StoreManifestIdentity,
    exact_path: PathBuf,
    window_size: usize,
    resolver_output_epoch: i64,
    scratch_path: PathBuf,
    scratch: Connection,
    active_phase: Option<ResolutionPhase>,
    phase_after: Option<(i64, String)>,
    max_emitted_chunk_size: usize,
    max_store_read_page: Cell<usize>,
    phase_reader_opens: Cell<usize>,
    visible_root_batches: usize,
    candidate_reader: RefCell<Option<Connection>>,
    resolution_cache: RefCell<HashMap<ResolutionLookupKey, TierOutcome>>,
}

impl StoreScratchResolutionSession {
    pub fn new(
        reader_factory: StoreConnectionFactory,
        identity: StoreManifestIdentity,
        exact_path: impl AsRef<Path>,
        window_size: usize,
        resolver_output_epoch: i64,
    ) -> Result<Self, StoreResolutionError> {
        let exact_path = exact_path.as_ref().to_path_buf();
        if identity.family_id.is_empty()
            || identity.view_id.is_empty()
            || identity.generation <= 0
            || identity.manifest_hash.is_empty()
            || resolver_output_epoch <= 0
        {
            return Err(StoreResolutionError::InvalidIdentity);
        }
        if window_size == 0 || window_size > MAX_STORE_RESOLUTION_WINDOW {
            return Err(StoreResolutionError::InvalidWindowSize {
                requested: window_size,
                maximum: MAX_STORE_RESOLUTION_WINDOW,
            });
        }
        if exact_path.exists() {
            return Err(StoreResolutionError::UnexpectedOutputPath(exact_path));
        }
        let mut session = Self {
            reader_factory,
            identity,
            exact_path,
            window_size,
            resolver_output_epoch,
            scratch_path: PathBuf::new(),
            scratch: Connection::open_in_memory()?,
            active_phase: None,
            phase_after: None,
            max_emitted_chunk_size: 0,
            max_store_read_page: Cell::new(0),
            phase_reader_opens: Cell::new(0),
            visible_root_batches: 0,
            candidate_reader: RefCell::new(None),
            resolution_cache: RefCell::new(HashMap::new()),
        };
        session.validate_manifest()?;
        session.initialize_scratch()?;
        Ok(session)
    }

    pub fn manifest_window(
        &self,
        after_path: Option<&str>,
    ) -> Result<Vec<StoreManifestEntry>, StoreResolutionError> {
        let connection = self.open_reader()?;
        let mut statement = connection.prepare(
            "SELECT me.path, fv.language, me.status, me.version_id
             FROM manifest_entries AS me
             LEFT JOIN file_versions AS fv ON fv.version_id = me.version_id
             WHERE me.view_id = ?1 AND me.generation = ?2
               AND (?3 IS NULL OR me.path > ?3 COLLATE BINARY)
             ORDER BY me.path COLLATE BINARY
             LIMIT ?4",
        )?;
        let rows = statement
            .query_map(
                params![
                    self.identity.view_id,
                    self.identity.generation,
                    after_path,
                    i64::try_from(self.window_size)
                        .map_err(|_| StoreResolutionError::InvalidIdentity)?
                ],
                |row| {
                    Ok(StoreManifestEntry {
                        path: row.get(0)?,
                        language: row.get(1)?,
                        status: row.get(2)?,
                        version_id: row.get(3)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn extraction_versions_window(
        &self,
        after_version_id: Option<i64>,
    ) -> Result<Vec<i64>, StoreResolutionError> {
        let connection = self.open_reader()?;
        let mut statement = connection.prepare(
            "SELECT me.version_id
             FROM manifest_entries AS me
             WHERE me.view_id = ?1 AND me.generation = ?2
               AND me.status IN ('indexed', 'failed_preserved')
               AND (?3 IS NULL OR me.version_id > ?3)
             ORDER BY me.version_id
             LIMIT ?4",
        )?;
        let versions = statement
            .query_map(
                params![
                    self.identity.view_id,
                    self.identity.generation,
                    after_version_id,
                    i64::try_from(self.window_size)
                        .map_err(|_| StoreResolutionError::InvalidIdentity)?
                ],
                |row| row.get(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        self.max_store_read_page
            .set(self.max_store_read_page.get().max(versions.len()));
        Ok(versions)
    }

    pub fn exact_path(&self) -> &Path {
        &self.exact_path
    }

    pub fn resolver_output_epoch(&self) -> i64 {
        self.resolver_output_epoch
    }

    pub fn max_emitted_chunk_size(&self) -> usize {
        self.max_emitted_chunk_size
    }

    pub fn max_store_read_page(&self) -> usize {
        self.max_store_read_page.get()
    }

    pub fn phase_reader_opens(&self) -> usize {
        self.phase_reader_opens.get()
    }

    #[cfg(feature = "test-store-resolution-contract")]
    pub fn visible_root_batches(&self) -> usize {
        self.visible_root_batches
    }

    #[cfg(feature = "test-store-resolution-contract")]
    pub fn scratch_pragma_values(
        &self,
    ) -> Result<ResolutionScratchPragmaValues, StoreResolutionError> {
        Ok((
            self.scratch
                .query_row("PRAGMA page_size", [], |row| row.get(0))?,
            self.scratch
                .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?,
            self.scratch
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))?,
            self.scratch
                .query_row("PRAGMA synchronous", [], |row| row.get(0))?,
            self.scratch
                .query_row("PRAGMA foreign_keys", [], |row| row.get(0))?,
            self.scratch
                .query_row("PRAGMA secure_delete", [], |row| row.get(0))?,
            self.scratch
                .query_row("PRAGMA wal_autocheckpoint", [], |row| row.get(0))?,
        ))
    }

    #[cfg(feature = "test-store-resolution-contract")]
    pub fn inject_frozen_phase_key_for_test(
        &mut self,
        phase: ResolutionPhase,
        version_id: i64,
        local_id: &str,
    ) -> Result<(), StoreResolutionError> {
        let phase = phase_code(phase);
        self.scratch.execute(
            "INSERT OR IGNORE INTO phase_ready(phase) VALUES (?1)",
            [phase],
        )?;
        self.scratch.execute(
            "INSERT INTO phase_keys(phase,version_id,local_id) VALUES (?1,?2,?3)",
            params![phase, version_id, local_id],
        )?;
        Ok(())
    }

    pub fn finish_exact(mut self) -> Result<ResolutionFileIdentity, StoreResolutionError> {
        let mut writer = ResolutionBaseWriter::new(
            &self.exact_path,
            self.identity.manifest_hash.clone(),
            self.resolver_output_epoch,
        )?;
        {
            let mut statement = self
                .scratch
                .prepare("SELECT version_id FROM visible_versions ORDER BY version_id")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                writer.push_source_version(row.get(0)?)?;
            }
        }
        {
            let mut statement = self.scratch.prepare(
                "SELECT version_id,identifier_id,target_version_id,target_symbol_id,
                        tier,confidence,method,outcome,candidates
                 FROM identifier_resolutions ORDER BY version_id,identifier_id",
            )?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                writer.push_identifier_resolution(ResolutionIdentifierRow {
                    version_id: row.get(0)?,
                    identifier_id: row.get(1)?,
                    target_version_id: row.get(2)?,
                    target_symbol_id: row.get(3)?,
                    tier: row.get(4)?,
                    confidence: row.get(5)?,
                    method: row.get(6)?,
                    outcome: row.get(7)?,
                    candidates: row.get(8)?,
                })?;
            }
        }
        {
            let mut statement = self.scratch.prepare(
                "SELECT version_id,pending_relationship_id,target_version_id,target_symbol_id,
                        tier,confidence,method
                 FROM pending_resolutions ORDER BY version_id,pending_relationship_id",
            )?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                writer.push_pending_resolution(ResolutionPendingRow {
                    version_id: row.get(0)?,
                    pending_relationship_id: row.get(1)?,
                    target_version_id: row.get(2)?,
                    target_symbol_id: row.get(3)?,
                    tier: row.get(4)?,
                    confidence: row.get(5)?,
                    method: row.get(6)?,
                })?;
            }
        }
        let factory = self.reader_factory.clone();
        let identity = self.identity.clone();
        let result = writer.finish_with_target_lookup(move |version_id, symbol_id| {
            let connection = factory.open_reader().map_err(|error| {
                ResolutionValidationError::InvalidMetadata {
                    key: "store_reader".to_string(),
                    value: error.to_string(),
                }
            })?;
            let exists = connection
                .query_row(
                    "SELECT EXISTS(
                       SELECT 1 FROM symbols AS s
                       WHERE s.version_id = ?1 AND s.symbol_id = ?2
                         AND EXISTS (
                           SELECT 1 FROM manifest_entries AS me
                           WHERE me.view_id = ?3 AND me.generation = ?4
                             AND me.status IN ('indexed','failed_preserved')
                             AND me.version_id = s.version_id
                         )
                     )",
                    params![version_id, symbol_id, identity.view_id, identity.generation],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(ResolutionValidationError::Sqlite)?;
            Ok(exists)
        })?;
        self.remove_scratch()?;
        Ok(result)
    }

    fn open_reader(&self) -> Result<Connection, StoreResolutionError> {
        Ok(self.reader_factory.open_reader()?)
    }

    fn sql_window_limit(&self) -> Result<i64, StoreResolutionError> {
        i64::try_from(self.window_size).map_err(|_| StoreResolutionError::InvalidWindowSize {
            requested: self.window_size,
            maximum: MAX_STORE_RESOLUTION_WINDOW,
        })
    }

    fn open_phase_reader(&self) -> Result<Connection, StoreResolutionError> {
        self.phase_reader_opens
            .set(self.phase_reader_opens.get() + 1);
        self.open_reader()
    }

    fn with_candidate_reader<T>(
        &self,
        read: impl FnOnce(&Connection) -> Result<T, StoreResolutionError>,
    ) -> Result<T, StoreResolutionError> {
        if self.candidate_reader.borrow().is_none() {
            *self.candidate_reader.borrow_mut() = Some(self.open_phase_reader()?);
        }
        let reader = self.candidate_reader.borrow();
        read(reader.as_ref().expect("candidate reader initialized"))
    }

    fn reset_candidate_window(&mut self) -> Result<(), StoreResolutionError> {
        let reader = self.open_phase_reader()?;
        *self.candidate_reader.get_mut() = Some(reader);
        self.resolution_cache.get_mut().clear();
        Ok(())
    }

    fn initialize_scratch(&mut self) -> Result<(), StoreResolutionError> {
        let mut scratch_path = self.exact_path.as_os_str().to_os_string();
        scratch_path.push(".work");
        self.scratch_path = PathBuf::from(scratch_path);
        if self.scratch_path.exists() {
            return Err(StoreResolutionError::UnexpectedOutputPath(
                self.scratch_path.clone(),
            ));
        }
        self.scratch = create_resolution_scratch_connection(&self.scratch_path)?;
        self.scratch.execute_batch(
            "CREATE TABLE visible_versions(
               version_id INTEGER PRIMARY KEY CHECK(version_id > 0)
             ) STRICT;
             CREATE TABLE identifier_resolutions(
               version_id INTEGER NOT NULL,
               identifier_id TEXT NOT NULL,
               target_version_id INTEGER,
               target_symbol_id TEXT,
               tier INTEGER,
               confidence REAL,
               method TEXT,
               outcome TEXT NOT NULL,
               candidates INTEGER,
               PRIMARY KEY(version_id,identifier_id)
             ) STRICT;
             CREATE TABLE pending_resolutions(
               version_id INTEGER NOT NULL,
               pending_relationship_id TEXT NOT NULL,
               target_version_id INTEGER NOT NULL,
               target_symbol_id TEXT NOT NULL,
               tier INTEGER NOT NULL,
               confidence REAL NOT NULL,
               method TEXT NOT NULL,
               PRIMARY KEY(version_id,pending_relationship_id)
             ) STRICT;
             CREATE TABLE phase_keys(
               phase INTEGER NOT NULL,
               version_id INTEGER NOT NULL,
               local_id TEXT NOT NULL,
               PRIMARY KEY(phase,version_id,local_id)
             ) STRICT;
             CREATE TABLE phase_ready(
               phase INTEGER PRIMARY KEY
             ) STRICT;
             CREATE TEMP TABLE tier_candidate_accumulator(
               version_id INTEGER NOT NULL,
               symbol_id TEXT NOT NULL,
               confidence REAL NOT NULL,
               PRIMARY KEY(version_id,symbol_id)
             ) STRICT;",
        )?;
        #[cfg(feature = "test-store-resolution-contract")]
        julie_extract_artifact::store::test_hooks::crash_if(
            "resolution_exact_after_scratch_create",
        );
        Ok(())
    }

    fn remove_scratch(&mut self) -> Result<(), StoreResolutionError> {
        if self.scratch_path.as_os_str().is_empty() {
            return Ok(());
        }
        self.scratch
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        let placeholder = Connection::open_in_memory()?;
        drop(std::mem::replace(&mut self.scratch, placeholder));
        for suffix in ["", "-wal", "-shm"] {
            let path = PathBuf::from(format!("{}{}", self.scratch_path.display(), suffix));
            if path.exists() {
                std::fs::remove_file(&path).map_err(|error| {
                    StoreResolutionError::Artifact(ResolutionValidationError::Io(error))
                })?;
            }
        }
        self.scratch_path.clear();
        Ok(())
    }

    fn validate_manifest(&self) -> Result<(), StoreResolutionError> {
        let connection = self.open_reader()?;
        let family_id: String = connection.query_row(
            "SELECT value FROM store_meta WHERE key = 'family_id'",
            [],
            |row| row.get(0),
        )?;
        if family_id != self.identity.family_id {
            return Err(StoreResolutionError::InvalidIdentity);
        }
        let hash = connection
            .query_row(
                "SELECT manifest_hash FROM manifests WHERE view_id = ?1 AND generation = ?2",
                params![self.identity.view_id, self.identity.generation],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if hash.as_deref() != Some(self.identity.manifest_hash.as_str()) {
            return Err(StoreResolutionError::InvalidIdentity);
        }
        let incomplete = connection
            .query_row(
                "SELECT me.path, me.version_id
                 FROM manifest_entries AS me
                 JOIN file_versions AS fv ON fv.version_id = me.version_id
                 WHERE me.view_id = ?1 AND me.generation = ?2
                   AND me.status IN ('indexed', 'failed_preserved')
                   AND fv.complete_l2 IS NULL
                 ORDER BY me.path COLLATE BINARY
                 LIMIT 1",
                params![self.identity.view_id, self.identity.generation],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        if let Some((path, version_id)) = incomplete {
            return Err(StoreResolutionError::InputIncomplete { path, version_id });
        }
        Ok(())
    }
}

impl CandidateLookup for StoreScratchResolutionSession {
    type Error = StoreResolutionError;

    fn symbol_by_id(
        &self,
        source_key: &str,
        local_id: &str,
    ) -> Result<Option<CandidateHit>, Self::Error> {
        let version_id = parse_source_key(source_key)?;
        self.with_candidate_reader(|connection| {
            let hit = connection
                .query_row(
                    "SELECT s.version_id, s.symbol_id, s.language, s.name, s.kind,
                        s.parent_symbol_id, s.visibility, s.signature, s.metadata_json
                 FROM symbols AS s
                 WHERE s.version_id = ?1 AND s.symbol_id = ?2
                   AND EXISTS (
                     SELECT 1 FROM manifest_entries AS me
                     WHERE me.view_id = ?3 AND me.generation = ?4
                       AND me.status IN ('indexed', 'failed_preserved')
                       AND me.version_id = s.version_id
                   )",
                    params![
                        version_id,
                        local_id,
                        self.identity.view_id,
                        self.identity.generation
                    ],
                    candidate_hit,
                )
                .optional()?
                .flatten();
            Ok(hit)
        })
    }

    fn visit_by_name<F>(&self, name: &str, mut visitor: F) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, Self::Error>,
    {
        let sql = "SELECT s.version_id, s.symbol_id, s.language, s.name, s.kind,
                    s.parent_symbol_id, s.visibility, s.signature, s.metadata_json
             FROM symbols AS s
             WHERE s.name = ?1
               AND EXISTS (
                 SELECT 1 FROM manifest_entries AS me
                 WHERE me.view_id = ?2 AND me.generation = ?3
                   AND me.status IN ('indexed', 'failed_preserved')
                   AND me.version_id = s.version_id
               )
               AND (s.version_id,s.symbol_id)>(?4,?5)
             ORDER BY s.version_id, s.symbol_id COLLATE BINARY LIMIT ?6";
        let mut after = (0, String::new());
        loop {
            let (page, next) = self.candidate_page(
                sql,
                vec![
                    name.to_string().into(),
                    self.identity.view_id.clone().into(),
                    self.identity.generation.into(),
                    after.0.into(),
                    after.1.clone().into(),
                    self.sql_window_limit()?.into(),
                ],
            )?;
            let Some(next) = next else {
                break;
            };
            for hit in page {
                if !visitor(self, hit)? {
                    return Ok(());
                }
            }
            after = next;
        }
        Ok(())
    }

    fn visit_children_named<F>(
        &self,
        source_key: &str,
        parent_id: &str,
        name: &str,
        mut visitor: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, Self::Error>,
    {
        let version_id = parse_source_key(source_key)?;
        let sql = "SELECT s.version_id, s.symbol_id, s.language, s.name, s.kind,
                    s.parent_symbol_id, s.visibility, s.signature, s.metadata_json
             FROM symbols AS s
             WHERE s.version_id = ?1 AND s.parent_symbol_id = ?2 AND s.name = ?3
               AND EXISTS (
                 SELECT 1 FROM manifest_entries AS me
                 WHERE me.view_id = ?4 AND me.generation = ?5
                   AND me.status IN ('indexed', 'failed_preserved')
                   AND me.version_id = s.version_id
               ) AND s.symbol_id>?6
             ORDER BY s.symbol_id COLLATE BINARY LIMIT ?7";
        let mut after = String::new();
        loop {
            let (page, next) = self.candidate_page(
                sql,
                vec![
                    version_id.into(),
                    parent_id.to_string().into(),
                    name.to_string().into(),
                    self.identity.view_id.clone().into(),
                    self.identity.generation.into(),
                    after.clone().into(),
                    self.sql_window_limit()?.into(),
                ],
            )?;
            let Some(next) = next else {
                break;
            };
            for hit in page {
                if !visitor(self, hit)? {
                    return Ok(());
                }
            }
            after = next.1;
        }
        Ok(())
    }

    fn visit_top_level_named<F>(
        &self,
        source_key: &str,
        name: &str,
        mut visitor: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, Self::Error>,
    {
        let version_id = parse_source_key(source_key)?;
        let sql = "SELECT s.version_id, s.symbol_id, s.language, s.name, s.kind,
                    s.parent_symbol_id, s.visibility, s.signature, s.metadata_json
             FROM symbols AS s
             WHERE s.version_id = ?1 AND s.parent_symbol_id IS NULL AND s.name = ?2
               AND EXISTS (
                 SELECT 1 FROM manifest_entries AS me
                 WHERE me.view_id = ?3 AND me.generation = ?4
                   AND me.status IN ('indexed', 'failed_preserved')
                   AND me.version_id = s.version_id
               ) AND s.symbol_id>?5
             ORDER BY s.symbol_id COLLATE BINARY LIMIT ?6";
        let mut after = String::new();
        loop {
            let (page, next) = self.candidate_page(
                sql,
                vec![
                    version_id.into(),
                    name.to_string().into(),
                    self.identity.view_id.clone().into(),
                    self.identity.generation.into(),
                    after.clone().into(),
                    self.sql_window_limit()?.into(),
                ],
            )?;
            let Some(next) = next else {
                break;
            };
            for hit in page {
                if !visitor(self, hit)? {
                    return Ok(());
                }
            }
            after = next.1;
        }
        Ok(())
    }

    fn visit_type_facts<F>(
        &self,
        symbol_id: &SemanticSymbolId,
        mut visitor: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, TypeFact) -> Result<bool, Self::Error>,
    {
        let SemanticVersionId::Store(version_id) = symbol_id.version else {
            return Ok(());
        };
        let sql = "SELECT tf.type_fact_id, tf.symbol_id, tf.resolved_type, tf.is_inferred
             FROM type_facts AS tf
             WHERE tf.version_id = ?1 AND tf.symbol_id = ?2
               AND EXISTS (
                 SELECT 1 FROM manifest_entries AS me
                 WHERE me.view_id = ?3 AND me.generation = ?4
                   AND me.status IN ('indexed', 'failed_preserved')
                   AND me.version_id = tf.version_id
               ) AND tf.type_fact_id>?5
             ORDER BY tf.type_fact_id COLLATE BINARY LIMIT ?6";
        let mut after = String::new();
        loop {
            let page = {
                let connection = self.open_reader()?;
                let mut statement = connection.prepare(sql)?;
                statement
                    .query_map(
                        params![
                            version_id,
                            symbol_id.local_id,
                            self.identity.view_id,
                            self.identity.generation,
                            after,
                            self.sql_window_limit()?
                        ],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                TypeFact {
                                    symbol_id: row.get(1)?,
                                    resolved_type: row.get(2)?,
                                    is_inferred: row.get::<_, i64>(3)? != 0,
                                },
                            ))
                        },
                    )?
                    .collect::<Result<Vec<_>, _>>()?
            };
            self.max_store_read_page
                .set(self.max_store_read_page.get().max(page.len()));
            if page.is_empty() {
                break;
            }
            for (_, fact) in &page {
                if !visitor(self, fact.clone())? {
                    return Ok(());
                }
            }
            after = page.last().expect("non-empty type fact page").0.clone();
        }
        Ok(())
    }

    fn visit_imports<F>(&self, source_key: &str, mut visitor: F) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, ImportRecord) -> Result<bool, Self::Error>,
    {
        let version_id = parse_source_key(source_key)?;
        let sql = "SELECT s.symbol_id, s.path, s.language, s.name, s.metadata_json
             FROM symbols AS s
             WHERE s.version_id = ?1 AND s.kind = 'import'
               AND EXISTS (
                 SELECT 1 FROM manifest_entries AS me
                 WHERE me.view_id = ?2 AND me.generation = ?3
                   AND me.status IN ('indexed', 'failed_preserved')
                   AND me.version_id = s.version_id
               ) AND s.symbol_id>?4
             ORDER BY s.symbol_id COLLATE BINARY LIMIT ?5";
        let mut after = String::new();
        loop {
            let page = {
                let connection = self.open_reader()?;
                let mut statement = connection.prepare(sql)?;
                statement
                    .query_map(
                        params![
                            version_id,
                            self.identity.view_id,
                            self.identity.generation,
                            after,
                            self.sql_window_limit()?
                        ],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, String>(3)?,
                                row.get::<_, Option<String>>(4)?,
                            ))
                        },
                    )?
                    .collect::<Result<Vec<_>, _>>()?
            };
            self.max_store_read_page
                .set(self.max_store_read_page.get().max(page.len()));
            if page.is_empty() {
                break;
            }
            for (_, path, language, name, metadata_json) in &page {
                let (local_name, imported_name, source, is_type_only, is_default, is_namespace) =
                    resolution::import_binding(name, metadata_json.as_deref());
                let module_file_id = self.select_module_version(
                    &resolution::import_module_candidates(path, source.as_deref(), language),
                    language,
                )?;
                let import = ImportRecord {
                    file_id: source_key.to_string(),
                    local_name,
                    imported_name,
                    source,
                    module_file_id,
                    is_type_only,
                    is_default,
                    is_namespace,
                };
                if !visitor(self, import)? {
                    return Ok(());
                }
            }
            after = page.last().expect("non-empty import page").0.clone();
        }
        Ok(())
    }

    fn reset_tier_candidates(&self) -> Result<(), Self::Error> {
        self.scratch
            .execute("DELETE FROM tier_candidate_accumulator", [])?;
        Ok(())
    }

    fn record_tier_candidate(
        &self,
        semantic_id: SemanticSymbolId,
        confidence: f64,
    ) -> Result<(), Self::Error> {
        let version_id = store_version(&semantic_id.version)?;
        self.scratch.execute(
            "INSERT INTO tier_candidate_accumulator(version_id,symbol_id,confidence)
             VALUES (?1,?2,?3)
             ON CONFLICT(version_id,symbol_id) DO UPDATE SET
               confidence=MAX(confidence,excluded.confidence)",
            params![version_id, semantic_id.local_id, confidence],
        )?;
        Ok(())
    }

    fn tier_candidate_summary(&self) -> Result<CandidateSummary, Self::Error> {
        let exact_count = self.scratch.query_row(
            "SELECT COUNT(*) FROM tier_candidate_accumulator",
            [],
            |row| row.get::<_, i64>(0),
        )? as u64;
        let mut statement = self.scratch.prepare(
            "SELECT version_id,symbol_id,confidence
             FROM tier_candidate_accumulator
             ORDER BY version_id,symbol_id COLLATE BINARY LIMIT 2",
        )?;
        let evidence = statement
            .query_map([], |row| {
                Ok(CandidateEvidence {
                    semantic_id: SemanticSymbolId {
                        version: SemanticVersionId::Store(row.get(0)?),
                        local_id: row.get(1)?,
                    },
                    confidence: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CandidateSummary {
            evidence,
            exact_count,
        })
    }

    fn cached_resolution(&self, edge: &UnresolvedEdge) -> Option<TierOutcome> {
        self.resolution_cache
            .borrow()
            .get(&ResolutionLookupKey::from(edge))
            .cloned()
    }

    fn cache_resolution(&self, edge: &UnresolvedEdge, outcome: &TierOutcome) {
        self.resolution_cache
            .borrow_mut()
            .insert(ResolutionLookupKey::from(edge), outcome.clone());
    }
}

impl ResolutionSession for StoreScratchResolutionSession {
    type Error = StoreResolutionError;

    fn corpus_identity(&self) -> Result<ResolutionCorpusIdentity, Self::Error> {
        Ok(ResolutionCorpusIdentity::Store {
            family_id: self.identity.family_id.clone(),
            view_id: self.identity.view_id.clone(),
            manifest_generation: self.identity.generation,
            manifest_hash: self.identity.manifest_hash.clone(),
        })
    }

    fn prior_resolution_state(
        &mut self,
    ) -> Result<Option<crate::resolution_session::SessionResolutionState>, Self::Error> {
        Ok(None)
    }

    fn current_revision(&mut self) -> Result<i64, Self::Error> {
        Ok(self.resolver_output_epoch)
    }

    fn open_resolution_pass(
        &mut self,
        _request: &ResolutionPassRequest,
    ) -> Result<ResolutionWorklists, Self::Error> {
        let reset = self.scratch.transaction()?;
        reset.execute("DELETE FROM visible_versions", [])?;
        reset.execute("DELETE FROM phase_keys", [])?;
        reset.execute("DELETE FROM phase_ready", [])?;
        reset.commit()?;
        self.visible_root_batches = 0;
        let mut after = None;
        loop {
            let versions = self.extraction_versions_window(after)?;
            if versions.is_empty() {
                break;
            }
            let transaction = self.scratch.transaction()?;
            {
                let mut insert =
                    transaction.prepare("INSERT INTO visible_versions(version_id) VALUES (?1)")?;
                for version_id in &versions {
                    insert.execute([version_id])?;
                }
            }
            transaction.commit()?;
            self.visible_root_batches += 1;
            after = versions.last().copied();
        }
        self.active_phase = None;
        self.phase_after = None;
        Ok(ResolutionWorklists {
            effective_full: true,
            ..ResolutionWorklists::default()
        })
    }

    fn qualify_version(&self, source_key: &str) -> Result<SemanticVersionId, Self::Error> {
        Ok(SemanticVersionId::Store(parse_source_key(source_key)?))
    }

    fn resolve_edge(
        &mut self,
        edge: &resolution::UnresolvedEdge,
    ) -> Result<resolution::TierOutcome, Self::Error> {
        resolution::resolve_with_candidate_lookup(self, edge)
    }

    fn target_symbol_name(
        &mut self,
        symbol_id: &SemanticSymbolId,
    ) -> Result<Option<String>, Self::Error> {
        let SemanticVersionId::Store(version_id) = symbol_id.version else {
            return Ok(None);
        };
        let connection = self.open_reader()?;
        Ok(connection
            .query_row(
                "SELECT s.name FROM symbols AS s
                 WHERE s.version_id=?1 AND s.symbol_id=?2
                   AND EXISTS (
                     SELECT 1 FROM manifest_entries AS me
                     WHERE me.view_id=?3 AND me.generation=?4
                       AND me.status IN ('indexed','failed_preserved')
                       AND me.version_id=s.version_id
                   )",
                params![
                    version_id,
                    symbol_id.local_id,
                    self.identity.view_id,
                    self.identity.generation
                ],
                |row| row.get(0),
            )
            .optional()?)
    }

    fn locate_identifier(
        &self,
        version: &SemanticVersionId,
        name: &str,
        start_byte: Option<i64>,
        end_byte: Option<i64>,
        start_line: i64,
    ) -> Result<Option<String>, Self::Error> {
        let SemanticVersionId::Store(version_id) = version else {
            return Ok(None);
        };
        self.with_candidate_reader(|connection| {
            let ids = if let (Some(start_byte), Some(end_byte)) = (start_byte, end_byte) {
                connection
                    .prepare_cached(
                        "SELECT identifier_id FROM identifiers
                         WHERE version_id=?1 AND name=?2
                           AND start_byte>=?3 AND start_byte<=?4 AND end_byte<=?4
                         ORDER BY identifier_id COLLATE BINARY LIMIT 2",
                    )?
                    .query_map(params![version_id, name, start_byte, end_byte], |row| {
                        row.get::<_, String>(0)
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                connection
                    .prepare_cached(
                        "SELECT identifier_id FROM identifiers
                         WHERE version_id=?1 AND name=?2 AND start_line=?3
                         ORDER BY identifier_id COLLATE BINARY LIMIT 2",
                    )?
                    .query_map(params![version_id, name, start_line], |row| {
                        row.get::<_, String>(0)
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };
            let identifier_id = (ids.len() == 1).then(|| ids[0].clone());
            Ok(identifier_id)
        })
    }

    fn identifier_is_covered(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        self.scratch_identifier_exists(identifier_id)
    }

    fn propagation_is_covered(
        &mut self,
        _identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    fn propagation_is_owned(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        self.scratch_identifier_exists(identifier_id)
    }

    fn next_phase_chunk(
        &mut self,
        worklists: &ResolutionWorklists,
    ) -> Result<Option<ResolutionPhaseChunk>, Self::Error> {
        if self.active_phase != Some(worklists.phase) {
            self.freeze_phase(worklists.phase)?;
            self.active_phase = Some(worklists.phase);
            self.phase_after = None;
        }
        if matches!(
            worklists.phase,
            ResolutionPhase::ResolvedPending
                | ResolutionPhase::PropagationCovered
                | ResolutionPhase::ResolvedIdentifiers
                | ResolutionPhase::PropagationOwned
        ) {
            return Ok(None);
        }
        if worklists.phase == ResolutionPhase::WorkspaceGated {
            let languages = self.workspace_gated_languages()?;
            self.active_phase = None;
            return Ok(
                (!languages.is_empty()).then_some(ResolutionPhaseChunk::WorkspaceGated(languages))
            );
        }
        let phase = phase_code(worklists.phase);
        let (after_version, after_local) = self.phase_after.clone().unwrap_or((0, String::new()));
        let keys = {
            let mut statement = self.scratch.prepare(
                "SELECT version_id,local_id FROM phase_keys
                 WHERE phase=?1 AND (version_id,local_id)>(?2,?3)
                 ORDER BY version_id,local_id COLLATE BINARY LIMIT ?4",
            )?;
            statement
                .query_map(
                    params![phase, after_version, after_local, self.sql_window_limit()?],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        if keys.is_empty() {
            *self.candidate_reader.get_mut() = None;
            self.resolution_cache.get_mut().clear();
            return Ok(None);
        }
        self.phase_after = keys.last().cloned();
        self.max_emitted_chunk_size = self.max_emitted_chunk_size.max(keys.len());
        self.reset_candidate_window()?;
        match worklists.phase {
            ResolutionPhase::Pending => Ok(Some(ResolutionPhaseChunk::Pending(
                self.load_pending_page(&keys)?,
            ))),
            ResolutionPhase::Relationships => Ok(Some(ResolutionPhaseChunk::Relationships(
                self.load_relationship_page(&keys)?,
            ))),
            ResolutionPhase::Identifiers => Ok(Some(ResolutionPhaseChunk::Identifiers(
                self.load_identifier_page(&keys)?,
            ))),
            _ => Ok(None),
        }
    }

    fn flush(&mut self, writes: ResolutionWriteBatch) -> Result<ResolutionCounts, Self::Error> {
        let transaction = self.scratch.transaction()?;
        {
            let mut pending_upsert = transaction.prepare(
                "INSERT INTO pending_resolutions
                 (version_id,pending_relationship_id,target_version_id,target_symbol_id,tier,confidence,method)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(version_id,pending_relationship_id) DO UPDATE SET
                   target_version_id=excluded.target_version_id,
                   target_symbol_id=excluded.target_symbol_id,tier=excluded.tier,
                   confidence=excluded.confidence,method=excluded.method",
            )?;
            let mut pending_delete = transaction.prepare(
                "DELETE FROM pending_resolutions
                 WHERE version_id=?1 AND pending_relationship_id=?2",
            )?;
            let mut identifier_upsert = transaction.prepare(
                "INSERT INTO identifier_resolutions
                 (version_id,identifier_id,target_version_id,target_symbol_id,tier,confidence,method,outcome,candidates)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
                 ON CONFLICT(version_id,identifier_id) DO UPDATE SET
                   target_version_id=excluded.target_version_id,
                   target_symbol_id=excluded.target_symbol_id,tier=excluded.tier,
                   confidence=excluded.confidence,method=excluded.method,
                   outcome=excluded.outcome,candidates=excluded.candidates",
            )?;
            let mut identifier_delete = transaction.prepare(
                "DELETE FROM identifier_resolutions WHERE version_id=?1 AND identifier_id=?2",
            )?;
            for write in writes.writes {
                match write {
                    ResolutionWrite::Pending {
                        pending_relationship_id,
                        target_symbol_id,
                        tier,
                        confidence,
                        method,
                        ..
                    } => {
                        let version_id = store_version(&pending_relationship_id.version)?;
                        let target_version_id = store_version(&target_symbol_id.version)?;
                        pending_upsert.execute(params![
                            version_id,
                            pending_relationship_id.local_id,
                            target_version_id,
                            target_symbol_id.local_id,
                            tier,
                            confidence,
                            method
                        ])?;
                    }
                    ResolutionWrite::DemotePending {
                        pending_relationship_id,
                    } => {
                        pending_delete.execute(params![
                            store_version(&pending_relationship_id.version)?,
                            pending_relationship_id.local_id
                        ])?;
                    }
                    ResolutionWrite::Identifier {
                        identifier_id,
                        target_symbol_id,
                        outcome,
                        tier,
                        confidence,
                        method,
                        candidates,
                        ..
                    } => {
                        let target_version_id = target_symbol_id
                            .as_ref()
                            .map(|id| store_version(&id.version))
                            .transpose()?;
                        identifier_upsert.execute(params![
                            store_version(&identifier_id.version)?,
                            identifier_id.local_id,
                            target_version_id,
                            target_symbol_id.map(|id| id.local_id),
                            tier,
                            confidence,
                            method,
                            outcome.as_str(),
                            candidates
                        ])?;
                    }
                    ResolutionWrite::DemoteIdentifier { identifier_id } => {
                        identifier_delete.execute(params![
                            store_version(&identifier_id.version)?,
                            identifier_id.local_id
                        ])?;
                    }
                }
            }
        }
        transaction.commit()?;
        Ok(ResolutionCounts::default())
    }

    fn aggregate_report(&mut self) -> Result<Vec<ResolutionReportRow>, Self::Error> {
        Ok(Vec::new())
    }
}

impl StoreScratchResolutionSession {
    fn candidate_page(
        &self,
        sql: &str,
        bind: Vec<rusqlite::types::Value>,
    ) -> Result<CandidatePage, StoreResolutionError> {
        self.with_candidate_reader(|connection| {
            let mut statement = connection.prepare_cached(sql)?;
            let mut rows = statement.query(rusqlite::params_from_iter(bind))?;
            let mut hits = Vec::new();
            let mut last = None;
            let mut page_rows = 0usize;
            while let Some(row) = rows.next()? {
                page_rows += 1;
                last = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
                if let Some(hit) = candidate_hit(row)? {
                    hits.push(hit);
                }
            }
            self.max_store_read_page
                .set(self.max_store_read_page.get().max(page_rows));
            Ok((hits, last))
        })
    }

    fn scratch_identifier_exists(
        &self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, StoreResolutionError> {
        let version_id = store_version(&identifier_id.version)?;
        Ok(self.scratch.query_row(
            "SELECT EXISTS(SELECT 1 FROM identifier_resolutions WHERE version_id=?1 AND identifier_id=?2)",
            params![version_id, identifier_id.local_id],
            |row| row.get(0),
        )?)
    }

    fn freeze_phase(&mut self, phase: ResolutionPhase) -> Result<(), StoreResolutionError> {
        let code = phase_code(phase);
        let sql_window_limit = self.sql_window_limit()?;
        if self.scratch.query_row(
            "SELECT EXISTS(SELECT 1 FROM phase_ready WHERE phase=?1)",
            [code],
            |row| row.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        let table = match phase {
            ResolutionPhase::Pending => Some(("pending_relationships", "pending_relationship_id")),
            ResolutionPhase::Relationships => Some(("relationships", "relationship_id")),
            ResolutionPhase::Identifiers => Some(("identifiers", "identifier_id")),
            _ => None,
        };
        let transaction = self.scratch.transaction()?;
        if let Some((table, id_column)) = table {
            let sql = format!(
                "SELECT source.version_id,source.{id_column}
                 FROM {table} AS source
                 WHERE EXISTS (
                   SELECT 1 FROM manifest_entries AS me
                   WHERE me.view_id=?1 AND me.generation=?2
                     AND me.status IN ('indexed','failed_preserved')
                     AND me.version_id=source.version_id
                 )
                   AND (source.version_id,source.{id_column})>(?3,?4)
                 ORDER BY source.version_id,source.{id_column} COLLATE BINARY LIMIT ?5"
            );
            let mut after = (0, String::new());
            loop {
                let keys = {
                    self.phase_reader_opens
                        .set(self.phase_reader_opens.get() + 1);
                    let connection = self.reader_factory.open_reader()?;
                    let mut statement = connection.prepare(&sql)?;
                    statement
                        .query_map(
                            params![
                                self.identity.view_id,
                                self.identity.generation,
                                after.0,
                                after.1,
                                sql_window_limit
                            ],
                            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                        )?
                        .collect::<Result<Vec<_>, _>>()?
                };
                self.max_store_read_page
                    .set(self.max_store_read_page.get().max(keys.len()));
                if keys.is_empty() {
                    break;
                }
                let insert = format!(
                    "WITH incoming(version_id,local_id) AS (VALUES {})
                     INSERT INTO phase_keys(phase,version_id,local_id)
                     SELECT ?,incoming.version_id,incoming.local_id FROM incoming",
                    key_values_clause(keys.len())
                );
                let mut bind = key_params(&keys);
                bind.push(rusqlite::types::Value::Integer(code));
                transaction.execute(&insert, rusqlite::params_from_iter(bind))?;
                after = keys.last().cloned().expect("non-empty key page");
            }
            match phase {
                ResolutionPhase::Pending => {
                    transaction.execute(
                        "DELETE FROM phase_keys
                         WHERE phase=?1 AND EXISTS (
                           SELECT 1 FROM pending_resolutions AS written
                           WHERE written.version_id=phase_keys.version_id
                             AND written.pending_relationship_id=phase_keys.local_id
                         )",
                        [code],
                    )?;
                }
                ResolutionPhase::Identifiers => {
                    transaction.execute(
                        "DELETE FROM phase_keys
                         WHERE phase=?1 AND EXISTS (
                           SELECT 1 FROM identifier_resolutions AS written
                           WHERE written.version_id=phase_keys.version_id
                             AND written.identifier_id=phase_keys.local_id
                         )",
                        [code],
                    )?;
                }
                _ => {}
            }
        }
        transaction.execute("INSERT INTO phase_ready(phase) VALUES (?1)", [code])?;
        transaction.commit()?;
        Ok(())
    }

    fn load_pending_page(
        &self,
        keys: &[(i64, String)],
    ) -> Result<Vec<PendingWorkItem>, StoreResolutionError> {
        let connection = self.open_phase_reader()?;
        let sql = format!(
            "WITH wanted(version_id,local_id) AS (VALUES {})
             SELECT pr.version_id,pr.pending_relationship_id,pr.from_symbol_id,pr.caller_scope_symbol_id,
                    pr.path,fv.language,pr.kind,pr.target_display_name,pr.target_terminal_name,
                    pr.target_receiver,pr.target_namespace_json,pr.target_import_context,
                    pr.start_line,pr.start_byte,pr.end_byte,pr.confidence
             FROM wanted
             JOIN pending_relationships AS pr
               ON pr.version_id=wanted.version_id
              AND pr.pending_relationship_id=wanted.local_id
             JOIN file_versions AS fv ON fv.version_id=pr.version_id
             ORDER BY pr.version_id,pr.pending_relationship_id COLLATE BINARY",
            key_values_clause(keys.len())
        );
        let mut statement = connection.prepare(&sql)?;
        let keyed_rows = statement
            .query_map(rusqlite::params_from_iter(key_params(keys)), |row| {
                Ok((
                    (row.get::<_, i64>(0)?, row.get::<_, String>(1)?),
                    PendingWorkItem {
                        pending_relationship_id: row.get(1)?,
                        from_symbol_id: row.get(2)?,
                        caller_scope_symbol_id: row.get(3)?,
                        file_id: row.get::<_, i64>(0)?.to_string(),
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
                        confidence: row.get(15)?,
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        self.validate_hydrated_keys("pending", keys, &keyed_rows)?;
        Ok(keyed_rows.into_iter().map(|(_, row)| row).collect())
    }

    fn load_relationship_page(
        &self,
        keys: &[(i64, String)],
    ) -> Result<Vec<crate::resolution_session::SessionRelationship>, StoreResolutionError> {
        let connection = self.open_phase_reader()?;
        let sql = format!(
            "WITH wanted(version_id,local_id) AS (VALUES {})
             SELECT r.version_id,r.relationship_id,r.to_symbol_id,r.kind,r.start_line,r.start_byte,
                    r.end_byte,r.confidence
             FROM wanted
             JOIN relationships AS r
               ON r.version_id=wanted.version_id AND r.relationship_id=wanted.local_id
             ORDER BY r.version_id,r.relationship_id COLLATE BINARY",
            key_values_clause(keys.len())
        );
        let mut statement = connection.prepare(&sql)?;
        let keyed_rows = statement
            .query_map(rusqlite::params_from_iter(key_params(keys)), |row| {
                Ok((
                    (row.get::<_, i64>(0)?, row.get::<_, String>(1)?),
                    crate::resolution_session::SessionRelationship {
                        target_symbol_id: SemanticSymbolId {
                            version: SemanticVersionId::Store(row.get(0)?),
                            local_id: row.get(2)?,
                        },
                        source_version_id: SemanticVersionId::Store(row.get(0)?),
                        kind: row.get(3)?,
                        start_line: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                        start_byte: row.get(5)?,
                        end_byte: row.get(6)?,
                        confidence: row.get(7)?,
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        self.validate_hydrated_keys("relationships", keys, &keyed_rows)?;
        Ok(keyed_rows.into_iter().map(|(_, row)| row).collect())
    }

    fn load_identifier_page(
        &self,
        keys: &[(i64, String)],
    ) -> Result<Vec<IdentifierWorkItem>, StoreResolutionError> {
        let connection = self.open_phase_reader()?;
        let sql = format!(
            "WITH wanted(version_id,local_id) AS (VALUES {})
             SELECT i.version_id,i.identifier_id,i.path,i.language,i.name,i.kind,
                    i.containing_symbol_id,i.start_line,i.start_byte,i.end_byte,
                    json_extract(i.metadata_json,'$.receiver'),
                    json_extract(i.metadata_json,'$.receiver_qualifier'),
                    json_extract(i.metadata_json,'$.import_context'),i.confidence
             FROM wanted
             JOIN identifiers AS i
               ON i.version_id=wanted.version_id AND i.identifier_id=wanted.local_id
             ORDER BY i.version_id,i.identifier_id COLLATE BINARY",
            key_values_clause(keys.len())
        );
        let mut statement = connection.prepare(&sql)?;
        let keyed_rows = statement
            .query_map(rusqlite::params_from_iter(key_params(keys)), |row| {
                Ok((
                    (row.get::<_, i64>(0)?, row.get::<_, String>(1)?),
                    IdentifierWorkItem {
                        identifier_id: row.get(1)?,
                        file_id: row.get::<_, i64>(0)?.to_string(),
                        path: row.get(2)?,
                        language: row.get(3)?,
                        name: row.get(4)?,
                        kind: row.get(5)?,
                        containing_symbol_id: row.get(6)?,
                        start_line: row.get(7)?,
                        start_byte: row.get(8)?,
                        end_byte: row.get(9)?,
                        receiver: row.get(10)?,
                        receiver_qualifier: row.get(11)?,
                        import_context: row.get(12)?,
                        confidence: row.get(13)?,
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        self.validate_hydrated_keys("identifiers", keys, &keyed_rows)?;
        Ok(keyed_rows.into_iter().map(|(_, row)| row).collect())
    }

    fn validate_hydrated_keys<T>(
        &self,
        phase: &'static str,
        expected: &[(i64, String)],
        actual: &[((i64, String), T)],
    ) -> Result<(), StoreResolutionError> {
        self.max_store_read_page
            .set(self.max_store_read_page.get().max(actual.len()));
        if actual.len() != expected.len() || actual.iter().map(|(key, _)| key).ne(expected.iter()) {
            return Err(StoreResolutionError::PhaseHydrationMismatch {
                phase,
                expected: expected.len(),
                actual: actual.len(),
            });
        }
        Ok(())
    }

    fn workspace_gated_languages(
        &self,
    ) -> Result<std::collections::BTreeSet<String>, StoreResolutionError> {
        let connection = self.open_reader()?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT fv.language
             FROM pending_relationships AS pr
             JOIN file_versions AS fv ON fv.version_id=pr.version_id
             WHERE EXISTS (
               SELECT 1 FROM manifest_entries AS me
               WHERE me.view_id=?1 AND me.generation=?2
                 AND me.status IN ('indexed','failed_preserved')
                 AND me.version_id=pr.version_id
             ) ORDER BY fv.language COLLATE BINARY",
        )?;
        let mut gated = std::collections::BTreeSet::new();
        for language in statement.query_map(
            params![self.identity.view_id, self.identity.generation],
            |row| row.get::<_, String>(0),
        )? {
            let language = language?;
            if !resolution::tier2_enabled(&language) {
                gated.insert(language);
            }
        }
        Ok(gated)
    }

    fn select_module_version(
        &self,
        candidates: &[String],
        language: &str,
    ) -> Result<Option<String>, StoreResolutionError> {
        for candidate in candidates {
            let connection = self.open_reader()?;
            let version_id = connection
                .query_row(
                    "SELECT me.version_id
                     FROM manifest_entries AS me
                     JOIN file_versions AS fv ON fv.version_id = me.version_id
                     WHERE me.view_id = ?1 AND me.generation = ?2
                       AND me.status IN ('indexed', 'failed_preserved')
                       AND me.path = ?3 AND fv.language = ?4
                     LIMIT 1",
                    params![
                        self.identity.view_id,
                        self.identity.generation,
                        candidate,
                        language
                    ],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            if let Some(version_id) = version_id {
                return Ok(Some(version_id.to_string()));
            }
        }
        Ok(None)
    }
}

fn parse_source_key(source_key: &str) -> Result<i64, StoreResolutionError> {
    source_key
        .parse()
        .map_err(|_| StoreResolutionError::InvalidIdentity)
}

fn store_version(version: &SemanticVersionId) -> Result<i64, StoreResolutionError> {
    match version {
        SemanticVersionId::Store(version_id) if *version_id > 0 => Ok(*version_id),
        _ => Err(StoreResolutionError::InvalidIdentity),
    }
}

fn phase_code(phase: ResolutionPhase) -> i64 {
    match phase {
        ResolutionPhase::ResolvedPending => 1,
        ResolutionPhase::PropagationCovered => 2,
        ResolutionPhase::ResolvedIdentifiers => 3,
        ResolutionPhase::Pending => 4,
        ResolutionPhase::Relationships => 5,
        ResolutionPhase::Identifiers => 6,
        ResolutionPhase::PropagationOwned => 7,
        ResolutionPhase::WorkspaceGated => 8,
    }
}

fn key_values_clause(count: usize) -> String {
    std::iter::repeat_n("(?,?)", count)
        .collect::<Vec<_>>()
        .join(",")
}

fn key_params(keys: &[(i64, String)]) -> Vec<rusqlite::types::Value> {
    keys.iter()
        .flat_map(|(version_id, local_id)| {
            [
                rusqlite::types::Value::Integer(*version_id),
                rusqlite::types::Value::Text(local_id.clone()),
            ]
        })
        .collect()
}

fn candidate_hit(row: &Row<'_>) -> rusqlite::Result<Option<CandidateHit>> {
    let version_id: i64 = row.get(0)?;
    let symbol_id: String = row.get(1)?;
    let language: String = row.get(2)?;
    let name: String = row.get(3)?;
    let kind: String = row.get(4)?;
    let Some(kind) = SymbolKind::try_from_string(&kind) else {
        return Ok(None);
    };
    let metadata_json: Option<String> = row.get(8)?;
    Ok(Some(CandidateHit {
        semantic_id: SemanticSymbolId {
            version: SemanticVersionId::Store(version_id),
            local_id: symbol_id.clone(),
        },
        symbol: CandidateSymbol {
            symbol_id,
            file_id: version_id.to_string(),
            language,
            name,
            kind,
            parent_symbol_id: row.get(5)?,
            visibility: row.get(6)?,
            signature: row.get(7)?,
            is_static: resolution::parse_is_static_metadata(metadata_json.as_deref()),
        },
    }))
}

impl Drop for StoreScratchResolutionSession {
    fn drop(&mut self) {
        let _ = self.remove_scratch();
    }
}
