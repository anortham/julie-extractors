#![cfg(feature = "test-store-resolution-contract")]

use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use julie_extract_artifact::store::{
    ManifestStore, ResolutionBaseBegin, ResolutionBaseCatalog, ResolutionBaseReader,
    ResolutionBaseWriter, ResolutionBindingStore, ResolutionDiffMarker, ResolutionExactPublish,
    ResolutionFileIdentity, ResolutionIdentifierRow, ResolutionPendingRow,
    ResolutionPublicationFence, ResolutionPublicationMarker, ResolutionScratchReader,
    StoreConnectionFactory, StoreLayout, apply_base_delta, stream_resolution_diff_with_markers,
};
use julie_extract_cli::resolution::run_resolution_session;
use julie_extract_cli::store::resolution_session::{
    StoreManifestIdentity, StoreScratchResolutionSession,
};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

const FAMILY_ID: &str = "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11";
const VIEW_ID: &str = "view-miller-scale";
const ROOT: &str = "/synthetic/miller";
const RESOLVER_OUTPUT_EPOCH: i64 = 6;
const WINDOW_SIZE: usize = 300;
const MILLER_FILE_ROWS: usize = 1_538;
const MILLER_IDENTIFIER_ROWS: usize = 392_134;
const MILLER_PENDING_ROWS: usize = 89_538;
const MILLER_RESOLVED_PENDING_ROWS: usize = 10_412;
const PAIRS: [&str; 2] = ["miller-unchanged", "miller-mutated"];
const NOW: &str = "2026-08-08T12:00:00.000Z";

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
    semantic_differences: u64,
    applied_differences: u64,
    exact_gap_mismatches: u64,
    foreground_bind_ms: u64,
    foreground_identifier_work: u64,
    background_pipeline_ms: u64,
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

    fn mark_publication(&mut self, marker: ResolutionPublicationMarker) {
        match marker {
            ResolutionPublicationMarker::StoreTransactionStart => {
                self.mark(ResolutionDiffMarker::DeltaWriteStart);
            }
            ResolutionPublicationMarker::StoreTransactionEnd => {
                self.mark(ResolutionDiffMarker::DeltaWriteEnd);
            }
        }
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
    let fixture_root = out_dir.join(format!("fixture-{}", std::process::id()));
    reset_owned_directory(&fixture_root);
    let rows = std::env::var("JULIE_STORE_RESOLUTION_PERF_ROWS")
        .ok()
        .map(|value| value.parse().unwrap())
        .unwrap_or(MILLER_IDENTIFIER_ROWS);
    build_store_fixture(
        &fixture_root,
        rows,
        scaled_pending_rows(rows),
        scaled_resolved_pending_rows(rows),
    );

    for run in 1..=runs {
        let run_dir = out_dir.join(format!("run-{run:03}"));
        reset_owned_directory(&run_dir);
        for pair in PAIRS {
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
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let mut sample: Sample =
                serde_json::from_slice(&fs::read(&worker_output).unwrap()).unwrap();
            sample.peak_rss_bytes = parse_peak_rss(&output.stderr);
            fs::write(
                run_dir.join(format!("{pair}.json")),
                serde_json::to_vec_pretty(&sample).unwrap(),
            )
            .unwrap();
            fs::remove_file(worker_output).unwrap();
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

fn measure_pair(store_root: &Path, pair: &str, run: usize, out_dir: &Path) -> Sample {
    assert!(PAIRS.contains(&pair));
    let layout = StoreLayout::open(store_root).unwrap();
    let base_generation = 1;
    let exact_generation = if pair == "miller-unchanged" { 3 } else { 2 };
    let base_identity = manifest_identity(&layout, base_generation);
    let exact_identity = manifest_identity(&layout, exact_generation);
    let sample_root = out_dir.join(format!("worker-{pair}-{run}"));
    fs::create_dir_all(&sample_root).unwrap();

    let foreground_started = Instant::now();
    let _ = manifest_identity(&layout, exact_generation);
    let foreground_bind_ms = elapsed_ms(foreground_started.elapsed());

    let (base_path, _, _, _) = build_exact(&layout, base_identity, sample_root.join("base.db"));
    let (exact_path, exact_file, resolution_compute_ms, store_fresh_ms) = build_exact(
        &layout,
        exact_identity.clone(),
        sample_root.join("exact.db"),
    );
    let (repeat_path, _, _, _) =
        build_exact(&layout, exact_identity, sample_root.join("repeat.db"));
    let semantic_differences = base_differences(&exact_path, &repeat_path);

    let base = ResolutionBaseReader::open(&base_path).unwrap();
    let exact = ResolutionBaseReader::open(&exact_path).unwrap();
    let delta_path = sample_root.join("delta.db");
    let mut gaps = Vec::new();
    let mut scratch_timeline = WriteTimeline::default();
    let diff_started = Instant::now();
    let diff = stream_resolution_diff_with_markers(
        &base,
        &exact,
        &delta_path,
        WINDOW_SIZE,
        |gap| {
            gaps.push(gap);
            Ok(())
        },
        |marker| scratch_timeline.mark(marker),
    )
    .unwrap();
    let diff_total = diff_started.elapsed();
    scratch_timeline.finish(diff_total);
    let delta = ResolutionScratchReader::open(&delta_path).unwrap();
    let write_duration =
        publish_real_store_delta(&layout, pair, run, exact_generation, &delta, &gaps);
    let applied_path = sample_root.join("applied.db");
    apply_delta(&base, &delta, &exact, &applied_path);
    let applied_differences = base_differences(&applied_path, &exact_path);
    let exact_gap_mismatches = u64::from(diff.gaps != gaps.len() as u64);
    let diff_ms = elapsed_ms(diff_total);
    let delta_write_ms = elapsed_ms(write_duration);
    let integrity_ms = store_fresh_ms.saturating_sub(resolution_compute_ms);
    let time_to_exact_ms = store_fresh_ms + diff_ms + delta_write_ms;

    Sample {
        pair: pair.to_string(),
        run,
        resolution_compute_ms,
        store_fresh_ms,
        diff_ms,
        delta_write_ms,
        publish_ms: 0,
        time_to_exact_ms,
        integrity_ms,
        identifier_rows: exact_file.counts.identifiers,
        pending_rows: exact_file.counts.pending,
        peak_rss_bytes: 0,
        base_bytes: fs::metadata(base_path).unwrap().len(),
        delta_bytes: fs::metadata(delta_path).unwrap().len(),
        semantic_differences,
        applied_differences,
        exact_gap_mismatches,
        foreground_bind_ms,
        foreground_identifier_work: 0,
        background_pipeline_ms: time_to_exact_ms,
    }
}

fn publish_real_store_delta(
    layout: &StoreLayout,
    pair: &str,
    run: usize,
    source_generation: i64,
    scratch: &ResolutionScratchReader,
    gaps: &[julie_extract_artifact::store::ResolutionGapFact],
) -> Duration {
    let view_id = format!("perf-{pair}-{run}");
    let request_id = format!("perf-publish-{pair}-{run}");
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let manifest_hash = prepare_publication_view(layout, &view_id, source_generation, &request_id);
    let bindings = ResolutionBindingStore::new(factory);
    let bound = bindings
        .bind_base(
            &view_id,
            RESOLVER_OUTPUT_EPOCH,
            &format!("perf-bind-{pair}-{run}"),
            NOW,
        )
        .unwrap();
    assert_eq!(bound.state.as_str(), "converging");
    let fence = publication_fence(layout, &request_id);
    let publication = ResolutionExactPublish {
        view_id,
        manifest_generation: 1,
        manifest_hash,
        base_id: bound.base_id,
        previous_delta_generation: bound.delta_generation,
        resolver_output_epoch: RESOLVER_OUTPUT_EPOCH,
        request_id,
        created_at: NOW.to_string(),
    };
    let mut timeline = WriteTimeline::default();
    let total_started = Instant::now();
    let published = bindings
        .publish_exact_with_markers(
            &publication,
            &fence,
            scratch,
            gaps,
            WINDOW_SIZE,
            || Ok(()),
            |marker| {
                timeline.mark_publication(marker);
            },
        )
        .unwrap();
    assert_eq!(published.state.as_str(), "exact");
    let elapsed = timeline.finish(total_started.elapsed());
    complete_publication_request(layout, &publication.request_id);
    elapsed
}

fn complete_publication_request(layout: &StoreLayout, request_id: &str) {
    let terminal_sequence = Connection::open(layout.store_db())
        .unwrap()
        .query_row(
            "SELECT sequence FROM store_log
             WHERE request_id=?1 AND event_kind='resolution_exact_published'",
            [request_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "UPDATE requests
             SET state='committed',claim_owner=NULL,claim_heartbeat_at=NULL,
                 terminal_log_sequence=?1,result_json='{}',updated_at=1001
             WHERE request_id=?2 AND state='claimed'",
            params![terminal_sequence, request_id],
        )
        .unwrap();
}

fn prepare_publication_view(
    layout: &StoreLayout,
    view_id: &str,
    source_generation: i64,
    request_id: &str,
) -> String {
    let mut connection = Connection::open(layout.store_db()).unwrap();
    let transaction = connection.transaction().unwrap();
    let manifest_hash = transaction
        .query_row(
            "SELECT manifest_hash FROM manifests WHERE view_id=?1 AND generation=?2",
            params![VIEW_ID, source_generation],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO views(view_id,root,created_at,updated_at) VALUES (?1,?2,?3,?3)",
            params![view_id, ROOT, NOW],
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES (?1,1,?2,?3,?4)",
            params![view_id, manifest_hash, request_id, NOW],
        )
        .unwrap();
    transaction
        .execute(
            "INSERT INTO manifest_entries
             (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at,
              error_class,error_json)
             SELECT ?1,1,path,language,version_id,status,observed_content_hash,indexed_at,
                    error_class,error_json
             FROM manifest_entries WHERE view_id=?2 AND generation=?3",
            params![view_id, VIEW_ID, source_generation],
        )
        .unwrap();
    transaction
        .execute(
            "UPDATE views SET current_generation=1 WHERE view_id=?1",
            [view_id],
        )
        .unwrap();
    transaction.commit().unwrap();
    manifest_hash
}

fn publication_fence(layout: &StoreLayout, request_id: &str) -> ResolutionPublicationFence {
    let connection = Connection::open(layout.coordinator_db()).unwrap();
    connection
        .execute(
            "INSERT INTO requests
             (request_id,idempotency_key,kind,payload_json,state,requester_id,
              requester_deadline,claim_owner,claim_heartbeat_at,terminal_log_sequence,
              result_json,error_json,created_at,updated_at)
             VALUES (?1,?2,'resolve','{}','claimed','performance',NULL,'performance-holder',1000,
                     NULL,NULL,NULL,1000,1000)",
            params![request_id, format!("key-{request_id}")],
        )
        .unwrap();
    connection
        .execute(
            "INSERT OR REPLACE INTO writer_lease
             (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,fencing_token)
             VALUES ('store-writer','performance-holder',?1,42,1000,2000,7)",
            [env!("CARGO_PKG_VERSION")],
        )
        .unwrap();
    ResolutionPublicationFence {
        claim_owner: "performance-holder".to_string(),
        holder_id: "performance-holder".to_string(),
        holder_pid: 42,
        fencing_token: 7,
        now_ms: 1000,
    }
}

fn manifest_identity(layout: &StoreLayout, generation: i64) -> StoreManifestIdentity {
    let connection = Connection::open(layout.store_db()).unwrap();
    let manifest_hash = connection
        .query_row(
            "SELECT manifest_hash FROM manifests WHERE view_id=?1 AND generation=?2",
            params![VIEW_ID, generation],
            |row| row.get(0),
        )
        .unwrap();
    StoreManifestIdentity {
        family_id: FAMILY_ID.to_string(),
        view_id: VIEW_ID.to_string(),
        generation,
        manifest_hash,
    }
}

fn build_exact(
    layout: &StoreLayout,
    identity: StoreManifestIdentity,
    path: PathBuf,
) -> (PathBuf, ResolutionFileIdentity, u64, u64) {
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let mut session = StoreScratchResolutionSession::new(
        factory,
        identity,
        &path,
        WINDOW_SIZE,
        RESOLVER_OUTPUT_EPOCH,
    )
    .unwrap();
    let fresh_started = Instant::now();
    let compute_started = Instant::now();
    run_resolution_session(&mut session, true, true).unwrap();
    let resolution_compute_ms = elapsed_ms(compute_started.elapsed());
    let file = session.finish_exact().unwrap();
    let store_fresh_ms = elapsed_ms(fresh_started.elapsed());
    (path, file, resolution_compute_ms, store_fresh_ms)
}

fn apply_delta(
    base: &ResolutionBaseReader,
    delta: &ResolutionScratchReader,
    exact: &ResolutionBaseReader,
    path: &Path,
) {
    let versions = exact.source_versions().unwrap();
    let visible = versions.iter().copied().collect::<BTreeSet<_>>();
    let mut writer = ResolutionBaseWriter::new(
        path,
        exact.file_identity().manifest_hash.clone(),
        exact.file_identity().resolver_output_epoch,
    )
    .unwrap();
    for version in versions {
        writer.push_source_version(version).unwrap();
    }
    let writer = RefCell::new(writer);
    apply_base_delta(
        base,
        delta,
        WINDOW_SIZE,
        |version| Ok(visible.contains(&version)),
        |row| writer.borrow_mut().push_identifier_resolution(row),
        |row| writer.borrow_mut().push_pending_resolution(row),
    )
    .unwrap();
    writer
        .into_inner()
        .finish_with_target_lookup(|_, _| Ok(true))
        .unwrap();
}

fn base_differences(left: &Path, right: &Path) -> u64 {
    let left = ResolutionBaseReader::open(left).unwrap();
    let right = ResolutionBaseReader::open(right).unwrap();
    let mut differences =
        u64::from(left.source_versions().unwrap() != right.source_versions().unwrap());
    differences += identifier_differences(&left, &right);
    differences += pending_differences(&left, &right);
    differences
}

fn identifier_differences(left: &ResolutionBaseReader, right: &ResolutionBaseReader) -> u64 {
    let mut left = IdentifierCursor::new(left);
    let mut right = IdentifierCursor::new(right);
    merge_differences(
        || left.next(),
        || right.next(),
        |row| (row.version_id, row.identifier_id.clone()),
    )
}

fn pending_differences(left: &ResolutionBaseReader, right: &ResolutionBaseReader) -> u64 {
    let mut left = PendingCursor::new(left);
    let mut right = PendingCursor::new(right);
    merge_differences(
        || left.next(),
        || right.next(),
        |row| (row.version_id, row.pending_relationship_id.clone()),
    )
}

fn merge_differences<T, N1, N2, K>(mut left: N1, mut right: N2, key: K) -> u64
where
    T: PartialEq,
    N1: FnMut() -> Option<T>,
    N2: FnMut() -> Option<T>,
    K: Fn(&T) -> (i64, String),
{
    let mut differences = 0;
    let mut left_row = left();
    let mut right_row = right();
    while left_row.is_some() || right_row.is_some() {
        match (&left_row, &right_row) {
            (Some(left_value), Some(right_value)) => match key(left_value).cmp(&key(right_value)) {
                std::cmp::Ordering::Less => {
                    differences += 1;
                    left_row = left();
                }
                std::cmp::Ordering::Greater => {
                    differences += 1;
                    right_row = right();
                }
                std::cmp::Ordering::Equal => {
                    differences += u64::from(left_value != right_value);
                    left_row = left();
                    right_row = right();
                }
            },
            (Some(_), None) => {
                differences += 1;
                left_row = left();
            }
            (None, Some(_)) => {
                differences += 1;
                right_row = right();
            }
            (None, None) => break,
        }
    }
    differences
}

struct IdentifierCursor<'a> {
    reader: &'a ResolutionBaseReader,
    rows: VecDeque<ResolutionIdentifierRow>,
    after: Option<(i64, String)>,
}

impl<'a> IdentifierCursor<'a> {
    fn new(reader: &'a ResolutionBaseReader) -> Self {
        Self {
            reader,
            rows: VecDeque::new(),
            after: None,
        }
    }

    fn next(&mut self) -> Option<ResolutionIdentifierRow> {
        if self.rows.is_empty() {
            self.rows = self
                .reader
                .identifier_window(
                    self.after
                        .as_ref()
                        .map(|(version, id)| (*version, id.as_str())),
                    1_024,
                )
                .unwrap()
                .into();
        }
        let row = self.rows.pop_front();
        if let Some(row) = &row {
            self.after = Some((row.version_id, row.identifier_id.clone()));
        }
        row
    }
}

struct PendingCursor<'a> {
    reader: &'a ResolutionBaseReader,
    rows: VecDeque<ResolutionPendingRow>,
    after: Option<(i64, String)>,
}

impl<'a> PendingCursor<'a> {
    fn new(reader: &'a ResolutionBaseReader) -> Self {
        Self {
            reader,
            rows: VecDeque::new(),
            after: None,
        }
    }

    fn next(&mut self) -> Option<ResolutionPendingRow> {
        if self.rows.is_empty() {
            self.rows = self
                .reader
                .pending_window(
                    self.after
                        .as_ref()
                        .map(|(version, id)| (*version, id.as_str())),
                    1_024,
                )
                .unwrap()
                .into();
        }
        let row = self.rows.pop_front();
        if let Some(row) = &row {
            self.after = Some((row.version_id, row.pending_relationship_id.clone()));
        }
        row
    }
}

fn build_store_fixture(
    store_root: &Path,
    identifier_rows: usize,
    pending_rows: usize,
    resolved_pending_rows: usize,
) {
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
    let reuse_new = insert_version(&transaction, "src/file-1535.cs", "hash-reused");
    insert_resolution_rows(
        &transaction,
        versions[0],
        "src/miller-scale.cs",
        "target",
        identifier_rows,
        pending_rows,
        resolved_pending_rows,
    );
    for &index in &[1533usize, 1534, 1535, 1536, 1537] {
        let target_name = if matches!(index, 1533 | 1534) {
            "ambiguous".to_string()
        } else {
            format!("target-{index:04}")
        };
        insert_resolution_rows(
            &transaction,
            versions[index],
            &format!("src/file-{index:04}.cs"),
            &target_name,
            4,
            4,
            usize::from(!matches!(index, 1533 | 1534)) * 4,
        );
    }
    insert_resolution_rows(
        &transaction,
        reuse_new,
        "src/file-1535.cs",
        "target-reused",
        4,
        4,
        4,
    );
    transaction
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
         VALUES (?1,1,?2,'request-one',?5),
                (?1,2,?3,'request-two',?5),
                (?1,3,?4,'request-three',?5)",
            params![VIEW_ID, "1".repeat(64), "2".repeat(64), "3".repeat(64), NOW],
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
        transaction.execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES (?1,3,?2,'csharp',?3,'indexed',?4,?5)",
            params![VIEW_ID, path, version, format!("hash-{index:04}"), NOW],
        ).unwrap();
        if matches!(index, 1533 | 1534) {
            continue;
        }
        if index == 1535 {
            transaction.execute(
                "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
                 VALUES (?1,2,?2,'csharp',?3,'indexed','hash-reused',?4)",
                params![VIEW_ID, path, reuse_new, NOW],
            ).unwrap();
        } else if index == 1536 {
            transaction.execute(
                "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at,error_class,error_json)
                 VALUES (?1,2,?2,'csharp',NULL,'failed',?3,?4,'parse','{}')",
                params![VIEW_ID, path, format!("hash-{index:04}"), NOW],
            ).unwrap();
        } else {
            let status = if index == 1537 {
                "failed_preserved"
            } else {
                "indexed"
            };
            if status == "failed_preserved" {
                transaction.execute(
                    "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at,error_class,error_json)
                     VALUES (?1,2,?2,'csharp',?3,?4,?5,?6,'parse','{}')",
                    params![VIEW_ID, path, version, status, format!("hash-{index:04}"), NOW],
                ).unwrap();
            } else {
                transaction.execute(
                    "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
                     VALUES (?1,2,?2,'csharp',?3,?4,?5,?6)",
                    params![VIEW_ID, path, version, status, format!("hash-{index:04}"), NOW],
                ).unwrap();
            }
        }
    }
    transaction
        .execute(
            "UPDATE views SET current_generation=2 WHERE view_id=?1",
            [VIEW_ID],
        )
        .unwrap();
    transaction.commit().unwrap();
    ensure_ready_performance_base(&layout);
}

fn ensure_ready_performance_base(layout: &StoreLayout) {
    let identity = manifest_identity(layout, 1);
    let factory = StoreConnectionFactory::new(layout.clone(), FAMILY_ID, env!("CARGO_PKG_VERSION"));
    let catalog = ResolutionBaseCatalog::new(factory.clone());
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
    identifier_rows: usize,
    pending_rows: usize,
    resolved_pending_rows: usize,
) {
    assert!(resolved_pending_rows <= pending_rows);
    transaction.execute(
        "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
         VALUES (?1,'target',?2,'csharp',?3,'function',1,1,1,10,0,10,0,0,0),
                (?1,'caller',?2,'csharp','caller','function',2,1,500000,1,11,5000000,0,0,0)",
        params![version, path, target_name],
    ).unwrap();
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
    for index in 0..identifier_rows {
        let row_target_name = if index < pending_rows && index >= resolved_pending_rows {
            "ambiguous"
        } else {
            target_name
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
                row_target_name,
                line,
                start,
                start + 6
            ])
            .unwrap();
        if index < pending_rows {
            pending_insert
                .execute(params![
                    version,
                    format!("pending-{index:08}"),
                    site,
                    path,
                    row_target_name,
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
