#![cfg(feature = "test-store-resolution-contract")]

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use julie_extract_artifact::resolution_store::{ResolutionCounts, ResolutionReportRow};
use julie_extract_artifact::store::{
    ManifestStore, ResolutionBaseBegin, ResolutionBaseCatalog, ResolutionBaseReader,
    ResolutionBaseWriter, ResolutionBindingStore, ResolutionDiffMarker, ResolutionFileIdentity,
    StoreConnectionFactory, StoreLayout,
};
use julie_extract_cli::resolution::{self, run_resolution_session};
use julie_extract_cli::resolution_session::{
    ResolutionCorpusIdentity, ResolutionPassRequest, ResolutionPhase, ResolutionPhaseChunk,
    ResolutionSession, ResolutionWorklists, ResolutionWriteBatch, SemanticIdentifierId,
    SemanticSymbolId, SemanticVersionId, SessionResolutionState,
};
use julie_extract_cli::store::resolution_session::{
    CandidateQueryFamily, CandidateQueryTelemetry, FinishExactPhase, FinishExactPhaseSample,
    StoreManifestIdentity, StoreScratchResolutionSession,
};
use julie_extract_cli::store::{
    StoreArgs, StoreCommand, StoreRequestControls, StoreResolveArgs, dispatch,
};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const FAMILY_ID: &str = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
const VIEW_ID: &str = "view-miller-scale";
const ROOT: &str = "/synthetic/miller";
const RESOLVER_OUTPUT_EPOCH: i64 = 6;
const WINDOW_SIZE: usize = 300;
const MILLER_FILE_ROWS: usize = 1_538;
const MILLER_IDENTIFIER_ROWS: usize = 392_134;
const MILLER_PENDING_ROWS: usize = 89_538;
const MILLER_RESOLVED_PENDING_ROWS: usize = 10_412;
const MILLER_DISTINCT_IDENTIFIER_NAMES: usize = 20_109;
const MILLER_CHANGED_FILES: usize = 98;
const REBASE_GAP_BYTES: usize = 64 * 1024 * 1024 + 1;
const ONE_FILE_IDENTIFIER_ROWS: usize = 10_000;
const ONE_FILE_STABLE_IDENTIFIER_ROWS: usize = 40_000;
const ONE_FILE_STABLE_PENDING_ROWS: usize = 8_000;
const ONE_FILE_STABLE_RESOLVED_PENDING_ROWS: usize = 1_000;
const ONE_FILE_SCOPED_MAX_MS: u64 = 5_000;
const TARGET_VALIDATION_DISTINCT_TARGETS: usize = 2_048;
const TARGET_VALIDATION_MAX: Duration = Duration::from_secs(2);
const CANDIDATE_RESOLUTION_DISTINCT_NAMES: usize = 20_000;
const CANDIDATE_RESOLUTION_MAX: Duration = Duration::from_millis(3_500);
const REPEATED_NAME_IDENTIFIERS: usize = 32;
const REPEATED_NAME_CANDIDATES: usize = WINDOW_SIZE + 1;
const REPEATED_NAME_TOP_LEVEL_QUERY_BOUND: usize = 3;
const IDENTIFIER_WRITER_ROWS: usize = 100_000;
const IDENTIFIER_WRITER_MAX: Duration = Duration::from_millis(2_500);
const PAIRS: [&str; 2] = ["miller-unchanged", "miller-mutated"];
const NOW: &str = "2026-08-08T12:00:00.000Z";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplayMode {
    ForcedFull,
    Scoped,
}

impl ReplayMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::ForcedFull => "full",
            Self::Scoped => "scoped",
        }
    }

    fn env_value(self) -> &'static str {
        match self {
            Self::ForcedFull => "off",
            Self::Scoped => "on",
        }
    }
}

fn replay_mode(pair: &str) -> ReplayMode {
    match pair {
        "miller-unchanged" => ReplayMode::ForcedFull,
        "miller-mutated" => ReplayMode::Scoped,
        other => panic!("unexpected replay pair {other}"),
    }
}

#[test]
fn replay_pair_contract_runs_forced_full_before_scoped() {
    assert_eq!(replay_mode(PAIRS[0]), ReplayMode::ForcedFull);
    assert_eq!(replay_mode(PAIRS[1]), ReplayMode::Scoped);
}

#[test]
fn performance_residual_subtracts_resolution_and_diff_without_scope_twice() {
    let phase_timings_ms = BTreeMap::from([
        ("resolution".to_string(), 200),
        ("scope".to_string(), 50),
        ("diff".to_string(), 100),
    ]);

    assert_eq!(derived_residual_ms(1_000, &phase_timings_ms), 700);
}

#[test]
fn one_file_default_incremental_matches_full_escape_hatch() {
    let fixture = tempfile::tempdir().unwrap();
    let full_store_root = fixture.path().join("full-store");
    let scoped_store_root = fixture.path().join("scoped-store");
    let changed_file_shape = Some(ResolutionRowShape {
        identifiers: ONE_FILE_IDENTIFIER_ROWS,
        pending: scaled_pending_rows(ONE_FILE_IDENTIFIER_ROWS),
        resolved_pending: scaled_resolved_pending_rows(ONE_FILE_IDENTIFIER_ROWS),
        distinct_target_names: MILLER_DISTINCT_IDENTIFIER_NAMES
            .saturating_sub(1)
            .min(ONE_FILE_IDENTIFIER_ROWS.max(1)),
    });
    build_store_fixture_with_changed_file_rows(
        &full_store_root,
        ONE_FILE_IDENTIFIER_ROWS,
        scaled_pending_rows(ONE_FILE_IDENTIFIER_ROWS),
        scaled_resolved_pending_rows(ONE_FILE_IDENTIFIER_ROWS),
        1,
        changed_file_shape,
        Some(ResolutionRowShape {
            identifiers: ONE_FILE_STABLE_IDENTIFIER_ROWS,
            pending: ONE_FILE_STABLE_PENDING_ROWS,
            resolved_pending: ONE_FILE_STABLE_RESOLVED_PENDING_ROWS,
            distinct_target_names: 1,
        }),
    );
    build_store_fixture_with_changed_file_rows(
        &scoped_store_root,
        ONE_FILE_IDENTIFIER_ROWS,
        scaled_pending_rows(ONE_FILE_IDENTIFIER_ROWS),
        scaled_resolved_pending_rows(ONE_FILE_IDENTIFIER_ROWS),
        1,
        changed_file_shape,
        Some(ResolutionRowShape {
            identifiers: ONE_FILE_STABLE_IDENTIFIER_ROWS,
            pending: ONE_FILE_STABLE_PENDING_ROWS,
            resolved_pending: ONE_FILE_STABLE_RESOLVED_PENDING_ROWS,
            distinct_target_names: 1,
        }),
    );
    let full_layout = StoreLayout::open(full_store_root.join("family")).unwrap();
    let scoped_layout = StoreLayout::open(scoped_store_root.join("family")).unwrap();
    let full_view = "one-file-full";
    let scoped_view = "one-file-default";
    prepare_replay_view_with_changed_files(
        &full_layout,
        full_view,
        ReplayMode::ForcedFull,
        1,
        false,
        Some(ONE_FILE_IDENTIFIER_ROWS),
    );
    prepare_replay_view_with_changed_files(
        &scoped_layout,
        scoped_view,
        ReplayMode::Scoped,
        1,
        false,
        Some(ONE_FILE_IDENTIFIER_ROWS),
    );

    let full = run_resolve_with_instant(
        &full_store_root.join("family"),
        full_view,
        "one-file-full-resolve",
        Some("off"),
    );
    let scoped = run_resolve_with_instant(
        &scoped_store_root.join("family"),
        scoped_view,
        "one-file-default-resolve",
        None,
    );
    assert_eq!(full.report["resolution"]["resolution_mode"], "full");
    assert_eq!(scoped.report["resolution"]["resolution_mode"], "scoped");
    assert_eq!(scoped.report["resolution"]["fallback_reason"], Value::Null);
    assert_eq!(scoped.report["resolution"]["scope_file_count"], 1);
    eprintln!(
        "one_file_resolution full_mode={} full_wall_ms={} scoped_mode={} scoped_wall_ms={}",
        full.report["resolution"]["resolution_mode"],
        full.wall_ms,
        scoped.report["resolution"]["resolution_mode"],
        scoped.wall_ms
    );
    assert!(
        scoped.wall_ms <= ONE_FILE_SCOPED_MAX_MS,
        "default-on one-file resolution exceeded the 5s observation: {} ms",
        scoped.wall_ms
    );

    let full_artifact = fixture.path().join("one-file-full.sqlite");
    let scoped_artifact = fixture.path().join("one-file-scoped.sqlite");
    export_view(&full_store_root.join("family"), full_view, &full_artifact);
    export_view(
        &scoped_store_root.join("family"),
        scoped_view,
        &scoped_artifact,
    );
    assert_eq!(
        artifact_semantic_digest(&full_artifact),
        artifact_semantic_digest(&scoped_artifact)
    );
}

#[test]
fn scoped_non_rebase_resolve_opens_validated_base_once() {
    let fixture = tempfile::tempdir().unwrap();
    let store_root = fixture.path().join("scoped-store");
    let changed_file_shape = Some(ResolutionRowShape {
        identifiers: ONE_FILE_IDENTIFIER_ROWS,
        pending: scaled_pending_rows(ONE_FILE_IDENTIFIER_ROWS),
        resolved_pending: scaled_resolved_pending_rows(ONE_FILE_IDENTIFIER_ROWS),
        distinct_target_names: MILLER_DISTINCT_IDENTIFIER_NAMES
            .saturating_sub(1)
            .min(ONE_FILE_IDENTIFIER_ROWS.max(1)),
    });
    build_store_fixture_with_changed_file_rows(
        &store_root,
        ONE_FILE_IDENTIFIER_ROWS,
        scaled_pending_rows(ONE_FILE_IDENTIFIER_ROWS),
        scaled_resolved_pending_rows(ONE_FILE_IDENTIFIER_ROWS),
        1,
        changed_file_shape,
        Some(ResolutionRowShape {
            identifiers: ONE_FILE_STABLE_IDENTIFIER_ROWS,
            pending: ONE_FILE_STABLE_PENDING_ROWS,
            resolved_pending: ONE_FILE_STABLE_RESOLVED_PENDING_ROWS,
            distinct_target_names: 1,
        }),
    );
    let layout = StoreLayout::open(store_root.join("family")).unwrap();
    prepare_replay_view_with_changed_files(
        &layout,
        "one-file-default",
        ReplayMode::Scoped,
        1,
        false,
        Some(ONE_FILE_IDENTIFIER_ROWS),
    );

    ResolutionBaseReader::reset_test_open_count();
    let outcome = dispatch(StoreArgs {
        command: StoreCommand::Resolve(StoreResolveArgs {
            store: store_root.join("family"),
            family: Some(FAMILY_ID.to_string()),
            view: "one-file-default".to_string(),
            request: StoreRequestControls {
                request_id: Some("one-file-open-count".to_string()),
                idempotency_key: Some("one-file-open-count".to_string()),
                request_timeout_seconds: 30,
            },
            json: true,
        }),
    });

    assert_eq!(outcome.exit_code(), 0);
    assert_eq!(ResolutionBaseReader::test_open_count(), 1);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Sample {
    pair: String,
    run: usize,
    resolution_compute_ms: u64,
    store_fresh_ms: u64,
    diff_ms: u64,
    delta_write_ms: u64,
    publish_ms: u64,
    time_to_exact_ms: u64,
    integrity_ms: u64,
    identifier_rows: u64,
    pending_rows: u64,
    peak_rss_bytes: u64,
    base_bytes: u64,
    delta_bytes: u64,
    semantic_differences: Option<u64>,
    applied_differences: Option<u64>,
    foreground_bind_ms: u64,
    foreground_identifier_work: Option<u64>,
    background_pipeline_ms: u64,
    resolution_mode: String,
    cpu_user_ms: u64,
    cpu_system_ms: u64,
    phase_timings_ms: BTreeMap<String, u64>,
    scope_file_count: u64,
    scope_name_count: u64,
    scope_row_count: u64,
    fallback_reason: Option<String>,
    canonical_semantic_digest: String,
    row_level_differences: Option<u64>,
    fixture_snapshot_digest: String,
    artifact_path: PathBuf,
    exact_gap_rows: u64,
    exact_gap_files: u64,
    cumulative_gap_bytes_before: u64,
    cumulative_gap_bytes_after: u64,
    cumulative_delta_rows_before: u64,
    cumulative_delta_rows_after: u64,
    rebased: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CandidateQueryMetric {
    executions: usize,
    rows: usize,
}

impl From<CandidateQueryTelemetry> for CandidateQueryMetric {
    fn from(telemetry: CandidateQueryTelemetry) -> Self {
        Self {
            executions: telemetry.executions,
            rows: telemetry.rows_read,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CandidateQuerySample {
    by_name: CandidateQueryMetric,
    children_named: CandidateQueryMetric,
    filtered_by_name: CandidateQueryMetric,
    filtered_name_summary: CandidateQueryMetric,
    identifier_hydration: CandidateQueryMetric,
    imports: CandidateQueryMetric,
    locate_identifier: CandidateQueryMetric,
    module_version: CandidateQueryMetric,
    pending_hydration: CandidateQueryMetric,
    prime_window: CandidateQueryMetric,
    relationship_hydration: CandidateQueryMetric,
    symbol_by_id: CandidateQueryMetric,
    top_level_named: CandidateQueryMetric,
    type_facts: CandidateQueryMetric,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CandidateQueryDiagnostic {
    elapsed_ms: u64,
    configured_resolution_mode: String,
    configured_scope_file_count: usize,
    max_store_read_page: usize,
    max_candidate_cache_entries: usize,
    locate_identifier_by_phase: LocateIdentifierPhaseSample,
    queries: CandidateQuerySample,
    finish_exact: Vec<FinishExactPhaseSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CandidateQuerySnapshot {
    elapsed_ms: u64,
    total_executions: usize,
    locate_identifier_by_phase: LocateIdentifierPhaseSample,
    queries: CandidateQuerySample,
    finish_exact: Vec<FinishExactPhaseSample>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
struct LocateIdentifierPhaseSample {
    resolved_pending: usize,
    pending: usize,
    relationships: usize,
    other_or_unset: usize,
}

struct SnapshottingSession<'a> {
    inner: &'a mut StoreScratchResolutionSession,
    output: PathBuf,
    started: Instant,
    next_execution_threshold: usize,
    phase: Cell<Option<ResolutionPhase>>,
    locate_identifier_by_phase: Cell<LocateIdentifierPhaseSample>,
}

impl SnapshottingSession<'_> {
    fn locate_identifier_phase_sample(&self) -> LocateIdentifierPhaseSample {
        self.locate_identifier_by_phase.get()
    }

    fn record_locate_identifier(&self) {
        let mut sample = self.locate_identifier_by_phase.get();
        match self.phase.get() {
            Some(ResolutionPhase::ResolvedPending) => sample.resolved_pending += 1,
            Some(ResolutionPhase::Pending) => sample.pending += 1,
            Some(ResolutionPhase::Relationships) => sample.relationships += 1,
            _ => sample.other_or_unset += 1,
        }
        self.locate_identifier_by_phase.set(sample);
    }

    fn persist_if_due(&mut self) {
        let queries = candidate_query_sample(self.inner);
        let total_executions = queries.total_executions();
        if total_executions < self.next_execution_threshold {
            return;
        }
        let snapshot = CandidateQuerySnapshot {
            elapsed_ms: elapsed_ms(self.started.elapsed()),
            total_executions,
            locate_identifier_by_phase: self.locate_identifier_phase_sample(),
            queries,
            finish_exact: Vec::new(),
        };
        persist_json_atomically(&self.output, &snapshot);
        self.next_execution_threshold = total_executions.next_power_of_two().saturating_mul(2);
    }
}

fn persist_json_atomically(path: &Path, value: &impl Serialize) {
    let pending = path.with_extension("json.pending");
    fs::write(&pending, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    fs::rename(pending, path).unwrap();
}

fn finish_exact_with_diagnostics(
    session: StoreScratchResolutionSession,
    started: Instant,
    output: &Path,
    live_output: &Path,
    mut diagnostic: CandidateQueryDiagnostic,
) -> ResolutionFileIdentity {
    let total_executions = diagnostic.queries.total_executions();
    let queries = diagnostic.queries.clone();
    let locate_identifier_by_phase = diagnostic.locate_identifier_by_phase;
    let mut finish_exact = Vec::new();
    let identity = session
        .finish_exact_observing(|sample| {
            finish_exact.push(sample);
            persist_json_atomically(
                live_output,
                &CandidateQuerySnapshot {
                    elapsed_ms: elapsed_ms(started.elapsed()),
                    total_executions,
                    locate_identifier_by_phase,
                    queries: queries.clone(),
                    finish_exact: finish_exact.clone(),
                },
            );
        })
        .unwrap();
    diagnostic.finish_exact = finish_exact;
    persist_json_atomically(output, &diagnostic);
    identity
}

impl CandidateQuerySample {
    fn total_executions(&self) -> usize {
        self.by_name.executions
            + self.children_named.executions
            + self.filtered_by_name.executions
            + self.filtered_name_summary.executions
            + self.identifier_hydration.executions
            + self.imports.executions
            + self.locate_identifier.executions
            + self.module_version.executions
            + self.pending_hydration.executions
            + self.prime_window.executions
            + self.relationship_hydration.executions
            + self.symbol_by_id.executions
            + self.top_level_named.executions
            + self.type_facts.executions
    }
}

fn candidate_query_sample(session: &StoreScratchResolutionSession) -> CandidateQuerySample {
    let metric = |family| session.candidate_query_telemetry(family).into();
    CandidateQuerySample {
        by_name: metric(CandidateQueryFamily::ByName),
        children_named: metric(CandidateQueryFamily::ChildrenNamed),
        filtered_by_name: metric(CandidateQueryFamily::FilteredByName),
        filtered_name_summary: metric(CandidateQueryFamily::FilteredNameSummary),
        identifier_hydration: metric(CandidateQueryFamily::IdentifierHydration),
        imports: metric(CandidateQueryFamily::Imports),
        locate_identifier: metric(CandidateQueryFamily::LocateIdentifier),
        module_version: metric(CandidateQueryFamily::ModuleVersion),
        pending_hydration: metric(CandidateQueryFamily::PendingHydration),
        prime_window: metric(CandidateQueryFamily::PrimeWindow),
        relationship_hydration: metric(CandidateQueryFamily::RelationshipHydration),
        symbol_by_id: metric(CandidateQueryFamily::SymbolById),
        top_level_named: metric(CandidateQueryFamily::TopLevelNamed),
        type_facts: metric(CandidateQueryFamily::TypeFacts),
    }
}

fn query_diagnostic_identity(
    layout: &StoreLayout,
    view_id: &str,
    generation: i64,
) -> StoreManifestIdentity {
    let connection = Connection::open(layout.store_db()).unwrap();
    let (current_generation, state, base_id, delta_generation): (i64, String, String, i64) =
        connection
            .query_row(
                "SELECT current_generation,resolution_state,resolution_base_id,
                        resolution_delta_generation
                 FROM views WHERE view_id=?1",
                [view_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
    assert_eq!(current_generation, generation);
    assert_eq!(state, "converging");
    let (bound_manifest_generation, base_state, base_manifest_hash): (i64, String, String) =
        connection
            .query_row(
                "SELECT delta.manifest_generation,base.state,base.manifest_hash
                 FROM resolution_deltas AS delta
                 JOIN resolution_bases AS base ON base.base_id=delta.base_id
                 WHERE delta.view_id=?1 AND delta.delta_generation=?2
                   AND delta.base_id=?3",
                params![view_id, delta_generation, base_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
    assert_eq!(bound_manifest_generation, generation);
    assert_eq!(base_state, "ready");
    let identity = view_manifest_identity(layout, view_id, generation);
    assert_ne!(base_manifest_hash, identity.manifest_hash);
    identity
}

impl ResolutionSession for SnapshottingSession<'_> {
    type Error = <StoreScratchResolutionSession as ResolutionSession>::Error;

    fn corpus_identity(&self) -> Result<ResolutionCorpusIdentity, Self::Error> {
        self.inner.corpus_identity()
    }

    fn prior_resolution_state(&mut self) -> Result<Option<SessionResolutionState>, Self::Error> {
        self.inner.prior_resolution_state()
    }

    fn current_revision(&mut self) -> Result<i64, Self::Error> {
        self.inner.current_revision()
    }

    fn open_resolution_pass(
        &mut self,
        request: &ResolutionPassRequest,
    ) -> Result<ResolutionWorklists, Self::Error> {
        self.inner.open_resolution_pass(request)
    }

    fn qualify_version(&self, source_key: &str) -> Result<SemanticVersionId, Self::Error> {
        self.inner.qualify_version(source_key)
    }

    fn resolve_edge(
        &mut self,
        edge: &resolution::UnresolvedEdge,
    ) -> Result<resolution::TierOutcome, Self::Error> {
        let outcome = self.inner.resolve_edge(edge)?;
        self.persist_if_due();
        Ok(outcome)
    }

    fn target_symbol_name(
        &mut self,
        symbol_id: &SemanticSymbolId,
    ) -> Result<Option<String>, Self::Error> {
        self.inner.target_symbol_name(symbol_id)
    }

    fn locate_identifier(
        &self,
        version: &SemanticVersionId,
        name: &str,
        start_byte: Option<i64>,
        end_byte: Option<i64>,
        start_line: i64,
    ) -> Result<Option<String>, Self::Error> {
        let identifier = self
            .inner
            .locate_identifier(version, name, start_byte, end_byte, start_line)?;
        self.record_locate_identifier();
        Ok(identifier)
    }

    fn identifier_is_covered(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        self.inner.identifier_is_covered(identifier_id)
    }

    fn propagation_is_covered(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        self.inner.propagation_is_covered(identifier_id)
    }

    fn propagation_is_owned(
        &mut self,
        identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        self.inner.propagation_is_owned(identifier_id)
    }

    fn next_phase_chunk(
        &mut self,
        worklists: &ResolutionWorklists,
    ) -> Result<Option<ResolutionPhaseChunk>, Self::Error> {
        self.phase.set(Some(worklists.phase));
        let chunk = self.inner.next_phase_chunk(worklists)?;
        self.persist_if_due();
        Ok(chunk)
    }

    fn flush(&mut self, writes: ResolutionWriteBatch) -> Result<ResolutionCounts, Self::Error> {
        let counts = self.inner.flush(writes)?;
        self.persist_if_due();
        Ok(counts)
    }

    fn aggregate_report(&mut self) -> Result<Vec<ResolutionReportRow>, Self::Error> {
        self.inner.aggregate_report()
    }

    fn prepare_shadow(
        &mut self,
        worklists: &ResolutionWorklists,
        revision: i64,
    ) -> Result<(), Self::Error> {
        self.inner.prepare_shadow(worklists, revision)
    }

    fn verify_shadow(&mut self) -> Result<(), Self::Error> {
        self.inner.verify_shadow()
    }
}

#[derive(Default)]
struct WriteTimeline {
    active: Option<Instant>,
    elapsed: Duration,
    intervals: usize,
    invalid: bool,
}

impl WriteTimeline {
    fn mark(&mut self, marker: ResolutionDiffMarker) {
        match marker {
            ResolutionDiffMarker::DeltaWriteStart => {
                if self.active.replace(Instant::now()).is_some() {
                    self.invalid = true;
                }
            }
            ResolutionDiffMarker::DeltaWriteEnd => {
                let Some(start) = self.active.take() else {
                    self.invalid = true;
                    return;
                };
                self.elapsed += start.elapsed();
                self.intervals += 1;
            }
        }
    }

    fn finish(&self, total: Duration) -> Duration {
        assert!(!self.invalid);
        assert!(self.active.is_none());
        assert!(self.intervals > 0);
        assert!(self.elapsed <= total);
        self.elapsed
    }
}

#[test]
fn metric_markers_reject_missing_overlapping_and_widened_intervals() {
    let mut missing = WriteTimeline::default();
    missing.mark(ResolutionDiffMarker::DeltaWriteStart);
    assert!(std::panic::catch_unwind(|| missing.finish(Duration::from_secs(1))).is_err());

    let mut overlapping = WriteTimeline::default();
    overlapping.mark(ResolutionDiffMarker::DeltaWriteStart);
    overlapping.mark(ResolutionDiffMarker::DeltaWriteStart);
    overlapping.mark(ResolutionDiffMarker::DeltaWriteEnd);
    assert!(std::panic::catch_unwind(|| overlapping.finish(Duration::from_secs(1))).is_err());

    let widened = WriteTimeline {
        elapsed: Duration::from_secs(2),
        intervals: 1,
        ..WriteTimeline::default()
    };
    assert!(std::panic::catch_unwind(|| widened.finish(Duration::from_secs(1))).is_err());
}

#[test]
fn performance_override_preserves_the_miller_pending_ratio() {
    assert_eq!(scaled_pending_rows(392_134), 89_538);
    assert_eq!(scaled_pending_rows(10_000), 2_283);
    assert_eq!(scaled_pending_rows(1), 1);
    assert_eq!(scaled_resolved_pending_rows(392_134), 10_412);
    assert_eq!(scaled_resolved_pending_rows(10_000), 265);
    assert_eq!(scaled_resolved_pending_rows(1), 1);
}

#[test]
fn performance_gate_resets_only_its_owned_directories() {
    let temp = tempfile::tempdir().unwrap();
    let owned = temp.path().join("run-001");
    let sibling = temp.path().join("keep.txt");
    fs::create_dir_all(&owned).unwrap();
    fs::write(owned.join("stale.db"), b"stale").unwrap();
    fs::write(&sibling, b"keep").unwrap();

    reset_owned_directory(&owned);

    assert!(owned.is_dir());
    assert!(!owned.join("stale.db").exists());
    assert_eq!(fs::read(sibling).unwrap(), b"keep");
}

#[test]
fn target_validation_finishes_with_high_distinct_target_cardinality() {
    let temp = tempfile::tempdir().unwrap();
    let layout = build_target_validation_fixture(temp.path(), TARGET_VALIDATION_DISTINCT_TARGETS);
    let identity = manifest_identity(&layout, 1);
    let factory = StoreConnectionFactory::new(layout, FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let exact_path = temp.path().join("exact.db");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        identity,
        &exact_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();
    run_resolution_session(&mut session, true, true).unwrap();

    let started = Instant::now();
    let exact = session.finish_exact().unwrap();
    let elapsed = started.elapsed();

    assert_eq!(
        exact.counts.identifiers,
        TARGET_VALIDATION_DISTINCT_TARGETS as u64
    );
    let connection = Connection::open(exact_path).unwrap();
    let distinct_targets: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM (
               SELECT DISTINCT target_version_id,target_symbol_id
               FROM identifier_resolutions
               WHERE target_version_id IS NOT NULL
             )",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(distinct_targets, TARGET_VALIDATION_DISTINCT_TARGETS as i64);
    assert!(
        elapsed <= TARGET_VALIDATION_MAX,
        "target validation took {elapsed:?}, expected at most {TARGET_VALIDATION_MAX:?}"
    );
}

#[test]
fn candidate_resolution_finishes_with_high_distinct_name_cardinality() {
    let temp = tempfile::tempdir().unwrap();
    let layout = build_target_validation_fixture(temp.path(), CANDIDATE_RESOLUTION_DISTINCT_NAMES);
    let identity = manifest_identity(&layout, 1);
    let factory = StoreConnectionFactory::new(layout, FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let exact_path = temp.path().join("exact.db");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        identity,
        &exact_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();

    let started = Instant::now();
    run_resolution_session(&mut session, true, true).unwrap();
    let elapsed = started.elapsed();
    assert!(session.max_candidate_cache_entries() <= WINDOW_SIZE * 3);

    let exact = session.finish_exact().unwrap();
    assert_eq!(
        exact.counts.identifiers,
        CANDIDATE_RESOLUTION_DISTINCT_NAMES as u64
    );
    assert!(
        elapsed <= CANDIDATE_RESOLUTION_MAX,
        "candidate resolution took {elapsed:?}, expected at most {CANDIDATE_RESOLUTION_MAX:?}"
    );
}

#[test]
fn repeated_name_high_fanout_reports_candidate_query_families_and_exact_output() {
    let temp = tempfile::tempdir().unwrap();
    let layout = build_repeated_name_candidate_fixture(
        temp.path(),
        REPEATED_NAME_IDENTIFIERS,
        REPEATED_NAME_CANDIDATES,
    );
    let identity = manifest_identity(&layout, 1);
    let factory = StoreConnectionFactory::new(layout, FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let exact_path = temp.path().join("exact.db");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        identity,
        &exact_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();

    run_resolution_session(&mut session, true, true).unwrap();
    let prime = session.candidate_query_telemetry(CandidateQueryFamily::PrimeWindow);
    let top_level = session.candidate_query_telemetry(CandidateQueryFamily::TopLevelNamed);
    let summary = session.candidate_query_telemetry(CandidateQueryFamily::FilteredNameSummary);
    let identifier_hydration =
        session.candidate_query_telemetry(CandidateQueryFamily::IdentifierHydration);
    let exact = session.finish_exact().unwrap();

    assert_eq!(exact.counts.identifiers, REPEATED_NAME_IDENTIFIERS as u64);
    let exact_connection = Connection::open(&exact_path).unwrap();
    let ambiguous: i64 = exact_connection
        .query_row(
            "SELECT COUNT(*) FROM identifier_resolutions WHERE outcome='ambiguous'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(ambiguous, REPEATED_NAME_IDENTIFIERS as i64);
    assert_eq!(prime.executions, 1);
    assert_eq!(prime.rows_read, WINDOW_SIZE);
    assert_eq!(summary.executions, 1);
    assert_eq!(summary.rows_read, 2);
    assert_eq!(identifier_hydration.executions, 1);
    assert_eq!(identifier_hydration.rows_read, REPEATED_NAME_IDENTIFIERS);
    assert!(
        top_level.executions <= REPEATED_NAME_TOP_LEVEL_QUERY_BOUND,
        "top-level candidate query executions were {}, expected at most {} for {} identifiers sharing one name and {} candidate rows",
        top_level.executions,
        REPEATED_NAME_TOP_LEVEL_QUERY_BOUND,
        REPEATED_NAME_IDENTIFIERS,
        REPEATED_NAME_CANDIDATES
    );
}

#[test]
fn candidate_query_sample_serializes_all_fixed_families() {
    let temp = tempfile::tempdir().unwrap();
    let layout = build_repeated_name_candidate_fixture(temp.path(), 1, WINDOW_SIZE + 1);
    let identity = manifest_identity(&layout, 1);
    let factory = StoreConnectionFactory::new(layout, FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let exact_path = temp.path().join("exact.db");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        identity,
        exact_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();
    run_resolution_session(&mut session, true, true).unwrap();

    let value = serde_json::to_value(candidate_query_sample(&session)).unwrap();
    let keys = value
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();

    assert_eq!(
        keys,
        vec![
            "by_name",
            "children_named",
            "filtered_by_name",
            "filtered_name_summary",
            "identifier_hydration",
            "imports",
            "locate_identifier",
            "module_version",
            "pending_hydration",
            "prime_window",
            "relationship_hydration",
            "symbol_by_id",
            "top_level_named",
            "type_facts",
        ]
    );
    assert_eq!(value["prime_window"]["executions"], 1);
    assert_eq!(value["prime_window"]["rows"], WINDOW_SIZE);
}

#[test]
fn finish_exact_observer_retains_every_cumulative_phase_and_preserves_identity() {
    let temp = tempfile::tempdir().unwrap();
    let baseline_layout =
        build_repeated_name_candidate_fixture(&temp.path().join("baseline"), 4, 5);
    let baseline_identity = manifest_identity(&baseline_layout, 1);
    let baseline_factory =
        StoreConnectionFactory::new(baseline_layout, FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let baseline_path = temp.path().join("baseline.db");
    let mut baseline_session = StoreScratchResolutionSession::new(
        baseline_factory,
        baseline_identity,
        &baseline_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();
    run_resolution_session(&mut baseline_session, true, true).unwrap();
    let baseline = baseline_session.finish_exact().unwrap();

    let observed_layout =
        build_repeated_name_candidate_fixture(&temp.path().join("observed"), 4, 5);
    let observed_identity = manifest_identity(&observed_layout, 1);
    let observed_factory =
        StoreConnectionFactory::new(observed_layout, FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let observed_path = temp.path().join("observed.db");
    let retained_path = temp.path().join("finish-live.json");
    let mut observed_session = StoreScratchResolutionSession::new(
        observed_factory,
        observed_identity,
        &observed_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();
    run_resolution_session(&mut observed_session, true, true).unwrap();
    let mut samples = Vec::<FinishExactPhaseSample>::new();
    let observed = observed_session
        .finish_exact_observing(|sample| {
            samples.push(sample);
            fs::write(&retained_path, serde_json::to_vec_pretty(&samples).unwrap()).unwrap();
        })
        .unwrap();

    assert_eq!(
        samples
            .iter()
            .map(|sample| sample.phase)
            .collect::<Vec<_>>(),
        vec![
            FinishExactPhase::PriorOverlay,
            FinishExactPhase::IdentifierTotality,
            FinishExactPhase::WriterInit,
            FinishExactPhase::SourceVersions,
            FinishExactPhase::IdentifierRows,
            FinishExactPhase::PendingRows,
            FinishExactPhase::WriterFinish,
            FinishExactPhase::ScratchCleanup,
        ]
    );
    assert!(
        samples
            .windows(2)
            .all(|pair| pair[0].cumulative_micros <= pair[1].cumulative_micros)
    );
    let retained: Vec<FinishExactPhaseSample> =
        serde_json::from_slice(&fs::read(retained_path).unwrap()).unwrap();
    assert_eq!(retained, samples);
    assert_eq!(
        serde_json::to_value(&samples).unwrap(),
        serde_json::json!([
            {"phase": "prior_overlay", "cumulative_micros": samples[0].cumulative_micros},
            {"phase": "identifier_totality", "cumulative_micros": samples[1].cumulative_micros},
            {"phase": "writer_init", "cumulative_micros": samples[2].cumulative_micros},
            {"phase": "source_versions", "cumulative_micros": samples[3].cumulative_micros},
            {"phase": "identifier_rows", "cumulative_micros": samples[4].cumulative_micros},
            {"phase": "pending_rows", "cumulative_micros": samples[5].cumulative_micros},
            {"phase": "writer_finish", "cumulative_micros": samples[6].cumulative_micros},
            {"phase": "scratch_cleanup", "cumulative_micros": samples[7].cumulative_micros},
        ])
    );
    assert_eq!(observed.manifest_hash, baseline.manifest_hash);
    assert_eq!(
        observed.resolver_output_epoch,
        baseline.resolver_output_epoch
    );
    assert_eq!(observed.catalog_hash, baseline.catalog_hash);
    assert_eq!(observed.file_bytes, baseline.file_bytes);
    assert_eq!(observed.file_sha256, baseline.file_sha256);
    assert_eq!(observed.counts, baseline.counts);
    assert_eq!(observed.counts.identifiers, 4);
    assert_eq!(observed.counts.pending, 0);
    assert_eq!(
        Connection::open(observed_path)
            .unwrap()
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
}

#[test]
fn finish_exact_live_diagnostic_retains_last_completed_phase_when_observer_stops() {
    let temp = tempfile::tempdir().unwrap();
    let layout = build_repeated_name_candidate_fixture(temp.path(), 4, 5);
    let identity = manifest_identity(&layout, 1);
    let factory = StoreConnectionFactory::new(layout, FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let exact_path = temp.path().join("exact.db");
    let output = temp.path().join("finish-live.json");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        identity,
        &exact_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();
    run_resolution_session(&mut session, true, true).unwrap();
    let queries = candidate_query_sample(&session);
    let mut completed = Vec::new();

    let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        session
            .finish_exact_observing(|sample| {
                completed.push(sample);
                persist_json_atomically(
                    &output,
                    &CandidateQuerySnapshot {
                        elapsed_ms: 17,
                        total_executions: queries.total_executions(),
                        locate_identifier_by_phase: LocateIdentifierPhaseSample::default(),
                        queries: queries.clone(),
                        finish_exact: completed.clone(),
                    },
                );
                if sample.phase == FinishExactPhase::WriterInit {
                    panic!("simulated diagnostic stop");
                }
            })
            .unwrap();
    }));

    assert!(stopped.is_err());
    let retained: CandidateQuerySnapshot =
        serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert_eq!(
        retained
            .finish_exact
            .iter()
            .map(|sample| sample.phase)
            .collect::<Vec<_>>(),
        vec![
            FinishExactPhase::PriorOverlay,
            FinishExactPhase::IdentifierTotality,
            FinishExactPhase::WriterInit,
        ]
    );
    assert!(
        retained
            .finish_exact
            .windows(2)
            .all(|pair| { pair[0].cumulative_micros <= pair[1].cumulative_micros })
    );
    assert_eq!(
        serde_json::to_value(&retained).unwrap()["finish_exact"][2]["phase"],
        "writer_init"
    );
}

#[test]
fn finish_exact_diagnostic_publishes_complete_fixed_samples_to_final_and_live_json() {
    let temp = tempfile::tempdir().unwrap();
    let layout = build_repeated_name_candidate_fixture(temp.path(), 4, 5);
    let identity = manifest_identity(&layout, 1);
    let factory = StoreConnectionFactory::new(layout, FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let exact_path = temp.path().join("exact.db");
    let output = temp.path().join("final.json");
    let live_output = temp.path().join("live.json");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        identity,
        &exact_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();
    run_resolution_session(&mut session, true, true).unwrap();
    let queries = candidate_query_sample(&session);
    let diagnostic = CandidateQueryDiagnostic {
        elapsed_ms: 23,
        configured_resolution_mode: "scoped".to_string(),
        configured_scope_file_count: 2,
        max_store_read_page: session.max_store_read_page(),
        max_candidate_cache_entries: session.max_candidate_cache_entries(),
        locate_identifier_by_phase: LocateIdentifierPhaseSample::default(),
        queries,
        finish_exact: Vec::new(),
    };

    let identity =
        finish_exact_with_diagnostics(session, Instant::now(), &output, &live_output, diagnostic);

    let final_diagnostic: CandidateQueryDiagnostic =
        serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    let live_diagnostic: CandidateQuerySnapshot =
        serde_json::from_slice(&fs::read(live_output).unwrap()).unwrap();
    let expected = vec![
        FinishExactPhase::PriorOverlay,
        FinishExactPhase::IdentifierTotality,
        FinishExactPhase::WriterInit,
        FinishExactPhase::SourceVersions,
        FinishExactPhase::IdentifierRows,
        FinishExactPhase::PendingRows,
        FinishExactPhase::WriterFinish,
        FinishExactPhase::ScratchCleanup,
    ];
    assert_eq!(
        final_diagnostic
            .finish_exact
            .iter()
            .map(|sample| sample.phase)
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(live_diagnostic.finish_exact, final_diagnostic.finish_exact);
    assert_eq!(identity.counts.identifiers, 4);
    assert_eq!(identity.counts.pending, 0);
    assert_eq!(
        serde_json::to_value(final_diagnostic).unwrap()["finish_exact"][7]["phase"],
        "scratch_cleanup"
    );
}

#[test]
fn finish_exact_cumulative_timing_excludes_observer_persistence_work() {
    let temp = tempfile::tempdir().unwrap();
    let layout = build_repeated_name_candidate_fixture(temp.path(), 1, 1);
    let identity = manifest_identity(&layout, 1);
    let factory = StoreConnectionFactory::new(layout, FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let exact_path = temp.path().join("exact.db");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        identity,
        &exact_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();
    run_resolution_session(&mut session, true, true).unwrap();
    let mut samples = Vec::new();
    let wall_started = Instant::now();

    session
        .finish_exact_observing(|sample| {
            samples.push(sample);
            std::thread::sleep(Duration::from_millis(10));
        })
        .unwrap();

    let wall_micros = u64::try_from(wall_started.elapsed().as_micros()).unwrap();
    let final_cumulative = samples.last().unwrap().cumulative_micros;
    assert_eq!(samples.len(), 8);
    assert!(
        final_cumulative.saturating_add(50_000) < wall_micros,
        "final cumulative {final_cumulative}us should exclude observer wall {wall_micros}us"
    );
}

#[test]
fn streaming_identifier_writer_reuses_one_statement_at_high_cardinality() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("identifier-writer.db");
    let mut writer = ResolutionBaseWriter::new(&path, "manifest-a", RESOLVER_OUTPUT_EPOCH).unwrap();
    writer.push_source_version(10).unwrap();
    writer.push_source_version(20).unwrap();
    let started = Instant::now();

    for index in 0..IDENTIFIER_WRITER_ROWS {
        writer
            .push_identifier_resolution(julie_extract_artifact::store::ResolutionIdentifierRow {
                version_id: 10,
                identifier_id: format!("identifier-{index:08}"),
                target_version_id: Some(20),
                target_symbol_id: Some("symbol-4".to_string()),
                tier: Some(1),
                confidence: Some(0.95),
                method: Some("same_file".to_string()),
                outcome: "resolved".to_string(),
                candidates: Some(1),
            })
            .unwrap();
    }
    let insert_elapsed = started.elapsed();
    let identity = writer
        .finish_with_target_lookup(|version_id, symbol_id| {
            Ok(version_id == 20 && symbol_id == "symbol-4")
        })
        .unwrap();

    assert_eq!(identity.manifest_hash, "manifest-a");
    assert_eq!(identity.resolver_output_epoch, RESOLVER_OUTPUT_EPOCH);
    assert_eq!(identity.counts.identifiers, IDENTIFIER_WRITER_ROWS as u64);
    assert_eq!(identity.counts.pending, 0);
    assert_eq!(identity.path, path);
    assert!(!identity.file_sha256.is_empty());
    let connection = Connection::open(&identity.path).unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT identifier_id FROM identifier_resolutions ORDER BY version_id,identifier_id LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "identifier-00000000"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT identifier_id FROM identifier_resolutions ORDER BY version_id DESC,identifier_id DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "identifier-00099999"
    );
    assert_eq!(
        connection
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    eprintln!(
        "identifier_writer_rows={} insert_elapsed_ms={}",
        IDENTIFIER_WRITER_ROWS,
        insert_elapsed.as_millis()
    );
    assert!(
        insert_elapsed <= IDENTIFIER_WRITER_MAX,
        "streaming {IDENTIFIER_WRITER_ROWS} identifier rows took {insert_elapsed:?}, expected at most {IDENTIFIER_WRITER_MAX:?}"
    );
}

#[test]
fn snapshotting_session_attributes_locator_calls_to_fixed_phases() {
    let temp = tempfile::tempdir().unwrap();
    let layout = build_repeated_name_candidate_fixture(temp.path(), 1, 1);
    let identity = manifest_identity(&layout, 1);
    let factory = StoreConnectionFactory::new(layout, FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let exact_path = temp.path().join("exact.db");
    let output = temp.path().join("snapshot.json");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        identity,
        exact_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();
    let mut snapshotting = SnapshottingSession {
        inner: &mut session,
        output,
        started: Instant::now(),
        next_execution_threshold: usize::MAX,
        phase: Cell::new(None),
        locate_identifier_by_phase: Cell::new(LocateIdentifierPhaseSample::default()),
    };
    let worklists = snapshotting
        .open_resolution_pass(&ResolutionPassRequest::full())
        .unwrap();
    let version = SemanticVersionId::Store(1);
    let locate = |session: &SnapshottingSession<'_>| {
        session
            .locate_identifier(&version, "shared_target", Some(131), Some(137), 3)
            .unwrap()
    };

    assert_eq!(
        locate(&snapshotting).as_deref(),
        Some("identifier-00000000")
    );
    for (phase, calls) in [
        (ResolutionPhase::ResolvedPending, 1),
        (ResolutionPhase::Pending, 2),
        (ResolutionPhase::Relationships, 3),
        (ResolutionPhase::Identifiers, 4),
    ] {
        let mut phase_worklists = worklists.clone();
        phase_worklists.phase = phase;
        snapshotting.next_phase_chunk(&phase_worklists).unwrap();
        for _ in 0..calls {
            locate(&snapshotting);
        }
    }

    assert_eq!(
        snapshotting.locate_identifier_phase_sample(),
        LocateIdentifierPhaseSample {
            resolved_pending: 1,
            pending: 2,
            relationships: 3,
            other_or_unset: 5,
        }
    );
}

#[test]
fn live_candidate_query_snapshot_persists_before_resolution_finishes() {
    let temp = tempfile::tempdir().unwrap();
    let layout = build_repeated_name_candidate_fixture(temp.path(), 4, WINDOW_SIZE + 1);
    let identity = manifest_identity(&layout, 1);
    let factory = StoreConnectionFactory::new(layout, FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let exact_path = temp.path().join("exact.db");
    let output = temp.path().join("snapshot.json");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        identity,
        exact_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();
    let mut snapshotting = SnapshottingSession {
        inner: &mut session,
        output: output.clone(),
        started: Instant::now(),
        next_execution_threshold: 1,
        phase: Cell::new(None),
        locate_identifier_by_phase: Cell::new(LocateIdentifierPhaseSample::default()),
    };

    run_resolution_session(&mut snapshotting, true, true).unwrap();
    let snapshot: CandidateQuerySnapshot =
        serde_json::from_slice(&fs::read(output).unwrap()).unwrap();

    assert!(snapshot.total_executions >= 1);
    assert_eq!(snapshot.queries.prime_window.executions, 1);
    assert_eq!(snapshot.queries.prime_window.rows, WINDOW_SIZE);
}

#[test]
fn store_resolution_performance_worker() {
    let Ok(store_root) = std::env::var("JULIE_STORE_RESOLUTION_PERF_WORKER_STORE") else {
        return;
    };
    let pair = std::env::var("JULIE_STORE_RESOLUTION_PERF_WORKER_PAIR").unwrap();
    let run = std::env::var("JULIE_STORE_RESOLUTION_PERF_WORKER_RUN")
        .unwrap()
        .parse()
        .unwrap();
    let output = PathBuf::from(std::env::var("JULIE_STORE_RESOLUTION_PERF_WORKER_OUTPUT").unwrap());
    let sample = measure_pair(Path::new(&store_root), &pair, run, output.parent().unwrap());
    fs::write(output, serde_json::to_vec_pretty(&sample).unwrap()).unwrap();
}

#[test]
fn store_resolution_query_diagnostic_fixture() {
    let Ok(store_root) = std::env::var("JULIE_STORE_RESOLUTION_QUERY_PREPARE_STORE") else {
        return;
    };
    let view_id = std::env::var("JULIE_STORE_RESOLUTION_QUERY_VIEW").unwrap();
    let layout = StoreLayout::open(store_root).unwrap();
    prepare_replay_view(&layout, &view_id, ReplayMode::Scoped);
}

#[test]
fn store_resolution_query_diagnostic_worker() {
    let Ok(store_root) = std::env::var("JULIE_STORE_RESOLUTION_QUERY_STORE") else {
        return;
    };
    let view_id = std::env::var("JULIE_STORE_RESOLUTION_QUERY_VIEW").unwrap();
    let generation = std::env::var("JULIE_STORE_RESOLUTION_QUERY_GENERATION")
        .unwrap()
        .parse()
        .unwrap();
    let output = PathBuf::from(std::env::var("JULIE_STORE_RESOLUTION_QUERY_OUTPUT").unwrap());
    let layout = StoreLayout::open(&store_root).unwrap();
    let identity = query_diagnostic_identity(&layout, &view_id, generation);
    let factory = StoreConnectionFactory::new(layout, FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let exact_path = output.with_extension("exact.db");
    assert!(!exact_path.exists());
    let mut session = StoreScratchResolutionSession::new(
        factory,
        identity,
        &exact_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();
    let started = Instant::now();
    let snapshot_output = output.with_extension("live.json");
    let mut snapshotting = SnapshottingSession {
        inner: &mut session,
        output: snapshot_output.clone(),
        started,
        next_execution_threshold: 1,
        phase: Cell::new(None),
        locate_identifier_by_phase: Cell::new(LocateIdentifierPhaseSample::default()),
    };
    run_resolution_session(&mut snapshotting, false, true).unwrap();
    let locate_identifier_by_phase = snapshotting.locate_identifier_phase_sample();
    drop(snapshotting);
    let elapsed_ms = elapsed_ms(started.elapsed());
    let diagnostic = CandidateQueryDiagnostic {
        elapsed_ms,
        configured_resolution_mode: "scoped".to_string(),
        configured_scope_file_count: MILLER_CHANGED_FILES,
        max_store_read_page: session.max_store_read_page(),
        max_candidate_cache_entries: session.max_candidate_cache_entries(),
        locate_identifier_by_phase,
        queries: candidate_query_sample(&session),
        finish_exact: Vec::new(),
    };
    finish_exact_with_diagnostics(session, started, &output, &snapshot_output, diagnostic);
}

#[test]
fn store_resolution_query_diagnostic_readiness() {
    let Ok(store_root) = std::env::var("JULIE_STORE_RESOLUTION_QUERY_STORE") else {
        return;
    };
    let view_id = std::env::var("JULIE_STORE_RESOLUTION_QUERY_VIEW").unwrap();
    let generation = std::env::var("JULIE_STORE_RESOLUTION_QUERY_GENERATION")
        .unwrap()
        .parse()
        .unwrap();
    let layout = StoreLayout::open(store_root).unwrap();
    query_diagnostic_identity(&layout, &view_id, generation);
}

#[test]
fn store_resolution_performance_gate() {
    let Ok(out_dir) = std::env::var("JULIE_STORE_RESOLUTION_PERF_OUT_DIR") else {
        return;
    };
    let runs: usize = std::env::var("JULIE_STORE_RESOLUTION_PERF_RUNS")
        .unwrap()
        .parse()
        .unwrap();
    assert!(runs >= 3);
    let out_dir = PathBuf::from(out_dir);
    fs::create_dir_all(&out_dir).unwrap();
    let rows = std::env::var("JULIE_STORE_RESOLUTION_PERF_ROWS")
        .ok()
        .map(|value| value.parse().unwrap())
        .unwrap_or(MILLER_IDENTIFIER_ROWS);

    for run in 1..=runs {
        let run_dir = out_dir.join(format!("run-{run:03}"));
        reset_owned_directory(&run_dir);
        let mut samples = Vec::with_capacity(PAIRS.len());
        for pair in PAIRS {
            let fixture_root = run_dir.join(format!("fixture-{pair}"));
            build_store_fixture(
                &fixture_root,
                rows,
                scaled_pending_rows(rows),
                scaled_resolved_pending_rows(rows),
            );
            let worker_output = run_dir.join(format!(".{pair}.worker.json"));
            let mut command = timed_worker_command();
            let output = command
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "store_resolution_performance_worker",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(
                    "JULIE_STORE_RESOLUTION_PERF_WORKER_STORE",
                    fixture_root.join("family"),
                )
                .env("JULIE_STORE_RESOLUTION_PERF_WORKER_PAIR", pair)
                .env("JULIE_STORE_RESOLUTION_PERF_WORKER_RUN", run.to_string())
                .env("JULIE_STORE_RESOLUTION_PERF_WORKER_OUTPUT", &worker_output)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let mut sample: Sample =
                serde_json::from_slice(&fs::read(&worker_output).unwrap()).unwrap();
            sample.peak_rss_bytes = sample.peak_rss_bytes.max(parse_peak_rss(&output.stderr));
            samples.push(sample);
            fs::remove_file(worker_output).unwrap();
        }
        let row_level_differences =
            artifact_semantic_differences(&samples[0].artifact_path, &samples[1].artifact_path);
        assert_eq!(row_level_differences, 0);
        assert_eq!(
            samples[0].canonical_semantic_digest,
            samples[1].canonical_semantic_digest
        );
        for sample in &mut samples {
            sample.semantic_differences = Some(row_level_differences);
            sample.applied_differences = Some(row_level_differences);
            sample.row_level_differences = Some(row_level_differences);
            fs::write(
                run_dir.join(format!("{}.json", sample.pair)),
                serde_json::to_vec_pretty(sample).unwrap(),
            )
            .unwrap();
        }
        let forced_full = samples
            .iter()
            .find(|sample| sample.resolution_mode == "full")
            .unwrap();
        let scoped = samples
            .iter()
            .find(|sample| sample.resolution_mode == "scoped")
            .unwrap();
        assert!(scoped.time_to_exact_ms < 30_000);
        if rows == MILLER_IDENTIFIER_ROWS {
            assert!(scoped.time_to_exact_ms < forced_full.time_to_exact_ms);
        }
    }
}

fn reset_owned_directory(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("failed to reset {}: {error}", path.display()),
    }
    fs::create_dir_all(path).unwrap();
}

fn timed_worker_command() -> Command {
    let mut command = Command::new("/usr/bin/time");
    #[cfg(target_os = "macos")]
    command.arg("-l");
    #[cfg(not(target_os = "macos"))]
    command.arg("-v");
    command
}

fn parse_peak_rss(stderr: &[u8]) -> u64 {
    let text = String::from_utf8_lossy(stderr);
    #[cfg(target_os = "macos")]
    {
        text.lines()
            .find(|line| line.contains("maximum resident set size"))
            .and_then(|line| line.split_whitespace().next())
            .and_then(|value| value.parse().ok())
            .unwrap()
    }
    #[cfg(not(target_os = "macos"))]
    {
        text.lines()
            .find(|line| line.contains("Maximum resident set size"))
            .and_then(|line| line.split(':').nth(1))
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap()
            * 1_024
    }
}

fn derived_residual_ms(wall_ms: u64, phase_timings_ms: &BTreeMap<String, u64>) -> u64 {
    let resolution_ms = phase_timings_ms.get("resolution").copied().unwrap_or(0);
    let diff_ms = phase_timings_ms.get("diff").copied().unwrap_or(0);
    wall_ms.saturating_sub(resolution_ms.saturating_add(diff_ms))
}

fn measure_pair(store_root: &Path, pair: &str, run: usize, out_dir: &Path) -> Sample {
    assert!(PAIRS.contains(&pair));
    let layout = StoreLayout::open(store_root).unwrap();
    let mode = replay_mode(pair);
    let sample_root = out_dir.join(format!("worker-{pair}-{run}"));
    fs::create_dir_all(&sample_root).unwrap();
    let view_id = format!("replay-{pair}-{run}");
    let fixture_snapshot_digest = fixture_snapshot_digest(&layout);
    let before = prepare_replay_view(&layout, &view_id, mode);
    let request_id = format!("replay-resolve-{pair}-{run}");
    let timed = run_timed_resolve(store_root, &view_id, &request_id, mode);
    let reported_mode = timed.report["resolution"]["resolution_mode"]
        .as_str()
        .unwrap();
    assert_eq!(reported_mode, mode.as_str());
    assert_eq!(timed.report["resolution"]["state"], "exact");
    let phase_timings_ms = timed.report["resolution"]["phase_timings_ms"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(phase, value)| (phase.clone(), value.as_u64().unwrap()))
        .collect::<BTreeMap<_, _>>();
    let scope_file_count = timed.report["resolution"]["scope_file_count"]
        .as_u64()
        .unwrap();
    let scope_name_count = timed.report["resolution"]["scope_name_count"]
        .as_u64()
        .unwrap();
    let scope_row_count = timed.report["resolution"]["scope_row_count"]
        .as_u64()
        .unwrap();
    let fallback_reason = timed.report["resolution"]["fallback_reason"]
        .as_str()
        .map(str::to_string);
    if mode == ReplayMode::Scoped {
        assert_eq!(scope_file_count, MILLER_CHANGED_FILES as u64);
        assert!(fallback_reason.is_none());
    }
    let artifact_path = sample_root.join("resolved.db");
    export_view(store_root, &view_id, &artifact_path);
    let canonical_semantic_digest = artifact_semantic_digest(&artifact_path);
    let artifact = Connection::open(&artifact_path).unwrap();
    let identifier_rows = artifact
        .query_row("SELECT COUNT(*) FROM identifier_resolutions", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|value| u64::try_from(value).unwrap())
        .unwrap();
    let pending_rows = artifact
        .query_row("SELECT COUNT(*) FROM pending_resolutions", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|value| u64::try_from(value).unwrap())
        .unwrap();
    let after = current_resolution_storage(&layout, &view_id);
    let rebased = Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM store_log
             WHERE request_id=?1 AND event_kind='resolution_exact_rebased')",
            [&request_id],
            |row| row.get::<_, bool>(0),
        )
        .unwrap();
    if mode == ReplayMode::Scoped {
        assert!(rebased);
        assert!(before.gap_bytes > 64 * 1024 * 1024);
        assert!(after.gap_bytes <= 64 * 1024 * 1024);
        assert_eq!(after.delta_rows, 0);
    }
    let resolution_compute_ms = phase_timings_ms.get("resolution").copied().unwrap_or(0);
    let diff_ms = phase_timings_ms.get("diff").copied().unwrap_or(0);
    let scoped_ms = phase_timings_ms.get("scope").copied().unwrap_or(0);
    let publish_ms = derived_residual_ms(timed.wall_ms, &phase_timings_ms);
    let base_bytes = current_resolution_base_bytes(&layout, &view_id);

    Sample {
        pair: pair.to_string(),
        run,
        resolution_compute_ms,
        store_fresh_ms: timed.wall_ms,
        diff_ms,
        delta_write_ms: publish_ms,
        publish_ms,
        time_to_exact_ms: timed.wall_ms,
        integrity_ms: publish_ms,
        identifier_rows,
        pending_rows,
        peak_rss_bytes: timed.peak_rss_bytes,
        base_bytes,
        delta_bytes: after.gap_bytes,
        semantic_differences: None,
        applied_differences: None,
        foreground_bind_ms: scoped_ms,
        foreground_identifier_work: None,
        background_pipeline_ms: timed.wall_ms,
        resolution_mode: reported_mode.to_string(),
        cpu_user_ms: timed.cpu_user_ms,
        cpu_system_ms: timed.cpu_system_ms,
        phase_timings_ms,
        scope_file_count,
        scope_name_count,
        scope_row_count,
        fallback_reason,
        canonical_semantic_digest,
        row_level_differences: None,
        fixture_snapshot_digest,
        artifact_path,
        exact_gap_rows: after.gap_rows,
        exact_gap_files: after.gap_files,
        cumulative_gap_bytes_before: before.gap_bytes,
        cumulative_gap_bytes_after: after.gap_bytes,
        cumulative_delta_rows_before: before.delta_rows,
        cumulative_delta_rows_after: after.delta_rows,
        rebased,
    }
}

struct TimedResolve {
    report: Value,
    wall_ms: u64,
    cpu_user_ms: u64,
    cpu_system_ms: u64,
    peak_rss_bytes: u64,
}

#[derive(Clone, Copy)]
struct ResolutionStorage {
    gap_bytes: u64,
    gap_rows: u64,
    gap_files: u64,
    delta_rows: u64,
}

#[derive(Clone, Copy)]
struct ResolutionRowShape {
    identifiers: usize,
    pending: usize,
    resolved_pending: usize,
    distinct_target_names: usize,
}

fn prepare_replay_view(layout: &StoreLayout, view_id: &str, mode: ReplayMode) -> ResolutionStorage {
    prepare_replay_view_with_changed_files(layout, view_id, mode, MILLER_CHANGED_FILES, true, None)
}

fn prepare_replay_view_with_changed_files(
    layout: &StoreLayout,
    view_id: &str,
    mode: ReplayMode,
    changed_files: usize,
    install_gap: bool,
    minimum_scope_rows: Option<usize>,
) -> ResolutionStorage {
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let mut connection = factory.open_writer().unwrap();
    let mut manifests = ManifestStore::new(&mut connection);
    let base_entries = manifests.entries(VIEW_ID, 1).unwrap();
    let current_entries = manifests.entries(VIEW_ID, 2).unwrap();
    manifests.ensure_view(view_id, ROOT).unwrap();
    let base = manifests
        .publish(
            view_id,
            None,
            base_entries,
            &format!("replay-base-{view_id}"),
        )
        .unwrap();
    assert_eq!(base.generation, 1);
    drop(connection);
    ensure_ready_replay_base(layout, view_id);
    let bindings = ResolutionBindingStore::new(factory.clone());
    let bound = bindings
        .bind_base(
            view_id,
            RESOLVER_OUTPUT_EPOCH,
            &format!("replay-bind-{view_id}"),
            NOW,
        )
        .unwrap();
    assert_eq!(bound.state.as_str(), "exact");
    if mode == ReplayMode::Scoped && install_gap {
        install_canonical_gap_payload(layout, view_id, REBASE_GAP_BYTES);
    }
    let before = current_resolution_storage(layout, view_id);
    let mut connection = factory.open_writer().unwrap();
    let current = ManifestStore::new(&mut connection)
        .publish(
            view_id,
            Some(1),
            current_entries,
            &format!("replay-current-{view_id}"),
        )
        .unwrap();
    assert_eq!(current.generation, 2);
    let scope: (i64, bool) = Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT change_count,scope_usable FROM resolution_scope_batches
             WHERE view_id=?1 ORDER BY transition_id DESC LIMIT 1",
            [view_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(scope, (changed_files as i64, true));
    if let Some(minimum_scope_rows) = minimum_scope_rows {
        let changed_path = format!("src/file-{:04}.cs", MILLER_FILE_ROWS - changed_files);
        assert_changed_scope_rows(layout, view_id, &changed_path, minimum_scope_rows);
    }
    before
}

fn assert_changed_scope_rows(
    layout: &StoreLayout,
    view_id: &str,
    changed_path: &str,
    minimum_rows: usize,
) {
    let connection = Connection::open(layout.store_db()).unwrap();
    let (scope_file_count, predecessor_version, replacement_version): (
        i64,
        Option<i64>,
        Option<i64>,
    ) = connection
        .query_row(
            "SELECT COUNT(*),MIN(journal.old_version_id),MIN(journal.new_version_id)
             FROM resolution_scope_journal AS journal
             JOIN resolution_scope_batches AS batch
               ON batch.transition_id=journal.transition_id
             WHERE batch.view_id=?1
               AND batch.to_manifest_generation=2
               AND journal.path=?2",
            [view_id, changed_path],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(scope_file_count, 1);
    let predecessor_version = predecessor_version.expect("scope predecessor version missing");
    let replacement_version = replacement_version.expect("scope replacement version missing");
    let expected_identifiers = i64::try_from(minimum_rows).unwrap();
    let expected_pending = i64::try_from(scaled_pending_rows(minimum_rows)).unwrap();
    let expected_scope_rows = expected_identifiers + expected_pending;
    for (label, version_id) in [
        ("predecessor", predecessor_version),
        ("replacement", replacement_version),
    ] {
        let (identifier_rows, pending_rows, relationship_rows): (i64, i64, i64) = connection
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM identifiers WHERE version_id=?1),
                    (SELECT COUNT(*) FROM pending_relationships WHERE version_id=?1),
                    (SELECT COUNT(*) FROM relationships WHERE version_id=?1)",
                [version_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(identifier_rows, expected_identifiers, "{label} identifiers");
        assert_eq!(pending_rows, expected_pending, "{label} pending rows");
        assert_eq!(relationship_rows, 0, "{label} relationship rows");
        assert_eq!(
            identifier_rows + pending_rows + relationship_rows,
            expected_scope_rows,
            "{label} scope rows"
        );
    }
}

fn install_canonical_gap_payload(layout: &StoreLayout, view_id: &str, bytes: usize) {
    let prefix = r#"{"files":[1],"rows":[{"kind":"added","local_id":""#;
    let suffix = r#"","table":"identifier","version_id":1}]}"#;
    let padding = bytes.checked_sub(prefix.len() + suffix.len()).unwrap();
    Connection::open(layout.store_db())
        .unwrap()
        .execute(
            "UPDATE resolution_deltas
             SET exact_gap_rows=1,exact_gap_files=1,
                 exact_gap_json=?1 || substr(replace(hex(zeroblob((?2 + 1) / 2)),'0','x'),1,?2) || ?3
             WHERE view_id=?4
               AND delta_generation=(SELECT resolution_delta_generation FROM views WHERE view_id=?4)",
            params![
                prefix,
                i64::try_from(padding).unwrap(),
                suffix,
                view_id
            ],
        )
        .unwrap();
}

fn run_timed_resolve(
    store_root: &Path,
    view_id: &str,
    request_id: &str,
    mode: ReplayMode,
) -> TimedResolve {
    run_timed_resolve_with_delta(store_root, view_id, request_id, Some(mode.env_value()))
}

fn run_timed_resolve_with_delta(
    store_root: &Path,
    view_id: &str,
    request_id: &str,
    delta: Option<&str>,
) -> TimedResolve {
    let mut command = timed_worker_command();
    let started = Instant::now();
    match delta {
        Some(value) => {
            command.env("JULIE_STORE_RESOLUTION_DELTA", value);
        }
        None => {
            command.env_remove("JULIE_STORE_RESOLUTION_DELTA");
        }
    }
    let output = command
        .arg(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "resolve",
            "--store",
            store_root.to_str().unwrap(),
            "--view",
            view_id,
            "--request-id",
            request_id,
            "--idempotency-key",
            request_id,
            "--json",
        ])
        .output()
        .unwrap();
    let wall_ms = elapsed_ms(started.elapsed());
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    TimedResolve {
        report: serde_json::from_slice(&output.stdout).unwrap(),
        wall_ms,
        cpu_user_ms: parse_cpu_ms(&output.stderr, "User time"),
        cpu_system_ms: parse_cpu_ms(&output.stderr, "System time"),
        peak_rss_bytes: parse_peak_rss(&output.stderr),
    }
}

fn run_resolve_with_instant(
    store_root: &Path,
    view_id: &str,
    request_id: &str,
    delta: Option<&str>,
) -> TimedResolve {
    let mut command = Command::new(env!("CARGO_BIN_EXE_julie-extract"));
    match delta {
        Some(value) => {
            command.env("JULIE_STORE_RESOLUTION_DELTA", value);
        }
        None => {
            command.env_remove("JULIE_STORE_RESOLUTION_DELTA");
        }
    }
    let started = Instant::now();
    let output = command
        .args([
            "store",
            "resolve",
            "--store",
            store_root.to_str().unwrap(),
            "--view",
            view_id,
            "--request-id",
            request_id,
            "--idempotency-key",
            request_id,
            "--json",
        ])
        .output()
        .unwrap();
    let wall_ms = elapsed_ms(started.elapsed());
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    TimedResolve {
        report: serde_json::from_slice(&output.stdout).unwrap(),
        wall_ms,
        cpu_user_ms: 0,
        cpu_system_ms: 0,
        peak_rss_bytes: 0,
    }
}

fn parse_cpu_ms(stderr: &[u8], label: &str) -> u64 {
    String::from_utf8_lossy(stderr)
        .lines()
        .find(|line| line.contains(label))
        .and_then(|line| line.rsplit(':').next())
        .and_then(|value| value.trim().parse::<f64>().ok())
        .map(|seconds| (seconds * 1_000.0).round() as u64)
        .unwrap()
}

fn export_view(store_root: &Path, view_id: &str, artifact_path: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "export",
            "--store",
            store_root.to_str().unwrap(),
            "--view",
            view_id,
            "--out",
            artifact_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn current_resolution_storage(layout: &StoreLayout, view_id: &str) -> ResolutionStorage {
    let values: (i64, i64, i64, i64) = Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT COALESCE(SUM(length(CAST(delta.exact_gap_json AS BLOB))),0),
                    COALESCE(SUM(delta.exact_gap_rows),0),
                    COALESCE(SUM(delta.exact_gap_files),0),
                    COALESCE(SUM(delta.identifier_replacements + delta.pending_replacements +
                                 delta.pending_tombstones),0)
             FROM views AS view
             JOIN resolution_deltas AS delta
               ON delta.view_id=view.view_id AND delta.base_id=view.resolution_base_id
            WHERE view.view_id=?1",
            [view_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    ResolutionStorage {
        gap_bytes: u64::try_from(values.0).unwrap(),
        gap_rows: u64::try_from(values.1).unwrap(),
        gap_files: u64::try_from(values.2).unwrap(),
        delta_rows: u64::try_from(values.3).unwrap(),
    }
}

fn current_resolution_base_bytes(layout: &StoreLayout, view_id: &str) -> u64 {
    let relative_path: String = Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT base.relative_path FROM views AS view
             JOIN resolution_bases AS base ON base.base_id=view.resolution_base_id
             WHERE view.view_id=?1",
            [view_id],
            |row| row.get(0),
        )
        .unwrap();
    fs::metadata(layout.generation_dir().join(relative_path))
        .unwrap()
        .len()
}

fn fixture_snapshot_digest(layout: &StoreLayout) -> String {
    let connection = Connection::open(layout.store_db()).unwrap();
    let mut digest = Sha256::new();
    for query in [
        "SELECT * FROM file_versions ORDER BY version_id",
        "SELECT * FROM manifests WHERE view_id='view-miller-scale' ORDER BY generation",
        "SELECT * FROM manifest_entries WHERE view_id='view-miller-scale' ORDER BY generation,path COLLATE BINARY",
        "SELECT * FROM symbols ORDER BY version_id,symbol_id COLLATE BINARY",
        "SELECT * FROM symbol_annotations ORDER BY 1,2",
        "SELECT * FROM reference_sites ORDER BY version_id,reference_site_id COLLATE BINARY",
        "SELECT * FROM identifiers ORDER BY version_id,identifier_id COLLATE BINARY",
        "SELECT * FROM pending_relationships ORDER BY version_id,pending_relationship_id COLLATE BINARY",
        "SELECT * FROM relationships ORDER BY version_id,relationship_id COLLATE BINARY",
        "SELECT * FROM type_facts ORDER BY 1,2",
        "SELECT * FROM type_argument_usages ORDER BY 1,2",
        "SELECT * FROM type_arguments ORDER BY 1,2",
        "SELECT * FROM literals ORDER BY 1,2",
        "SELECT * FROM source_regions ORDER BY 1,2",
        "SELECT * FROM structural_facts ORDER BY 1,2",
        "SELECT * FROM complexity_metrics ORDER BY 1,2",
        "SELECT * FROM parse_diagnostics ORDER BY 1,2",
    ] {
        let mut statement = connection.prepare(query).unwrap();
        let column_count = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for index in 0..column_count {
                digest.update(format!("{:?}\0", row.get_ref(index).unwrap()).as_bytes());
            }
            digest.update(b"\n");
        }
        digest.update(b"\x1e");
    }
    format!("{:x}", digest.finalize())
}

fn artifact_semantic_digest(path: &Path) -> String {
    let connection = Connection::open(path).unwrap();
    let mut digest = Sha256::new();
    for query in [
        format!(
            "{} ORDER BY file.path COLLATE BINARY",
            artifact_projection("main", "files")
        ),
        format!(
            "{} ORDER BY source.path COLLATE BINARY,identifier.start_byte,identifier.identifier_id COLLATE BINARY",
            artifact_projection("main", "identifiers")
        ),
        format!(
            "{} ORDER BY source.path COLLATE BINARY,pending.start_byte,pending.pending_relationship_id COLLATE BINARY",
            artifact_projection("main", "pending")
        ),
    ] {
        let mut statement = connection.prepare(&query).unwrap();
        let column_count = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for index in 0..column_count {
                digest.update(format!("{:?}\0", row.get_ref(index).unwrap()).as_bytes());
            }
            digest.update(b"\n");
        }
        digest.update(b"\x1e");
    }
    format!("{:x}", digest.finalize())
}

fn artifact_semantic_differences(left: &Path, right: &Path) -> u64 {
    let connection = Connection::open(left).unwrap();
    connection
        .execute("ATTACH DATABASE ?1 AS compared", [right.to_str().unwrap()])
        .unwrap();
    ["files", "identifiers", "pending"]
        .into_iter()
        .map(|table| {
            let left = artifact_projection("main", table);
            let right = artifact_projection("compared", table);
            let forward = format!("SELECT COUNT(*) FROM ({left} EXCEPT {right})");
            let reverse = format!("SELECT COUNT(*) FROM ({right} EXCEPT {left})");
            let forward = connection
                .query_row(&forward, [], |row| row.get::<_, i64>(0))
                .unwrap();
            let reverse = connection
                .query_row(&reverse, [], |row| row.get::<_, i64>(0))
                .unwrap();
            u64::try_from(forward + reverse).unwrap()
        })
        .sum()
}

fn artifact_projection(schema: &str, table: &str) -> String {
    match table {
        "files" => format!(
            "SELECT file.path,file.language,file.content_hash
             FROM {schema}.files AS file"
        ),
        "identifiers" => format!(
            "SELECT source.path,identifier.name,identifier.kind,identifier.start_byte,
                    identifier.end_byte,resolution.outcome,target_file.path,target.name,
                    resolution.tier,resolution.confidence,resolution.method,resolution.candidates
             FROM {schema}.identifier_resolutions AS resolution
             JOIN {schema}.identifiers AS identifier
               ON identifier.identifier_id=resolution.identifier_id
             JOIN {schema}.files AS source ON source.file_id=identifier.file_id
             LEFT JOIN {schema}.symbols AS target
               ON target.symbol_id=resolution.target_symbol_id
             LEFT JOIN {schema}.files AS target_file ON target_file.file_id=target.file_id"
        ),
        "pending" => format!(
            "SELECT source.path,pending.kind,pending.target_terminal_name,pending.start_byte,
                    pending.end_byte,target_file.path,target.name,resolution.tier,
                    resolution.confidence,resolution.method
             FROM {schema}.pending_resolutions AS resolution
             JOIN {schema}.pending_relationships AS pending
               ON pending.pending_relationship_id=resolution.pending_relationship_id
             JOIN {schema}.files AS source ON source.file_id=pending.file_id
             JOIN {schema}.symbols AS target ON target.symbol_id=resolution.target_symbol_id
             JOIN {schema}.files AS target_file ON target_file.file_id=target.file_id"
        ),
        other => panic!("unexpected artifact projection {other}"),
    }
}

fn manifest_identity(layout: &StoreLayout, generation: i64) -> StoreManifestIdentity {
    view_manifest_identity(layout, VIEW_ID, generation)
}

fn view_manifest_identity(
    layout: &StoreLayout,
    view_id: &str,
    generation: i64,
) -> StoreManifestIdentity {
    let connection = Connection::open(layout.store_db()).unwrap();
    let manifest_hash = connection
        .query_row(
            "SELECT manifest_hash FROM manifests WHERE view_id=?1 AND generation=?2",
            params![view_id, generation],
            |row| row.get(0),
        )
        .unwrap();
    StoreManifestIdentity {
        family_id: FAMILY_ID.to_string(),
        view_id: view_id.to_string(),
        generation,
        manifest_hash,
    }
}

fn build_store_fixture(
    store_root: &Path,
    identifier_rows: usize,
    pending_rows: usize,
    resolved_pending_rows: usize,
) {
    build_store_fixture_with_changed_files(
        store_root,
        identifier_rows,
        pending_rows,
        resolved_pending_rows,
        MILLER_CHANGED_FILES,
    );
}

fn build_store_fixture_with_changed_files(
    store_root: &Path,
    identifier_rows: usize,
    pending_rows: usize,
    resolved_pending_rows: usize,
    changed_files: usize,
) {
    build_store_fixture_with_changed_file_rows(
        store_root,
        identifier_rows,
        pending_rows,
        resolved_pending_rows,
        changed_files,
        None,
        None,
    );
}

fn build_store_fixture_with_changed_file_rows(
    store_root: &Path,
    identifier_rows: usize,
    pending_rows: usize,
    resolved_pending_rows: usize,
    changed_files: usize,
    changed_file_shape: Option<ResolutionRowShape>,
    stable_file_shape: Option<ResolutionRowShape>,
) {
    assert!(changed_files > 0 && changed_files <= MILLER_FILE_ROWS);
    if changed_file_shape.is_some() {
        assert_eq!(changed_files, 1);
    }
    let layout = StoreLayout::create(
        store_root.join("family"),
        FAMILY_ID,
        env!("CARGO_PKG_VERSION"),
    )
    .unwrap();
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view(VIEW_ID, ROOT)
        .unwrap();
    let transaction = connection.transaction().unwrap();
    let mut versions = Vec::with_capacity(MILLER_FILE_ROWS);
    for index in 0..MILLER_FILE_ROWS {
        let path = if index == 0 {
            "src/miller-scale.cs".to_string()
        } else {
            format!("src/file-{index:04}.cs")
        };
        versions.push(insert_version(
            &transaction,
            &path,
            &format!("hash-{index:04}"),
        ));
    }
    let dense_shape = ResolutionRowShape {
        identifiers: identifier_rows,
        pending: pending_rows,
        resolved_pending: resolved_pending_rows,
        distinct_target_names: MILLER_DISTINCT_IDENTIFIER_NAMES
            .saturating_sub(1)
            .min(identifier_rows.max(1)),
    };
    let sparse_shape = ResolutionRowShape {
        identifiers: 1,
        pending: 1,
        resolved_pending: 1,
        distinct_target_names: 1,
    };
    let stable_shape = match (stable_file_shape, changed_file_shape) {
        (Some(shape), _) => shape,
        (None, Some(_)) => sparse_shape,
        (None, None) => dense_shape,
    };
    insert_resolution_rows(
        &transaction,
        versions[0],
        "src/miller-scale.cs",
        "target",
        stable_shape,
    );
    let changed_start = MILLER_FILE_ROWS - changed_files;
    let mut changed_versions = Vec::with_capacity(changed_files);
    for (index, version) in versions.iter().copied().enumerate().skip(changed_start) {
        let path = format!("src/file-{index:04}.cs");
        insert_resolution_rows(
            &transaction,
            version,
            &path,
            &format!("target-base-{index:04}"),
            changed_file_shape.unwrap_or(sparse_shape),
        );
        let changed = insert_version(&transaction, &path, &format!("hash-changed-{index:04}"));
        insert_resolution_rows(
            &transaction,
            changed,
            &path,
            &format!("target-changed-{index:04}"),
            changed_file_shape.unwrap_or(sparse_shape),
        );
        changed_versions.push(changed);
    }
    transaction
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
         VALUES (?1,1,?2,'request-one',?4),
                (?1,2,?3,'request-two',?4)",
            params![VIEW_ID, "1".repeat(64), "2".repeat(64), NOW],
        )
        .unwrap();
    for (index, version) in versions.iter().copied().enumerate() {
        let path = if index == 0 {
            "src/miller-scale.cs".to_string()
        } else {
            format!("src/file-{index:04}.cs")
        };
        transaction.execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES (?1,1,?2,'csharp',?3,'indexed',?4,?5)",
            params![VIEW_ID, path, version, format!("hash-{index:04}"), NOW],
        ).unwrap();
        let (current_version, current_hash) = if index >= changed_start {
            (
                changed_versions[index - changed_start],
                format!("hash-changed-{index:04}"),
            )
        } else {
            (version, format!("hash-{index:04}"))
        };
        transaction
            .execute(
                "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
                 VALUES (?1,2,?2,'csharp',?3,'indexed',?4,?5)",
                params![VIEW_ID, path, current_version, current_hash, NOW],
            )
            .unwrap();
    }
    transaction
        .execute(
            "UPDATE views SET current_generation=2 WHERE view_id=?1",
            [VIEW_ID],
        )
        .unwrap();
    transaction.commit().unwrap();
}

fn ensure_ready_replay_base(layout: &StoreLayout, view_id: &str) {
    let identity = view_manifest_identity(layout, view_id, 1);
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let catalog = ResolutionBaseCatalog::new(factory.clone());
    if catalog
        .find_ready(&identity.manifest_hash, RESOLVER_OUTPUT_EPOCH)
        .unwrap()
        .is_some()
    {
        return;
    }
    let build = match catalog
        .begin_build(
            &identity.manifest_hash,
            RESOLVER_OUTPUT_EPOCH,
            "performance-base",
            NOW,
        )
        .unwrap()
    {
        ResolutionBaseBegin::Build(build) => build,
        other => panic!("expected a new performance base, got {other:?}"),
    };
    let mut session = StoreScratchResolutionSession::new(
        factory,
        identity,
        &build.scratch_path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();
    run_resolution_session(&mut session, true, true).unwrap();
    session.finish_exact().unwrap();
    catalog.publish_scratch(&build).unwrap();
    catalog.mark_ready(&build, NOW).unwrap();
}

fn build_target_validation_fixture(root: &Path, target_count: usize) -> StoreLayout {
    let layout =
        StoreLayout::create(root.join("family"), FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view(VIEW_ID, ROOT)
        .unwrap();
    let transaction = connection.transaction().unwrap();
    let path = "src/high-cardinality.cs";
    let version = insert_version(&transaction, path, "high-cardinality-hash");
    transaction
        .execute(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,'caller',?2,'csharp','caller','function',1,1,100000,1,0,1000000,0,0,0)",
            params![version, path],
        )
        .unwrap();
    let mut symbol_insert = transaction
        .prepare(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,?2,?3,'csharp',?4,'function',?5,1,?5,10,?6,?7,0,0,0)",
        )
        .unwrap();
    let mut site_insert = transaction
        .prepare(
            "INSERT INTO reference_sites(version_id,reference_site_id,path,language,containing_symbol_id,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_exact,provenance,level)
             VALUES (?1,?2,?3,'csharp','caller',?4,1,?4,7,?5,?6,1,'target_token',2)",
        )
        .unwrap();
    let mut identifier_insert = transaction
        .prepare(
            "INSERT INTO identifiers(version_id,identifier_id,reference_site_id,path,language,name,kind,
             containing_symbol_id,start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,?2,?3,?4,'csharp',?5,'call','caller',?6,1,?6,7,?7,?8,1.0)",
        )
        .unwrap();
    for index in 0..target_count {
        let target = format!("target-{index:08}");
        let site = format!("site-{index:08}");
        let identifier = format!("identifier-{index:08}");
        let line = i64::try_from(index).unwrap() + 2;
        let start = i64::try_from(index).unwrap() * 20 + 100;
        symbol_insert
            .execute(params![
                version,
                target,
                path,
                target,
                line,
                start,
                start + 10
            ])
            .unwrap();
        site_insert
            .execute(params![version, site, path, line, start + 11, start + 17])
            .unwrap();
        identifier_insert
            .execute(params![
                version,
                identifier,
                site,
                path,
                target,
                line,
                start + 11,
                start + 17
            ])
            .unwrap();
    }
    drop(identifier_insert);
    drop(site_insert);
    drop(symbol_insert);
    transaction
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES (?1,1,?2,'target-validation',?3)",
            params![VIEW_ID, "4".repeat(64), NOW],
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES (?1,1,?2,'csharp',?3,'indexed','high-cardinality-hash',?4)",
            params![VIEW_ID, path, version, NOW],
        )
        .unwrap();
    transaction
        .execute(
            "UPDATE views SET current_generation=1 WHERE view_id=?1",
            [VIEW_ID],
        )
        .unwrap();
    transaction.commit().unwrap();
    layout
}

fn build_repeated_name_candidate_fixture(
    root: &Path,
    identifier_count: usize,
    candidate_count: usize,
) -> StoreLayout {
    let layout =
        StoreLayout::create(root.join("family"), FAMILY_ID, env!("CARGO_PKG_VERSION")).unwrap();
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view(VIEW_ID, ROOT)
        .unwrap();
    let transaction = connection.transaction().unwrap();
    let path = "src/repeated-name.cs";
    let version = insert_version(&transaction, path, "repeated-name-hash");
    let mut symbol_insert = transaction
        .prepare(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,?2,?3,'csharp',?4,'function',?5,1,?6,10,?7,?8,0,0,0)",
        )
        .unwrap();
    for index in 0..candidate_count {
        let line = i64::try_from(index).unwrap() + 2;
        let start = i64::try_from(index).unwrap() * 20 + 100;
        symbol_insert
            .execute(params![
                version,
                format!("candidate-{index:08}"),
                path,
                "shared_target",
                line,
                line,
                start,
                start + 10
            ])
            .unwrap();
    }
    let mut site_insert = transaction
        .prepare(
            "INSERT INTO reference_sites(version_id,reference_site_id,path,language,containing_symbol_id,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_exact,provenance,level)
             VALUES (?1,?2,?3,'csharp',?4,?5,1,?5,7,?6,?7,1,'target_token',2)",
        )
        .unwrap();
    let mut identifier_insert = transaction
        .prepare(
            "INSERT INTO identifiers(version_id,identifier_id,reference_site_id,path,language,name,kind,
             containing_symbol_id,start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,?2,?3,?4,'csharp','shared_target','call',?5,?6,1,?6,7,?7,?8,?9)",
        )
        .unwrap();
    for index in 0..identifier_count {
        let caller = format!("caller-{index:08}");
        let line = i64::try_from(candidate_count + index).unwrap() + 2;
        let start = i64::try_from(candidate_count + index).unwrap() * 20 + 100;
        symbol_insert
            .execute(params![
                version,
                caller,
                path,
                format!("caller_{index:08}"),
                line,
                line,
                start,
                start + 10
            ])
            .unwrap();
        let site = format!("site-{index:08}");
        site_insert
            .execute(params![
                version,
                site,
                path,
                caller,
                line,
                start + 11,
                start + 17
            ])
            .unwrap();
        identifier_insert
            .execute(params![
                version,
                format!("identifier-{index:08}"),
                site,
                path,
                caller,
                line,
                start + 11,
                start + 17,
                1.0 - (index as f64 * 0.001)
            ])
            .unwrap();
    }
    drop(identifier_insert);
    drop(site_insert);
    drop(symbol_insert);
    transaction
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES (?1,1,?2,'repeated-name',?3)",
            params![VIEW_ID, "5".repeat(64), NOW],
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES (?1,1,?2,'csharp',?3,'indexed','repeated-name-hash',?4)",
            params![VIEW_ID, path, version, NOW],
        )
        .unwrap();
    transaction
        .execute(
            "UPDATE views SET current_generation=1 WHERE view_id=?1",
            [VIEW_ID],
        )
        .unwrap();
    transaction.commit().unwrap();
    layout
}

fn insert_version(transaction: &rusqlite::Transaction<'_>, path: &str, hash: &str) -> i64 {
    transaction.execute(
        "INSERT INTO file_versions(path,content_hash,extraction_epoch,language,content_bytes,complete_l1,complete_l2,complete_l3)
         VALUES (?1,?2,1,'csharp',1,1,1,1)",
        params![path, hash],
    ).unwrap();
    transaction.last_insert_rowid()
}

fn insert_resolution_rows(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
    path: &str,
    target_name: &str,
    shape: ResolutionRowShape,
) {
    assert!(shape.resolved_pending <= shape.pending);
    assert!(shape.distinct_target_names > 0);
    transaction.execute(
        "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
         VALUES (?1,'caller',?2,'csharp','caller','function',2,1,500000,1,11,5000000,0,0,0)",
        params![version, path],
    ).unwrap();
    let mut symbol_insert = transaction.prepare(
        "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
         VALUES (?1,?2,?3,'csharp',?4,'function',1,1,1,10,0,10,0,0,0)",
    ).unwrap();
    for index in 0..shape.distinct_target_names {
        let symbol_id = if shape.distinct_target_names == 1 {
            "target".to_string()
        } else {
            format!("target-{index:05}")
        };
        let name = if shape.distinct_target_names == 1 {
            target_name.to_string()
        } else {
            format!("{target_name}-{index:05}")
        };
        symbol_insert
            .execute(params![version, symbol_id, path, name])
            .unwrap();
    }
    drop(symbol_insert);
    let mut site_insert = transaction.prepare(
        "INSERT INTO reference_sites(version_id,reference_site_id,path,language,containing_symbol_id,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_exact,provenance,level)
         VALUES (?1,?2,?3,'csharp','caller',?4,1,?4,7,?5,?6,1,'target_token',2)",
    ).unwrap();
    let mut identifier_insert = transaction.prepare(
        "INSERT INTO identifiers(version_id,identifier_id,reference_site_id,path,language,name,kind,
         containing_symbol_id,start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
         VALUES (?1,?2,?3,?4,'csharp',?5,'call','caller',?6,1,?6,7,?7,?8,1.0)",
    ).unwrap();
    let mut pending_insert = transaction.prepare(
        "INSERT INTO pending_relationships(version_id,pending_relationship_id,reference_site_id,
         from_symbol_id,caller_scope_symbol_id,path,kind,target_display_name,target_terminal_name,
         target_namespace_json,start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
         VALUES (?1,?2,?3,'caller','caller',?4,'calls',?5,?5,'[]',?6,1,?6,7,?7,?8,1.0)",
    ).unwrap();
    for index in 0..shape.identifiers {
        let row_target_name = if index < shape.pending && index >= shape.resolved_pending {
            "ambiguous".to_string()
        } else if shape.distinct_target_names == 1 {
            target_name.to_string()
        } else {
            format!("{target_name}-{:05}", index % shape.distinct_target_names)
        };
        let site = format!("site-{index:08}");
        let identifier = format!("identifier-{index:08}");
        let line = i64::try_from(index).unwrap() + 3;
        let start = i64::try_from(index).unwrap() * 10 + 100;
        site_insert
            .execute(params![version, site, path, line, start, start + 6])
            .unwrap();
        identifier_insert
            .execute(params![
                version,
                identifier,
                site,
                path,
                &row_target_name,
                line,
                start,
                start + 6
            ])
            .unwrap();
        if index < shape.pending {
            pending_insert
                .execute(params![
                    version,
                    format!("pending-{index:08}"),
                    site,
                    path,
                    &row_target_name,
                    line,
                    start,
                    start + 6
                ])
                .unwrap();
        }
    }
}

fn scaled_pending_rows(identifier_rows: usize) -> usize {
    identifier_rows
        .saturating_mul(MILLER_PENDING_ROWS)
        .checked_div(MILLER_IDENTIFIER_ROWS)
        .unwrap()
        .max(1)
        .min(identifier_rows)
}

fn scaled_resolved_pending_rows(identifier_rows: usize) -> usize {
    identifier_rows
        .saturating_mul(MILLER_RESOLVED_PENDING_ROWS)
        .checked_div(MILLER_IDENTIFIER_ROWS)
        .unwrap()
        .max(1)
        .min(scaled_pending_rows(identifier_rows))
}

fn elapsed_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos().div_ceil(1_000_000)).unwrap()
}
