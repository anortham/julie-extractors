use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::Instant;

use julie_extract_artifact::resolution_store::{
    IdentifierWorkItem, Outcome, PendingWorkItem, ResolutionCounts, ResolutionReportRow,
    ResolutionStatus,
};
use julie_extract_artifact::store::{
    ResolutionBaseWriter, ResolutionFileIdentity, ResolutionIdentifierRow, ResolutionPendingRow,
    ResolutionScopeState, ResolutionValidationError, StoreLayout,
    create_resolution_scratch_connection, resolution_scope_state,
};
use julie_extract_artifact::store::{StoreConnectionError, StoreConnectionFactory};
use julie_extractors::SymbolKind;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
#[cfg(feature = "test-store-resolution-contract")]
use serde::{Deserialize, Serialize};

use crate::resolution::{
    self, CandidateEvidence, CandidateHit, CandidateLookup, CandidateSummary, CandidateSymbol,
    EdgeOrigin, ImportRecord, ReferenceKind, TierOutcome, TypeFact, UnresolvedEdge,
};
use crate::resolution_session::{
    ResolutionCorpusIdentity, ResolutionPassRequest, ResolutionPhase, ResolutionPhaseChunk,
    ResolutionSession, ResolutionWorklists, ResolutionWrite, ResolutionWriteBatch,
    SemanticIdentifierId, SessionResolvedIdentifierWorkItem, SessionResolvedPendingWorkItem,
};
use crate::resolution_session::{SemanticSymbolId, SemanticVersionId};
use crate::store::delta_scope::{
    StoreDeltaScopeDecision, StoreDeltaScopeRequest, build_store_delta_scope,
};
use crate::store::prior_overlay::{
    PriorOverlayAccess, PriorOverlayKey, PriorOverlayPage, PriorOverlayReader,
};

const MAX_STORE_RESOLUTION_WINDOW: usize = 300;
type CandidatePage = (Vec<CandidateHit>, Option<(i64, String)>);
type PriorPhaseKeys = (Vec<(i64, String)>, Option<PriorOverlayKey>);
type PriorPhaseAccess = PriorOverlayAccess<PriorPhaseKeys>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum CandidateQueryFamily {
    PrimeWindow,
    SymbolById,
    ByName,
    FilteredByName,
    FilteredNameSummary,
    ChildrenNamed,
    TopLevelNamed,
    TypeFacts,
    Imports,
    ModuleVersion,
    LocateIdentifier,
    PendingHydration,
    RelationshipHydration,
    IdentifierHydration,
}

impl CandidateQueryFamily {
    const COUNT: usize = 14;

    const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CandidateQueryTelemetry {
    pub executions: usize,
    pub rows_read: usize,
}

#[cfg(feature = "test-store-resolution-contract")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PropagationCoverageTelemetry {
    pub reader_opens: usize,
    pub pending_query_executions: usize,
    pub pending_candidate_rows_read: usize,
    pub materialized_query_executions: usize,
    pub materialized_candidate_rows_read: usize,
}

#[derive(Debug, Clone)]
struct PropagationLocator {
    name: String,
    start_line: i64,
    start_byte: i64,
    end_byte: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinishExactBoundary {
    PriorOverlay,
    IdentifierTotality,
    WriterInit,
    SourceVersions,
    IdentifierRows,
    PendingRows,
    WriterFinish,
    ScratchCleanup,
}

#[cfg(feature = "test-store-resolution-contract")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishExactPhase {
    PriorOverlay,
    IdentifierTotality,
    WriterInit,
    SourceVersions,
    IdentifierRows,
    PendingRows,
    WriterFinish,
    ScratchCleanup,
}

#[cfg(feature = "test-store-resolution-contract")]
impl From<FinishExactBoundary> for FinishExactPhase {
    fn from(boundary: FinishExactBoundary) -> Self {
        match boundary {
            FinishExactBoundary::PriorOverlay => Self::PriorOverlay,
            FinishExactBoundary::IdentifierTotality => Self::IdentifierTotality,
            FinishExactBoundary::WriterInit => Self::WriterInit,
            FinishExactBoundary::SourceVersions => Self::SourceVersions,
            FinishExactBoundary::IdentifierRows => Self::IdentifierRows,
            FinishExactBoundary::PendingRows => Self::PendingRows,
            FinishExactBoundary::WriterFinish => Self::WriterFinish,
            FinishExactBoundary::ScratchCleanup => Self::ScratchCleanup,
        }
    }
}

#[cfg(feature = "test-store-resolution-contract")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishExactPhaseSample {
    pub phase: FinishExactPhase,
    pub cumulative_micros: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct StoreResolutionDecisionTelemetry {
    pub(crate) effective_full: bool,
    pub(crate) fallback_reason: Option<&'static str>,
    pub(crate) worklists: ResolutionWorklists,
    pub(crate) elapsed_millis: u64,
}

#[derive(Debug, Default)]
struct CandidateWindow {
    primed_names: BTreeSet<String>,
    by_name: HashMap<String, Vec<CandidateHit>>,
    by_id: HashMap<SemanticSymbolId, Option<CandidateHit>>,
    module_versions: HashMap<(Vec<String>, String), Option<String>>,
}

impl CandidateWindow {
    fn entry_count(&self) -> usize {
        self.by_name.values().map(Vec::len).sum::<usize>()
            + self.by_id.len()
            + self.module_versions.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FilteredSummaryKey {
    name: String,
    language: String,
    kinds: Vec<String>,
    version_id: Option<i64>,
    confidence_bits: u64,
}

#[derive(Debug, Default)]
struct TierCandidateAccumulator {
    buffered: BTreeMap<SemanticSymbolId, f64>,
    spilled: bool,
}

#[derive(Debug)]
struct BoundedCache<K, V> {
    capacity: usize,
    order: VecDeque<K>,
    values: HashMap<K, V>,
}

impl<K, V> BoundedCache<K, V>
where
    K: Clone + Eq + std::hash::Hash,
{
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::new(),
            values: HashMap::new(),
        }
    }

    fn get(&self, key: &K) -> Option<&V> {
        self.values.get(key)
    }

    fn insert(&mut self, key: K, value: V) {
        if let Some(existing) = self.values.get_mut(&key) {
            *existing = value;
            return;
        }
        if self.values.len() == self.capacity
            && let Some(evicted) = self.order.pop_front()
        {
            self.values.remove(&evicted);
        }
        self.order.push_back(key.clone());
        self.values.insert(key, value);
    }
}

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

fn incremental_error(detail: impl Into<String>) -> StoreResolutionError {
    StoreResolutionError::Artifact(ResolutionValidationError::InvalidMetadata {
        key: "incremental_resolution".to_string(),
        value: detail.into(),
    })
}

fn propagation_locator_matches(
    locator: &PropagationLocator,
    start_line: i64,
    start_byte: Option<i64>,
    end_byte: Option<i64>,
) -> bool {
    if let (Some(start_byte), Some(end_byte)) = (start_byte, end_byte) {
        locator.start_byte >= start_byte
            && locator.start_byte <= end_byte
            && locator.end_byte <= end_byte
    } else {
        locator.start_line == start_line
    }
}

#[derive(Debug)]
pub struct StoreScratchResolutionSession {
    reader_factory: StoreConnectionFactory,
    layout: StoreLayout,
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
    max_candidate_cache_entries: Cell<usize>,
    candidate_query_telemetry: Cell<[CandidateQueryTelemetry; CandidateQueryFamily::COUNT]>,
    #[cfg(feature = "test-store-resolution-contract")]
    propagation_coverage_telemetry: Cell<PropagationCoverageTelemetry>,
    visible_root_batches: usize,
    candidate_reader: RefCell<Option<Connection>>,
    candidate_window: RefCell<CandidateWindow>,
    filtered_summaries: RefCell<BoundedCache<FilteredSummaryKey, CandidateSummary>>,
    tier_candidates: RefCell<TierCandidateAccumulator>,
    resolution_cache: RefCell<HashMap<ResolutionLookupKey, TierOutcome>>,
    prior_overlay: Option<PriorOverlayReader>,
    prior_scope_state: Option<ResolutionScopeState>,
    decision_telemetry: Option<StoreResolutionDecisionTelemetry>,
    forced_full_without_prior_state: bool,
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
        let layout = store_layout_from_factory(&reader_factory)?;
        let mut session = Self {
            reader_factory,
            layout,
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
            max_candidate_cache_entries: Cell::new(0),
            candidate_query_telemetry: Cell::new(
                [CandidateQueryTelemetry::default(); CandidateQueryFamily::COUNT],
            ),
            #[cfg(feature = "test-store-resolution-contract")]
            propagation_coverage_telemetry: Cell::new(PropagationCoverageTelemetry::default()),
            visible_root_batches: 0,
            candidate_reader: RefCell::new(None),
            candidate_window: RefCell::new(CandidateWindow::default()),
            filtered_summaries: RefCell::new(BoundedCache::new(window_size)),
            tier_candidates: RefCell::new(TierCandidateAccumulator::default()),
            resolution_cache: RefCell::new(HashMap::new()),
            prior_overlay: None,
            prior_scope_state: None,
            decision_telemetry: None,
            forced_full_without_prior_state: false,
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

    pub(crate) fn decision_telemetry(&self) -> Option<&StoreResolutionDecisionTelemetry> {
        self.decision_telemetry.as_ref()
    }

    pub(crate) fn force_full_without_prior_state(&mut self) {
        self.forced_full_without_prior_state = true;
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

    pub fn max_candidate_cache_entries(&self) -> usize {
        self.max_candidate_cache_entries.get()
    }

    pub fn candidate_query_telemetry(
        &self,
        family: CandidateQueryFamily,
    ) -> CandidateQueryTelemetry {
        self.candidate_query_telemetry.get()[family.index()]
    }

    #[cfg(feature = "test-store-resolution-contract")]
    pub fn propagation_coverage_telemetry(&self) -> PropagationCoverageTelemetry {
        self.propagation_coverage_telemetry.get()
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

    pub fn finish_exact(self) -> Result<ResolutionFileIdentity, StoreResolutionError> {
        self.finish_exact_inner(|_| {})
    }

    #[cfg(feature = "test-store-resolution-contract")]
    pub fn finish_exact_observing(
        self,
        mut observer: impl FnMut(FinishExactPhaseSample),
    ) -> Result<ResolutionFileIdentity, StoreResolutionError> {
        let started = Instant::now();
        let mut observer_micros = 0u128;
        self.finish_exact_inner(|boundary| {
            let cumulative_micros = started
                .elapsed()
                .as_micros()
                .saturating_sub(observer_micros);
            let observer_started = Instant::now();
            observer(FinishExactPhaseSample {
                phase: boundary.into(),
                cumulative_micros: u64::try_from(cumulative_micros).unwrap_or(u64::MAX),
            });
            observer_micros =
                observer_micros.saturating_add(observer_started.elapsed().as_micros());
        })
    }

    fn finish_exact_inner(
        mut self,
        mut completed: impl FnMut(FinishExactBoundary),
    ) -> Result<ResolutionFileIdentity, StoreResolutionError> {
        self.materialize_prior_overlay()?;
        completed(FinishExactBoundary::PriorOverlay);
        self.validate_identifier_totality()?;
        completed(FinishExactBoundary::IdentifierTotality);
        let mut writer = ResolutionBaseWriter::new(
            &self.exact_path,
            self.identity.manifest_hash.clone(),
            self.resolver_output_epoch,
        )?;
        completed(FinishExactBoundary::WriterInit);
        {
            let mut statement = self
                .scratch
                .prepare("SELECT version_id FROM visible_versions ORDER BY version_id")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                writer.push_source_version(row.get(0)?)?;
            }
        }
        completed(FinishExactBoundary::SourceVersions);
        {
            let mut statement = self.scratch.prepare(
                "SELECT resolved.version_id,resolved.identifier_id,resolved.target_version_id,
                        resolved.target_symbol_id,resolved.tier,resolved.confidence,
                        resolved.method,resolved.outcome,resolved.candidates
                 FROM identifier_resolutions AS resolved
                 JOIN visible_versions AS visible ON visible.version_id=resolved.version_id
                 ORDER BY resolved.version_id,resolved.identifier_id",
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
        completed(FinishExactBoundary::IdentifierRows);
        {
            let mut statement = self.scratch.prepare(
                "SELECT resolved.version_id,resolved.pending_relationship_id,
                        resolved.target_version_id,resolved.target_symbol_id,
                        resolved.tier,resolved.confidence,resolved.method
                 FROM pending_resolutions AS resolved
                 JOIN visible_versions AS visible ON visible.version_id=resolved.version_id
                 ORDER BY resolved.version_id,resolved.pending_relationship_id",
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
        completed(FinishExactBoundary::PendingRows);
        let identity = self.identity.clone();
        let connection = self.reader_factory.open_reader().map_err(|error| {
            ResolutionValidationError::InvalidMetadata {
                key: "store_reader".to_string(),
                value: error.to_string(),
            }
        })?;
        let mut target_exists = connection
            .prepare(
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
            )
            .map_err(ResolutionValidationError::Sqlite)?;
        let result = writer.finish_with_target_lookup(move |version_id, symbol_id| {
            let exists = target_exists
                .query_row(
                    params![version_id, symbol_id, identity.view_id, identity.generation],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(ResolutionValidationError::Sqlite)?;
            Ok(exists)
        })?;
        completed(FinishExactBoundary::WriterFinish);
        self.remove_scratch()?;
        completed(FinishExactBoundary::ScratchCleanup);
        Ok(result)
    }

    fn open_reader(&self) -> Result<Connection, StoreResolutionError> {
        Ok(self.reader_factory.open_reader()?)
    }

    fn open_propagation_coverage_reader(&self) -> Result<Connection, StoreResolutionError> {
        #[cfg(feature = "test-store-resolution-contract")]
        self.propagation_coverage_telemetry.update(|mut telemetry| {
            telemetry.reader_opens = telemetry.reader_opens.saturating_add(1);
            telemetry
        });
        self.open_reader()
    }

    #[cfg(feature = "test-store-resolution-contract")]
    fn record_propagation_pending_query(&self) {
        self.propagation_coverage_telemetry.update(|mut telemetry| {
            telemetry.pending_query_executions =
                telemetry.pending_query_executions.saturating_add(1);
            telemetry
        });
    }

    #[cfg(feature = "test-store-resolution-contract")]
    fn record_propagation_pending_candidate_rows(&self, rows: usize) {
        self.propagation_coverage_telemetry.update(|mut telemetry| {
            telemetry.pending_candidate_rows_read =
                telemetry.pending_candidate_rows_read.saturating_add(rows);
            telemetry
        });
    }

    #[cfg(feature = "test-store-resolution-contract")]
    fn record_propagation_materialized_query(&self) {
        self.propagation_coverage_telemetry.update(|mut telemetry| {
            telemetry.materialized_query_executions =
                telemetry.materialized_query_executions.saturating_add(1);
            telemetry
        });
    }

    #[cfg(feature = "test-store-resolution-contract")]
    fn record_propagation_materialized_candidate_rows(&self, rows: usize) {
        self.propagation_coverage_telemetry.update(|mut telemetry| {
            telemetry.materialized_candidate_rows_read = telemetry
                .materialized_candidate_rows_read
                .saturating_add(rows);
            telemetry
        });
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

    fn record_candidate_query(&self, family: CandidateQueryFamily, rows_read: usize) {
        let mut telemetry = self.candidate_query_telemetry.get();
        let family = &mut telemetry[family.index()];
        family.executions = family.executions.saturating_add(1);
        family.rows_read = family.rows_read.saturating_add(rows_read);
        self.candidate_query_telemetry.set(telemetry);
    }

    fn reset_candidate_window(&mut self) -> Result<(), StoreResolutionError> {
        let reader = self.open_phase_reader()?;
        *self.candidate_reader.get_mut() = Some(reader);
        *self.candidate_window.get_mut() = CandidateWindow::default();
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
             CREATE TABLE identifier_touched(
               version_id INTEGER NOT NULL,
               identifier_id TEXT NOT NULL,
               PRIMARY KEY(version_id,identifier_id)
             ) STRICT;
             CREATE TABLE pending_touched(
               version_id INTEGER NOT NULL,
               pending_relationship_id TEXT NOT NULL,
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

    fn materialize_prior_overlay(&mut self) -> Result<(), StoreResolutionError> {
        let Some(state) = self.prior_scope_state.clone() else {
            return Ok(());
        };
        let base_path = self
            .layout
            .generation_dir()
            .join("bases")
            .join(format!("{}.db", state.base_id));
        self.scratch.execute(
            "ATTACH DATABASE ?1 AS prior_store",
            [sqlite_read_only_uri(self.layout.store_db(), false)?],
        )?;
        self.scratch.execute(
            "ATTACH DATABASE ?1 AS prior_base",
            [sqlite_read_only_uri(&base_path, true)?],
        )?;
        let copy_result = (|| -> Result<(), StoreResolutionError> {
            let transaction = self.scratch.transaction()?;
            validate_frozen_prior_overlay(&transaction, &state)?;
            transaction.execute(
                "INSERT OR IGNORE INTO identifier_resolutions
                 (version_id,identifier_id,target_version_id,target_symbol_id,tier,confidence,
                  method,outcome,candidates)
                 SELECT source.version_id,source.identifier_id,
                        CASE WHEN delta.version_id IS NOT NULL
                             THEN delta.target_version_id ELSE base.target_version_id END,
                        CASE WHEN delta.version_id IS NOT NULL
                             THEN delta.target_symbol_id ELSE base.target_symbol_id END,
                        CASE WHEN delta.version_id IS NOT NULL THEN delta.tier ELSE base.tier END,
                        CASE WHEN delta.version_id IS NOT NULL
                             THEN delta.confidence ELSE base.confidence END,
                        CASE WHEN delta.version_id IS NOT NULL THEN delta.method ELSE base.method END,
                        CASE WHEN delta.version_id IS NOT NULL THEN delta.outcome ELSE base.outcome END,
                        CASE WHEN delta.version_id IS NOT NULL
                             THEN delta.candidates ELSE base.candidates END
                 FROM prior_store.identifiers AS source
                 JOIN visible_versions AS visible ON visible.version_id=source.version_id
                 JOIN prior_store.manifest_entries AS predecessor
                   ON predecessor.view_id=?1 AND predecessor.generation=?2
                  AND predecessor.version_id=source.version_id
                 LEFT JOIN prior_base.identifier_resolutions AS base
                   ON base.version_id=source.version_id
                  AND base.identifier_id=source.identifier_id
                 LEFT JOIN prior_store.resolution_identifier_deltas AS delta
                   ON delta.view_id=?1 AND delta.delta_generation=?3
                  AND delta.version_id=source.version_id
                  AND delta.identifier_id=source.identifier_id
                 LEFT JOIN identifier_touched AS touched
                   ON touched.version_id=source.version_id
                  AND touched.identifier_id=source.identifier_id
                 WHERE touched.identifier_id IS NULL
                   AND (delta.version_id IS NOT NULL OR base.version_id IS NOT NULL)",
                params![
                    state.view_id,
                    state.predecessor_manifest_generation,
                    state.delta_generation
                ],
            )?;
            transaction.execute(
                "INSERT OR IGNORE INTO pending_resolutions
                 (version_id,pending_relationship_id,target_version_id,target_symbol_id,tier,
                  confidence,method)
                 SELECT source.version_id,source.pending_relationship_id,
                        CASE WHEN delta.operation='replace'
                             THEN delta.target_version_id ELSE base.target_version_id END,
                        CASE WHEN delta.operation='replace'
                             THEN delta.target_symbol_id ELSE base.target_symbol_id END,
                        CASE WHEN delta.operation='replace' THEN delta.tier ELSE base.tier END,
                        CASE WHEN delta.operation='replace'
                             THEN delta.confidence ELSE base.confidence END,
                        CASE WHEN delta.operation='replace' THEN delta.method ELSE base.method END
                 FROM prior_store.pending_relationships AS source
                 JOIN visible_versions AS visible ON visible.version_id=source.version_id
                 JOIN prior_store.manifest_entries AS predecessor
                   ON predecessor.view_id=?1 AND predecessor.generation=?2
                  AND predecessor.version_id=source.version_id
                 LEFT JOIN prior_base.pending_resolutions AS base
                   ON base.version_id=source.version_id
                  AND base.pending_relationship_id=source.pending_relationship_id
                 LEFT JOIN prior_store.resolution_pending_deltas AS delta
                   ON delta.view_id=?1 AND delta.delta_generation=?3
                  AND delta.version_id=source.version_id
                  AND delta.pending_relationship_id=source.pending_relationship_id
                 LEFT JOIN pending_touched AS touched
                   ON touched.version_id=source.version_id
                  AND touched.pending_relationship_id=source.pending_relationship_id
                 WHERE touched.pending_relationship_id IS NULL
                   AND (delta.operation='replace'
                        OR (delta.operation IS NULL AND base.version_id IS NOT NULL))",
                params![
                    state.view_id,
                    state.predecessor_manifest_generation,
                    state.delta_generation
                ],
            )?;
            transaction.commit()?;
            Ok(())
        })();
        let detach_base = self.scratch.execute("DETACH DATABASE prior_base", []);
        let detach_store = self.scratch.execute("DETACH DATABASE prior_store", []);
        copy_result?;
        detach_base?;
        detach_store?;
        Ok(())
    }

    fn validate_identifier_totality(&self) -> Result<(), StoreResolutionError> {
        let connection = self.open_reader()?;
        let mut after = (0, String::new());
        loop {
            let keys = {
                let mut statement = connection.prepare(
                    "SELECT i.version_id,i.identifier_id
                     FROM identifiers AS i
                     WHERE EXISTS (
                       SELECT 1 FROM manifest_entries AS me
                       WHERE me.view_id=?1 AND me.generation=?2
                         AND me.status IN ('indexed','failed_preserved')
                         AND me.version_id=i.version_id
                     ) AND (i.version_id,i.identifier_id)>(?3,?4)
                     ORDER BY i.version_id,i.identifier_id COLLATE BINARY LIMIT ?5",
                )?;
                statement
                    .query_map(
                        params![
                            self.identity.view_id,
                            self.identity.generation,
                            after.0,
                            after.1,
                            self.sql_window_limit()?
                        ],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                    )?
                    .collect::<Result<Vec<_>, _>>()?
            };
            if keys.is_empty() {
                break;
            }
            for (version_id, identifier_id) in &keys {
                let exists: bool = self.scratch.query_row(
                    "SELECT EXISTS(SELECT 1 FROM identifier_resolutions WHERE version_id=?1 AND identifier_id=?2)",
                    params![version_id, identifier_id],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(StoreResolutionError::Artifact(
                        ResolutionValidationError::IdentifierTotalityViolation {
                            version_id: *version_id,
                            identifier_id: identifier_id.clone(),
                        },
                    ));
                }
            }
            after = keys.last().cloned().expect("non-empty identifier page");
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
        let semantic_id = SemanticSymbolId {
            version: SemanticVersionId::Store(version_id),
            local_id: local_id.to_string(),
        };
        if let Some(hit) = self.candidate_window.borrow().by_id.get(&semantic_id) {
            return Ok(hit.clone());
        }
        let row = self.with_candidate_reader(|connection| {
            Ok(connection
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
                .optional()?)
        })?;
        self.record_candidate_query(CandidateQueryFamily::SymbolById, usize::from(row.is_some()));
        let hit = row.flatten();
        let mut window = self.candidate_window.borrow_mut();
        if window.by_id.len() < self.window_size {
            window.by_id.insert(semantic_id, hit.clone());
        }
        self.max_candidate_cache_entries.set(
            self.max_candidate_cache_entries
                .get()
                .max(window.entry_count()),
        );
        Ok(hit)
    }

    fn visit_by_name<F>(&self, name: &str, mut visitor: F) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, Self::Error>,
    {
        if let Some(hits) = self.cached_name_hits(name) {
            for hit in hits {
                if !visitor(self, hit)? {
                    break;
                }
            }
            return Ok(());
        }
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
                CandidateQueryFamily::ByName,
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

    fn visit_filtered_by_name<F>(
        &self,
        name: &str,
        language: &str,
        kinds: &[SymbolKind],
        source_key: Option<&str>,
        mut visitor: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, Self::Error>,
    {
        if let Some(hits) = self.cached_name_hits(name) {
            for hit in hits.into_iter().filter(|candidate| {
                candidate.symbol.language == language
                    && kinds.contains(&candidate.symbol.kind)
                    && source_key.is_none_or(|source_key| candidate.symbol.file_id == source_key)
            }) {
                if !visitor(self, hit)? {
                    break;
                }
            }
            return Ok(());
        }
        let version_id = source_key.map(parse_source_key).transpose()?;
        let kind_values = (0..kinds.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let version_filter = version_id.map_or("", |_| "AND s.version_id=?");
        let sql = format!(
            "SELECT s.version_id,s.symbol_id,s.language,s.name,s.kind,
                    s.parent_symbol_id,s.visibility,s.signature,s.metadata_json
             FROM symbols AS s
             WHERE s.name=? AND s.language=? AND s.kind IN ({kind_values})
               {version_filter}
               AND EXISTS (
                 SELECT 1 FROM manifest_entries AS me
                 WHERE me.view_id=? AND me.generation=?
                   AND me.status IN ('indexed','failed_preserved')
                   AND me.version_id=s.version_id
               )
               AND (s.version_id,s.symbol_id)>(?,?)
             ORDER BY s.version_id,s.symbol_id COLLATE BINARY LIMIT ?"
        );
        let mut after = (0, String::new());
        loop {
            let mut bind = vec![name.to_string().into(), language.to_string().into()];
            bind.extend(
                kinds
                    .iter()
                    .map(|kind| rusqlite::types::Value::Text(kind.to_string())),
            );
            if let Some(version_id) = version_id {
                bind.push(version_id.into());
            }
            bind.push(self.identity.view_id.clone().into());
            bind.push(self.identity.generation.into());
            bind.push(after.0.into());
            bind.push(after.1.clone().into());
            bind.push(self.sql_window_limit()?.into());
            let rows = self.with_candidate_reader(|connection| {
                let mut statement = connection.prepare(&sql)?;
                statement
                    .query_map(rusqlite::params_from_iter(bind), |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            candidate_hit(row)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(StoreResolutionError::from)
            })?;
            self.record_candidate_query(CandidateQueryFamily::FilteredByName, rows.len());
            self.max_store_read_page
                .set(self.max_store_read_page.get().max(rows.len()));
            if rows.is_empty() {
                break;
            }
            let mut last = None;
            for (version_id, symbol_id, hit) in rows {
                last = Some((version_id, symbol_id));
                if let Some(hit) = hit
                    && !visitor(self, hit)?
                {
                    return Ok(());
                }
            }
            let Some(next) = last else {
                break;
            };
            after = next;
        }
        Ok(())
    }

    fn filtered_name_summary(
        &self,
        name: &str,
        language: &str,
        kinds: &[SymbolKind],
        source_key: Option<&str>,
        confidence: f64,
    ) -> Result<CandidateSummary, Self::Error> {
        if let Some(hits) = self.cached_name_hits(name) {
            let candidates = hits
                .into_iter()
                .filter(|candidate| {
                    candidate.symbol.language == language
                        && kinds.contains(&candidate.symbol.kind)
                        && source_key
                            .is_none_or(|source_key| candidate.symbol.file_id == source_key)
                })
                .map(|candidate| (candidate.semantic_id, confidence))
                .collect::<BTreeMap<_, _>>();
            return Ok(CandidateSummary {
                evidence: candidates
                    .iter()
                    .take(2)
                    .map(|(semantic_id, confidence)| CandidateEvidence {
                        semantic_id: semantic_id.clone(),
                        confidence: *confidence,
                    })
                    .collect(),
                exact_count: candidates.len() as u64,
            });
        }
        let version_id = source_key.map(parse_source_key).transpose()?;
        let key = FilteredSummaryKey {
            name: name.to_string(),
            language: language.to_string(),
            kinds: kinds.iter().map(ToString::to_string).collect(),
            version_id,
            confidence_bits: confidence.to_bits(),
        };
        let cached_summary = self.filtered_summaries.borrow().get(&key).cloned();
        if let Some(summary) = cached_summary {
            return Ok(summary);
        }
        let kind_values = (0..kinds.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let version_filter = version_id.map_or("", |_| "AND s.version_id=?");
        let sql = format!(
            "SELECT s.version_id,s.symbol_id,COUNT(*) OVER()
             FROM symbols AS s
             WHERE s.name=? AND s.language=? AND s.kind IN ({kind_values})
               {version_filter}
               AND EXISTS (
                 SELECT 1 FROM manifest_entries AS me
                 WHERE me.view_id=? AND me.generation=?
                   AND me.status IN ('indexed','failed_preserved')
                   AND me.version_id=s.version_id
               )
             ORDER BY s.version_id,s.symbol_id COLLATE BINARY LIMIT 2"
        );
        let mut bind = vec![name.to_string().into(), language.to_string().into()];
        bind.extend(
            kinds
                .iter()
                .map(|kind| rusqlite::types::Value::Text(kind.to_string())),
        );
        if let Some(version_id) = version_id {
            bind.push(version_id.into());
        }
        bind.push(self.identity.view_id.clone().into());
        bind.push(self.identity.generation.into());
        let rows = self.with_candidate_reader(|connection| {
            let mut statement = connection.prepare(&sql)?;
            statement
                .query_map(rusqlite::params_from_iter(bind), |row| {
                    Ok((
                        SemanticSymbolId {
                            version: SemanticVersionId::Store(row.get(0)?),
                            local_id: row.get(1)?,
                        },
                        row.get::<_, i64>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(StoreResolutionError::from)
        })?;
        self.record_candidate_query(CandidateQueryFamily::FilteredNameSummary, rows.len());
        self.max_store_read_page
            .set(self.max_store_read_page.get().max(rows.len()));
        let summary = CandidateSummary {
            evidence: rows
                .iter()
                .map(|(semantic_id, _)| CandidateEvidence {
                    semantic_id: semantic_id.clone(),
                    confidence,
                })
                .collect(),
            exact_count: rows.first().map_or(0, |(_, count)| *count as u64),
        };
        self.filtered_summaries
            .borrow_mut()
            .insert(key, summary.clone());
        Ok(summary)
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
        if let Some(hits) = self.cached_name_hits(name) {
            for hit in hits.into_iter().filter(|hit| {
                hit.semantic_id.version == SemanticVersionId::Store(version_id)
                    && hit.symbol.parent_symbol_id.as_deref() == Some(parent_id)
            }) {
                if !visitor(self, hit)? {
                    break;
                }
            }
            return Ok(());
        }
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
                CandidateQueryFamily::ChildrenNamed,
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
        if let Some(hits) = self.cached_name_hits(name) {
            for hit in hits.into_iter().filter(|hit| {
                hit.semantic_id.version == SemanticVersionId::Store(version_id)
                    && hit.symbol.parent_symbol_id.is_none()
            }) {
                if !visitor(self, hit)? {
                    break;
                }
            }
            return Ok(());
        }
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
                CandidateQueryFamily::TopLevelNamed,
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
            let page = self.with_candidate_reader(|connection| {
                let mut statement = connection.prepare(sql)?;
                Ok(statement
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
                    .collect::<Result<Vec<_>, _>>()?)
            })?;
            self.record_candidate_query(CandidateQueryFamily::TypeFacts, page.len());
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
            let page = self.with_candidate_reader(|connection| {
                let mut statement = connection.prepare(sql)?;
                Ok(statement
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
                    .collect::<Result<Vec<_>, _>>()?)
            })?;
            self.record_candidate_query(CandidateQueryFamily::Imports, page.len());
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
        let mut accumulator = self.tier_candidates.borrow_mut();
        if accumulator.spilled {
            self.scratch
                .execute("DELETE FROM tier_candidate_accumulator", [])?;
        }
        accumulator.buffered.clear();
        accumulator.spilled = false;
        Ok(())
    }

    fn record_tier_candidate(
        &self,
        semantic_id: SemanticSymbolId,
        confidence: f64,
    ) -> Result<(), Self::Error> {
        let mut accumulator = self.tier_candidates.borrow_mut();
        if let Some(stored) = accumulator.buffered.get_mut(&semantic_id) {
            *stored = stored.max(confidence);
            return Ok(());
        }
        if accumulator.buffered.len() == self.window_size {
            accumulator.spilled = true;
            self.flush_tier_candidate_buffer(&mut accumulator)?;
        }
        accumulator.buffered.insert(semantic_id, confidence);
        Ok(())
    }

    fn tier_candidate_summary(&self) -> Result<CandidateSummary, Self::Error> {
        let mut accumulator = self.tier_candidates.borrow_mut();
        if !accumulator.spilled {
            return Ok(CandidateSummary {
                evidence: accumulator
                    .buffered
                    .iter()
                    .take(2)
                    .map(|(semantic_id, confidence)| CandidateEvidence {
                        semantic_id: semantic_id.clone(),
                        confidence: *confidence,
                    })
                    .collect(),
                exact_count: accumulator.buffered.len() as u64,
            });
        }
        self.flush_tier_candidate_buffer(&mut accumulator)?;
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
        if self.forced_full_without_prior_state {
            return Ok(None);
        }
        #[cfg(feature = "test-store-resolution-contract")]
        julie_extract_artifact::store::test_hooks::crash_if("resolution_prior_state_read");
        let Some(state) = self.prepare_prior_overlay()? else {
            return Ok(None);
        };
        Ok(Some(crate::resolution_session::SessionResolutionState {
            status: ResolutionStatus::Complete,
            version: resolution::RESOLUTION_VERSION,
            last_full_revision: state.predecessor_manifest_generation,
        }))
    }

    fn current_revision(&mut self) -> Result<i64, Self::Error> {
        Ok(self.resolver_output_epoch)
    }

    fn open_resolution_pass(
        &mut self,
        request: &ResolutionPassRequest,
    ) -> Result<ResolutionWorklists, Self::Error> {
        let started = Instant::now();
        self.decision_telemetry = None;
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
        if request.full {
            self.prior_overlay = None;
            self.prior_scope_state = None;
            let worklists = ResolutionWorklists {
                effective_full: true,
                ..ResolutionWorklists::default()
            };
            self.decision_telemetry = Some(StoreResolutionDecisionTelemetry {
                effective_full: true,
                fallback_reason: Some("resolution_requested_full"),
                worklists: worklists.clone(),
                elapsed_millis: elapsed_millis(started),
            });
            return Ok(worklists);
        }
        let connection = self.open_reader()?;
        let decision = build_store_delta_scope(
            &connection,
            StoreDeltaScopeRequest {
                view_id: &self.identity.view_id,
                manifest_generation: self.identity.generation,
                manifest_hash: &self.identity.manifest_hash,
                resolver_output_epoch: self.resolver_output_epoch,
                incremental_enabled: true,
            },
        )
        .map_err(|error| incremental_error(error.to_string()))?;
        let _effective_full = decision.worklists().effective_full;
        match decision {
            StoreDeltaScopeDecision::Scoped(worklists) => {
                let stored_state = resolution_scope_state(&connection, &self.identity.view_id)
                    .map_err(|error| incremental_error(error.to_string()))?;
                if self.prepare_prior_overlay()?.is_some() && stored_state == self.prior_scope_state
                {
                    self.decision_telemetry = Some(StoreResolutionDecisionTelemetry {
                        effective_full: false,
                        fallback_reason: None,
                        worklists: worklists.clone(),
                        elapsed_millis: elapsed_millis(started),
                    });
                    Ok(worklists)
                } else {
                    self.prior_overlay = None;
                    self.prior_scope_state = None;
                    let worklists = ResolutionWorklists {
                        effective_full: true,
                        ..ResolutionWorklists::default()
                    };
                    self.decision_telemetry = Some(StoreResolutionDecisionTelemetry {
                        effective_full: true,
                        fallback_reason: Some("resolution_prior_overlay_unavailable"),
                        worklists: worklists.clone(),
                        elapsed_millis: elapsed_millis(started),
                    });
                    Ok(worklists)
                }
            }
            StoreDeltaScopeDecision::Full { worklists, reason } => {
                self.prior_overlay = None;
                self.prior_scope_state = None;
                self.decision_telemetry = Some(StoreResolutionDecisionTelemetry {
                    effective_full: true,
                    fallback_reason: Some(reason.as_str()),
                    worklists: worklists.clone(),
                    elapsed_millis: elapsed_millis(started),
                });
                Ok(worklists)
            }
        }
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
        Ok(self
            .symbol_by_id(&version_id.to_string(), &symbol_id.local_id)?
            .map(|hit| hit.symbol.name))
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
        let (identifier_id, rows_read) = self.with_candidate_reader(|connection| {
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
            Ok((identifier_id, ids.len()))
        })?;
        self.record_candidate_query(CandidateQueryFamily::LocateIdentifier, rows_read);
        Ok(identifier_id)
    }

    fn identifier_is_covered(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        self.effective_identifier_exists(identifier_id)
    }

    fn propagation_is_covered(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        Ok(self.propagating_pending_exists(identifier_id)?
            || self.materialized_relationship_covers(identifier_id)?)
    }

    fn propagation_is_covered_batch(
        &mut self,
        identifiers: &[SemanticIdentifierId],
    ) -> Result<HashSet<SemanticIdentifierId>, Self::Error> {
        if identifiers.is_empty() {
            return Ok(HashSet::new());
        }
        let mut key_indices = HashMap::<(i64, String), Vec<usize>>::new();
        let mut unique_keys = BTreeSet::new();
        for (index, identifier) in identifiers.iter().enumerate() {
            let key = (
                store_version(&identifier.version)?,
                identifier.local_id.clone(),
            );
            key_indices.entry(key.clone()).or_default().push(index);
            unique_keys.insert(key);
        }
        let unique_keys = unique_keys.into_iter().collect::<Vec<_>>();
        let mut covered = vec![false; identifiers.len()];
        let connection = self.open_propagation_coverage_reader()?;
        self.propagating_pending_covers_batch(
            &connection,
            &unique_keys,
            &key_indices,
            &mut covered,
        )?;
        let uncovered = unique_keys
            .iter()
            .filter(|key| key_indices[*key].iter().any(|index| !covered[*index]))
            .cloned()
            .collect::<Vec<_>>();
        if !uncovered.is_empty() {
            self.materialized_relationship_covers_batch(
                &connection,
                &uncovered,
                &key_indices,
                &mut covered,
            )?;
        }
        Ok(identifiers
            .iter()
            .zip(covered)
            .filter_map(|(identifier, is_covered)| is_covered.then(|| identifier.clone()))
            .collect())
    }

    fn propagation_is_owned(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        self.effective_identifier_exists(identifier_id)
    }

    fn next_phase_chunk(
        &mut self,
        worklists: &ResolutionWorklists,
    ) -> Result<Option<ResolutionPhaseChunk>, Self::Error> {
        if self.active_phase != Some(worklists.phase) {
            self.freeze_phase(worklists)?;
            self.active_phase = Some(worklists.phase);
            self.phase_after = None;
        }
        if matches!(
            worklists.phase,
            ResolutionPhase::PropagationCovered | ResolutionPhase::PropagationOwned
        ) {
            return Ok(None);
        }
        if matches!(
            worklists.phase,
            ResolutionPhase::ResolvedPending | ResolutionPhase::ResolvedIdentifiers
        ) {
            self.prune_touched_prior_phase(worklists.phase)?;
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
        let chunk = match worklists.phase {
            ResolutionPhase::ResolvedPending => Some(ResolutionPhaseChunk::ResolvedPending(
                self.load_resolved_pending_page(&keys)?,
            )),
            ResolutionPhase::ResolvedIdentifiers => {
                Some(ResolutionPhaseChunk::ResolvedIdentifiers(
                    self.load_resolved_identifier_page(&keys)?,
                ))
            }
            ResolutionPhase::Pending => Some(ResolutionPhaseChunk::Pending(
                self.load_pending_page(&keys)?,
            )),
            ResolutionPhase::Relationships => Some(ResolutionPhaseChunk::Relationships(
                self.load_relationship_page(&keys)?,
            )),
            ResolutionPhase::Identifiers => Some(ResolutionPhaseChunk::Identifiers(
                self.load_identifier_page(&keys)?,
            )),
            _ => None,
        };
        if let Some(chunk) = &chunk {
            self.prime_candidate_window(chunk)?;
        }
        Ok(chunk)
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
            let mut pending_touch = transaction.prepare(
                "INSERT OR IGNORE INTO pending_touched(version_id,pending_relationship_id) VALUES (?1,?2)",
            )?;
            let mut identifier_touch = transaction.prepare(
                "INSERT OR IGNORE INTO identifier_touched(version_id,identifier_id) VALUES (?1,?2)",
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
                        pending_touch
                            .execute(params![version_id, pending_relationship_id.local_id])?;
                    }
                    ResolutionWrite::DemotePending {
                        pending_relationship_id,
                    } => {
                        let version_id = store_version(&pending_relationship_id.version)?;
                        pending_delete
                            .execute(params![version_id, pending_relationship_id.local_id])?;
                        pending_touch
                            .execute(params![version_id, pending_relationship_id.local_id])?;
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
                        let version_id = store_version(&identifier_id.version)?;
                        identifier_upsert.execute(params![
                            version_id,
                            identifier_id.local_id,
                            target_version_id,
                            target_symbol_id.map(|id| id.local_id),
                            tier,
                            confidence,
                            method,
                            outcome.as_str(),
                            candidates
                        ])?;
                        identifier_touch.execute(params![version_id, identifier_id.local_id])?;
                    }
                    ResolutionWrite::DemoteIdentifier { identifier_id } => {
                        let version_id = store_version(&identifier_id.version)?;
                        identifier_delete.execute(params![version_id, identifier_id.local_id])?;
                        identifier_touch.execute(params![version_id, identifier_id.local_id])?;
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
    fn cached_name_hits(&self, name: &str) -> Option<Vec<CandidateHit>> {
        let window = self.candidate_window.borrow();
        window
            .primed_names
            .contains(name)
            .then(|| window.by_name.get(name).cloned().unwrap_or_default())
    }

    fn flush_tier_candidate_buffer(
        &self,
        accumulator: &mut TierCandidateAccumulator,
    ) -> Result<(), StoreResolutionError> {
        if accumulator.buffered.is_empty() {
            return Ok(());
        }
        self.scratch
            .execute_batch("SAVEPOINT tier_candidate_flush")?;
        let result = (|| {
            let mut insert = self.scratch.prepare_cached(
                "INSERT INTO tier_candidate_accumulator(version_id,symbol_id,confidence)
                 VALUES (?1,?2,?3)
                 ON CONFLICT(version_id,symbol_id) DO UPDATE SET
                   confidence=MAX(confidence,excluded.confidence)",
            )?;
            for (semantic_id, confidence) in &accumulator.buffered {
                insert.execute(params![
                    store_version(&semantic_id.version)?,
                    semantic_id.local_id,
                    confidence
                ])?;
            }
            Ok::<_, StoreResolutionError>(())
        })();
        if result.is_ok() {
            self.scratch.execute_batch("RELEASE tier_candidate_flush")?;
            accumulator.buffered.clear();
            return Ok(());
        }
        let _ = self
            .scratch
            .execute_batch("ROLLBACK TO tier_candidate_flush; RELEASE tier_candidate_flush");
        result
    }

    fn prime_candidate_window(
        &self,
        chunk: &ResolutionPhaseChunk,
    ) -> Result<(), StoreResolutionError> {
        let names = match chunk {
            ResolutionPhaseChunk::Pending(items) => items
                .iter()
                .map(|item| item.target_terminal_name.clone())
                .collect::<BTreeSet<_>>(),
            ResolutionPhaseChunk::Identifiers(items) => items
                .iter()
                .map(|item| item.name.clone())
                .collect::<BTreeSet<_>>(),
            _ => BTreeSet::new(),
        };
        if names.is_empty() {
            return Ok(());
        }
        let values = (0..names.len())
            .map(|_| "(?)")
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "WITH wanted(name) AS (VALUES {values})
             SELECT s.version_id,s.symbol_id,s.language,s.name,s.kind,
                    s.parent_symbol_id,s.visibility,s.signature,s.metadata_json
             FROM wanted
             JOIN symbols AS s ON s.name=wanted.name
             WHERE EXISTS (
               SELECT 1 FROM manifest_entries AS me
               WHERE me.view_id=? AND me.generation=?
                 AND me.status IN ('indexed','failed_preserved')
                 AND me.version_id=s.version_id
             )
             ORDER BY s.name COLLATE BINARY,s.version_id,s.symbol_id COLLATE BINARY
             LIMIT ?"
        );
        let mut bind = names
            .iter()
            .cloned()
            .map(rusqlite::types::Value::Text)
            .collect::<Vec<_>>();
        bind.push(self.identity.view_id.clone().into());
        bind.push(self.identity.generation.into());
        bind.push(self.sql_window_limit()?.into());
        let rows = self.with_candidate_reader(|connection| {
            let mut statement = connection.prepare(&sql)?;
            statement
                .query_map(rusqlite::params_from_iter(bind), |row| {
                    Ok((row.get::<_, String>(3)?, candidate_hit(row)?))
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(StoreResolutionError::from)
        })?;
        self.record_candidate_query(CandidateQueryFamily::PrimeWindow, rows.len());
        self.max_store_read_page
            .set(self.max_store_read_page.get().max(rows.len()));
        let cutoff = (rows.len() == self.window_size)
            .then(|| rows.last().expect("full candidate page").0.clone());
        let mut window = self.candidate_window.borrow_mut();
        for name in names {
            if cutoff.as_ref().is_none_or(|cutoff| name < *cutoff) {
                window.primed_names.insert(name);
            }
        }
        for (name, hit) in rows {
            if !window.primed_names.contains(&name) {
                continue;
            }
            if let Some(hit) = hit {
                window
                    .by_id
                    .insert(hit.semantic_id.clone(), Some(hit.clone()));
                window.by_name.entry(name).or_default().push(hit);
            }
        }
        self.max_candidate_cache_entries.set(
            self.max_candidate_cache_entries
                .get()
                .max(window.entry_count()),
        );
        Ok(())
    }

    fn candidate_page(
        &self,
        family: CandidateQueryFamily,
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
            self.record_candidate_query(family, page_rows);
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

    fn prepare_prior_overlay(
        &mut self,
    ) -> Result<Option<ResolutionScopeState>, StoreResolutionError> {
        if let Some(state) = &self.prior_scope_state {
            return Ok(Some(state.clone()));
        }
        let connection = self.open_reader()?;
        let state = resolution_scope_state(&connection, &self.identity.view_id)
            .map_err(|error| incremental_error(error.to_string()))?;
        let Some(state) = state else {
            return Ok(None);
        };
        if state.current_manifest_generation != self.identity.generation
            || state.current_manifest_hash != self.identity.manifest_hash
            || state.resolver_output_epoch != self.resolver_output_epoch
        {
            return Ok(None);
        }
        match PriorOverlayReader::open(&self.layout, &state)
            .map_err(|error| incremental_error(error.to_string()))?
        {
            PriorOverlayAccess::Ready(reader) => {
                self.prior_overlay = Some(reader);
                self.prior_scope_state = Some(state.clone());
                Ok(Some(state))
            }
            PriorOverlayAccess::FullFallback(_) => Ok(None),
        }
    }

    fn effective_identifier_exists(
        &self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, StoreResolutionError> {
        let version_id = store_version(&identifier_id.version)?;
        let touched: bool = self.scratch.query_row(
            "SELECT EXISTS(SELECT 1 FROM identifier_touched WHERE version_id=?1 AND identifier_id=?2)",
            params![version_id, identifier_id.local_id],
            |row| row.get(0),
        )?;
        if touched {
            return self.scratch_identifier_exists(identifier_id);
        }
        let Some(prior) = &self.prior_overlay else {
            return Ok(false);
        };
        match prior
            .identifier(version_id, &identifier_id.local_id)
            .map_err(|error| incremental_error(error.to_string()))?
        {
            PriorOverlayAccess::Ready(row) => Ok(row.is_some()),
            PriorOverlayAccess::FullFallback(fallback) => Err(incremental_error(format!(
                "prior overlay changed during resolution: {fallback:?}"
            ))),
        }
    }

    fn propagating_pending_exists(
        &self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, StoreResolutionError> {
        let version_id = store_version(&identifier_id.version)?;
        let connection = self.open_propagation_coverage_reader()?;
        let sql = "SELECT pr.pending_relationship_id
             FROM identifiers AS i
             JOIN pending_relationships AS pr
               ON pr.version_id=i.version_id AND pr.target_terminal_name=i.name
              AND ((pr.start_byte IS NOT NULL AND pr.end_byte IS NOT NULL
                    AND i.start_byte>=pr.start_byte AND i.start_byte<=pr.end_byte
                    AND i.end_byte<=pr.end_byte)
                   OR ((pr.start_byte IS NULL OR pr.end_byte IS NULL)
                       AND i.start_line=pr.start_line))
             WHERE i.version_id=?1 AND i.identifier_id=?2
               AND pr.pending_relationship_id>?3
             ORDER BY pr.pending_relationship_id COLLATE BINARY LIMIT ?4";
        let mut after = String::new();
        loop {
            #[cfg(feature = "test-store-resolution-contract")]
            self.record_propagation_pending_query();
            let ids = connection
                .prepare(sql)?
                .query_map(
                    params![
                        version_id,
                        identifier_id.local_id,
                        after,
                        self.sql_window_limit()?
                    ],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?;
            self.max_store_read_page
                .set(self.max_store_read_page.get().max(ids.len()));
            if ids.is_empty() {
                break;
            }
            for local_id in &ids {
                let touched: bool = self.scratch.query_row(
                    "SELECT EXISTS(SELECT 1 FROM pending_touched WHERE version_id=?1 AND pending_relationship_id=?2)",
                    params![version_id, local_id],
                    |row| row.get(0),
                )?;
                if touched {
                    let exists: bool = self.scratch.query_row(
                        "SELECT EXISTS(SELECT 1 FROM pending_resolutions WHERE version_id=?1 AND pending_relationship_id=?2)",
                        params![version_id, local_id],
                        |row| row.get(0),
                    )?;
                    if exists {
                        return Ok(true);
                    }
                } else if let Some(prior) = &self.prior_overlay {
                    match prior
                        .pending(version_id, local_id)
                        .map_err(|error| incremental_error(error.to_string()))?
                    {
                        PriorOverlayAccess::Ready(Some(_)) => return Ok(true),
                        PriorOverlayAccess::Ready(None) => {}
                        PriorOverlayAccess::FullFallback(fallback) => {
                            return Err(incremental_error(format!(
                                "prior overlay changed during resolution: {fallback:?}"
                            )));
                        }
                    }
                }
            }
            after = ids
                .last()
                .cloned()
                .expect("non-empty pending coverage page");
        }
        Ok(false)
    }

    fn materialized_relationship_covers(
        &self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, StoreResolutionError> {
        let version_id = store_version(&identifier_id.version)?;
        let connection = self.open_propagation_coverage_reader()?;
        let sql = "SELECT r.relationship_id,r.kind,target.name,r.start_line,r.start_byte,r.end_byte
                   FROM identifiers AS i
                   JOIN relationships AS r ON r.version_id=i.version_id
                   JOIN symbols AS target
                     ON target.version_id=r.version_id AND target.symbol_id=r.to_symbol_id
                   WHERE i.version_id=?1 AND i.identifier_id=?2
                     AND r.relationship_id>?3
                     AND EXISTS (
                       SELECT 1 FROM manifest_entries AS me
                       WHERE me.view_id=?4 AND me.generation=?5
                         AND me.status IN ('indexed','failed_preserved')
                         AND me.version_id=r.version_id
                     )
                   ORDER BY r.relationship_id COLLATE BINARY LIMIT ?6";
        let mut after = String::new();
        loop {
            #[cfg(feature = "test-store-resolution-contract")]
            self.record_propagation_materialized_query();
            let rows = connection
                .prepare(sql)?
                .query_map(
                    params![
                        version_id,
                        identifier_id.local_id,
                        after,
                        self.identity.view_id,
                        self.identity.generation,
                        self.sql_window_limit()?
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                            row.get::<_, Option<i64>>(4)?,
                            row.get::<_, Option<i64>>(5)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?;
            #[cfg(feature = "test-store-resolution-contract")]
            self.record_propagation_materialized_candidate_rows(rows.len());
            self.max_store_read_page
                .set(self.max_store_read_page.get().max(rows.len()));
            if rows.is_empty() {
                break;
            }
            for (_, kind, name, start_line, start_byte, end_byte) in &rows {
                if ReferenceKind::from_relationship_kind(kind).is_some()
                    && self.locate_identifier(
                        &identifier_id.version,
                        name,
                        *start_byte,
                        *end_byte,
                        *start_line,
                    )? == Some(identifier_id.local_id.clone())
                {
                    return Ok(true);
                }
            }
            after = rows
                .last()
                .expect("non-empty relationship coverage page")
                .0
                .clone();
        }
        Ok(false)
    }

    fn propagating_pending_covers_batch(
        &self,
        connection: &Connection,
        keys: &[(i64, String)],
        key_indices: &HashMap<(i64, String), Vec<usize>>,
        covered: &mut [bool],
    ) -> Result<(), StoreResolutionError> {
        let identifier_locators = self.propagation_locators(connection, keys)?;
        let mut locators_by_name =
            BTreeMap::<(i64, String), Vec<(String, PropagationLocator)>>::new();
        for key in keys {
            let Some(locator) = identifier_locators.get(key) else {
                continue;
            };
            locators_by_name
                .entry((key.0, locator.name.clone()))
                .or_default()
                .push((key.1.clone(), locator.clone()));
        }
        let sql = format!(
            "WITH wanted(version_id,identifier_id) AS (VALUES {}),
             requested_names(version_id,name) AS (
               SELECT DISTINCT i.version_id,i.name
               FROM wanted
               JOIN identifiers AS i
                 ON i.version_id=wanted.version_id AND i.identifier_id=wanted.identifier_id
             )
             SELECT pr.version_id,requested_names.name,pr.pending_relationship_id,
                    COALESCE(pr.start_line,0),pr.start_byte,pr.end_byte
             FROM requested_names
             JOIN pending_relationships AS pr
               ON pr.version_id=requested_names.version_id
              AND pr.target_terminal_name=requested_names.name
             WHERE (pr.version_id,pr.pending_relationship_id)>(?,?)
             ORDER BY pr.version_id,pr.pending_relationship_id COLLATE BINARY
             LIMIT ?",
            key_values_clause(keys.len())
        );
        let mut after = (0_i64, String::new());
        loop {
            let mut bind = key_params(keys);
            bind.extend([
                after.0.into(),
                after.1.clone().into(),
                self.sql_window_limit()?.into(),
            ]);
            #[cfg(feature = "test-store-resolution-contract")]
            self.record_propagation_pending_query();
            let rows = connection
                .prepare(&sql)?
                .query_map(rusqlite::params_from_iter(bind), |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            #[cfg(feature = "test-store-resolution-contract")]
            self.record_propagation_pending_candidate_rows(rows.len());
            self.max_store_read_page
                .set(self.max_store_read_page.get().max(rows.len()));
            if rows.is_empty() {
                break;
            }
            let page_len = rows.len();
            let last_cursor = rows
                .last()
                .map(|row| (row.0, row.2.clone()))
                .expect("non-empty pending coverage page");
            let matched_rows = rows
                .into_iter()
                .filter(|(version_id, name, _, start_line, start_byte, end_byte)| {
                    locators_by_name
                        .get(&(*version_id, name.clone()))
                        .is_some_and(|locators| {
                            locators.iter().any(|(_, locator)| {
                                propagation_locator_matches(
                                    locator,
                                    *start_line,
                                    *start_byte,
                                    *end_byte,
                                )
                            })
                        })
                })
                .collect::<Vec<_>>();
            let pending_keys = matched_rows
                .iter()
                .map(|(version_id, _, pending_id, _, _, _)| (*version_id, pending_id.clone()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let scratch_states = self.scratch_pending_states(&pending_keys)?;
            let prior_covered = self.prior_pending_coverage(&pending_keys, &scratch_states)?;
            for (version_id, name, pending_id, start_line, start_byte, end_byte) in matched_rows {
                let pending_key = (version_id, pending_id);
                let is_covered = match scratch_states.get(&pending_key) {
                    Some((true, resolved)) => *resolved,
                    Some((false, _)) | None => prior_covered.contains(&pending_key),
                };
                if is_covered {
                    if let Some(locators) = locators_by_name.get(&(version_id, name)) {
                        for (identifier_id, _) in locators.iter().filter(|(_, locator)| {
                            propagation_locator_matches(locator, start_line, start_byte, end_byte)
                        }) {
                            let identifier_key = (version_id, identifier_id.clone());
                            for index in key_indices
                                .get(&identifier_key)
                                .expect("pending coverage identifier is in the input chunk")
                            {
                                covered[*index] = true;
                            }
                        }
                    }
                }
            }
            if keys.iter().all(|key| {
                key_indices
                    .get(key)
                    .expect("coverage key is in the input chunk")
                    .iter()
                    .all(|index| covered[*index])
            }) {
                break;
            }
            if page_len < self.window_size {
                break;
            }
            after = last_cursor;
        }
        Ok(())
    }

    fn propagation_locators(
        &self,
        connection: &Connection,
        keys: &[(i64, String)],
    ) -> Result<HashMap<(i64, String), PropagationLocator>, StoreResolutionError> {
        let sql = format!(
            "WITH wanted(version_id,identifier_id) AS (VALUES {})
             SELECT i.version_id,i.identifier_id,i.name,i.start_line,i.start_byte,i.end_byte
             FROM wanted
             JOIN identifiers AS i
               ON i.version_id=wanted.version_id AND i.identifier_id=wanted.identifier_id
             ORDER BY i.version_id,i.identifier_id COLLATE BINARY",
            key_values_clause(keys.len())
        );
        let rows = connection
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(key_params(keys)), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    PropagationLocator {
                        name: row.get(2)?,
                        start_line: row.get(3)?,
                        start_byte: row.get(4)?,
                        end_byte: row.get(5)?,
                    },
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(version_id, identifier_id, locator)| ((version_id, identifier_id), locator))
            .collect())
    }

    fn scratch_pending_states(
        &self,
        keys: &[(i64, String)],
    ) -> Result<HashMap<(i64, String), (bool, bool)>, StoreResolutionError> {
        if keys.is_empty() {
            return Ok(HashMap::new());
        }
        let sql = format!(
            "WITH wanted(version_id,local_id) AS (VALUES {})
             SELECT wanted.version_id,wanted.local_id,
                    EXISTS(
                      SELECT 1 FROM pending_touched AS touched
                      WHERE touched.version_id=wanted.version_id
                        AND touched.pending_relationship_id=wanted.local_id
                    ),
                    EXISTS(
                      SELECT 1 FROM pending_resolutions AS resolved
                      WHERE resolved.version_id=wanted.version_id
                        AND resolved.pending_relationship_id=wanted.local_id
                    )
             FROM wanted",
            key_values_clause(keys.len())
        );
        let rows = self
            .scratch
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(key_params(keys)), |row| {
                Ok((
                    (row.get::<_, i64>(0)?, row.get::<_, String>(1)?),
                    (row.get::<_, bool>(2)?, row.get::<_, bool>(3)?),
                ))
            })?
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(rows)
    }

    fn prior_pending_coverage(
        &self,
        keys: &[(i64, String)],
        scratch_states: &HashMap<(i64, String), (bool, bool)>,
    ) -> Result<BTreeSet<(i64, String)>, StoreResolutionError> {
        let Some(prior) = self.prior_overlay.as_ref() else {
            return Ok(BTreeSet::new());
        };
        let mut untouched = keys
            .iter()
            .filter(|key| {
                !scratch_states
                    .get(*key)
                    .is_some_and(|(touched, _)| *touched)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut covered = BTreeSet::new();
        let chunk_size = self.window_size.min(256);
        for chunk in untouched.chunks_mut(chunk_size) {
            let prior_keys = chunk
                .iter()
                .map(|(version_id, local_id)| PriorOverlayKey::new(*version_id, local_id.clone()))
                .collect::<Vec<_>>();
            match prior
                .pending_by_keys(&prior_keys)
                .map_err(|error| incremental_error(error.to_string()))?
            {
                PriorOverlayAccess::Ready(rows) => {
                    covered.extend(
                        rows.into_iter()
                            .map(|row| (row.version_id, row.pending_relationship_id)),
                    );
                }
                PriorOverlayAccess::FullFallback(fallback) => {
                    return Err(incremental_error(format!(
                        "prior overlay changed during resolution: {fallback:?}"
                    )));
                }
            }
        }
        untouched.clear();
        Ok(covered)
    }

    fn materialized_relationship_covers_batch(
        &self,
        connection: &Connection,
        keys: &[(i64, String)],
        key_indices: &HashMap<(i64, String), Vec<usize>>,
        covered: &mut [bool],
    ) -> Result<(), StoreResolutionError> {
        let sql = format!(
            "WITH wanted(version_id,identifier_id) AS (VALUES {})
             SELECT wanted.version_id,wanted.identifier_id,r.relationship_id,r.kind
             FROM wanted
             JOIN identifiers AS i
               ON i.version_id=wanted.version_id AND i.identifier_id=wanted.identifier_id
             JOIN relationships AS r ON r.version_id=i.version_id
             JOIN symbols AS target
               ON target.version_id=r.version_id AND target.symbol_id=r.to_symbol_id
              AND target.name=i.name
              AND ((r.start_byte IS NOT NULL AND r.end_byte IS NOT NULL
                    AND i.start_byte>=r.start_byte AND i.start_byte<=r.end_byte
                    AND i.end_byte<=r.end_byte)
                   OR ((r.start_byte IS NULL OR r.end_byte IS NULL)
                       AND i.start_line=COALESCE(r.start_line,0)))
             WHERE (r.version_id,r.relationship_id,wanted.identifier_id)>(?,?,?)
               AND EXISTS (
                 SELECT 1 FROM manifest_entries AS manifest
                 WHERE manifest.view_id=? AND manifest.generation=?
                   AND manifest.status IN ('indexed','failed_preserved')
                   AND manifest.version_id=r.version_id
               )
               AND NOT EXISTS (
                 SELECT 1 FROM identifiers AS second
                 WHERE second.version_id=i.version_id
                   AND second.identifier_id<>i.identifier_id
                   AND second.name=i.name
                   AND ((r.start_byte IS NOT NULL AND r.end_byte IS NOT NULL
                         AND second.start_byte>=r.start_byte
                         AND second.start_byte<=r.end_byte
                         AND second.end_byte<=r.end_byte)
                        OR ((r.start_byte IS NULL OR r.end_byte IS NULL)
                            AND second.start_line=COALESCE(r.start_line,0)))
               )
             ORDER BY r.version_id,r.relationship_id COLLATE BINARY,
                      wanted.identifier_id COLLATE BINARY
             LIMIT ?",
            key_values_clause(keys.len())
        );
        let mut after = (0_i64, String::new(), String::new());
        loop {
            let mut bind = key_params(keys);
            bind.extend([
                after.0.into(),
                after.1.clone().into(),
                after.2.clone().into(),
                self.identity.view_id.clone().into(),
                self.identity.generation.into(),
                self.sql_window_limit()?.into(),
            ]);
            #[cfg(feature = "test-store-resolution-contract")]
            self.record_propagation_materialized_query();
            let rows = connection
                .prepare(&sql)?
                .query_map(rusqlite::params_from_iter(bind), |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            #[cfg(feature = "test-store-resolution-contract")]
            self.record_propagation_materialized_candidate_rows(rows.len());
            self.max_store_read_page
                .set(self.max_store_read_page.get().max(rows.len()));
            if rows.is_empty() {
                break;
            }
            for (version_id, identifier_id, _relationship_id, kind) in &rows {
                if ReferenceKind::from_relationship_kind(kind).is_none() {
                    continue;
                }
                let identifier_key = (*version_id, identifier_id.clone());
                for index in key_indices
                    .get(&identifier_key)
                    .expect("materialized coverage identifier is in the input chunk")
                {
                    covered[*index] = true;
                }
            }
            if keys.iter().all(|key| {
                key_indices
                    .get(key)
                    .expect("coverage key is in the input chunk")
                    .iter()
                    .all(|index| covered[*index])
            }) {
                break;
            }
            if rows.len() < self.window_size {
                break;
            }
            let last = rows.last().expect("non-empty materialized coverage page");
            after = (last.0, last.2.clone(), last.1.clone());
        }
        Ok(())
    }

    fn freeze_prior_phase(
        &mut self,
        worklists: &ResolutionWorklists,
    ) -> Result<(), StoreResolutionError> {
        let code = phase_code(worklists.phase);
        let versions = store_versions(&worklists.selected_versions)?;
        let prior = self
            .prior_overlay
            .as_ref()
            .ok_or_else(|| incremental_error("prior overlay unavailable"))?;
        let transaction = self.scratch.transaction()?;
        let (touched_table, touched_id) = match worklists.phase {
            ResolutionPhase::ResolvedPending => ("pending_touched", "pending_relationship_id"),
            ResolutionPhase::ResolvedIdentifiers => ("identifier_touched", "identifier_id"),
            _ => unreachable!(),
        };
        let insert_sql = format!(
            "INSERT OR IGNORE INTO phase_keys(phase,version_id,local_id)
             SELECT ?1,?2,?3
             WHERE EXISTS (SELECT 1 FROM visible_versions WHERE version_id=?2)
               AND NOT EXISTS (
                 SELECT 1 FROM {touched_table}
                 WHERE version_id=?2 AND {touched_id}=?3
               )"
        );
        let mut insert = transaction.prepare_cached(&insert_sql)?;
        for names in worklists.recheck_names.chunks(256) {
            let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
            let mut after = None;
            loop {
                let access = match worklists.phase {
                    ResolutionPhase::ResolvedPending => prior
                        .pending_by_names(&name_refs, after.as_ref(), self.window_size)
                        .map(|page| {
                            map_prior_page(page, |row| {
                                (row.version_id, row.pending_relationship_id)
                            })
                        }),
                    ResolutionPhase::ResolvedIdentifiers => prior
                        .identifiers_by_names(&name_refs, after.as_ref(), self.window_size)
                        .map(|page| {
                            map_prior_page(page, |row| (row.version_id, row.identifier_id))
                        }),
                    _ => unreachable!(),
                }
                .map_err(|error| incremental_error(error.to_string()))?;
                let (rows, next) = prior_key_rows(access)?;
                for (version_id, local_id) in rows {
                    insert.execute(params![code, version_id, local_id])?;
                }
                let Some(next) = next else { break };
                after = Some(next);
            }
        }
        for versions in versions.chunks(256) {
            let mut after = None;
            loop {
                let access = match worklists.phase {
                    ResolutionPhase::ResolvedPending => prior
                        .pending_by_files(versions, after.as_ref(), self.window_size)
                        .map(|page| {
                            map_prior_page(page, |row| {
                                (row.version_id, row.pending_relationship_id)
                            })
                        }),
                    ResolutionPhase::ResolvedIdentifiers => prior
                        .identifiers_by_files(versions, after.as_ref(), self.window_size)
                        .map(|page| {
                            map_prior_page(page, |row| (row.version_id, row.identifier_id))
                        }),
                    _ => unreachable!(),
                }
                .map_err(|error| incremental_error(error.to_string()))?;
                let (rows, next) = prior_key_rows(access)?;
                for (version_id, local_id) in rows {
                    insert.execute(params![code, version_id, local_id])?;
                }
                let Some(next) = next else { break };
                after = Some(next);
            }
        }
        drop(insert);
        transaction.commit()?;
        Ok(())
    }

    fn prune_touched_prior_phase(
        &mut self,
        phase: ResolutionPhase,
    ) -> Result<(), StoreResolutionError> {
        let code = phase_code(phase);
        let (touched_table, touched_id) = match phase {
            ResolutionPhase::ResolvedPending => ("pending_touched", "pending_relationship_id"),
            ResolutionPhase::ResolvedIdentifiers => ("identifier_touched", "identifier_id"),
            _ => return Ok(()),
        };
        self.scratch.execute(
            &format!(
                "DELETE FROM phase_keys
                 WHERE phase=?1 AND EXISTS (
                   SELECT 1 FROM {touched_table}
                   WHERE version_id=phase_keys.version_id
                     AND {touched_id}=phase_keys.local_id
                 )"
            ),
            [code],
        )?;
        Ok(())
    }

    fn freeze_phase(
        &mut self,
        worklists: &ResolutionWorklists,
    ) -> Result<(), StoreResolutionError> {
        let phase = worklists.phase;
        let code = phase_code(phase);
        if self.scratch.query_row(
            "SELECT EXISTS(SELECT 1 FROM phase_ready WHERE phase=?1)",
            [code],
            |row| row.get::<_, bool>(0),
        )? {
            return Ok(());
        }
        if !worklists.effective_full
            && matches!(
                phase,
                ResolutionPhase::ResolvedPending | ResolutionPhase::ResolvedIdentifiers
            )
        {
            self.freeze_prior_phase(worklists)?;
            self.scratch
                .execute("INSERT INTO phase_ready(phase) VALUES (?1)", [code])?;
            return Ok(());
        }
        let table = match phase {
            ResolutionPhase::Pending => Some(("pending_relationships", "pending_relationship_id")),
            ResolutionPhase::Relationships => Some(("relationships", "relationship_id")),
            ResolutionPhase::Identifiers => Some(("identifiers", "identifier_id")),
            _ => None,
        };
        let phase_reader = if table.is_some() {
            self.phase_reader_opens
                .set(self.phase_reader_opens.get() + 1);
            Some(self.reader_factory.open_reader()?)
        } else {
            None
        };
        let transaction = self.scratch.transaction()?;
        if let Some((table, id_column)) = table {
            let connection = phase_reader.as_ref().expect("phase reader initialized");
            if worklists.effective_full {
                freeze_source_query(
                    connection,
                    &transaction,
                    &self.identity,
                    self.window_size,
                    code,
                    table,
                    id_column,
                    None,
                    &[],
                )?;
            } else {
                let versions = match phase {
                    ResolutionPhase::Relationships => &worklists.changed_versions,
                    ResolutionPhase::Pending | ResolutionPhase::Identifiers => {
                        &worklists.recheck_versions
                    }
                    _ => &worklists.recheck_versions,
                };
                for values in versions.chunks(256) {
                    let values = store_versions(values)?
                        .into_iter()
                        .map(rusqlite::types::Value::Integer)
                        .collect::<Vec<_>>();
                    freeze_source_query(
                        connection,
                        &transaction,
                        &self.identity,
                        self.window_size,
                        code,
                        table,
                        id_column,
                        None,
                        &values,
                    )?;
                }
                if matches!(
                    phase,
                    ResolutionPhase::Pending | ResolutionPhase::Identifiers
                ) {
                    let name_column = if phase == ResolutionPhase::Pending {
                        "target_terminal_name"
                    } else {
                        "name"
                    };
                    for names in worklists.recheck_names.chunks(256) {
                        let names = names
                            .iter()
                            .cloned()
                            .map(rusqlite::types::Value::Text)
                            .collect::<Vec<_>>();
                        freeze_source_query(
                            connection,
                            &transaction,
                            &self.identity,
                            self.window_size,
                            code,
                            table,
                            id_column,
                            Some(name_column),
                            &names,
                        )?;
                    }
                }
                if phase == ResolutionPhase::Identifiers {
                    let mut insert = transaction.prepare_cached(
                        "INSERT OR IGNORE INTO phase_keys(phase,version_id,local_id) VALUES (?1,?2,?3)",
                    )?;
                    for (identifier, _) in &worklists.repair_identifiers {
                        insert.execute(params![
                            code,
                            store_version(&identifier.version)?,
                            identifier.local_id
                        ])?;
                    }
                }
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
        if !worklists.effective_full
            && matches!(
                phase,
                ResolutionPhase::Pending | ResolutionPhase::Identifiers
            )
        {
            self.prune_effectively_covered_phase(phase)?;
        }
        Ok(())
    }

    fn prune_effectively_covered_phase(
        &mut self,
        phase: ResolutionPhase,
    ) -> Result<(), StoreResolutionError> {
        let code = phase_code(phase);
        let mut after = (0, String::new());
        loop {
            let keys = {
                let mut statement = self.scratch.prepare(
                    "SELECT version_id,local_id FROM phase_keys
                     WHERE phase=?1 AND (version_id,local_id)>(?2,?3)
                     ORDER BY version_id,local_id COLLATE BINARY LIMIT ?4",
                )?;
                statement
                    .query_map(
                        params![code, after.0, after.1, self.sql_window_limit()?],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                    )?
                    .collect::<Result<Vec<_>, _>>()?
            };
            if keys.is_empty() {
                break;
            }
            let mut covered = Vec::new();
            for (version_id, local_id) in &keys {
                let is_covered = if phase == ResolutionPhase::Identifiers {
                    self.effective_identifier_exists(&SemanticIdentifierId {
                        version: SemanticVersionId::Store(*version_id),
                        local_id: local_id.clone(),
                    })?
                } else {
                    self.effective_pending_exists(*version_id, local_id)?
                };
                if is_covered {
                    covered.push((*version_id, local_id.clone()));
                }
            }
            let transaction = self.scratch.transaction()?;
            {
                let mut delete = transaction.prepare_cached(
                    "DELETE FROM phase_keys WHERE phase=?1 AND version_id=?2 AND local_id=?3",
                )?;
                for (version_id, local_id) in covered {
                    delete.execute(params![code, version_id, local_id])?;
                }
            }
            transaction.commit()?;
            after = keys.last().cloned().expect("non-empty phase key page");
        }
        Ok(())
    }

    fn effective_pending_exists(
        &self,
        version_id: i64,
        local_id: &str,
    ) -> Result<bool, StoreResolutionError> {
        let touched: bool = self.scratch.query_row(
            "SELECT EXISTS(SELECT 1 FROM pending_touched WHERE version_id=?1 AND pending_relationship_id=?2)",
            params![version_id, local_id],
            |row| row.get(0),
        )?;
        if touched {
            return Ok(self.scratch.query_row(
                "SELECT EXISTS(SELECT 1 FROM pending_resolutions WHERE version_id=?1 AND pending_relationship_id=?2)",
                params![version_id, local_id],
                |row| row.get(0),
            )?);
        }
        let Some(prior) = &self.prior_overlay else {
            return Ok(false);
        };
        match prior
            .pending(version_id, local_id)
            .map_err(|error| incremental_error(error.to_string()))?
        {
            PriorOverlayAccess::Ready(row) => Ok(row.is_some()),
            PriorOverlayAccess::FullFallback(fallback) => Err(incremental_error(format!(
                "prior overlay changed during resolution: {fallback:?}"
            ))),
        }
    }

    fn load_pending_page(
        &self,
        keys: &[(i64, String)],
    ) -> Result<Vec<PendingWorkItem>, StoreResolutionError> {
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
        let keyed_rows = self.with_candidate_reader(|connection| {
            let mut statement = connection.prepare(&sql)?;
            Ok(statement
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
                .collect::<Result<Vec<_>, _>>()?)
        })?;
        self.record_candidate_query(CandidateQueryFamily::PendingHydration, keyed_rows.len());
        self.validate_hydrated_keys("pending", keys, &keyed_rows)?;
        Ok(keyed_rows.into_iter().map(|(_, row)| row).collect())
    }

    fn load_resolved_pending_page(
        &self,
        keys: &[(i64, String)],
    ) -> Result<Vec<SessionResolvedPendingWorkItem>, StoreResolutionError> {
        let pending = self.load_pending_page(keys)?;
        let prior = self
            .prior_overlay
            .as_ref()
            .ok_or_else(|| incremental_error("prior overlay unavailable"))?;
        keys.iter()
            .zip(pending)
            .map(|((version_id, local_id), pending)| {
                let row = ready_prior(
                    prior
                        .pending(*version_id, local_id)
                        .map_err(|error| incremental_error(error.to_string()))?,
                )?
                .ok_or_else(|| {
                    incremental_error(format!(
                        "frozen prior pending row disappeared: {version_id}:{local_id}"
                    ))
                })?;
                Ok(SessionResolvedPendingWorkItem {
                    pending,
                    target_symbol_id: SemanticSymbolId {
                        version: SemanticVersionId::Store(row.target_version_id),
                        local_id: row.target_symbol_id,
                    },
                    tier: row.tier,
                    confidence: row.confidence,
                    method: row.method,
                })
            })
            .collect()
    }

    fn load_relationship_page(
        &self,
        keys: &[(i64, String)],
    ) -> Result<Vec<crate::resolution_session::SessionRelationship>, StoreResolutionError> {
        let sql = format!(
            "WITH wanted(version_id,local_id) AS (VALUES {})
             SELECT r.version_id,r.relationship_id,r.to_symbol_id,r.kind,r.start_line,r.start_byte,
                    r.end_byte,r.confidence,
                    CASE WHEN COUNT(i.identifier_id)=1 THEN MIN(i.identifier_id) END
             FROM wanted
             JOIN relationships AS r
               ON r.version_id=wanted.version_id AND r.relationship_id=wanted.local_id
             LEFT JOIN symbols AS target
               ON target.version_id=r.version_id AND target.symbol_id=r.to_symbol_id
             LEFT JOIN identifiers AS i
               ON i.version_id=r.version_id AND i.name=target.name
              AND (
                (r.start_byte IS NOT NULL AND r.end_byte IS NOT NULL
                 AND i.start_byte>=r.start_byte AND i.start_byte<=r.end_byte
                 AND i.end_byte<=r.end_byte)
                OR
                ((r.start_byte IS NULL OR r.end_byte IS NULL)
                 AND i.start_line=COALESCE(r.start_line,0))
              )
             GROUP BY r.version_id,r.relationship_id,r.to_symbol_id,r.kind,r.start_line,
                      r.start_byte,r.end_byte,r.confidence
             ORDER BY r.version_id,r.relationship_id COLLATE BINARY",
            key_values_clause(keys.len())
        );
        let keyed_rows = self.with_candidate_reader(|connection| {
            let mut statement = connection.prepare(&sql)?;
            Ok(statement
                .query_map(rusqlite::params_from_iter(key_params(keys)), |row| {
                    Ok((
                        (row.get::<_, i64>(0)?, row.get::<_, String>(1)?),
                        crate::resolution_session::SessionRelationship {
                            target_symbol_id: SemanticSymbolId {
                                version: SemanticVersionId::Store(row.get(0)?),
                                local_id: row.get(2)?,
                            },
                            source_version_id: SemanticVersionId::Store(row.get(0)?),
                            located_identifier_id: row.get(8)?,
                            identifier_lookup_complete: true,
                            kind: row.get(3)?,
                            start_line: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                            start_byte: row.get(5)?,
                            end_byte: row.get(6)?,
                            confidence: row.get(7)?,
                        },
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?)
        })?;
        self.record_candidate_query(
            CandidateQueryFamily::RelationshipHydration,
            keyed_rows.len(),
        );
        self.validate_hydrated_keys("relationships", keys, &keyed_rows)?;
        Ok(keyed_rows.into_iter().map(|(_, row)| row).collect())
    }

    fn load_identifier_page(
        &self,
        keys: &[(i64, String)],
    ) -> Result<Vec<IdentifierWorkItem>, StoreResolutionError> {
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
        let keyed_rows = self.with_candidate_reader(|connection| {
            let mut statement = connection.prepare(&sql)?;
            Ok(statement
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
                .collect::<Result<Vec<_>, _>>()?)
        })?;
        self.record_candidate_query(CandidateQueryFamily::IdentifierHydration, keyed_rows.len());
        self.validate_hydrated_keys("identifiers", keys, &keyed_rows)?;
        Ok(keyed_rows.into_iter().map(|(_, row)| row).collect())
    }

    fn load_resolved_identifier_page(
        &self,
        keys: &[(i64, String)],
    ) -> Result<Vec<SessionResolvedIdentifierWorkItem>, StoreResolutionError> {
        let identifiers = self.load_identifier_page(keys)?;
        let prior = self
            .prior_overlay
            .as_ref()
            .ok_or_else(|| incremental_error("prior overlay unavailable"))?;
        keys.iter()
            .zip(identifiers)
            .map(|((version_id, local_id), identifier)| {
                let row = ready_prior(
                    prior
                        .identifier(*version_id, local_id)
                        .map_err(|error| incremental_error(error.to_string()))?,
                )?
                .ok_or_else(|| {
                    incremental_error(format!(
                        "frozen prior identifier row disappeared: {version_id}:{local_id}"
                    ))
                })?;
                let outcome = Outcome::parse(&row.outcome).ok_or_else(|| {
                    incremental_error(format!(
                        "invalid prior identifier outcome {:?}",
                        row.outcome
                    ))
                })?;
                Ok(SessionResolvedIdentifierWorkItem {
                    identifier,
                    target_symbol_id: row.target_version_id.zip(row.target_symbol_id).map(
                        |(version_id, local_id)| SemanticSymbolId {
                            version: SemanticVersionId::Store(version_id),
                            local_id,
                        },
                    ),
                    tier: row.tier,
                    confidence: row.confidence,
                    method: row.method,
                    outcome,
                    candidates: row.candidates,
                })
            })
            .collect()
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
        let cache_key = (candidates.to_vec(), language.to_string());
        if let Some(version) = self
            .candidate_window
            .borrow()
            .module_versions
            .get(&cache_key)
        {
            return Ok(version.clone());
        }
        let version = self.with_candidate_reader(|connection| {
            for candidate in candidates {
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
                self.record_candidate_query(
                    CandidateQueryFamily::ModuleVersion,
                    usize::from(version_id.is_some()),
                );
                if let Some(version_id) = version_id {
                    return Ok(Some(version_id.to_string()));
                }
            }
            Ok(None)
        })?;
        let mut window = self.candidate_window.borrow_mut();
        if window.module_versions.len() < self.window_size {
            window.module_versions.insert(cache_key, version.clone());
        }
        self.max_candidate_cache_entries.set(
            self.max_candidate_cache_entries
                .get()
                .max(window.entry_count()),
        );
        Ok(version)
    }
}

fn parse_source_key(source_key: &str) -> Result<i64, StoreResolutionError> {
    source_key
        .parse()
        .map_err(|_| StoreResolutionError::InvalidIdentity)
}

fn store_layout_from_factory(
    factory: &StoreConnectionFactory,
) -> Result<StoreLayout, StoreResolutionError> {
    let connection = factory.open_reader()?;
    let store_path: String = connection.query_row(
        "SELECT file FROM pragma_database_list WHERE name='main'",
        [],
        |row| row.get(0),
    )?;
    let store_path = PathBuf::from(store_path);
    let root = store_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| incremental_error("store layout root unavailable"))?;
    let layout = StoreLayout::open(root).map_err(|error| incremental_error(error.to_string()))?;
    if layout.store_db() != store_path {
        return Err(incremental_error(
            "connection generation does not match CURRENT",
        ));
    }
    Ok(layout)
}

fn map_prior_page<T>(
    access: PriorOverlayAccess<PriorOverlayPage<T>>,
    key: impl Fn(T) -> (i64, String),
) -> PriorPhaseAccess {
    match access {
        PriorOverlayAccess::Ready(page) => {
            PriorOverlayAccess::Ready((page.rows.into_iter().map(key).collect(), page.next))
        }
        PriorOverlayAccess::FullFallback(fallback) => PriorOverlayAccess::FullFallback(fallback),
    }
}

fn ready_prior<T>(access: PriorOverlayAccess<T>) -> Result<T, StoreResolutionError> {
    match access {
        PriorOverlayAccess::Ready(value) => Ok(value),
        PriorOverlayAccess::FullFallback(fallback) => Err(incremental_error(format!(
            "prior overlay changed during resolution: {fallback:?}"
        ))),
    }
}

fn prior_key_rows(access: PriorPhaseAccess) -> Result<PriorPhaseKeys, StoreResolutionError> {
    ready_prior(access)
}

fn store_versions(versions: &[SemanticVersionId]) -> Result<Vec<i64>, StoreResolutionError> {
    versions.iter().map(store_version).collect()
}

#[allow(clippy::too_many_arguments)]
fn freeze_source_query(
    connection: &Connection,
    transaction: &rusqlite::Transaction<'_>,
    identity: &StoreManifestIdentity,
    window_size: usize,
    phase: i64,
    table: &str,
    id_column: &str,
    name_column: Option<&str>,
    values: &[rusqlite::types::Value],
) -> Result<(), StoreResolutionError> {
    if !values.is_empty() || name_column.is_none() {
        let filter = if values.is_empty() {
            String::new()
        } else {
            let column = name_column.unwrap_or("version_id");
            format!("AND source.{column} IN (SELECT value FROM selected)")
        };
        let with = if values.is_empty() {
            String::new()
        } else {
            format!(
                "WITH selected(value) AS (VALUES {}) ",
                (0..values.len())
                    .map(|_| "(?)")
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        let sql = format!(
            "{with}SELECT source.version_id,source.{id_column}
             FROM {table} AS source
             WHERE EXISTS (
               SELECT 1 FROM manifest_entries AS me
               WHERE me.view_id=? AND me.generation=?
                 AND me.status IN ('indexed','failed_preserved')
                 AND me.version_id=source.version_id
             ) {filter}
               AND (source.version_id,source.{id_column})>(?,?)
             ORDER BY source.version_id,source.{id_column} COLLATE BINARY LIMIT ?"
        );
        let mut after = (0, String::new());
        loop {
            let mut bind = values.to_vec();
            bind.push(identity.view_id.clone().into());
            bind.push(identity.generation.into());
            bind.push(after.0.into());
            bind.push(after.1.clone().into());
            bind.push(i64::try_from(window_size).unwrap().into());
            let keys = connection
                .prepare(&sql)?
                .query_map(rusqlite::params_from_iter(bind), |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            if keys.is_empty() {
                break;
            }
            let insert = format!(
                "WITH incoming(version_id,local_id) AS (VALUES {})
                 INSERT OR IGNORE INTO phase_keys(phase,version_id,local_id)
                 SELECT ?,incoming.version_id,incoming.local_id FROM incoming",
                key_values_clause(keys.len())
            );
            let mut insert_bind = key_params(&keys);
            insert_bind.push(phase.into());
            transaction.execute(&insert, rusqlite::params_from_iter(insert_bind))?;
            after = keys.last().cloned().expect("non-empty source key page");
        }
    }
    Ok(())
}

fn store_version(version: &SemanticVersionId) -> Result<i64, StoreResolutionError> {
    match version {
        SemanticVersionId::Store(version_id) if *version_id > 0 => Ok(*version_id),
        _ => Err(StoreResolutionError::InvalidIdentity),
    }
}

fn sqlite_read_only_uri(path: &Path, immutable: bool) -> Result<String, StoreResolutionError> {
    let path = path.to_str().ok_or_else(|| {
        incremental_error(format!(
            "prior overlay path is not UTF-8: {}",
            path.display()
        ))
    })?;
    // StoreLayout canonicalizes every path it hands out (layout.rs:54, :133), and on Windows
    // std::fs::canonicalize returns the VERBATIM spelling: \\?\C:\... , or \\?\UNC\server\share for
    // a network path. Strip that prefix BEFORE the separator swap below. Left in place, \\?\ became
    // //?/ , the leading // made SQLite look for a URI authority, and '?' is absent from the
    // safe-byte set so it was percent-encoded — yielding file://%3F/C:/... and the SQLite error
    // "invalid uri authority: %3F". That failed every scoped resolve on Windows and pinned the view
    // at `converging`. Full resolves clear prior_scope_state and skip the overlay, which is why the
    // store still served data while never becoming exact.
    let stripped = path
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| path.strip_prefix(r"\\?\").map(str::to_string))
        .unwrap_or_else(|| path.to_string());
    let normalized = stripped.replace('\\', "/");
    // Emit an EMPTY authority explicitly. "file:" + "C:/x" parses only because it has no "//" at
    // all; anchoring the path with a leading slash keeps the authority empty for a drive path AND
    // for a UNC path, instead of letting the host name become the authority.
    let mut uri = String::from("file://");
    if !normalized.starts_with('/') {
        uri.push('/');
    }
    for byte in normalized.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b':' | b'-' | b'.' | b'_' | b'~') {
            uri.push(char::from(byte));
        } else {
            write!(uri, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    if immutable {
        uri.push_str("?mode=ro&immutable=1");
    } else {
        uri.push_str("?mode=ro");
    }
    Ok(uri)
}

fn validate_frozen_prior_overlay(
    transaction: &Transaction<'_>,
    state: &ResolutionScopeState,
) -> Result<(), StoreResolutionError> {
    let relative_path = format!("bases/{}.db", state.base_id);
    let checks = transaction.query_row(
        "SELECT
           EXISTS(
             SELECT 1 FROM prior_store.resolution_scope_state
             WHERE view_id=?1 AND predecessor_manifest_generation=?2
               AND predecessor_manifest_hash=?3 AND base_id=?4 AND delta_generation=?5
               AND resolver_output_epoch=?6 AND current_manifest_generation=?7
               AND current_manifest_hash=?8 AND journal_through_transition_id=?9
           ),
           EXISTS(
             SELECT 1 FROM prior_store.views AS view
             JOIN prior_store.manifests AS current
               ON current.view_id=view.view_id AND current.generation=view.current_generation
             JOIN prior_store.manifests AS predecessor
               ON predecessor.view_id=view.view_id AND predecessor.generation=?2
             WHERE view.view_id=?1 AND view.current_generation=?7
               AND current.manifest_hash=?8 AND predecessor.manifest_hash=?3
           ),
           EXISTS(
             SELECT 1 FROM prior_store.resolution_bases AS base
             WHERE base.base_id=?4 AND base.resolver_output_epoch=?6 AND base.state='ready'
               AND base.relative_path=?10 AND base.identifier_count>=0 AND base.pending_count>=0
               AND base.file_bytes>0 AND length(base.file_sha256)>0
           ),
           EXISTS(
             SELECT 1 FROM prior_store.resolution_bases AS base
             WHERE base.base_id=?4
               AND base.manifest_hash=(SELECT value FROM prior_base.base_meta WHERE key='manifest_hash')
               AND CAST(base.resolver_output_epoch AS TEXT)=
                   (SELECT value FROM prior_base.base_meta WHERE key='resolver_output_epoch')
               AND CAST(base.identifier_count AS TEXT)=
                   (SELECT value FROM prior_base.base_meta WHERE key='identifier_count')
               AND CAST(base.pending_count AS TEXT)=
                   (SELECT value FROM prior_base.base_meta WHERE key='pending_count')
               AND (SELECT value FROM prior_base.base_meta WHERE key='completed')='1'
           ),
           EXISTS(
             SELECT 1 FROM prior_store.resolution_bases AS base
             WHERE base.base_id=?4
               AND base.identifier_count=(SELECT COUNT(*) FROM prior_base.identifier_resolutions)
               AND base.pending_count=(SELECT COUNT(*) FROM prior_base.pending_resolutions)
           ),
           NOT EXISTS(
             SELECT version_id FROM prior_store.resolution_base_versions WHERE base_id=?4
             EXCEPT SELECT version_id FROM prior_base.resolution_base_versions
           ),
           NOT EXISTS(
             SELECT version_id FROM prior_base.resolution_base_versions
             EXCEPT SELECT version_id FROM prior_store.resolution_base_versions WHERE base_id=?4
           ),
           EXISTS(
             SELECT 1 FROM prior_store.resolution_deltas AS delta
             WHERE delta.view_id=?1 AND delta.delta_generation=?5 AND delta.base_id=?4
               AND delta.manifest_generation=?2 AND delta.manifest_hash=?3
               AND delta.resolver_output_epoch=?6
               AND delta.identifier_replacements=(
                 SELECT COUNT(*) FROM prior_store.resolution_identifier_deltas
                 WHERE view_id=?1 AND delta_generation=?5
               )
               AND delta.pending_replacements=(
                 SELECT COUNT(*) FROM prior_store.resolution_pending_deltas
                 WHERE view_id=?1 AND delta_generation=?5 AND operation='replace'
               )
               AND delta.pending_tombstones=(
                 SELECT COUNT(*) FROM prior_store.resolution_pending_deltas
                 WHERE view_id=?1 AND delta_generation=?5 AND operation='tombstone'
               )
           )",
        params![
            state.view_id,
            state.predecessor_manifest_generation,
            state.predecessor_manifest_hash,
            state.base_id,
            state.delta_generation,
            state.resolver_output_epoch,
            state.current_manifest_generation,
            state.current_manifest_hash,
            state.journal_through_transition_id,
            relative_path,
        ],
        |row| {
            Ok([
                row.get::<_, bool>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, bool>(5)?,
                row.get::<_, bool>(6)?,
                row.get::<_, bool>(7)?,
            ])
        },
    )?;
    let names = [
        "scope state",
        "manifest state",
        "ready base catalog identity",
        "ready base metadata identity",
        "ready base row counts",
        "catalog base roots",
        "attached base roots",
        "delta identity and counts",
    ];
    if let Some((name, _)) = names.iter().zip(checks).find(|(_, valid)| !valid) {
        Err(incremental_error(format!(
            "prior overlay {name} changed before exact materialization"
        )))
    } else {
        Ok(())
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

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A CANONICALIZED store path must still produce an attachable URI.
    ///
    /// `StoreLayout` canonicalizes every path it hands out (`layout.rs:54`, `:133`), and on Windows
    /// `std::fs::canonicalize` returns the VERBATIM spelling `\\?\C:\...`. The URI builder replaced
    /// `\` with `/` before removing that prefix, so it produced `file://%3F/C:/...`: the leading `//`
    /// made SQLite look for a URI authority, and `?` — absent from the safe-byte set — was
    /// percent-encoded to `%3F`. Every scoped resolve then died with
    /// `invalid uri authority: %3F`, `materialize_prior_overlay` never completed, and the view was
    /// pinned at `converging` forever. Full resolves were unaffected because they clear
    /// `prior_scope_state` and skip the overlay entirely, which is why the store still served data.
    ///
    /// The sibling test below builds its fixture path WITHOUT canonicalizing, which is why it passed
    /// throughout. Keep this one canonicalizing: that is what production does.
    #[test]
    fn prior_overlay_attach_uris_survive_a_canonicalized_store_path() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().canonicalize().unwrap();
        let store_path = canonical.join("prior store.db");
        let base_path = canonical.join("prior base.db");
        for path in [&store_path, &base_path] {
            let connection = Connection::open(path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE guarded(value INTEGER); INSERT INTO guarded VALUES (7);",
                )
                .unwrap();
        }

        let store_uri = sqlite_read_only_uri(&store_path, false).unwrap();
        let base_uri = sqlite_read_only_uri(&base_path, true).unwrap();
        assert!(
            !store_uri.contains("%3F"),
            "the verbatim prefix leaked into the URI authority: {store_uri}"
        );

        let scratch = Connection::open_in_memory().unwrap();
        scratch
            .execute("ATTACH DATABASE ?1 AS readonly_store", [store_uri])
            .expect("a canonicalized store path must attach");
        scratch
            .execute("ATTACH DATABASE ?1 AS readonly_base", [base_uri])
            .expect("a canonicalized base path must attach");

        assert_eq!(
            scratch
                .query_row("SELECT value FROM readonly_store.guarded", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            7
        );
        assert!(
            scratch
                .execute("UPDATE readonly_base.guarded SET value=8", [])
                .is_err(),
            "the base must stay read-only"
        );
    }

    #[test]
    fn prior_overlay_attach_uris_reject_store_and_base_writes() {
        let temp = tempfile::tempdir().unwrap();
        let store_path = temp.path().join("prior store.db");
        let base_path = temp.path().join("prior base.db");
        for path in [&store_path, &base_path] {
            let connection = Connection::open(path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE guarded(value INTEGER); INSERT INTO guarded VALUES (1);",
                )
                .unwrap();
        }

        let scratch = Connection::open_in_memory().unwrap();
        scratch
            .execute(
                "ATTACH DATABASE ?1 AS readonly_store",
                [sqlite_read_only_uri(&store_path, false).unwrap()],
            )
            .unwrap();
        scratch
            .execute(
                "ATTACH DATABASE ?1 AS readonly_base",
                [sqlite_read_only_uri(&base_path, true).unwrap()],
            )
            .unwrap();

        assert!(
            scratch
                .execute("UPDATE readonly_store.guarded SET value=2", [])
                .is_err()
        );
        assert!(
            scratch
                .execute("UPDATE readonly_base.guarded SET value=2", [])
                .is_err()
        );
        assert_eq!(
            scratch
                .query_row("SELECT value FROM readonly_store.guarded", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            scratch
                .query_row("SELECT value FROM readonly_base.guarded", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
