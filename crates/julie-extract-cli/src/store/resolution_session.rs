use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fmt::{self, Write as _};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
#[cfg(feature = "test-store-resolution-contract")]
use std::rc::Rc;
use std::time::Instant;

use julie_extract_artifact::resolution_store::{
    IdentifierWorkItem, Outcome, PendingWorkItem, ResolutionCounts, ResolutionReportRow,
    ResolutionStatus,
};
use julie_extract_artifact::store::{
    ResolutionBaseWriter, ResolutionDiffResult, ResolutionFileIdentity, ResolutionGapFact,
    ResolutionGapKind, ResolutionGapTable, ResolutionIdentifierRow, ResolutionPendingRow,
    ResolutionPendingTombstone, ResolutionScopeState, ResolutionScratchWriter,
    ResolutionValidatedBase, ResolutionValidationError, StoreLayout,
    create_resolution_scratch_connection, resolution_scope_state,
};
use julie_extract_artifact::store::{StoreConnectionError, StoreConnectionFactory};
use julie_extractors::SymbolKind;
use rusqlite::types::Value;
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Row, Transaction, params, params_from_iter,
};
#[cfg(feature = "test-store-resolution-contract")]
use serde::{Deserialize, Serialize};

use crate::resolution::{
    self, CandidateCacheAttribution, CandidateEvidence, CandidateHit, CandidateLookup,
    CandidatePageFamily, CandidateSummary, CandidateSymbol, ChildLookupCacheState,
    ChildLookupReason, EdgeOrigin, FilteredNameLookupAttribution, FilteredNameLookupReason,
    ImportRecord, ReferenceKind, SameWindowFingerprintCounts, TierOutcome,
    TopLevelLookupAttribution, TopLevelLookupReason, TypeFact, TypeFactsLookupAttribution,
    TypeFactsLookupReason, UnresolvedEdge,
};
#[cfg(feature = "test-store-resolution-contract")]
use crate::resolution::{CandidateLookupAttribution, PrimeWindowAttribution};
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
type VersionQualifiedKey = (i64, String);
type VersionQualifiedMap<T> = BTreeMap<VersionQualifiedKey, T>;
type PriorIdentifierDeltas = (VersionQualifiedMap<ResolutionIdentifierRow>, usize);
type PriorPendingDeltas = (VersionQualifiedMap<ScopedPendingDelta>, usize);
type PriorGapFacts = (
    VersionQualifiedMap<ResolutionGapKind>,
    VersionQualifiedMap<ResolutionGapKind>,
);
type ScratchPendingState = (bool, bool);
type ScratchPendingStates = HashMap<VersionQualifiedKey, ScratchPendingState>;

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
    pub elapsed_micros: u64,
}

#[derive(Debug, Clone, Copy)]
enum CandidatePageAttribution {
    Children {
        reason: Option<ChildLookupReason>,
        page_limit: Option<usize>,
        had_prior_page: bool,
    },
    FilteredByName {
        reason: Option<FilteredNameLookupReason>,
        page_limit: Option<usize>,
        had_prior_page: bool,
    },
    TopLevel {
        reason: Option<TopLevelLookupReason>,
        page_limit: Option<usize>,
        had_prior_page: bool,
    },
    TypeFacts {
        reason: Option<TypeFactsLookupReason>,
        page_limit: Option<usize>,
        had_prior_page: bool,
    },
}

#[cfg(feature = "test-store-resolution-contract")]
pub(crate) fn candidate_lookup_attribution_json(
    attribution: &CandidateLookupAttribution,
) -> serde_json::Value {
    serde_json::json!({
        "logical_lookups": attribution.logical_lookups,
        "empty_first": attribution.empty_first,
        "trailing_empty": attribution.trailing_empty,
        "short_positive": attribution.short_positive,
        "full_page": attribution.full_page,
        "page_limit": attribution.page_limit,
        "same_window_fingerprints": {
            "first_seen": attribution.same_window_fingerprints.first_seen,
            "repeat_same_window": attribution.same_window_fingerprints.repeat_same_window,
            "probe_overflow": attribution.same_window_fingerprints.probe_overflow,
        },
    })
}

#[cfg(feature = "test-store-resolution-contract")]
pub(crate) fn prime_window_attribution_json(
    attribution: &PrimeWindowAttribution,
) -> serde_json::Value {
    serde_json::json!({
        "windows": attribution.windows,
        "windows_hit_row_limit": attribution.windows_hit_row_limit,
        "names_wanted": attribution.names_wanted,
        "names_complete": attribution.names_complete,
        "names_skipped_cutoff": attribution.names_skipped_cutoff,
        "names_rejected_capacity": attribution.names_rejected_capacity,
        "rows_admitted": attribution.rows_admitted,
    })
}

impl TopLevelLookupAttribution {
    pub(crate) fn record_lookup(
        &mut self,
        reason: Option<TopLevelLookupReason>,
        fingerprint: SameWindowFingerprintCounts,
    ) {
        self.aggregate.record_lookup(fingerprint);
        if let Some(reason) = reason {
            self.reasons[reason.index()].record_lookup(fingerprint);
        }
    }

    pub(crate) fn record_page(
        &mut self,
        reason: Option<TopLevelLookupReason>,
        row_count: usize,
        page_limit: Option<usize>,
        had_prior_page: bool,
    ) {
        self.aggregate
            .record_page(row_count, page_limit, had_prior_page);
        if let Some(reason) = reason {
            self.reasons[reason.index()].record_page(row_count, page_limit, had_prior_page);
        }
    }
}

impl FilteredNameLookupAttribution {
    pub(crate) fn record_lookup(
        &mut self,
        reason: Option<FilteredNameLookupReason>,
        fingerprint: SameWindowFingerprintCounts,
    ) {
        self.aggregate.record_lookup(fingerprint);
        if let Some(reason) = reason {
            self.reasons[reason.index()].record_lookup(fingerprint);
        }
    }

    pub(crate) fn record_page(
        &mut self,
        reason: Option<FilteredNameLookupReason>,
        row_count: usize,
        page_limit: Option<usize>,
        had_prior_page: bool,
    ) {
        self.aggregate
            .record_page(row_count, page_limit, had_prior_page);
        if let Some(reason) = reason {
            self.reasons[reason.index()].record_page(row_count, page_limit, had_prior_page);
        }
    }
}

impl TypeFactsLookupAttribution {
    pub(crate) fn record_lookup(
        &mut self,
        reason: Option<TypeFactsLookupReason>,
        fingerprint: SameWindowFingerprintCounts,
    ) {
        self.aggregate.record_lookup(fingerprint);
        if let Some(reason) = reason {
            self.reasons[reason.index()].record_lookup(fingerprint);
        }
    }

    pub(crate) fn record_page(
        &mut self,
        reason: Option<TypeFactsLookupReason>,
        row_count: usize,
        page_limit: Option<usize>,
        had_prior_page: bool,
    ) {
        self.aggregate
            .record_page(row_count, page_limit, had_prior_page);
        if let Some(reason) = reason {
            self.reasons[reason.index()].record_page(row_count, page_limit, had_prior_page);
        }
    }
}

#[derive(Debug)]
struct SameWindowFingerprintTracker {
    capacity: usize,
    slots: Vec<u64>,
}

impl SameWindowFingerprintTracker {
    #[cfg(feature = "test-store-resolution-contract")]
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            slots: vec![0; capacity.saturating_mul(CandidatePageFamily::COUNT)],
        }
    }

    fn reset(&mut self) {
        self.slots.fill(0);
    }

    fn observe(
        &mut self,
        family: CandidatePageFamily,
        fingerprint: u64,
    ) -> SameWindowFingerprintCounts {
        let base = family.index().saturating_mul(self.capacity);
        let start = (fingerprint as usize % self.capacity).saturating_add(base);
        for offset in 0..self.capacity {
            let index = base + ((start - base + offset) % self.capacity);
            match self.slots[index] {
                value if value == fingerprint => {
                    return SameWindowFingerprintCounts {
                        repeat_same_window: 1,
                        ..SameWindowFingerprintCounts::default()
                    };
                }
                0 => {
                    self.slots[index] = fingerprint;
                    return SameWindowFingerprintCounts {
                        first_seen: 1,
                        ..SameWindowFingerprintCounts::default()
                    };
                }
                _ => {}
            }
        }
        SameWindowFingerprintCounts {
            probe_overflow: 1,
            ..SameWindowFingerprintCounts::default()
        }
    }
}

fn logical_fingerprint<T: Hash>(family: CandidatePageFamily, key: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    family.index().hash(&mut hasher);
    key.hash(&mut hasher);
    let fingerprint = hasher.finish();
    if fingerprint == 0 { 1 } else { fingerprint }
}

fn accumulate_candidate_query_telemetry(
    telemetry: &mut [CandidateQueryTelemetry; CandidateQueryFamily::COUNT],
    family: CandidateQueryFamily,
    rows_read: usize,
    elapsed_micros: u64,
) {
    let family = &mut telemetry[family.index()];
    family.executions = family.executions.saturating_add(1);
    family.rows_read = family.rows_read.saturating_add(rows_read);
    family.elapsed_micros = family.elapsed_micros.saturating_add(elapsed_micros);
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

#[cfg(feature = "test-store-resolution-contract")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScopedFinalizationTelemetry {
    pub current_identifier_queries: usize,
    pub current_identifier_rows: usize,
    pub current_pending_queries: usize,
    pub current_pending_rows: usize,
    pub prior_identifier_queries: usize,
    pub prior_identifier_rows: usize,
    pub prior_pending_queries: usize,
    pub prior_pending_rows: usize,
    pub base_identifier_queries: usize,
    pub base_identifier_rows: usize,
    pub base_pending_queries: usize,
    pub base_pending_rows: usize,
    pub base_identifier_target_queries: usize,
    pub base_identifier_target_rows: usize,
    pub base_pending_target_queries: usize,
    pub base_pending_target_rows: usize,
    pub base_keyed_reader_opens: usize,
    pub target_validation_queries: usize,
    pub target_validation_targets: usize,
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
    pub(crate) rebase_after_exact: bool,
    pub(crate) elapsed_millis: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct ChildrenNamedKey {
    version_id: i64,
    parent_symbol_id: String,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ScopeFrontierKey {
    version_id: i64,
    symbol_id: String,
}

#[derive(Debug, Default)]
struct CandidateWindow {
    primed_names: BTreeSet<String>,
    by_name: HashMap<String, Vec<CandidateHit>>,
    by_id: HashMap<SemanticSymbolId, Option<CandidateHit>>,
    children_named: BTreeMap<ChildrenNamedKey, Vec<CandidateHit>>,
    module_versions: HashMap<(Vec<String>, String), Option<String>>,
}

impl CandidateWindow {
    fn non_by_id_entry_count(&self) -> usize {
        self.by_name.values().map(Vec::len).sum::<usize>()
            + self.children_named.len()
            + self.children_named.values().map(Vec::len).sum::<usize>()
            + self.module_versions.len()
    }

    fn entry_count(&self) -> usize {
        self.non_by_id_entry_count()
            .saturating_add(self.by_id.len())
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
    candidate_cache_attribution: Cell<CandidateCacheAttribution>,
    candidate_query_timing_enabled: bool,
    same_window_fingerprints: RefCell<Option<SameWindowFingerprintTracker>>,
    #[cfg(feature = "test-store-resolution-contract")]
    propagation_coverage_telemetry: Cell<PropagationCoverageTelemetry>,
    #[cfg(feature = "test-store-resolution-contract")]
    scoped_finalization_telemetry: Rc<Cell<ScopedFinalizationTelemetry>>,
    visible_root_batches: usize,
    candidate_reader: RefCell<Option<Connection>>,
    candidate_window: RefCell<CandidateWindow>,
    filtered_summaries: RefCell<BoundedCache<FilteredSummaryKey, CandidateSummary>>,
    tier_candidates: RefCell<TierCandidateAccumulator>,
    resolution_cache: RefCell<HashMap<ResolutionLookupKey, TierOutcome>>,
    prior_overlay: Option<PriorOverlayReader>,
    prior_scope_state: Option<ResolutionScopeState>,
    validated_base: Option<ResolutionValidatedBase>,
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
            candidate_cache_attribution: Cell::new(CandidateCacheAttribution::default()),
            candidate_query_timing_enabled: false,
            same_window_fingerprints: RefCell::new(None),
            #[cfg(feature = "test-store-resolution-contract")]
            propagation_coverage_telemetry: Cell::new(PropagationCoverageTelemetry::default()),
            #[cfg(feature = "test-store-resolution-contract")]
            scoped_finalization_telemetry: Rc::new(Cell::new(
                ScopedFinalizationTelemetry::default(),
            )),
            visible_root_batches: 0,
            candidate_reader: RefCell::new(None),
            candidate_window: RefCell::new(CandidateWindow::default()),
            filtered_summaries: RefCell::new(BoundedCache::new(window_size)),
            tier_candidates: RefCell::new(TierCandidateAccumulator::default()),
            resolution_cache: RefCell::new(HashMap::new()),
            prior_overlay: None,
            prior_scope_state: None,
            validated_base: None,
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

    #[cfg(feature = "test-store-resolution-contract")]
    pub(crate) fn enable_candidate_query_timing(&mut self) {
        self.candidate_query_timing_enabled = true;
        if self.same_window_fingerprints.get_mut().is_none() {
            *self.same_window_fingerprints.get_mut() =
                Some(SameWindowFingerprintTracker::new(self.window_size));
        }
    }

    #[cfg(feature = "test-store-resolution-contract")]
    pub fn enable_candidate_query_timing_for_test(&mut self) {
        self.enable_candidate_query_timing();
    }

    #[cfg(feature = "test-store-resolution-contract")]
    pub fn same_window_fingerprint_tracker_allocated_for_test(&self) -> bool {
        self.same_window_fingerprints.borrow().is_some()
    }

    #[cfg(feature = "test-store-resolution-contract")]
    pub fn candidate_cache_attribution_for_test(&self) -> serde_json::Value {
        let attribution = self.candidate_cache_attribution.get();
        let child_calls = attribution
            .children_named
            .buckets
            .iter()
            .map(|states| states.iter().map(|bucket| bucket.calls).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let child_sql_pages = attribution
            .children_named
            .buckets
            .iter()
            .map(|states| {
                states
                    .iter()
                    .map(|bucket| bucket.sql_pages)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let reasoned = |aggregate: &CandidateLookupAttribution,
                        reasons: &[CandidateLookupAttribution],
                        names: &[&str]| {
            let mut value = candidate_lookup_attribution_json(aggregate);
            let reason_json = names
                .iter()
                .zip(reasons.iter())
                .map(|(name, attribution)| {
                    (
                        (*name).to_string(),
                        candidate_lookup_attribution_json(attribution),
                    )
                })
                .collect::<serde_json::Map<_, _>>();
            value["reasons"] = serde_json::Value::Object(reason_json);
            value
        };
        let page_attribution = serde_json::json!({
            "children_named": reasoned(
                &attribution.children_named.aggregate,
                &attribution.children_named.reasons,
                &[
                    "static_member",
                    "static_receiver_shadow_check",
                    "tier1_scope_terminal",
                    "tier3_typed_member",
                    "tier3_receiver_scope",
                ],
            ),
            "filtered_by_name": reasoned(
                &attribution.filtered_by_name.aggregate,
                &attribution.filtered_by_name.reasons,
                &["tier2_import", "unique_type", "unique_static"],
            ),
            "top_level_named": reasoned(
                &attribution.top_level_named.aggregate,
                &attribution.top_level_named.reasons,
                &["tier1_terminal", "tier3_receiver"],
            ),
            "type_facts": reasoned(
                &attribution.type_facts.aggregate,
                &attribution.type_facts.reasons,
                &["tier3_receiver"],
            ),
        });
        serde_json::json!({
            "prime_window": prime_window_attribution_json(&attribution.prime_window),
            "page_attribution": page_attribution,
            "child_calls": child_calls,
            "child_sql_pages": child_sql_pages,
            "batch_count_statements": attribution.children_named.batch_count_statements,
            "batch_fetch_statements": attribution.children_named.batch_fetch_statements,
            "by_id": {
                "cache_hits": attribution.by_id.cache_hits,
                "sql_misses": attribution.by_id.sql_misses,
                "accepted_insertions": attribution.by_id.accepted_insertions,
                "rejected_by_id_cap": attribution.by_id.rejected_by_id_cap,
                "rejected_by_aggregate_cap": attribution.by_id.rejected_by_aggregate_cap,
                "max_entries": attribution.by_id.max_entries,
                "max_non_by_id_entries": attribution.by_id.max_non_by_id_entries,
                "max_aggregate_entries": attribution.by_id.max_aggregate_entries,
                "phase_reset_count": attribution.by_id.phase_reset_count,
                "phase_reset_by_id_entries": attribution.by_id.phase_reset_by_id_entries,
                "phase_reset_aggregate_entries": attribution.by_id.phase_reset_aggregate_entries,
                "phase_reset_by_id_entries_total": attribution.by_id.phase_reset_by_id_entries_total,
                "phase_reset_aggregate_entries_total": attribution.by_id.phase_reset_aggregate_entries_total,
            },
        })
    }

    pub(crate) fn set_validated_base(&mut self, proof: ResolutionValidatedBase) {
        self.validated_base = Some(proof);
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

    #[cfg(feature = "test-store-resolution-contract")]
    pub fn remove_identifier_resolution_without_touch_for_test(
        &mut self,
        version_id: i64,
        identifier_id: &str,
    ) -> Result<(), StoreResolutionError> {
        self.scratch.execute(
            "DELETE FROM identifier_resolutions
             WHERE version_id=?1 AND identifier_id=?2",
            params![version_id, identifier_id],
        )?;
        self.scratch.execute(
            "DELETE FROM identifier_touched
             WHERE version_id=?1 AND identifier_id=?2",
            params![version_id, identifier_id],
        )?;
        Ok(())
    }

    pub fn finish_exact(self) -> Result<ResolutionFileIdentity, StoreResolutionError> {
        self.finish_exact_inner(|_| {})
    }

    pub fn finish_scoped_delta<F>(
        self,
        delta_path: impl AsRef<Path>,
        emit_gap: F,
    ) -> Result<ResolutionDiffResult, StoreResolutionError>
    where
        F: FnMut(ResolutionGapFact) -> Result<(), ResolutionValidationError>,
    {
        self.finish_scoped_delta_inner(delta_path.as_ref(), emit_gap)
    }

    #[cfg(feature = "test-store-resolution-contract")]
    pub fn finish_scoped_delta_observing<F>(
        self,
        delta_path: impl AsRef<Path>,
        emit_gap: F,
    ) -> Result<(ResolutionDiffResult, ScopedFinalizationTelemetry), StoreResolutionError>
    where
        F: FnMut(ResolutionGapFact) -> Result<(), ResolutionValidationError>,
    {
        let telemetry = self.scoped_finalization_telemetry.clone();
        let result = self.finish_scoped_delta_inner(delta_path.as_ref(), emit_gap)?;
        Ok((result, telemetry.get()))
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

    fn finish_scoped_delta_inner<F>(
        mut self,
        delta_path: &Path,
        mut emit_gap: F,
    ) -> Result<ResolutionDiffResult, StoreResolutionError>
    where
        F: FnMut(ResolutionGapFact) -> Result<(), ResolutionValidationError>,
    {
        let result = (|| {
            #[cfg(feature = "test-store-resolution-contract")]
            scoped_finalize_delay_for_test();
            let state = self
                .prior_scope_state
                .clone()
                .ok_or_else(|| incremental_error("scoped resolution has no frozen prior state"))?;
            if self.prior_overlay.is_none() {
                return Err(incremental_error(
                    "scoped resolution prior overlay unavailable",
                ));
            }
            let selected_versions = scoped_selected_versions(self.decision_telemetry.as_ref())?;
            let target_connection = self.reader_factory.open_reader()?;
            let removed_versions =
                removed_resolution_versions(&target_connection, &state, &self.identity)?;
            let removed_set = removed_versions.iter().copied().collect::<BTreeSet<_>>();
            let (prior_identifier_deltas, identifier_delta_page) =
                load_prior_identifier_deltas(&target_connection, &state, self.window_size)?;
            let (prior_pending_deltas, pending_delta_page) =
                load_prior_pending_deltas(&target_connection, &state, self.window_size)?;
            let (mut identifier_gaps, mut pending_gaps) =
                load_prior_gap_facts(&target_connection, &state)?;
            let mut prior_delta_versions = BTreeSet::new();
            prior_delta_versions.extend(
                prior_identifier_deltas
                    .keys()
                    .map(|(version_id, _)| *version_id),
            );
            prior_delta_versions.extend(
                prior_pending_deltas
                    .keys()
                    .map(|(version_id, _)| *version_id),
            );
            let visible_delta_versions = visible_versions_for_keys(
                &target_connection,
                &self.identity,
                &prior_delta_versions,
            )?;
            let mut prior_gap_versions = BTreeSet::new();
            prior_gap_versions.extend(identifier_gaps.keys().map(|(version_id, _)| *version_id));
            prior_gap_versions.extend(pending_gaps.keys().map(|(version_id, _)| *version_id));
            let base_gap_versions =
                base_versions_for_keys(&target_connection, &state, &prior_gap_versions)?;
            identifier_gaps.retain(|(version_id, _), _| {
                visible_delta_versions.contains(version_id)
                    || removed_set.contains(version_id)
                    || base_gap_versions.contains(version_id)
            });
            pending_gaps.retain(|(version_id, _), _| {
                visible_delta_versions.contains(version_id)
                    || removed_set.contains(version_id)
                    || base_gap_versions.contains(version_id)
            });
            let prior_identifier_keys: BTreeSet<(i64, String)> =
                prior_identifier_deltas.keys().cloned().collect();
            let prior_pending_keys: BTreeSet<(i64, String)> =
                prior_pending_deltas.keys().cloned().collect();
            let mut touched_identifiers =
                ScopedTouchedIdentifierCursor::new(&self.scratch, self.window_size);
            let mut touched_identifier_rows = Vec::new();
            while let Some(touched) = touched_identifiers.next()? {
                touched_identifier_rows.push(touched);
            }
            let mut touched_pending =
                ScopedTouchedPendingCursor::new(&self.scratch, self.window_size);
            let mut touched_pending_rows = Vec::new();
            while let Some(touched) = touched_pending.next()? {
                touched_pending_rows.push(touched);
            }
            let touched_identifier_values = touched_identifier_rows
                .into_iter()
                .map(|touched| ((touched.version_id, touched.identifier_id), touched.row))
                .collect::<BTreeMap<_, _>>();
            let touched_pending_values = touched_pending_rows
                .into_iter()
                .map(|touched| {
                    (
                        (touched.version_id, touched.pending_relationship_id),
                        touched.row,
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let identifier_keys = touched_identifier_values
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            let pending_keys = touched_pending_values.keys().cloned().collect::<Vec<_>>();
            let current_keys =
                load_scoped_current_keys(&target_connection, &self.identity, &selected_versions)?;
            let prior_selected = load_scoped_prior_rows(
                self.prior_overlay
                    .as_ref()
                    .expect("scoped prior overlay checked above"),
                &selected_versions,
                self.window_size,
            )?;
            let base_rows = load_scoped_base_rows(
                &self.layout,
                &state,
                &identifier_keys,
                &pending_keys,
                &removed_versions,
            )?;
            #[cfg(feature = "test-store-resolution-contract")]
            {
                let mut telemetry = self.scoped_finalization_telemetry.get();
                telemetry.current_identifier_queries = telemetry
                    .current_identifier_queries
                    .saturating_add(current_keys.identifier_queries);
                telemetry.current_identifier_rows = telemetry
                    .current_identifier_rows
                    .saturating_add(current_keys.identifier_rows);
                telemetry.current_pending_queries = telemetry
                    .current_pending_queries
                    .saturating_add(current_keys.pending_queries);
                telemetry.current_pending_rows = telemetry
                    .current_pending_rows
                    .saturating_add(current_keys.pending_rows);
                telemetry.prior_identifier_queries = telemetry
                    .prior_identifier_queries
                    .saturating_add(prior_selected.identifier_queries);
                telemetry.prior_identifier_rows = telemetry
                    .prior_identifier_rows
                    .saturating_add(prior_selected.identifier_rows);
                telemetry.prior_pending_queries = telemetry
                    .prior_pending_queries
                    .saturating_add(prior_selected.pending_queries);
                telemetry.prior_pending_rows = telemetry
                    .prior_pending_rows
                    .saturating_add(prior_selected.pending_rows);
                telemetry.base_identifier_queries = telemetry
                    .base_identifier_queries
                    .saturating_add(base_rows.identifier_queries);
                telemetry.base_identifier_rows = telemetry
                    .base_identifier_rows
                    .saturating_add(base_rows.identifier_rows);
                telemetry.base_pending_queries = telemetry
                    .base_pending_queries
                    .saturating_add(base_rows.pending_queries);
                telemetry.base_pending_rows = telemetry
                    .base_pending_rows
                    .saturating_add(base_rows.pending_rows);
                telemetry.base_identifier_target_queries = telemetry
                    .base_identifier_target_queries
                    .saturating_add(base_rows.identifier_target_queries);
                telemetry.base_identifier_target_rows = telemetry
                    .base_identifier_target_rows
                    .saturating_add(base_rows.identifier_target_rows);
                telemetry.base_pending_target_queries = telemetry
                    .base_pending_target_queries
                    .saturating_add(base_rows.pending_target_queries);
                telemetry.base_pending_target_rows = telemetry
                    .base_pending_target_rows
                    .saturating_add(base_rows.pending_target_rows);
                telemetry.base_keyed_reader_opens =
                    telemetry.base_keyed_reader_opens.saturating_add(1);
                self.scoped_finalization_telemetry.set(telemetry);
            }
            let identity = self.identity.clone();
            let mut target_validator = ScopedTargetValidator::new(&target_connection, &identity);
            let mut identifier_touched = BTreeSet::new();
            let mut identifier_changes = BTreeMap::new();
            for ((version_id, identifier_id), touched_row) in &touched_identifier_values {
                let key = (*version_id, identifier_id.clone());
                identifier_touched.insert(key.clone());
                let row = touched_row.as_ref().ok_or_else(|| {
                    StoreResolutionError::Artifact(
                        ResolutionValidationError::IdentifierTotalityViolation {
                            version_id: *version_id,
                            identifier_id: identifier_id.clone(),
                        },
                    )
                })?;
                target_validator.push(row.target_version_id, row.target_symbol_id.as_deref())?;
                let base = base_rows.identifiers.get(&key);
                identifier_gaps.remove(&key);
                if base != Some(row) {
                    identifier_changes.insert(key, row.clone());
                    identifier_gaps.insert(
                        (*version_id, identifier_id.clone()),
                        if base.is_some() {
                            ResolutionGapKind::Replaced
                        } else {
                            ResolutionGapKind::Added
                        },
                    );
                }
            }

            let mut identifiers = BTreeMap::new();
            for (key, row) in prior_identifier_deltas {
                if !identifier_touched.contains(&key)
                    && !removed_set.contains(&key.0)
                    && visible_delta_versions.contains(&key.0)
                {
                    target_validator
                        .push(row.target_version_id, row.target_symbol_id.as_deref())?;
                    identifiers.insert(key, row);
                }
            }
            identifiers.extend(identifier_changes);

            for key in &current_keys.identifiers {
                let row = if identifier_touched.contains(key) {
                    touched_identifier_values.get(key).and_then(Option::as_ref)
                } else {
                    prior_selected.identifiers.get(key)
                };
                let row = row.ok_or_else(|| {
                    StoreResolutionError::Artifact(
                        ResolutionValidationError::IdentifierTotalityViolation {
                            version_id: key.0,
                            identifier_id: key.1.clone(),
                        },
                    )
                })?;
                target_validator.push(row.target_version_id, row.target_symbol_id.as_deref())?;
            }

            let max_window_rows = identifier_delta_page
                .max(pending_delta_page)
                .max(touched_identifiers.max_page)
                .max(touched_pending.max_page)
                .max(current_keys.max_page)
                .max(prior_selected.max_page)
                .max(base_rows.max_page);
            for row in base_rows.identifiers.values() {
                if removed_set.contains(&row.version_id) {
                    identifier_gaps.insert(
                        (row.version_id, row.identifier_id.clone()),
                        ResolutionGapKind::Removed,
                    );
                } else if row
                    .target_version_id
                    .is_some_and(|version_id| removed_set.contains(&version_id))
                    && !identifier_touched.contains(&(row.version_id, row.identifier_id.clone()))
                    && !prior_identifier_keys.contains(&(row.version_id, row.identifier_id.clone()))
                {
                    target_validator
                        .push(row.target_version_id, row.target_symbol_id.as_deref())?;
                }
            }

            let mut pending_touched = BTreeSet::new();
            let mut pending_changes = BTreeMap::new();
            for ((version_id, pending_relationship_id), touched_row) in &touched_pending_values {
                let key = (*version_id, pending_relationship_id.clone());
                pending_touched.insert(key.clone());
                let base = base_rows.pending.get(&key);
                pending_gaps.remove(&key);
                match (touched_row.as_ref(), base) {
                    (Some(row), Some(base)) if row == base => {
                        target_validator
                            .push(Some(row.target_version_id), Some(&row.target_symbol_id))?;
                    }
                    (Some(row), base) => {
                        target_validator
                            .push(Some(row.target_version_id), Some(&row.target_symbol_id))?;
                        pending_changes.insert(key, ScopedPendingDelta::Replacement(row.clone()));
                        pending_gaps.insert(
                            (*version_id, pending_relationship_id.clone()),
                            if base.is_some() {
                                ResolutionGapKind::Replaced
                            } else {
                                ResolutionGapKind::Added
                            },
                        );
                    }
                    (None, Some(_)) => {
                        pending_changes.insert(key, ScopedPendingDelta::Tombstone);
                        pending_gaps.insert(
                            (*version_id, pending_relationship_id.clone()),
                            ResolutionGapKind::Removed,
                        );
                    }
                    (None, None) => {}
                }
            }

            let mut pending = BTreeMap::new();
            for (key, action) in prior_pending_deltas {
                if !pending_touched.contains(&key)
                    && !removed_set.contains(&key.0)
                    && visible_delta_versions.contains(&key.0)
                {
                    if let ScopedPendingDelta::Replacement(row) = &action {
                        target_validator
                            .push(Some(row.target_version_id), Some(&row.target_symbol_id))?;
                    }
                    pending.insert(key, action);
                }
            }
            for row in base_rows.pending.values() {
                if removed_set.contains(&row.version_id) {
                    let key = (row.version_id, row.pending_relationship_id.clone());
                    pending.insert(key, ScopedPendingDelta::Tombstone);
                    pending_gaps.insert(
                        (row.version_id, row.pending_relationship_id.clone()),
                        ResolutionGapKind::Removed,
                    );
                }
            }
            pending.extend(pending_changes);

            for key in &current_keys.pending {
                let row = if pending_touched.contains(key) {
                    touched_pending_values.get(key).and_then(Option::as_ref)
                } else {
                    prior_selected.pending.get(key)
                };
                if let Some(row) = row {
                    target_validator
                        .push(Some(row.target_version_id), Some(&row.target_symbol_id))?;
                }
            }
            for row in base_rows.pending.values() {
                if row.target_version_id.gt(&0)
                    && removed_set.contains(&row.target_version_id)
                    && !removed_set.contains(&row.version_id)
                    && !pending_touched
                        .contains(&(row.version_id, row.pending_relationship_id.clone()))
                    && !prior_pending_keys
                        .contains(&(row.version_id, row.pending_relationship_id.clone()))
                {
                    target_validator
                        .push(Some(row.target_version_id), Some(&row.target_symbol_id))?;
                }
            }

            target_validator.finish()?;
            #[cfg(feature = "test-store-resolution-contract")]
            {
                let mut telemetry = self.scoped_finalization_telemetry.get();
                telemetry.target_validation_queries = telemetry
                    .target_validation_queries
                    .saturating_add(target_validator.query_count);
                telemetry.target_validation_targets = telemetry
                    .target_validation_targets
                    .saturating_add(target_validator.target_count);
                self.scoped_finalization_telemetry.set(telemetry);
            }
            let mut writer = ResolutionScratchWriter::new(
                delta_path,
                self.identity.manifest_hash.clone(),
                self.resolver_output_epoch,
            )?;
            for row in identifiers.into_values() {
                writer.push_identifier_replacement(row)?;
            }
            for ((version_id, pending_relationship_id), action) in pending {
                match action {
                    ScopedPendingDelta::Replacement(row) => {
                        writer.push_pending_replacement(row)?;
                    }
                    ScopedPendingDelta::Tombstone => {
                        writer.push_pending_tombstone(ResolutionPendingTombstone {
                            version_id,
                            pending_relationship_id,
                        })?;
                    }
                }
            }
            let mut gaps = 0_u64;
            for ((version_id, local_id), kind) in identifier_gaps {
                emit_gap(ResolutionGapFact {
                    table: ResolutionGapTable::Identifier,
                    version_id,
                    local_id,
                    kind,
                })?;
                gaps = gaps.saturating_add(1);
            }
            for ((version_id, local_id), kind) in pending_gaps {
                emit_gap(ResolutionGapFact {
                    table: ResolutionGapTable::Pending,
                    version_id,
                    local_id,
                    kind,
                })?;
                gaps = gaps.saturating_add(1);
            }
            let delta = writer.finish()?;
            Ok(ResolutionDiffResult {
                delta,
                gaps,
                max_window_rows,
            })
        })();
        let cleanup = self.remove_scratch();
        match result {
            Ok(result) => {
                cleanup?;
                Ok(result)
            }
            Err(error) => {
                let _ = cleanup;
                Err(error)
            }
        }
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
        let connection = self.open_reader()?;
        connection.set_prepared_statement_cache_capacity(32);
        Ok(connection)
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

    fn candidate_query_started(&self) -> Option<Instant> {
        self.candidate_query_timing_enabled.then(Instant::now)
    }

    fn record_candidate_query(
        &self,
        family: CandidateQueryFamily,
        rows_read: usize,
        started: Option<Instant>,
    ) {
        let elapsed_micros = started
            .map(|started| u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        self.record_candidate_query_elapsed(family, rows_read, elapsed_micros);
    }

    fn record_candidate_query_elapsed(
        &self,
        family: CandidateQueryFamily,
        rows_read: usize,
        elapsed_micros: u64,
    ) {
        let mut telemetry = self.candidate_query_telemetry.get();
        accumulate_candidate_query_telemetry(&mut telemetry, family, rows_read, elapsed_micros);
        self.candidate_query_telemetry.set(telemetry);
    }

    fn observe_logical_lookup<T: Hash>(
        &self,
        family: CandidatePageFamily,
        key: &T,
    ) -> SameWindowFingerprintCounts {
        if !self.candidate_query_timing_enabled {
            return SameWindowFingerprintCounts::default();
        }
        let fingerprint = logical_fingerprint(family, key);
        self.same_window_fingerprints
            .borrow_mut()
            .as_mut()
            .map_or_else(SameWindowFingerprintCounts::default, |tracker| {
                tracker.observe(family, fingerprint)
            })
    }

    fn record_child_lookup_attribution(
        &self,
        reason: Option<ChildLookupReason>,
        fingerprint: SameWindowFingerprintCounts,
    ) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution
            .children_named
            .record_lookup(reason, fingerprint);
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_filtered_lookup_attribution(
        &self,
        reason: Option<FilteredNameLookupReason>,
        fingerprint: SameWindowFingerprintCounts,
    ) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution
            .filtered_by_name
            .record_lookup(reason, fingerprint);
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_top_level_lookup_attribution(
        &self,
        reason: Option<TopLevelLookupReason>,
        fingerprint: SameWindowFingerprintCounts,
    ) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution
            .top_level_named
            .record_lookup(reason, fingerprint);
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_type_facts_lookup_attribution(
        &self,
        reason: Option<TypeFactsLookupReason>,
        fingerprint: SameWindowFingerprintCounts,
    ) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution.type_facts.record_lookup(reason, fingerprint);
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_candidate_page_attribution(
        &self,
        attribution_kind: CandidatePageAttribution,
        row_count: usize,
    ) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        match attribution_kind {
            CandidatePageAttribution::Children {
                reason,
                page_limit,
                had_prior_page,
            } => attribution.children_named.record_page(
                reason,
                row_count,
                page_limit,
                had_prior_page,
            ),
            CandidatePageAttribution::FilteredByName {
                reason,
                page_limit,
                had_prior_page,
            } => attribution.filtered_by_name.record_page(
                reason,
                row_count,
                page_limit,
                had_prior_page,
            ),
            CandidatePageAttribution::TopLevel {
                reason,
                page_limit,
                had_prior_page,
            } => attribution.top_level_named.record_page(
                reason,
                row_count,
                page_limit,
                had_prior_page,
            ),
            CandidatePageAttribution::TypeFacts {
                reason,
                page_limit,
                had_prior_page,
            } => attribution
                .type_facts
                .record_page(reason, row_count, page_limit, had_prior_page),
        }
        self.candidate_cache_attribution.set(attribution);
        if let CandidatePageAttribution::Children {
            reason: Some(reason),
            ..
        } = attribution_kind
        {
            self.record_child_lookup_sql_page(reason);
        }
    }

    fn record_children_named_batch_page(&self, row_count: usize) {
        self.record_candidate_page_attribution(
            CandidatePageAttribution::Children {
                reason: None,
                page_limit: None,
                had_prior_page: false,
            },
            row_count,
        );
    }

    fn record_child_lookup(&self, reason: ChildLookupReason, state: ChildLookupCacheState) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution.children_named.record_call(reason, state);
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_child_lookup_sql_page(&self, reason: ChildLookupReason) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution.children_named.record_sql_page(reason);
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_children_named_batch_count(&self) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution.children_named.batch_count_statements = attribution
            .children_named
            .batch_count_statements
            .saturating_add(1);
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_children_named_batch_fetch(&self) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution.children_named.batch_fetch_statements = attribution
            .children_named
            .batch_fetch_statements
            .saturating_add(1);
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_by_id_cache_hit(&self) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution.by_id.cache_hits = attribution.by_id.cache_hits.saturating_add(1);
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_by_id_sql_miss(&self) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution.by_id.sql_misses = attribution.by_id.sql_misses.saturating_add(1);
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_by_id_accepted_insertion(&self) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution.by_id.accepted_insertions =
            attribution.by_id.accepted_insertions.saturating_add(1);
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_by_id_rejected_by_id_cap(&self) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution.by_id.rejected_by_id_cap =
            attribution.by_id.rejected_by_id_cap.saturating_add(1);
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_by_id_rejected_by_aggregate_cap(&self, count: usize) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution.by_id.rejected_by_aggregate_cap = attribution
            .by_id
            .rejected_by_aggregate_cap
            .saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_candidate_cache_occupancy(&self, window: &CandidateWindow) {
        self.max_candidate_cache_entries.set(
            self.max_candidate_cache_entries
                .get()
                .max(window.entry_count()),
        );
        if !self.candidate_query_timing_enabled {
            return;
        }
        let mut attribution = self.candidate_cache_attribution.get();
        attribution.by_id.max_entries = attribution
            .by_id
            .max_entries
            .max(u64::try_from(window.by_id.len()).unwrap_or(u64::MAX));
        attribution.by_id.max_non_by_id_entries = attribution
            .by_id
            .max_non_by_id_entries
            .max(u64::try_from(window.non_by_id_entry_count()).unwrap_or(u64::MAX));
        attribution.by_id.max_aggregate_entries = attribution
            .by_id
            .max_aggregate_entries
            .max(u64::try_from(window.entry_count()).unwrap_or(u64::MAX));
        self.candidate_cache_attribution.set(attribution);
    }

    fn record_candidate_cache_phase_reset(&self, by_id_len: usize, aggregate_len: usize) {
        if !self.candidate_query_timing_enabled {
            return;
        }
        let by_id_entries = u64::try_from(by_id_len).unwrap_or(u64::MAX);
        let aggregate_entries = u64::try_from(aggregate_len).unwrap_or(u64::MAX);
        let mut attribution = self.candidate_cache_attribution.get();
        attribution.by_id.phase_reset_count = attribution.by_id.phase_reset_count.saturating_add(1);
        attribution.by_id.phase_reset_by_id_entries = by_id_entries;
        attribution.by_id.phase_reset_aggregate_entries = aggregate_entries;
        attribution.by_id.phase_reset_by_id_entries_total = attribution
            .by_id
            .phase_reset_by_id_entries_total
            .saturating_add(by_id_entries);
        attribution.by_id.phase_reset_aggregate_entries_total = attribution
            .by_id
            .phase_reset_aggregate_entries_total
            .saturating_add(aggregate_entries);
        self.candidate_cache_attribution.set(attribution);
    }

    fn reset_candidate_window(&mut self) -> Result<(), StoreResolutionError> {
        let reader = self.open_phase_reader()?;
        let (by_id_len, aggregate_len) = {
            let window = self.candidate_window.get_mut();
            (window.by_id.len(), window.entry_count())
        };
        self.record_candidate_cache_phase_reset(by_id_len, aggregate_len);
        *self.candidate_reader.get_mut() = Some(reader);
        *self.candidate_window.get_mut() = CandidateWindow::default();
        self.resolution_cache.get_mut().clear();
        if let Some(tracker) = self.same_window_fingerprints.get_mut().as_mut() {
            tracker.reset();
        }
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

const SCOPED_TARGET_BATCH: usize = 256;
const SCOPED_DELTA_QUERY_BATCH: usize = 256;

#[derive(Debug)]
enum ScopedPendingDelta {
    Replacement(ResolutionPendingRow),
    Tombstone,
}

#[derive(Default)]
struct ScopedBaseRows {
    identifiers: BTreeMap<(i64, String), ResolutionIdentifierRow>,
    pending: BTreeMap<(i64, String), ResolutionPendingRow>,
    max_page: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    identifier_queries: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    identifier_rows: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    pending_queries: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    pending_rows: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    identifier_target_queries: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    identifier_target_rows: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    pending_target_queries: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    pending_target_rows: usize,
}

#[derive(Default)]
struct ScopedCurrentKeys {
    identifiers: BTreeSet<(i64, String)>,
    pending: BTreeSet<(i64, String)>,
    max_page: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    identifier_queries: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    identifier_rows: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    pending_queries: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    pending_rows: usize,
}

#[derive(Default)]
struct ScopedPriorRows {
    identifiers: BTreeMap<(i64, String), ResolutionIdentifierRow>,
    pending: BTreeMap<(i64, String), ResolutionPendingRow>,
    max_page: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    identifier_queries: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    identifier_rows: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    pending_queries: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    pending_rows: usize,
}

fn scoped_selected_versions(
    telemetry: Option<&StoreResolutionDecisionTelemetry>,
) -> Result<BTreeSet<i64>, StoreResolutionError> {
    let telemetry = telemetry
        .ok_or_else(|| incremental_error("scoped resolution decision telemetry missing"))?;
    let mut versions = BTreeSet::new();
    for version in telemetry
        .worklists
        .selected_versions
        .iter()
        .chain(telemetry.worklists.changed_versions.iter())
    {
        if let SemanticVersionId::Store(version_id) = version {
            versions.insert(*version_id);
        }
    }
    Ok(versions)
}

fn load_scoped_current_keys(
    connection: &Connection,
    identity: &StoreManifestIdentity,
    versions: &BTreeSet<i64>,
) -> Result<ScopedCurrentKeys, StoreResolutionError> {
    let mut keys = ScopedCurrentKeys::default();
    let versions = versions.iter().copied().collect::<Vec<_>>();
    for chunk in versions.chunks(SCOPED_DELTA_QUERY_BATCH) {
        if chunk.is_empty() {
            continue;
        }
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let mut values = vec![
            Value::Text(identity.view_id.clone()),
            Value::Integer(identity.generation),
        ];
        values.extend(chunk.iter().copied().map(Value::Integer));
        let identifier_sql = format!(
            "SELECT i.version_id,i.identifier_id
             FROM identifiers AS i
             JOIN manifest_entries AS manifest
               ON manifest.view_id=?1 AND manifest.generation=?2
              AND manifest.status IN ('indexed','failed_preserved')
              AND manifest.version_id=i.version_id
             WHERE i.version_id IN ({placeholders})
             ORDER BY i.version_id,i.identifier_id COLLATE BINARY"
        );
        let page = connection
            .prepare(&identifier_sql)?
            .query_map(params_from_iter(values.iter()), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        #[cfg(feature = "test-store-resolution-contract")]
        {
            keys.identifier_queries = keys.identifier_queries.saturating_add(1);
            keys.identifier_rows = keys.identifier_rows.saturating_add(page.len());
        }
        keys.max_page = keys.max_page.max(page.len());
        keys.identifiers.extend(page);

        let mut pending_values = vec![
            Value::Text(identity.view_id.clone()),
            Value::Integer(identity.generation),
        ];
        pending_values.extend(chunk.iter().copied().map(Value::Integer));
        let pending_sql = format!(
            "SELECT pending.version_id,pending.pending_relationship_id
             FROM pending_relationships AS pending
             JOIN manifest_entries AS manifest
               ON manifest.view_id=?1 AND manifest.generation=?2
              AND manifest.status IN ('indexed','failed_preserved')
              AND manifest.version_id=pending.version_id
             WHERE pending.version_id IN ({placeholders})
             ORDER BY pending.version_id,pending.pending_relationship_id COLLATE BINARY"
        );
        let page = connection
            .prepare(&pending_sql)?
            .query_map(params_from_iter(pending_values.iter()), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        #[cfg(feature = "test-store-resolution-contract")]
        {
            keys.pending_queries = keys.pending_queries.saturating_add(1);
            keys.pending_rows = keys.pending_rows.saturating_add(page.len());
        }
        keys.max_page = keys.max_page.max(page.len());
        keys.pending.extend(page);
    }
    Ok(keys)
}

fn load_scoped_prior_rows(
    prior: &PriorOverlayReader,
    versions: &BTreeSet<i64>,
    window_size: usize,
) -> Result<ScopedPriorRows, StoreResolutionError> {
    let mut rows = ScopedPriorRows::default();
    let versions = versions.iter().copied().collect::<Vec<_>>();
    for chunk in versions.chunks(SCOPED_DELTA_QUERY_BATCH) {
        if chunk.is_empty() {
            continue;
        }
        let mut after = None;
        loop {
            let page = match prior
                .identifiers_by_files(chunk, after.as_ref(), window_size)
                .map_err(|error| incremental_error(error.to_string()))?
            {
                PriorOverlayAccess::Ready(page) => page,
                PriorOverlayAccess::FullFallback(fallback) => {
                    return Err(incremental_error(format!(
                        "prior overlay changed during scoped validation: {fallback:?}"
                    )));
                }
            };
            #[cfg(feature = "test-store-resolution-contract")]
            {
                rows.identifier_queries = rows.identifier_queries.saturating_add(1);
                rows.identifier_rows = rows.identifier_rows.saturating_add(page.rows.len());
            }
            rows.max_page = rows.max_page.max(page.rows.len());
            for row in page.rows {
                rows.identifiers
                    .insert((row.version_id, row.identifier_id.clone()), row);
            }
            let Some(next) = page.next else { break };
            after = Some(next);
        }

        let mut after = None;
        loop {
            let page = match prior
                .pending_by_files(chunk, after.as_ref(), window_size)
                .map_err(|error| incremental_error(error.to_string()))?
            {
                PriorOverlayAccess::Ready(page) => page,
                PriorOverlayAccess::FullFallback(fallback) => {
                    return Err(incremental_error(format!(
                        "prior overlay changed during scoped validation: {fallback:?}"
                    )));
                }
            };
            #[cfg(feature = "test-store-resolution-contract")]
            {
                rows.pending_queries = rows.pending_queries.saturating_add(1);
                rows.pending_rows = rows.pending_rows.saturating_add(page.rows.len());
            }
            rows.max_page = rows.max_page.max(page.rows.len());
            for row in page.rows {
                rows.pending
                    .insert((row.version_id, row.pending_relationship_id.clone()), row);
            }
            let Some(next) = page.next else { break };
            after = Some(next);
        }
    }
    Ok(rows)
}

fn load_scoped_base_rows(
    layout: &StoreLayout,
    state: &ResolutionScopeState,
    identifier_keys: &[(i64, String)],
    pending_keys: &[(i64, String)],
    removed_versions: &[i64],
) -> Result<ScopedBaseRows, StoreResolutionError> {
    let base_path = layout
        .generation_dir()
        .join("bases")
        .join(format!("{}.db", state.base_id));
    let connection = Connection::open_with_flags(
        base_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch("PRAGMA query_only=ON")?;
    let mut rows = ScopedBaseRows::default();
    load_scoped_base_identifier_keys(&connection, identifier_keys, &mut rows)?;
    load_scoped_base_pending_keys(&connection, pending_keys, &mut rows)?;
    load_scoped_base_identifier_versions(&connection, removed_versions, &mut rows)?;
    load_scoped_base_pending_versions(&connection, removed_versions, &mut rows)?;
    load_scoped_base_identifier_targets(&connection, removed_versions, &mut rows)?;
    load_scoped_base_pending_targets(&connection, removed_versions, &mut rows)?;
    Ok(rows)
}

fn load_scoped_base_identifier_keys(
    connection: &Connection,
    keys: &[(i64, String)],
    rows: &mut ScopedBaseRows,
) -> Result<(), StoreResolutionError> {
    for chunk in keys.chunks(SCOPED_DELTA_QUERY_BATCH) {
        if chunk.is_empty() {
            continue;
        }
        let values = key_values_clause(chunk.len());
        let sql = format!(
            "WITH wanted(version_id,identifier_id) AS (VALUES {values})
             SELECT source.version_id,source.identifier_id,source.target_version_id,
                    source.target_symbol_id,source.tier,source.confidence,source.method,
                    source.outcome,source.candidates
             FROM wanted
             JOIN identifier_resolutions AS source
               ON source.version_id=wanted.version_id
              AND source.identifier_id=wanted.identifier_id
             ORDER BY source.version_id,source.identifier_id COLLATE BINARY"
        );
        let page = connection
            .prepare(&sql)?
            .query_map(params_from_iter(key_params(chunk)), |row| {
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
        #[cfg(feature = "test-store-resolution-contract")]
        {
            rows.identifier_queries = rows.identifier_queries.saturating_add(1);
            rows.identifier_rows = rows.identifier_rows.saturating_add(page.len());
        }
        rows.max_page = rows.max_page.max(page.len());
        for row in page {
            rows.identifiers
                .insert((row.version_id, row.identifier_id.clone()), row);
        }
    }
    Ok(())
}

fn load_scoped_base_pending_keys(
    connection: &Connection,
    keys: &[(i64, String)],
    rows: &mut ScopedBaseRows,
) -> Result<(), StoreResolutionError> {
    for chunk in keys.chunks(SCOPED_DELTA_QUERY_BATCH) {
        if chunk.is_empty() {
            continue;
        }
        let values = key_values_clause(chunk.len());
        let sql = format!(
            "WITH wanted(version_id,pending_relationship_id) AS (VALUES {values})
             SELECT source.version_id,source.pending_relationship_id,source.target_version_id,
                    source.target_symbol_id,source.tier,source.confidence,source.method
             FROM wanted
             JOIN pending_resolutions AS source
               ON source.version_id=wanted.version_id
              AND source.pending_relationship_id=wanted.pending_relationship_id
             ORDER BY source.version_id,source.pending_relationship_id COLLATE BINARY"
        );
        let page = connection
            .prepare(&sql)?
            .query_map(params_from_iter(key_params(chunk)), |row| {
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
        #[cfg(feature = "test-store-resolution-contract")]
        {
            rows.pending_queries = rows.pending_queries.saturating_add(1);
            rows.pending_rows = rows.pending_rows.saturating_add(page.len());
        }
        rows.max_page = rows.max_page.max(page.len());
        for row in page {
            rows.pending
                .insert((row.version_id, row.pending_relationship_id.clone()), row);
        }
    }
    Ok(())
}

fn load_scoped_base_identifier_versions(
    connection: &Connection,
    versions: &[i64],
    rows: &mut ScopedBaseRows,
) -> Result<(), StoreResolutionError> {
    for chunk in versions.chunks(SCOPED_DELTA_QUERY_BATCH) {
        if chunk.is_empty() {
            continue;
        }
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT version_id,identifier_id,target_version_id,target_symbol_id,tier,
                    confidence,method,outcome,candidates
             FROM identifier_resolutions
             WHERE version_id IN ({placeholders})
             ORDER BY version_id,identifier_id COLLATE BINARY"
        );
        let page = connection
            .prepare(&sql)?
            .query_map(params_from_iter(chunk.iter().copied()), |row| {
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
        #[cfg(feature = "test-store-resolution-contract")]
        {
            rows.identifier_queries = rows.identifier_queries.saturating_add(1);
            rows.identifier_rows = rows.identifier_rows.saturating_add(page.len());
        }
        rows.max_page = rows.max_page.max(page.len());
        for row in page {
            rows.identifiers
                .insert((row.version_id, row.identifier_id.clone()), row);
        }
    }
    Ok(())
}

fn load_scoped_base_pending_versions(
    connection: &Connection,
    versions: &[i64],
    rows: &mut ScopedBaseRows,
) -> Result<(), StoreResolutionError> {
    for chunk in versions.chunks(SCOPED_DELTA_QUERY_BATCH) {
        if chunk.is_empty() {
            continue;
        }
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT version_id,pending_relationship_id,target_version_id,target_symbol_id,
                    tier,confidence,method
             FROM pending_resolutions
             WHERE version_id IN ({placeholders})
             ORDER BY version_id,pending_relationship_id COLLATE BINARY"
        );
        let page = connection
            .prepare(&sql)?
            .query_map(params_from_iter(chunk.iter().copied()), |row| {
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
        #[cfg(feature = "test-store-resolution-contract")]
        {
            rows.pending_queries = rows.pending_queries.saturating_add(1);
            rows.pending_rows = rows.pending_rows.saturating_add(page.len());
        }
        rows.max_page = rows.max_page.max(page.len());
        for row in page {
            rows.pending
                .insert((row.version_id, row.pending_relationship_id.clone()), row);
        }
    }
    Ok(())
}

fn load_scoped_base_identifier_targets(
    connection: &Connection,
    target_versions: &[i64],
    rows: &mut ScopedBaseRows,
) -> Result<(), StoreResolutionError> {
    for chunk in target_versions.chunks(SCOPED_DELTA_QUERY_BATCH) {
        if chunk.is_empty() {
            continue;
        }
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT version_id,identifier_id,target_version_id,target_symbol_id,tier,
                    confidence,method,outcome,candidates
             FROM identifier_resolutions
             WHERE target_version_id IN ({placeholders})
             ORDER BY version_id,identifier_id COLLATE BINARY"
        );
        let page = connection
            .prepare(&sql)?
            .query_map(params_from_iter(chunk.iter().copied()), |row| {
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
        #[cfg(feature = "test-store-resolution-contract")]
        {
            rows.identifier_target_queries = rows.identifier_target_queries.saturating_add(1);
            rows.identifier_target_rows = rows.identifier_target_rows.saturating_add(page.len());
        }
        rows.max_page = rows.max_page.max(page.len());
        for row in page {
            rows.identifiers
                .insert((row.version_id, row.identifier_id.clone()), row);
        }
    }
    Ok(())
}

fn load_scoped_base_pending_targets(
    connection: &Connection,
    target_versions: &[i64],
    rows: &mut ScopedBaseRows,
) -> Result<(), StoreResolutionError> {
    for chunk in target_versions.chunks(SCOPED_DELTA_QUERY_BATCH) {
        if chunk.is_empty() {
            continue;
        }
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT version_id,pending_relationship_id,target_version_id,target_symbol_id,
                    tier,confidence,method
             FROM pending_resolutions
             WHERE target_version_id IN ({placeholders})
             ORDER BY version_id,pending_relationship_id COLLATE BINARY"
        );
        let page = connection
            .prepare(&sql)?
            .query_map(params_from_iter(chunk.iter().copied()), |row| {
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
        #[cfg(feature = "test-store-resolution-contract")]
        {
            rows.pending_target_queries = rows.pending_target_queries.saturating_add(1);
            rows.pending_target_rows = rows.pending_target_rows.saturating_add(page.len());
        }
        rows.max_page = rows.max_page.max(page.len());
        for row in page {
            rows.pending
                .insert((row.version_id, row.pending_relationship_id.clone()), row);
        }
    }
    Ok(())
}

fn removed_resolution_versions(
    connection: &Connection,
    state: &ResolutionScopeState,
    identity: &StoreManifestIdentity,
) -> Result<Vec<i64>, StoreResolutionError> {
    let mut statement = connection.prepare(
        "WITH RECURSIVE chain(
             transition_id,previous_transition_id,from_manifest_generation,
             from_manifest_hash,to_manifest_generation,to_manifest_hash
         ) AS (
             SELECT transition_id,previous_transition_id,from_manifest_generation,
                    from_manifest_hash,to_manifest_generation,to_manifest_hash
             FROM resolution_scope_batches
             WHERE view_id=?1 AND transition_id=?2
             UNION ALL
             SELECT previous.transition_id,previous.previous_transition_id,
                    previous.from_manifest_generation,previous.from_manifest_hash,
                    previous.to_manifest_generation,previous.to_manifest_hash
             FROM resolution_scope_batches AS previous
             JOIN chain AS current
               ON current.previous_transition_id=previous.transition_id
              AND previous.view_id=?1
             WHERE NOT (
                 current.from_manifest_generation=?3
                 AND current.from_manifest_hash=?4
             )
         )
         SELECT DISTINCT journal.old_version_id
         FROM chain
         JOIN resolution_scope_journal AS journal
           ON journal.transition_id=chain.transition_id
         JOIN resolution_base_versions AS roots
           ON roots.base_id=?5 AND roots.version_id=journal.old_version_id
         LEFT JOIN manifest_entries AS current
           ON current.view_id=?6 AND current.generation=?7
          AND current.version_id=journal.old_version_id
          AND current.status IN ('indexed','failed_preserved')
         WHERE journal.old_version_id IS NOT NULL
           AND current.version_id IS NULL
         ORDER BY journal.old_version_id",
    )?;
    Ok(statement
        .query_map(
            params![
                state.view_id,
                state.journal_through_transition_id,
                state.predecessor_manifest_generation,
                state.predecessor_manifest_hash,
                state.base_id,
                identity.view_id,
                identity.generation,
            ],
            |row| row.get::<_, i64>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?)
}

fn visible_versions_for_keys(
    connection: &Connection,
    identity: &StoreManifestIdentity,
    versions: &BTreeSet<i64>,
) -> Result<BTreeSet<i64>, StoreResolutionError> {
    let mut visible = BTreeSet::new();
    let versions = versions.iter().copied().collect::<Vec<_>>();
    for chunk in versions.chunks(SCOPED_DELTA_QUERY_BATCH) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT version_id
             FROM manifest_entries
             WHERE view_id=?1 AND generation=?2
               AND status IN ('indexed','failed_preserved')
               AND version_id IN ({placeholders})"
        );
        let mut values = vec![
            Value::Text(identity.view_id.clone()),
            Value::Integer(identity.generation),
        ];
        values.extend(chunk.iter().copied().map(Value::Integer));
        let rows = connection
            .prepare(&sql)?
            .query_map(params_from_iter(values), |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        visible.extend(rows);
    }
    Ok(visible)
}

fn base_versions_for_keys(
    connection: &Connection,
    state: &ResolutionScopeState,
    versions: &BTreeSet<i64>,
) -> Result<BTreeSet<i64>, StoreResolutionError> {
    let mut base_versions = BTreeSet::new();
    let versions = versions.iter().copied().collect::<Vec<_>>();
    for chunk in versions.chunks(SCOPED_DELTA_QUERY_BATCH) {
        if chunk.is_empty() {
            continue;
        }
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT version_id
             FROM resolution_base_versions
             WHERE base_id=?1 AND version_id IN ({placeholders})"
        );
        let mut values = vec![Value::Text(state.base_id.clone())];
        values.extend(chunk.iter().copied().map(Value::Integer));
        let rows = connection
            .prepare(&sql)?
            .query_map(params_from_iter(values), |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        base_versions.extend(rows);
    }
    Ok(base_versions)
}

fn load_prior_identifier_deltas(
    connection: &Connection,
    state: &ResolutionScopeState,
    window_size: usize,
) -> Result<PriorIdentifierDeltas, StoreResolutionError> {
    let limit =
        i64::try_from(window_size).map_err(|_| StoreResolutionError::InvalidWindowSize {
            requested: window_size,
            maximum: MAX_STORE_RESOLUTION_WINDOW,
        })?;
    let mut rows_by_key = BTreeMap::new();
    let mut after = (0, String::new());
    let mut max_page = 0;
    loop {
        let mut statement = connection.prepare(
            "SELECT version_id,identifier_id,target_version_id,target_symbol_id,tier,
                    confidence,method,outcome,candidates
             FROM resolution_identifier_deltas
             WHERE view_id=?1 AND delta_generation=?2
               AND (version_id,identifier_id)>(?3,?4)
             ORDER BY version_id,identifier_id COLLATE BINARY LIMIT ?5",
        )?;
        let page = statement
            .query_map(
                params![
                    state.view_id,
                    state.delta_generation,
                    after.0,
                    after.1,
                    limit
                ],
                |row| {
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
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        if page.is_empty() {
            break;
        }
        max_page = max_page.max(page.len());
        for row in page {
            after = (row.version_id, row.identifier_id.clone());
            rows_by_key.insert((row.version_id, row.identifier_id.clone()), row);
        }
    }
    Ok((rows_by_key, max_page))
}

fn load_prior_pending_deltas(
    connection: &Connection,
    state: &ResolutionScopeState,
    window_size: usize,
) -> Result<PriorPendingDeltas, StoreResolutionError> {
    let limit =
        i64::try_from(window_size).map_err(|_| StoreResolutionError::InvalidWindowSize {
            requested: window_size,
            maximum: MAX_STORE_RESOLUTION_WINDOW,
        })?;
    let mut rows_by_key = BTreeMap::new();
    let mut after = (0, String::new());
    let mut max_page = 0;
    loop {
        let mut statement = connection.prepare(
            "SELECT version_id,pending_relationship_id,operation,target_version_id,
                    target_symbol_id,tier,confidence,method
             FROM resolution_pending_deltas
             WHERE view_id=?1 AND delta_generation=?2
               AND (version_id,pending_relationship_id)>(?3,?4)
             ORDER BY version_id,pending_relationship_id COLLATE BINARY LIMIT ?5",
        )?;
        let page = statement
            .query_map(
                params![
                    state.view_id,
                    state.delta_generation,
                    after.0,
                    after.1,
                    limit
                ],
                |row| {
                    let version_id = row.get::<_, i64>(0)?;
                    let pending_relationship_id = row.get::<_, String>(1)?;
                    let operation = row.get::<_, String>(2)?;
                    let action = match operation.as_str() {
                        "replace" => ScopedPendingDelta::Replacement(ResolutionPendingRow {
                            version_id,
                            pending_relationship_id: pending_relationship_id.clone(),
                            target_version_id: row
                                .get::<_, Option<i64>>(3)?
                                .ok_or(rusqlite::Error::InvalidQuery)?,
                            target_symbol_id: row
                                .get::<_, Option<String>>(4)?
                                .ok_or(rusqlite::Error::InvalidQuery)?,
                            tier: row
                                .get::<_, Option<i64>>(5)?
                                .ok_or(rusqlite::Error::InvalidQuery)?,
                            confidence: row
                                .get::<_, Option<f64>>(6)?
                                .ok_or(rusqlite::Error::InvalidQuery)?,
                            method: row
                                .get::<_, Option<String>>(7)?
                                .ok_or(rusqlite::Error::InvalidQuery)?,
                        }),
                        "tombstone" => ScopedPendingDelta::Tombstone,
                        _ => return Err(rusqlite::Error::InvalidQuery),
                    };
                    Ok(((version_id, pending_relationship_id), action))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        if page.is_empty() {
            break;
        }
        max_page = max_page.max(page.len());
        for (key, action) in page {
            after = key.clone();
            rows_by_key.insert(key, action);
        }
    }
    Ok((rows_by_key, max_page))
}

fn load_prior_gap_facts(
    connection: &Connection,
    state: &ResolutionScopeState,
) -> Result<PriorGapFacts, StoreResolutionError> {
    let (declared_rows, declared_files, payload) = connection.query_row(
        "SELECT exact_gap_rows,exact_gap_files,exact_gap_json
         FROM resolution_deltas
         WHERE view_id=?1 AND delta_generation=?2",
        params![state.view_id, state.delta_generation],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    let declared_rows = usize::try_from(declared_rows).map_err(|_| {
        StoreResolutionError::Artifact(ResolutionValidationError::InvalidMetadata {
            key: "exact_gap_rows".to_string(),
            value: "declared prior gap row count is negative or out of range".to_string(),
        })
    })?;
    let declared_files = usize::try_from(declared_files).map_err(|_| {
        StoreResolutionError::Artifact(ResolutionValidationError::InvalidMetadata {
            key: "exact_gap_files".to_string(),
            value: "declared prior gap file count is negative or out of range".to_string(),
        })
    })?;
    let value = serde_json::from_str::<serde_json::Value>(&payload).map_err(|_| {
        StoreResolutionError::Artifact(ResolutionValidationError::InvalidMetadata {
            key: "exact_gap_json".to_string(),
            value: "prior gap payload is not valid JSON".to_string(),
        })
    })?;
    let rows = value
        .get("rows")
        .and_then(serde_json::Value::as_array)
        .or_else(|| value.as_array())
        .cloned()
        .ok_or_else(|| {
            StoreResolutionError::Artifact(ResolutionValidationError::InvalidMetadata {
                key: "exact_gap_json".to_string(),
                value: "prior gap payload is not an object with rows".to_string(),
            })
        })?;
    let declared_file_ids = value
        .get("files")
        .map(|files| {
            let files = files.as_array().ok_or_else(|| {
                StoreResolutionError::Artifact(ResolutionValidationError::InvalidMetadata {
                    key: "exact_gap_json".to_string(),
                    value: "prior gap files is not an array".to_string(),
                })
            })?;
            let mut ids = BTreeSet::new();
            for file in files {
                let file_id = file.as_i64().ok_or_else(|| {
                    StoreResolutionError::Artifact(ResolutionValidationError::InvalidMetadata {
                        key: "exact_gap_json".to_string(),
                        value: "prior gap file id is not an integer".to_string(),
                    })
                })?;
                if file_id <= 0 || !ids.insert(file_id) {
                    return Err(StoreResolutionError::Artifact(
                        ResolutionValidationError::InvalidMetadata {
                            key: "exact_gap_json".to_string(),
                            value: "prior gap files contains an invalid or duplicate id"
                                .to_string(),
                        },
                    ));
                }
            }
            Ok(ids)
        })
        .transpose()?
        .unwrap_or_default();
    let mut identifiers = BTreeMap::new();
    let mut pending = BTreeMap::new();
    let mut parsed_file_ids = BTreeSet::new();
    for row in rows {
        let table = row.get("table").and_then(serde_json::Value::as_str);
        let kind = row.get("kind").and_then(serde_json::Value::as_str);
        let version_id = row.get("version_id").and_then(serde_json::Value::as_i64);
        let local_id = row
            .get("local_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        let (Some(table), Some(kind), Some(version_id), Some(local_id)) =
            (table, kind, version_id, local_id)
        else {
            return Err(StoreResolutionError::Artifact(
                ResolutionValidationError::InvalidMetadata {
                    key: "exact_gap_json".to_string(),
                    value: "prior gap row is incomplete".to_string(),
                },
            ));
        };
        if version_id <= 0 {
            return Err(StoreResolutionError::Artifact(
                ResolutionValidationError::InvalidMetadata {
                    key: "exact_gap_json".to_string(),
                    value: "prior gap row has an invalid version id".to_string(),
                },
            ));
        }
        parsed_file_ids.insert(version_id);
        let kind = match kind {
            "added" => ResolutionGapKind::Added,
            "replaced" => ResolutionGapKind::Replaced,
            "removed" => ResolutionGapKind::Removed,
            _ => {
                return Err(StoreResolutionError::Artifact(
                    ResolutionValidationError::InvalidMetadata {
                        key: "exact_gap_json".to_string(),
                        value: format!("unknown prior gap kind {kind:?}"),
                    },
                ));
            }
        };
        match table {
            "identifier" => {
                if identifiers.insert((version_id, local_id), kind).is_some() {
                    return Err(StoreResolutionError::Artifact(
                        ResolutionValidationError::InvalidMetadata {
                            key: "exact_gap_json".to_string(),
                            value: "prior gap rows contain a duplicate identifier key".to_string(),
                        },
                    ));
                }
            }
            "pending" => {
                if pending.insert((version_id, local_id), kind).is_some() {
                    return Err(StoreResolutionError::Artifact(
                        ResolutionValidationError::InvalidMetadata {
                            key: "exact_gap_json".to_string(),
                            value: "prior gap rows contain a duplicate pending key".to_string(),
                        },
                    ));
                }
            }
            _ => {
                return Err(StoreResolutionError::Artifact(
                    ResolutionValidationError::InvalidMetadata {
                        key: "exact_gap_json".to_string(),
                        value: format!("unknown prior gap table {table:?}"),
                    },
                ));
            }
        }
    }
    let parsed_rows = identifiers.len().saturating_add(pending.len());
    if parsed_rows != declared_rows {
        return Err(StoreResolutionError::Artifact(
            ResolutionValidationError::InvalidMetadata {
                key: "exact_gap_rows".to_string(),
                value: format!(
                    "declared prior gap row count {declared_rows} does not match parsed {parsed_rows}"
                ),
            },
        ));
    }
    if parsed_file_ids.len() != declared_files {
        return Err(StoreResolutionError::Artifact(
            ResolutionValidationError::InvalidMetadata {
                key: "exact_gap_files".to_string(),
                value: format!(
                    "declared prior gap file count {declared_files} does not match parsed {}",
                    parsed_file_ids.len()
                ),
            },
        ));
    }
    if !declared_file_ids.is_empty() && declared_file_ids != parsed_file_ids {
        return Err(StoreResolutionError::Artifact(
            ResolutionValidationError::InvalidMetadata {
                key: "exact_gap_json".to_string(),
                value: "declared prior gap files do not match row versions".to_string(),
            },
        ));
    }
    Ok((identifiers, pending))
}

struct ScopedTargetValidator<'a> {
    connection: &'a Connection,
    identity: &'a StoreManifestIdentity,
    targets: BTreeSet<(i64, String)>,
    #[cfg(feature = "test-store-resolution-contract")]
    query_count: usize,
    #[cfg(feature = "test-store-resolution-contract")]
    target_count: usize,
}

impl<'a> ScopedTargetValidator<'a> {
    fn new(connection: &'a Connection, identity: &'a StoreManifestIdentity) -> Self {
        Self {
            connection,
            identity,
            targets: BTreeSet::new(),
            #[cfg(feature = "test-store-resolution-contract")]
            query_count: 0,
            #[cfg(feature = "test-store-resolution-contract")]
            target_count: 0,
        }
    }

    fn push(
        &mut self,
        target_version_id: Option<i64>,
        target_symbol_id: Option<&str>,
    ) -> Result<(), StoreResolutionError> {
        let (Some(target_version_id), Some(target_symbol_id)) =
            (target_version_id, target_symbol_id)
        else {
            return Ok(());
        };
        let inserted = self
            .targets
            .insert((target_version_id, target_symbol_id.to_string()));
        #[cfg(feature = "test-store-resolution-contract")]
        if inserted {
            self.target_count = self.target_count.saturating_add(1);
        }
        #[cfg(not(feature = "test-store-resolution-contract"))]
        let _ = inserted;
        if self.targets.len() >= SCOPED_TARGET_BATCH {
            self.flush()?;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), StoreResolutionError> {
        self.flush()
    }

    fn flush(&mut self) -> Result<(), StoreResolutionError> {
        if self.targets.is_empty() {
            return Ok(());
        }
        let targets = std::mem::take(&mut self.targets);
        #[cfg(feature = "test-store-resolution-contract")]
        {
            self.query_count = self.query_count.saturating_add(1);
        }
        let values = std::iter::repeat_n("(?,?)", targets.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "WITH targets(version_id,symbol_id) AS (VALUES {values})
             SELECT targets.version_id,targets.symbol_id
             FROM targets
             WHERE NOT EXISTS (
               SELECT 1 FROM symbols AS s
               WHERE s.version_id=targets.version_id
                 AND s.symbol_id=targets.symbol_id
                 AND EXISTS (
                   SELECT 1 FROM manifest_entries AS me
                   WHERE me.view_id=?
                     AND me.generation=?
                     AND me.status IN ('indexed','failed_preserved')
                     AND me.version_id=s.version_id
                 )
             )
             ORDER BY targets.version_id,targets.symbol_id COLLATE BINARY
             LIMIT 1"
        );
        let mut bind = Vec::with_capacity(targets.len() * 2 + 2);
        for (version_id, symbol_id) in targets {
            bind.push(Value::Integer(version_id));
            bind.push(Value::Text(symbol_id));
        }
        bind.push(Value::Text(self.identity.view_id.clone()));
        bind.push(Value::Integer(self.identity.generation));
        let missing = self
            .connection
            .query_row(&sql, params_from_iter(bind), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .optional()?;
        if let Some((version_id, symbol_id)) = missing {
            return Err(StoreResolutionError::Artifact(
                ResolutionValidationError::TargetMissing {
                    version_id,
                    symbol_id,
                },
            ));
        }
        Ok(())
    }
}

struct ScopedTouchedIdentifier {
    version_id: i64,
    identifier_id: String,
    row: Option<ResolutionIdentifierRow>,
}

struct ScopedTouchedIdentifierCursor<'a> {
    scratch: &'a Connection,
    window_size: usize,
    rows: VecDeque<ScopedTouchedIdentifier>,
    after: Option<(i64, String)>,
    max_page: usize,
}

impl<'a> ScopedTouchedIdentifierCursor<'a> {
    fn new(scratch: &'a Connection, window_size: usize) -> Self {
        Self {
            scratch,
            window_size,
            rows: VecDeque::new(),
            after: None,
            max_page: 0,
        }
    }

    fn next(&mut self) -> Result<Option<ScopedTouchedIdentifier>, StoreResolutionError> {
        if self.rows.is_empty() {
            let limit = i64::try_from(self.window_size).map_err(|_| {
                StoreResolutionError::InvalidWindowSize {
                    requested: self.window_size,
                    maximum: MAX_STORE_RESOLUTION_WINDOW,
                }
            })?;
            let (after_version, after_id) = self
                .after
                .as_ref()
                .map_or((0, ""), |(version, id)| (*version, id.as_str()));
            let mut statement = self.scratch.prepare(
                "SELECT touched.version_id,touched.identifier_id,
                        resolved.version_id,resolved.identifier_id,resolved.target_version_id,
                        resolved.target_symbol_id,resolved.tier,resolved.confidence,
                        resolved.method,resolved.outcome,resolved.candidates
                 FROM identifier_touched AS touched
                 JOIN visible_versions AS visible ON visible.version_id=touched.version_id
                 LEFT JOIN identifier_resolutions AS resolved
                   ON resolved.version_id=touched.version_id
                  AND resolved.identifier_id=touched.identifier_id
                 WHERE (touched.version_id,touched.identifier_id)>(?1,?2)
                 ORDER BY touched.version_id,touched.identifier_id COLLATE BINARY LIMIT ?3",
            )?;
            let page = statement
                .query_map(params![after_version, after_id, limit], |row| {
                    let version_id = row.get(0)?;
                    let identifier_id = row.get(1)?;
                    let row = row
                        .get::<_, Option<i64>>(2)?
                        .map(|resolved_version| {
                            Ok::<ResolutionIdentifierRow, rusqlite::Error>(
                                ResolutionIdentifierRow {
                                    version_id: resolved_version,
                                    identifier_id: row.get(3)?,
                                    target_version_id: row.get(4)?,
                                    target_symbol_id: row.get(5)?,
                                    tier: row.get(6)?,
                                    confidence: row.get(7)?,
                                    method: row.get(8)?,
                                    outcome: row.get(9)?,
                                    candidates: row.get(10)?,
                                },
                            )
                        })
                        .transpose()?;
                    Ok(ScopedTouchedIdentifier {
                        version_id,
                        identifier_id,
                        row,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
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

struct ScopedTouchedPending {
    version_id: i64,
    pending_relationship_id: String,
    row: Option<ResolutionPendingRow>,
}

struct ScopedTouchedPendingCursor<'a> {
    scratch: &'a Connection,
    window_size: usize,
    rows: VecDeque<ScopedTouchedPending>,
    after: Option<(i64, String)>,
    max_page: usize,
}

impl<'a> ScopedTouchedPendingCursor<'a> {
    fn new(scratch: &'a Connection, window_size: usize) -> Self {
        Self {
            scratch,
            window_size,
            rows: VecDeque::new(),
            after: None,
            max_page: 0,
        }
    }

    fn next(&mut self) -> Result<Option<ScopedTouchedPending>, StoreResolutionError> {
        if self.rows.is_empty() {
            let limit = i64::try_from(self.window_size).map_err(|_| {
                StoreResolutionError::InvalidWindowSize {
                    requested: self.window_size,
                    maximum: MAX_STORE_RESOLUTION_WINDOW,
                }
            })?;
            let (after_version, after_id) = self
                .after
                .as_ref()
                .map_or((0, ""), |(version, id)| (*version, id.as_str()));
            let mut statement = self.scratch.prepare(
                "SELECT touched.version_id,touched.pending_relationship_id,
                        resolved.version_id,resolved.pending_relationship_id,
                        resolved.target_version_id,resolved.target_symbol_id,
                        resolved.tier,resolved.confidence,resolved.method
                 FROM pending_touched AS touched
                 JOIN visible_versions AS visible ON visible.version_id=touched.version_id
                 LEFT JOIN pending_resolutions AS resolved
                   ON resolved.version_id=touched.version_id
                  AND resolved.pending_relationship_id=touched.pending_relationship_id
                 WHERE (touched.version_id,touched.pending_relationship_id)>(?1,?2)
                 ORDER BY touched.version_id,touched.pending_relationship_id COLLATE BINARY LIMIT ?3",
            )?;
            let page = statement
                .query_map(params![after_version, after_id, limit], |row| {
                    let version_id = row.get(0)?;
                    let pending_relationship_id = row.get(1)?;
                    let row = row
                        .get::<_, Option<i64>>(2)?
                        .map(|resolved_version| {
                            Ok::<ResolutionPendingRow, rusqlite::Error>(ResolutionPendingRow {
                                version_id: resolved_version,
                                pending_relationship_id: row.get(3)?,
                                target_version_id: row.get(4)?,
                                target_symbol_id: row.get(5)?,
                                tier: row.get(6)?,
                                confidence: row.get(7)?,
                                method: row.get(8)?,
                            })
                        })
                        .transpose()?;
                    Ok(ScopedTouchedPending {
                        version_id,
                        pending_relationship_id,
                        row,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
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
            self.record_by_id_cache_hit();
            return Ok(hit.clone());
        }
        self.record_by_id_sql_miss();
        let started = self.candidate_query_started();
        let row = self.with_candidate_reader(|connection| {
            let mut statement = connection.prepare_cached(
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
            )?;
            Ok(statement
                .query_row(
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
        self.record_candidate_query(
            CandidateQueryFamily::SymbolById,
            usize::from(row.is_some()),
            started,
        );
        let hit = row.flatten();
        let mut window = self.candidate_window.borrow_mut();
        let by_id_cap_reached = window.by_id.len() >= self.window_size;
        let aggregate_cap_reached = window.entry_count() >= self.candidate_cache_capacity();
        if by_id_cap_reached {
            self.record_by_id_rejected_by_id_cap();
        }
        if aggregate_cap_reached {
            self.record_by_id_rejected_by_aggregate_cap(1);
        }
        if !by_id_cap_reached && !aggregate_cap_reached {
            window.by_id.insert(semantic_id, hit.clone());
            self.record_by_id_accepted_insertion();
        }
        self.record_candidate_cache_occupancy(&window);
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
                self.window_size,
                None,
            )?;
            for hit in page {
                if !visitor(self, hit)? {
                    return Ok(());
                }
            }
            let Some(next) = next else {
                break;
            };
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
        visitor: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, Self::Error>,
    {
        self.visit_filtered_by_name_inner(None, name, language, kinds, source_key, visitor)
    }

    fn visit_filtered_by_name_with_reason<F>(
        &self,
        reason: FilteredNameLookupReason,
        name: &str,
        language: &str,
        kinds: &[SymbolKind],
        source_key: Option<&str>,
        visitor: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, Self::Error>,
    {
        self.visit_filtered_by_name_inner(Some(reason), name, language, kinds, source_key, visitor)
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
        let started = self.candidate_query_started();
        let rows = self.with_candidate_reader(|connection| {
            let mut statement = connection.prepare_cached(&sql)?;
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
        self.record_candidate_query(
            CandidateQueryFamily::FilteredNameSummary,
            rows.len(),
            started,
        );
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
        visitor: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, Self::Error>,
    {
        self.visit_children_named_inner(None, source_key, parent_id, name, visitor)
    }

    fn visit_children_named_with_reason<F>(
        &self,
        reason: ChildLookupReason,
        source_key: &str,
        parent_id: &str,
        name: &str,
        visitor: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, Self::Error>,
    {
        self.visit_children_named_inner(Some(reason), source_key, parent_id, name, visitor)
    }

    fn visit_top_level_named<F>(
        &self,
        source_key: &str,
        name: &str,
        visitor: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, Self::Error>,
    {
        self.visit_top_level_named_inner(None, source_key, name, visitor)
    }

    fn visit_top_level_named_with_reason<F>(
        &self,
        reason: TopLevelLookupReason,
        source_key: &str,
        name: &str,
        visitor: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, Self::Error>,
    {
        self.visit_top_level_named_inner(Some(reason), source_key, name, visitor)
    }

    fn visit_type_facts<F>(
        &self,
        symbol_id: &SemanticSymbolId,
        visitor: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, TypeFact) -> Result<bool, Self::Error>,
    {
        self.visit_type_facts_inner(None, symbol_id, visitor)
    }

    fn visit_type_facts_with_reason<F>(
        &self,
        reason: TypeFactsLookupReason,
        symbol_id: &SemanticSymbolId,
        visitor: F,
    ) -> Result<(), Self::Error>
    where
        F: FnMut(&Self, TypeFact) -> Result<bool, Self::Error>,
    {
        self.visit_type_facts_inner(Some(reason), symbol_id, visitor)
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
            let started = self.candidate_query_started();
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
            self.record_candidate_query(CandidateQueryFamily::Imports, page.len(), started);
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
                rebase_after_exact: false,
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
        let decision_rebase_after_exact = decision.rebase_after_exact();
        match decision {
            StoreDeltaScopeDecision::Scoped { worklists, .. } => {
                let stored_state = resolution_scope_state(&connection, &self.identity.view_id)
                    .map_err(|error| incremental_error(error.to_string()))?;
                if self.prepare_prior_overlay()?.is_some() && stored_state == self.prior_scope_state
                {
                    self.decision_telemetry = Some(StoreResolutionDecisionTelemetry {
                        effective_full: false,
                        fallback_reason: None,
                        worklists: worklists.clone(),
                        rebase_after_exact: decision_rebase_after_exact,
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
                        rebase_after_exact: false,
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
                    rebase_after_exact: decision_rebase_after_exact,
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
        let started = self.candidate_query_started();
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
        self.record_candidate_query(CandidateQueryFamily::LocateIdentifier, rows_read, started);
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
            .filter(|(_, is_covered)| *is_covered)
            .map(|(identifier, _)| identifier.clone())
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
    fn visit_filtered_by_name_inner<F>(
        &self,
        reason: Option<FilteredNameLookupReason>,
        name: &str,
        language: &str,
        kinds: &[SymbolKind],
        source_key: Option<&str>,
        mut visitor: F,
    ) -> Result<(), StoreResolutionError>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, StoreResolutionError>,
    {
        let fingerprint = self.observe_logical_lookup(
            CandidatePageFamily::FilteredByName,
            &(name, language, kinds, source_key),
        );
        self.record_filtered_lookup_attribution(reason, fingerprint);
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
        let mut had_prior_page = false;
        loop {
            let sql_page_limit = self.sql_window_limit()?;
            let page_limit = usize::try_from(sql_page_limit).map_err(|_| {
                StoreResolutionError::InvalidWindowSize {
                    requested: self.window_size,
                    maximum: MAX_STORE_RESOLUTION_WINDOW,
                }
            })?;
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
            bind.push(sql_page_limit.into());
            let started = self.candidate_query_started();
            let (row_count, page, last) = self.with_candidate_reader(|connection| {
                let mut statement = connection.prepare_cached(&sql)?;
                let mut rows = statement.query(rusqlite::params_from_iter(bind))?;
                let mut hits = Vec::new();
                let mut row_count = 0usize;
                let mut last = None;
                while let Some(row) = rows.next()? {
                    row_count += 1;
                    last = Some((row.get::<_, i64>(0)?, row.get::<_, String>(1)?));
                    if let Some(hit) = candidate_hit(row)? {
                        hits.push(hit);
                    }
                }
                Ok((row_count, hits, last))
            })?;
            self.max_store_read_page
                .set(self.max_store_read_page.get().max(row_count));
            self.record_candidate_query(CandidateQueryFamily::FilteredByName, row_count, started);
            self.record_candidate_page_attribution(
                CandidatePageAttribution::FilteredByName {
                    reason,
                    page_limit: Some(page_limit),
                    had_prior_page,
                },
                row_count,
            );
            for hit in page {
                if !visitor(self, hit)? {
                    return Ok(());
                }
            }
            if row_count < page_limit {
                break;
            }
            let Some(next) = last else {
                break;
            };
            after = next;
            had_prior_page = true;
        }
        Ok(())
    }

    fn visit_top_level_named_inner<F>(
        &self,
        reason: Option<TopLevelLookupReason>,
        source_key: &str,
        name: &str,
        mut visitor: F,
    ) -> Result<(), StoreResolutionError>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, StoreResolutionError>,
    {
        let version_id = parse_source_key(source_key)?;
        let fingerprint =
            self.observe_logical_lookup(CandidatePageFamily::TopLevelNamed, &(version_id, name));
        self.record_top_level_lookup_attribution(reason, fingerprint);
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
        let mut had_prior_page = false;
        loop {
            let page_limit = self.window_size;
            let sql_page_limit = self.sql_window_limit()?;
            let (page, next) = self.candidate_page(
                CandidateQueryFamily::TopLevelNamed,
                sql,
                vec![
                    version_id.into(),
                    name.to_string().into(),
                    self.identity.view_id.clone().into(),
                    self.identity.generation.into(),
                    after.clone().into(),
                    sql_page_limit.into(),
                ],
                page_limit,
                Some(CandidatePageAttribution::TopLevel {
                    reason,
                    page_limit: Some(page_limit),
                    had_prior_page,
                }),
            )?;
            for hit in page {
                if !visitor(self, hit)? {
                    return Ok(());
                }
            }
            let Some(next) = next else {
                break;
            };
            after = next.1;
            had_prior_page = true;
        }
        Ok(())
    }

    fn visit_children_named_inner<F>(
        &self,
        reason: Option<ChildLookupReason>,
        source_key: &str,
        parent_id: &str,
        name: &str,
        mut visitor: F,
    ) -> Result<(), StoreResolutionError>
    where
        F: FnMut(&Self, CandidateHit) -> Result<bool, StoreResolutionError>,
    {
        let version_id = parse_source_key(source_key)?;
        let key = ChildrenNamedKey {
            version_id,
            parent_symbol_id: parent_id.to_string(),
            name: name.to_string(),
        };
        let fingerprint = self.observe_logical_lookup(CandidatePageFamily::ChildrenNamed, &key);
        self.record_child_lookup_attribution(reason, fingerprint);
        if let Some(hits) = self.cached_children_named_hits(&key) {
            if let Some(reason) = reason {
                self.record_child_lookup(reason, ChildLookupCacheState::ExactCacheHit);
            }
            for hit in hits {
                if !visitor(self, hit)? {
                    break;
                }
            }
            return Ok(());
        }
        if let Some(hits) = self.cached_name_hits(name) {
            if let Some(reason) = reason {
                self.record_child_lookup(reason, ChildLookupCacheState::NameCacheHit);
            }
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
        if let Some(reason) = reason {
            self.record_child_lookup(reason, ChildLookupCacheState::ScalarMiss);
        }
        let hit_capacity = {
            let window = self.candidate_window.borrow();
            let headroom = self
                .non_by_id_cache_capacity()
                .saturating_sub(window.non_by_id_entry_count());
            (headroom > 0).then(|| self.window_size.min(headroom.saturating_sub(1)))
        };
        let mut buffered_hits = Vec::new();
        let mut buffer_enabled = hit_capacity.is_some();
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
        let mut had_prior_page = false;
        loop {
            let page_limit = self.window_size;
            let sql_page_limit = self.sql_window_limit()?;
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
                    sql_page_limit.into(),
                ],
                page_limit,
                Some(CandidatePageAttribution::Children {
                    reason,
                    page_limit: Some(page_limit),
                    had_prior_page,
                }),
            )?;
            if buffer_enabled {
                let capacity = hit_capacity.expect("cache capacity exists while buffering");
                if buffered_hits.len().saturating_add(page.len()) <= capacity {
                    buffered_hits.extend(page.iter().cloned());
                } else {
                    buffered_hits.clear();
                    buffer_enabled = false;
                }
            }
            for hit in page {
                if !visitor(self, hit)? {
                    return Ok(());
                }
            }
            let Some(next) = next else {
                if buffer_enabled {
                    let mut window = self.candidate_window.borrow_mut();
                    self.cache_exact_children(&mut window, key, buffered_hits);
                    self.record_candidate_cache_occupancy(&window);
                }
                break;
            };
            after = next.1;
            had_prior_page = true;
        }
        Ok(())
    }

    fn visit_type_facts_inner<F>(
        &self,
        reason: Option<TypeFactsLookupReason>,
        symbol_id: &SemanticSymbolId,
        mut visitor: F,
    ) -> Result<(), StoreResolutionError>
    where
        F: FnMut(&Self, TypeFact) -> Result<bool, StoreResolutionError>,
    {
        let SemanticVersionId::Store(version_id) = symbol_id.version else {
            return Ok(());
        };
        let fingerprint = self.observe_logical_lookup(
            CandidatePageFamily::TypeFacts,
            &(version_id, &symbol_id.local_id),
        );
        self.record_type_facts_lookup_attribution(reason, fingerprint);
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
        let mut had_prior_page = false;
        loop {
            let sql_page_limit = self.sql_window_limit()?;
            let page_limit = usize::try_from(sql_page_limit).map_err(|_| {
                StoreResolutionError::InvalidWindowSize {
                    requested: self.window_size,
                    maximum: MAX_STORE_RESOLUTION_WINDOW,
                }
            })?;
            let started = self.candidate_query_started();
            let page = self.with_candidate_reader(|connection| {
                let mut statement = connection.prepare_cached(sql)?;
                Ok(statement
                    .query_map(
                        params![
                            version_id,
                            symbol_id.local_id,
                            self.identity.view_id,
                            self.identity.generation,
                            after,
                            sql_page_limit
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
            self.record_candidate_query(CandidateQueryFamily::TypeFacts, page.len(), started);
            self.max_store_read_page
                .set(self.max_store_read_page.get().max(page.len()));
            self.record_candidate_page_attribution(
                CandidatePageAttribution::TypeFacts {
                    reason,
                    page_limit: Some(page_limit),
                    had_prior_page,
                },
                page.len(),
            );
            if page.is_empty() {
                break;
            }
            for (_, fact) in &page {
                if !visitor(self, fact.clone())? {
                    return Ok(());
                }
            }
            if page.len() < page_limit {
                break;
            }
            after = page.last().expect("non-empty type fact page").0.clone();
            had_prior_page = true;
        }
        Ok(())
    }

    fn candidate_cache_capacity(&self) -> usize {
        self.window_size.saturating_mul(3)
    }

    fn non_by_id_cache_capacity(&self) -> usize {
        self.window_size.saturating_mul(2)
    }

    fn cached_name_hits(&self, name: &str) -> Option<Vec<CandidateHit>> {
        let window = self.candidate_window.borrow();
        window
            .primed_names
            .contains(name)
            .then(|| window.by_name.get(name).cloned().unwrap_or_default())
    }

    fn cached_children_named_hits(&self, key: &ChildrenNamedKey) -> Option<Vec<CandidateHit>> {
        let window = self.candidate_window.borrow();
        window.children_named.get(key).cloned()
    }

    fn cache_complete_name(
        &self,
        window: &mut CandidateWindow,
        name: String,
        hits: Vec<CandidateHit>,
    ) -> bool {
        let additions = hits.len();
        if window.non_by_id_entry_count().saturating_add(additions)
            > self.non_by_id_cache_capacity()
        {
            return false;
        }
        window.primed_names.insert(name.clone());
        window.by_name.insert(name, hits);
        true
    }

    fn cache_exact_children(
        &self,
        window: &mut CandidateWindow,
        key: ChildrenNamedKey,
        hits: Vec<CandidateHit>,
    ) {
        let key_addition = usize::from(!window.children_named.contains_key(&key));
        let additions = key_addition.saturating_add(hits.len());
        if window.non_by_id_entry_count().saturating_add(additions)
            > self.non_by_id_cache_capacity()
        {
            return;
        }
        window.children_named.insert(key, hits);
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
        if let ResolutionPhaseChunk::Identifiers(items) = chunk {
            return self.prime_identifier_children(items);
        }
        let names = match chunk {
            ResolutionPhaseChunk::Pending(items) => items
                .iter()
                .map(|item| item.target_terminal_name.clone())
                .collect::<BTreeSet<_>>(),
            _ => BTreeSet::new(),
        };
        if names.is_empty() {
            return self.prime_exact_children(chunk);
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
        let page_limit = self.window_size;
        let sql_page_limit = self.sql_window_limit()?;
        bind.push(sql_page_limit.into());
        let started = self.candidate_query_started();
        let rows = self.with_candidate_reader(|connection| {
            let mut statement = connection.prepare(&sql)?;
            statement
                .query_map(rusqlite::params_from_iter(bind), |row| {
                    Ok((row.get::<_, String>(3)?, candidate_hit(row)?))
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(StoreResolutionError::from)
        })?;
        self.record_candidate_query(CandidateQueryFamily::PrimeWindow, rows.len(), started);
        self.max_store_read_page
            .set(self.max_store_read_page.get().max(rows.len()));
        let windows_hit_row_limit = u64::from(rows.len() == page_limit);
        let cutoff =
            (rows.len() == page_limit).then(|| rows.last().expect("full candidate page").0.clone());
        let mut hits_by_name = HashMap::<String, Vec<CandidateHit>>::new();
        for (name, hit) in rows {
            if let Some(hit) = hit {
                hits_by_name.entry(name).or_default().push(hit);
            }
        }
        let mut names_complete = 0_u64;
        let mut names_skipped_cutoff = 0_u64;
        let mut names_rejected_capacity = 0_u64;
        let mut rows_admitted = 0_u64;
        let mut window = self.candidate_window.borrow_mut();
        for name in names {
            if cutoff.as_ref().is_some_and(|cutoff| name >= *cutoff) {
                names_skipped_cutoff = names_skipped_cutoff.saturating_add(1);
                continue;
            }
            let hits = hits_by_name.remove(&name).unwrap_or_default();
            if self.cache_complete_name(&mut window, name, hits.clone()) {
                names_complete = names_complete.saturating_add(1);
                rows_admitted =
                    rows_admitted.saturating_add(u64::try_from(hits.len()).unwrap_or(u64::MAX));
            } else {
                names_rejected_capacity = names_rejected_capacity.saturating_add(1);
            }
        }
        if self.candidate_query_timing_enabled {
            let mut attribution = self.candidate_cache_attribution.get();
            attribution.prime_window.windows = attribution.prime_window.windows.saturating_add(1);
            attribution.prime_window.windows_hit_row_limit = attribution
                .prime_window
                .windows_hit_row_limit
                .saturating_add(windows_hit_row_limit);
            attribution.prime_window.names_wanted =
                attribution.prime_window.names_wanted.saturating_add(
                    names_complete
                        .saturating_add(names_skipped_cutoff)
                        .saturating_add(names_rejected_capacity),
                );
            attribution.prime_window.names_complete = attribution
                .prime_window
                .names_complete
                .saturating_add(names_complete);
            attribution.prime_window.names_skipped_cutoff = attribution
                .prime_window
                .names_skipped_cutoff
                .saturating_add(names_skipped_cutoff);
            attribution.prime_window.names_rejected_capacity = attribution
                .prime_window
                .names_rejected_capacity
                .saturating_add(names_rejected_capacity);
            attribution.prime_window.rows_admitted = attribution
                .prime_window
                .rows_admitted
                .saturating_add(rows_admitted);
            self.candidate_cache_attribution.set(attribution);
        }
        self.record_candidate_cache_occupancy(&window);
        drop(window);
        self.prime_exact_children(chunk)
    }

    fn exact_children_keys(
        &self,
        chunk: &ResolutionPhaseChunk,
    ) -> Result<Vec<ChildrenNamedKey>, StoreResolutionError> {
        let mut keys = BTreeSet::new();
        match chunk {
            ResolutionPhaseChunk::Pending(items) => {
                for item in items {
                    let Some(parent_symbol_id) = item.caller_scope_symbol_id.as_deref() else {
                        continue;
                    };
                    let version_id = parse_source_key(&item.file_id)?;
                    for name in [
                        Some(item.target_terminal_name.as_str()),
                        item.target_receiver.as_deref(),
                    ] {
                        let Some(name) = name.filter(|name| !name.is_empty()) else {
                            continue;
                        };
                        keys.insert(ChildrenNamedKey {
                            version_id,
                            parent_symbol_id: parent_symbol_id.to_string(),
                            name: name.to_string(),
                        });
                    }
                }
            }
            ResolutionPhaseChunk::Identifiers(items) => {
                for item in items {
                    let Some(parent_symbol_id) = item.containing_symbol_id.as_deref() else {
                        continue;
                    };
                    let version_id = parse_source_key(&item.file_id)?;
                    for name in [Some(item.name.as_str()), item.receiver.as_deref()] {
                        let Some(name) = name.filter(|name| !name.is_empty()) else {
                            continue;
                        };
                        keys.insert(ChildrenNamedKey {
                            version_id,
                            parent_symbol_id: parent_symbol_id.to_string(),
                            name: name.to_string(),
                        });
                    }
                }
            }
            _ => {}
        }
        Ok(keys.into_iter().take(self.window_size).collect())
    }

    fn prime_identifier_children(
        &self,
        items: &[IdentifierWorkItem],
    ) -> Result<(), StoreResolutionError> {
        let planning_capacity = self.candidate_cache_capacity();
        let mut frontier = Vec::<(ScopeFrontierKey, Vec<String>)>::new();
        for item in items {
            if ReferenceKind::from_identifier_kind(&item.kind).is_none() {
                continue;
            }
            let Some(scope_id) = item.containing_symbol_id.as_deref() else {
                continue;
            };
            let key = ScopeFrontierKey {
                version_id: parse_source_key(&item.file_id)?,
                symbol_id: scope_id.to_string(),
            };
            let mut names = Vec::new();
            for name in [Some(item.name.as_str()), item.receiver.as_deref()] {
                let Some(name) = name.filter(|name| !name.is_empty()) else {
                    continue;
                };
                if !names.iter().any(|existing| existing == name) {
                    names.push(name.to_string());
                }
            }
            if !names.is_empty() && frontier.len() < planning_capacity {
                frontier.push((key, names));
            }
        }
        let mut emitted = HashSet::<ChildrenNamedKey>::new();
        while !frontier.is_empty() {
            let mut keys = Vec::new();
            let mut planning_exhausted = false;
            for (scope, names) in &frontier {
                for name in names {
                    let key = ChildrenNamedKey {
                        version_id: scope.version_id,
                        parent_symbol_id: scope.symbol_id.clone(),
                        name: name.clone(),
                    };
                    if emitted.contains(&key) {
                        continue;
                    }
                    if emitted.len() >= planning_capacity {
                        planning_exhausted = true;
                        break;
                    }
                    emitted.insert(key.clone());
                    keys.push(key);
                }
                if planning_exhausted {
                    break;
                }
            }
            if keys.is_empty() {
                break;
            }
            for batch in keys.chunks(self.window_size) {
                self.prime_exact_children_keys(batch)?;
            }
            if planning_exhausted {
                break;
            }

            let parents = self.load_scope_parents(&frontier)?;
            let mut next = Vec::<(ScopeFrontierKey, Vec<String>)>::new();
            for (scope, names) in &frontier {
                let Some(Some(parent_symbol_id)) = parents.get(scope) else {
                    continue;
                };
                let parent = ScopeFrontierKey {
                    version_id: scope.version_id,
                    symbol_id: parent_symbol_id.clone(),
                };
                if next.len() >= planning_capacity {
                    break;
                }
                next.push((parent, names.clone()));
            }
            frontier = next
        }
        Ok(())
    }

    fn load_scope_parents(
        &self,
        frontier: &[(ScopeFrontierKey, Vec<String>)],
    ) -> Result<HashMap<ScopeFrontierKey, Option<String>>, StoreResolutionError> {
        let planning_capacity = self.candidate_cache_capacity();
        let mut parents = HashMap::new();
        let mut unique_frontier = Vec::with_capacity(frontier.len());
        let mut seen = HashSet::new();
        for (scope, _) in frontier {
            if seen.contains(scope) {
                continue;
            }
            if seen.len() >= planning_capacity {
                break;
            }
            seen.insert(scope.clone());
            unique_frontier.push(scope.clone());
        }
        for batch in unique_frontier.chunks(self.window_size) {
            let values = (0..batch.len())
                .map(|_| "(?,?,?)".to_string())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "WITH wanted(ordinal,version_id,symbol_id) AS (VALUES {values})
                 SELECT wanted.ordinal,wanted.version_id,wanted.symbol_id,s.parent_symbol_id
                 FROM wanted
                 JOIN symbols AS s
                   ON s.version_id=wanted.version_id AND s.symbol_id=wanted.symbol_id
                 WHERE EXISTS (
                   SELECT 1 FROM manifest_entries AS me
                   WHERE me.view_id=? AND me.generation=?
                     AND me.status IN ('indexed', 'failed_preserved')
                     AND me.version_id=s.version_id
                 )
                 ORDER BY wanted.ordinal"
            );
            let mut reordered: Vec<Value> = Vec::with_capacity(batch.len() * 3 + 2);
            for (ordinal, scope) in batch.iter().enumerate() {
                reordered.push((ordinal as i64).into());
                reordered.push(scope.version_id.into());
                reordered.push(scope.symbol_id.clone().into());
            }
            reordered.push(self.identity.view_id.clone().into());
            reordered.push(self.identity.generation.into());
            let started = self.candidate_query_started();
            let rows = self.with_candidate_reader(|connection| {
                let mut statement = connection.prepare(&sql)?;
                statement
                    .query_map(params_from_iter(reordered), |row| {
                        Ok((
                            row.get::<_, i64>(0)? as usize,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(StoreResolutionError::from)
            })?;
            self.record_candidate_query(CandidateQueryFamily::SymbolById, rows.len(), started);
            self.max_store_read_page
                .set(self.max_store_read_page.get().max(rows.len()));
            for (_, version_id, symbol_id, parent_symbol_id) in rows {
                parents.insert(
                    ScopeFrontierKey {
                        version_id,
                        symbol_id,
                    },
                    parent_symbol_id,
                );
            }
        }
        Ok(parents)
    }

    fn prime_exact_children(
        &self,
        chunk: &ResolutionPhaseChunk,
    ) -> Result<(), StoreResolutionError> {
        let keys = self.exact_children_keys(chunk)?;
        self.prime_exact_children_keys(&keys)
    }

    fn prime_exact_children_keys(
        &self,
        keys: &[ChildrenNamedKey],
    ) -> Result<(), StoreResolutionError> {
        if keys.is_empty() {
            return Ok(());
        }
        let values = keys
            .iter()
            .enumerate()
            .map(|(ordinal, _)| format!("({ordinal},?,?,?)"))
            .collect::<Vec<_>>()
            .join(",");
        let count_sql = format!(
            "WITH wanted(ordinal,version_id,parent_symbol_id,name) AS (VALUES {values})
             SELECT wanted.ordinal,COUNT(s.symbol_id)
             FROM wanted
             LEFT JOIN symbols AS s
               ON s.version_id=wanted.version_id
              AND s.parent_symbol_id=wanted.parent_symbol_id
              AND s.name=wanted.name
              AND EXISTS (
                SELECT 1 FROM manifest_entries AS me
                WHERE me.view_id=? AND me.generation=?
                  AND me.status IN ('indexed','failed_preserved')
                  AND me.version_id=s.version_id
              )
             GROUP BY wanted.ordinal
             ORDER BY wanted.ordinal"
        );
        let mut bind: Vec<rusqlite::types::Value> = Vec::with_capacity(keys.len() * 3 + 2);
        for key in keys {
            bind.push(key.version_id.into());
            bind.push(key.parent_symbol_id.clone().into());
            bind.push(key.name.clone().into());
        }
        bind.push(self.identity.view_id.clone().into());
        bind.push(self.identity.generation.into());
        let started = self.candidate_query_started();
        let counts = self.with_candidate_reader(|connection| {
            let mut statement = connection.prepare(&count_sql)?;
            statement
                .query_map(params_from_iter(bind), |row| {
                    Ok((
                        row.get::<_, i64>(0)? as usize,
                        row.get::<_, i64>(1)? as usize,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()
                .map_err(StoreResolutionError::from)
        })?;
        self.record_candidate_query(CandidateQueryFamily::ChildrenNamed, counts.len(), started);
        self.record_children_named_batch_count();
        self.record_children_named_batch_page(counts.len());
        self.max_store_read_page
            .set(self.max_store_read_page.get().max(counts.len()));

        let mut remaining_rows = self.window_size;
        let mut selected = Vec::new();
        {
            let window = self.candidate_window.borrow();
            let mut remaining_entries = self
                .non_by_id_cache_capacity()
                .saturating_sub(window.non_by_id_entry_count());
            for (ordinal, raw_count) in counts {
                let key_cost = 1;
                let admission_cost = raw_count.saturating_add(key_cost);
                if raw_count > remaining_rows || admission_cost > remaining_entries {
                    continue;
                }
                selected.push((ordinal, raw_count));
                remaining_rows = remaining_rows.saturating_sub(raw_count);
                remaining_entries = remaining_entries.saturating_sub(admission_cost);
            }
        }
        if selected.is_empty() {
            return Ok(());
        }
        let positive = selected
            .iter()
            .filter(|(_, raw_count)| *raw_count > 0)
            .collect::<Vec<_>>();
        let mut hits_by_ordinal = BTreeMap::new();
        if !positive.is_empty() {
            let values = positive
                .iter()
                .map(|(ordinal, _)| format!("({ordinal},?,?,?)", ordinal = ordinal))
                .collect::<Vec<_>>()
                .join(",");
            let fetch_sql = format!(
                "WITH wanted(ordinal,version_id,parent_symbol_id,name) AS (VALUES {values})
                 SELECT s.version_id,s.symbol_id,s.language,s.name,s.kind,
                        s.parent_symbol_id,s.visibility,s.signature,s.metadata_json,wanted.ordinal
                 FROM wanted
                 JOIN symbols AS s INDEXED BY idx_read_symbols_parent
                   ON s.version_id=wanted.version_id
                  AND s.parent_symbol_id=wanted.parent_symbol_id
                  AND s.name=wanted.name
                 WHERE EXISTS (
                   SELECT 1 FROM manifest_entries AS me
                   WHERE me.view_id=? AND me.generation=?
                     AND me.status IN ('indexed','failed_preserved')
                     AND me.version_id=s.version_id
                 )
                 ORDER BY wanted.ordinal,s.symbol_id COLLATE BINARY"
            );
            let mut bind: Vec<rusqlite::types::Value> = Vec::with_capacity(positive.len() * 3 + 2);
            for (ordinal, _) in &positive {
                let key = &keys[*ordinal];
                bind.push(key.version_id.into());
                bind.push(key.parent_symbol_id.clone().into());
                bind.push(key.name.clone().into());
            }
            bind.push(self.identity.view_id.clone().into());
            bind.push(self.identity.generation.into());
            let started = self.candidate_query_started();
            let rows = self.with_candidate_reader(|connection| {
                let mut statement = connection.prepare(&fetch_sql)?;
                let mut rows = statement.query(params_from_iter(bind))?;
                let mut hits = Vec::new();
                let mut page_rows = 0usize;
                while let Some(row) = rows.next()? {
                    page_rows += 1;
                    hits.push((row.get::<_, i64>(9)? as usize, candidate_hit(row)?));
                }
                Ok((page_rows, hits))
            })?;
            let (page_rows, rows) = rows;
            self.record_candidate_query(CandidateQueryFamily::ChildrenNamed, page_rows, started);
            self.record_children_named_batch_fetch();
            self.record_children_named_batch_page(page_rows);
            self.max_store_read_page
                .set(self.max_store_read_page.get().max(page_rows));
            for (ordinal, hit) in rows {
                if let Some(hit) = hit {
                    hits_by_ordinal
                        .entry(ordinal)
                        .or_insert_with(Vec::new)
                        .push(hit);
                }
            }
        }
        let mut window = self.candidate_window.borrow_mut();
        for (ordinal, _) in selected {
            let hits = hits_by_ordinal.remove(&ordinal).unwrap_or_default();
            self.cache_exact_children(&mut window, keys[ordinal].clone(), hits);
        }
        self.record_candidate_cache_occupancy(&window);
        Ok(())
    }

    fn candidate_page(
        &self,
        family: CandidateQueryFamily,
        sql: &str,
        bind: Vec<rusqlite::types::Value>,
        page_limit: usize,
        attribution: Option<CandidatePageAttribution>,
    ) -> Result<CandidatePage, StoreResolutionError> {
        let started = self.candidate_query_started();
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
            self.record_candidate_query(family, page_rows, started);
            if let Some(attribution) = attribution {
                self.record_candidate_page_attribution(attribution, page_rows);
            }
            let next = (page_rows == page_limit).then_some(last).flatten();
            Ok((hits, next))
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
        let access = match self.validated_base.as_ref() {
            Some(proof) => {
                PriorOverlayReader::open_with_validated_base(&self.layout, &state, proof)
            }
            None => PriorOverlayReader::open(&self.layout, &state),
        };
        match access.map_err(|error| incremental_error(error.to_string()))? {
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
                if is_covered && let Some(locators) = locators_by_name.get(&(version_id, name)) {
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
    ) -> Result<ScratchPendingStates, StoreResolutionError> {
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
        let started = self.candidate_query_started();
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
        self.record_candidate_query(
            CandidateQueryFamily::PendingHydration,
            keyed_rows.len(),
            started,
        );
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
        let started = self.candidate_query_started();
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
            started,
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
        let started = self.candidate_query_started();
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
        self.record_candidate_query(
            CandidateQueryFamily::IdentifierHydration,
            keyed_rows.len(),
            started,
        );
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
                let started = self.candidate_query_started();
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
                    started,
                );
                if let Some(version_id) = version_id {
                    return Ok(Some(version_id.to_string()));
                }
            }
            Ok(None)
        })?;
        let mut window = self.candidate_window.borrow_mut();
        if window.module_versions.len() < self.window_size
            && window.non_by_id_entry_count() < self.non_by_id_cache_capacity()
        {
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

#[cfg(feature = "test-store-resolution-contract")]
fn scoped_finalize_delay_for_test() {
    if let Some(delay_ms) =
        std::env::var_os("JULIE_EXTRACT_STORE_RESOLUTION_DELAY_SCOPED_FINALIZE_MS")
            .and_then(|value| value.to_str()?.parse::<u64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }
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

    #[test]
    fn candidate_query_timing_is_zero_when_disabled_and_accumulates_when_enabled() {
        let mut telemetry = [CandidateQueryTelemetry::default(); CandidateQueryFamily::COUNT];
        accumulate_candidate_query_telemetry(
            &mut telemetry,
            CandidateQueryFamily::IdentifierHydration,
            2,
            0,
        );
        assert_eq!(
            telemetry[CandidateQueryFamily::IdentifierHydration.index()].elapsed_micros,
            0
        );
        accumulate_candidate_query_telemetry(
            &mut telemetry,
            CandidateQueryFamily::IdentifierHydration,
            3,
            17,
        );
        accumulate_candidate_query_telemetry(
            &mut telemetry,
            CandidateQueryFamily::IdentifierHydration,
            4,
            23,
        );
        assert_eq!(
            telemetry[CandidateQueryFamily::IdentifierHydration.index()],
            CandidateQueryTelemetry {
                executions: 3,
                rows_read: 9,
                elapsed_micros: 40,
            }
        );
    }

    #[cfg(feature = "test-store-resolution-contract")]
    #[test]
    fn fixed_candidate_statements_reuse_across_fixed_families() {
        use julie_extract_artifact::store::ManifestStore;
        use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let temp = tempfile::tempdir().unwrap();
        let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
        let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
        let mut connection = factory.open_writer().unwrap();
        ManifestStore::new(&mut connection)
            .ensure_view("view-a", "/repo")
            .unwrap();
        connection
            .execute(
                "INSERT INTO file_versions(path,content_hash,extraction_epoch,language,content_bytes,complete_l1,complete_l2)
                 VALUES ('src/lib.rs','hash-src/lib.rs',1,'rust',1,1,1)",
                [],
            )
            .unwrap();
        let version = connection.last_insert_rowid();
        for (symbol_id, name) in [
            ("a-1", "summary-a"),
            ("a-2", "summary-a"),
            ("b-1", "summary-b"),
            ("b-2", "summary-b"),
        ] {
            connection
                .execute(
                    "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
                     start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
                     VALUES (?1,?2,'src/lib.rs','rust',?3,'function',1,1,1,1,0,1,0,0,0)",
                    params![version, symbol_id, name],
                )
                .unwrap();
        }
        for (type_fact_id, symbol_id, resolved_type, is_inferred) in
            [("fact-a", "a-1", "TypeA", 0), ("fact-b", "b-1", "TypeB", 1)]
        {
            connection
                .execute(
                    "INSERT INTO type_facts(version_id,type_fact_id,symbol_id,language,resolved_type,is_inferred)
                     VALUES (?1,?2,?3,'rust',?4,?5)",
                    params![version, type_fact_id, symbol_id, resolved_type, is_inferred],
                )
                .unwrap();
        }
        connection
            .execute(
                "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
                 VALUES ('view-a',1,'manifest-a','request-a',?1)",
                ["2026-08-08T12:00:00Z"],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
                 VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash-src/lib.rs',?2)",
                params![version, "2026-08-08T12:00:00Z"],
            )
            .unwrap();
        drop(connection);

        let session = StoreScratchResolutionSession::new(
            factory,
            StoreManifestIdentity {
                family_id: "family-a".to_string(),
                view_id: "view-a".to_string(),
                generation: 1,
                manifest_hash: "manifest-a".to_string(),
            },
            temp.path().join("exact.db"),
            6,
            6,
        )
        .unwrap();
        let prepare_count = Arc::new(AtomicUsize::new(0));
        let hook_count = Arc::clone(&prepare_count);
        let type_outer_read = Arc::new(AtomicBool::new(false));
        let type_nested_selects = Arc::new(AtomicUsize::new(0));
        let hook_type_outer_read = Arc::clone(&type_outer_read);
        let hook_type_nested_selects = Arc::clone(&type_nested_selects);
        let reader = session.open_phase_reader().unwrap();
        reader
            .authorizer(Some(move |context: AuthContext<'_>| {
                match context.action {
                    AuthAction::Read {
                        table_name: "type_facts",
                        ..
                    } => {
                        hook_type_outer_read.store(true, Ordering::Relaxed);
                    }
                    AuthAction::Select if hook_type_outer_read.swap(false, Ordering::Relaxed) => {
                        hook_type_nested_selects.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
                if matches!(context.action, AuthAction::Select) {
                    hook_count.fetch_add(1, Ordering::Relaxed);
                }
                Authorization::Allow
            }))
            .unwrap();
        *session.candidate_reader.borrow_mut() = Some(reader);

        let source_key = version.to_string();
        let first_symbol = session.symbol_by_id(&source_key, "a-1").unwrap().unwrap();
        assert_eq!(first_symbol.symbol.symbol_id, "a-1");
        assert!(prepare_count.load(Ordering::Relaxed) > 0);

        let first_facts = {
            let mut facts = Vec::new();
            let symbol_id = SemanticSymbolId {
                version: SemanticVersionId::Store(version),
                local_id: "a-1".to_string(),
            };
            session
                .visit_type_facts(&symbol_id, |_, fact| {
                    facts.push(fact);
                    Ok(true)
                })
                .unwrap();
            facts
        };
        assert_eq!(
            first_facts,
            vec![TypeFact {
                symbol_id: "a-1".to_string(),
                resolved_type: "TypeA".to_string(),
                is_inferred: false,
            }]
        );
        let eight_kinds = (0..8).map(|_| SymbolKind::Function).collect::<Vec<_>>();
        let first_summary = session
            .filtered_name_summary("summary-a", "rust", &eight_kinds, None, 0.5)
            .unwrap();
        assert_eq!(
            first_summary
                .evidence
                .iter()
                .map(|evidence| evidence.semantic_id.local_id.as_str())
                .collect::<Vec<_>>(),
            ["a-1", "a-2"]
        );
        assert_eq!(first_summary.exact_count, 2);

        let symbol_prepares_before = prepare_count.load(Ordering::Relaxed);
        let second_symbol = session.symbol_by_id(&source_key, "b-1").unwrap().unwrap();
        assert_eq!(second_symbol.symbol.symbol_id, "b-1");
        let symbol_prepare_delta = prepare_count.load(Ordering::Relaxed) - symbol_prepares_before;

        type_outer_read.store(false, Ordering::Relaxed);
        let fact_prepares_before = type_nested_selects.load(Ordering::Relaxed);
        let second_facts = {
            let mut facts = Vec::new();
            let symbol_id = SemanticSymbolId {
                version: SemanticVersionId::Store(version),
                local_id: "a-1".to_string(),
            };
            session
                .visit_type_facts(&symbol_id, |_, fact| {
                    facts.push(fact);
                    Ok(true)
                })
                .unwrap();
            facts
        };
        let fact_prepare_delta = type_nested_selects.load(Ordering::Relaxed) - fact_prepares_before;
        assert_eq!(second_facts, first_facts);

        type_outer_read.store(false, Ordering::Relaxed);
        let summary_prepares_before = prepare_count.load(Ordering::Relaxed);
        let second_summary = session
            .filtered_name_summary("summary-b", "rust", &eight_kinds, None, 0.5)
            .unwrap();
        let summary_prepare_delta = prepare_count.load(Ordering::Relaxed) - summary_prepares_before;
        assert_eq!(
            second_summary
                .evidence
                .iter()
                .map(|evidence| evidence.semantic_id.local_id.as_str())
                .collect::<Vec<_>>(),
            ["b-1", "b-2"]
        );
        assert_eq!(second_summary.exact_count, 2);
        assert_eq!(
            (
                symbol_prepare_delta,
                fact_prepare_delta,
                summary_prepare_delta
            ),
            (0, 0, 0)
        );
    }

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
