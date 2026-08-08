use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::store::{
    CoordinatorError, CoordinatorExecutor, CoordinatorPolicy, CoordinatorRequest, ExecutionContext,
    ExecutionQuantum, LeaseDisposition, LeaseHolder, PidLiveness, RequestKind, StoreCoordinator,
    StoreLayout, StoreLevel, StoreLog, StoreLogEntry, UnixMillisClock, compare_versions,
};
use rusqlite::{Connection, Transaction};

fn layout(root: &Path) -> StoreLayout {
    StoreLayout::create(root, "family-a", "2.30.0").unwrap()
}

fn append_terminal(layout: &StoreLayout, request_id: &str, result_json: &str) -> i64 {
    let mut connection = Connection::open(layout.store_db()).unwrap();
    let transaction = connection.transaction().unwrap();
    let sequence = StoreLog::append_terminal(
        &transaction,
        &StoreLogEntry::new(
            request_id,
            "coordinator_test_terminal",
            result_json,
            "1970-01-01T00:00:00Z",
        ),
    )
    .unwrap();
    transaction.commit().unwrap();
    sequence
}

struct TempDir(PathBuf);

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

impl TempDir {
    fn new() -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "julie-coordinator-{}-{suffix}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn enqueue_deduplicates_by_idempotency_key_and_acquires_lease() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let request = CoordinatorRequest::new(
        "request-1",
        "idem-1",
        RequestKind::Update,
        "{}",
        "requester-1",
        1_000,
        1,
    );

    let first = coordinator.enqueue(request.clone()).unwrap();
    let second = coordinator.enqueue(request).unwrap();

    assert!(first.inserted);
    assert!(!second.inserted);
    assert_eq!(first.request.request_id, second.request.request_id);

    let lease = coordinator
        .try_acquire_or_takeover(LeaseHolder::new("holder-a", "2.30.0", 41), 10)
        .unwrap();
    assert!(lease.acquired());
}

#[test]
fn terminal_log_reconciles_a_coord_tear_without_reexecution() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let request = CoordinatorRequest::new(
        "request-1",
        "idem-1",
        RequestKind::Update,
        "{}",
        "requester-1",
        1_000,
        1,
    );
    coordinator.enqueue(request).unwrap();
    append_terminal(&layout, "request-1", "{\"ok\":true}");

    let outcome = coordinator.reconcile("request-1").unwrap();

    assert!(outcome.committed_in_fact);
    assert_eq!(outcome.next_chunk_index, 0);
    assert_eq!(
        coordinator.request("request-1").unwrap().state.as_str(),
        "committed"
    );
}

#[test]
fn idempotency_key_reuse_with_different_input_is_a_typed_conflict() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            "request-1",
            "idem-1",
            RequestKind::Update,
            "{}",
            "requester-1",
            1_000,
            1,
        ))
        .unwrap();

    let error = coordinator
        .enqueue(CoordinatorRequest::new(
            "request-2",
            "idem-1",
            RequestKind::Delete,
            "{\"paths\":[]}",
            "requester-2",
            2_000,
            2,
        ))
        .unwrap_err();

    assert!(matches!(
        error,
        CoordinatorError::IdempotencyConflict {
            idempotency_key,
            existing_request_id
        } if idempotency_key == "idem-1" && existing_request_id == "request-1"
    ));
}

#[test]
fn requester_retry_with_a_new_request_id_reuses_the_original_request() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);

    let retry = coordinator
        .enqueue(CoordinatorRequest::new(
            "request-2",
            "idem-1",
            RequestKind::Update,
            "{}",
            "another-requester",
            2_000,
            2,
        ))
        .unwrap();

    assert!(!retry.inserted);
    assert_eq!(retry.request.request_id, "request-1");
}

#[test]
fn two_concurrent_submitters_create_one_request_and_one_effect() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let barrier = Arc::new(Barrier::new(2));
    let handles = (1..=2)
        .map(|index| {
            let layout = layout.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut coordinator = StoreCoordinator::open(&layout).unwrap();
                barrier.wait();
                coordinator.enqueue(CoordinatorRequest::new(
                    format!("request-{index}"),
                    "shared-idempotency-key",
                    RequestKind::Update,
                    "{}",
                    format!("requester-{index}"),
                    1_000,
                    index,
                ))
            })
        })
        .collect::<Vec<_>>();
    let outcomes = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.inserted).count(),
        1
    );
    assert_eq!(
        outcomes[0].request.request_id,
        outcomes[1].request.request_id
    );
    let connection = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM requests", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[derive(Debug)]
struct FixedLiveness(bool);

impl PidLiveness for FixedLiveness {
    fn is_alive(&self, _pid: u32) -> bool {
        self.0
    }
}

#[test]
fn live_holder_is_not_displaced_but_dead_or_expired_holder_is_fenced() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut live = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    let first = live
        .try_acquire_or_takeover(LeaseHolder::new("holder-a", "2.30.0", 41), 10)
        .unwrap();
    assert_eq!(first, LeaseDisposition::Acquired { fencing_token: 10 });
    assert_eq!(
        live.try_acquire_or_takeover(LeaseHolder::new("holder-b", "2.31.0", 42), 11)
            .unwrap(),
        LeaseDisposition::HeldByOther
    );
    assert_eq!(
        live.try_acquire_or_takeover(LeaseHolder::new("holder-b", "2.31.0", 42), 5_010)
            .unwrap(),
        LeaseDisposition::Acquired {
            fencing_token: 5_010
        }
    );

    live.release_lease("holder-b", 5_010).unwrap();
    live.try_acquire_or_takeover(LeaseHolder::new("holder-a", "2.30.0", 41), 6_000)
        .unwrap();
    let mut dead = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(false)).unwrap();
    assert_eq!(
        dead.try_acquire_or_takeover(LeaseHolder::new("holder-b", "2.31.0", 42), 6_001)
            .unwrap(),
        LeaseDisposition::Acquired {
            fencing_token: 6_001
        }
    );
}

#[test]
fn heartbeat_and_release_require_the_current_owner_and_fencing_token() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator =
        StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    let lease = coordinator
        .try_acquire_or_takeover(LeaseHolder::new("holder-a", "2.30.0", 41), 10)
        .unwrap();
    let LeaseDisposition::Acquired { fencing_token } = lease else {
        panic!("lease was not acquired");
    };

    assert!(
        !coordinator
            .heartbeat_lease("holder-b", fencing_token, 20)
            .unwrap()
    );
    assert!(
        !coordinator
            .heartbeat_lease("holder-a", fencing_token + 1, 20)
            .unwrap()
    );
    assert!(
        coordinator
            .heartbeat_lease("holder-a", fencing_token, 20)
            .unwrap()
    );
    assert!(
        !coordinator
            .release_lease("holder-a", fencing_token + 1)
            .unwrap()
    );
    assert!(
        coordinator
            .release_lease("holder-a", fencing_token)
            .unwrap()
    );
}

#[test]
fn writer_below_the_store_floor_is_typed_and_never_acquires() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let connection = Connection::open(layout.store_db()).unwrap();
    connection
        .execute(
            "UPDATE store_meta SET value = '2.31.0' WHERE key = 'min_writer_version'",
            [],
        )
        .unwrap();
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();

    let error = coordinator
        .try_acquire_or_takeover(LeaseHolder::new("holder-a", "2.30.0", 41), 10)
        .unwrap_err();

    assert!(matches!(
        error,
        CoordinatorError::WriterVersionTooOld { running, required }
            if running == "2.30.0" && required == "2.31.0"
    ));
}

#[test]
fn coordinator_version_order_matches_connection_contract_semantics() {
    assert_eq!(
        compare_versions("v2.30.0+build.1", "2.30").unwrap(),
        std::cmp::Ordering::Equal
    );
    assert_eq!(
        compare_versions("2.30.0-rc.1", "2.30.0").unwrap(),
        std::cmp::Ordering::Less
    );
    assert_eq!(
        compare_versions("2.30.0-rc.2", "2.30.0-rc.1").unwrap(),
        std::cmp::Ordering::Greater
    );
}

#[test]
fn progress_resumes_at_the_next_request_global_chunk_across_levels() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            "request-1",
            "idem-1",
            RequestKind::Import,
            "{}",
            "requester-1",
            1_000,
            1,
        ))
        .unwrap();
    let mut store = Connection::open(layout.store_db()).unwrap();
    let transaction = store.transaction().unwrap();
    StoreLog::append_progress(
        &transaction,
        &StoreLogEntry::new("request-1", "l1", "{}", "1970-01-01T00:00:00Z")
            .with_level(StoreLevel::L1),
        0,
    )
    .unwrap();
    StoreLog::append_progress(
        &transaction,
        &StoreLogEntry::new("request-1", "l2", "{}", "1970-01-01T00:00:01Z")
            .with_level(StoreLevel::L2),
        1,
    )
    .unwrap();
    transaction.commit().unwrap();

    let outcome = coordinator.reconcile("request-1").unwrap();

    assert!(!outcome.committed_in_fact);
    assert_eq!(outcome.next_chunk_index, 2);
    assert_eq!(
        coordinator.request("request-1").unwrap().state.as_str(),
        "queued"
    );
}

#[test]
fn coordinator_terminal_state_without_store_terminal_is_typed_corruption() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            "request-1",
            "idem-1",
            RequestKind::Update,
            "{}",
            "requester-1",
            1_000,
            1,
        ))
        .unwrap();
    let connection = Connection::open(layout.coordinator_db()).unwrap();
    connection
        .execute(
            "UPDATE requests SET state = 'committed', terminal_log_sequence = 99,
             result_json = '{}', updated_at = 2 WHERE request_id = 'request-1'",
            [],
        )
        .unwrap();

    let error = coordinator.reconcile("request-1").unwrap_err();

    assert!(matches!(
        error,
        CoordinatorError::CoordinatorAheadOfStore { request_id }
            if request_id == "request-1"
    ));
}

#[derive(Debug, Default)]
struct TestClock(AtomicI64);

impl TestClock {
    fn set(&self, now: i64) {
        self.0.store(now, Ordering::SeqCst);
    }

    fn advance(&self, delta: i64) {
        self.0.fetch_add(delta, Ordering::SeqCst);
    }
}

impl UnixMillisClock for TestClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

struct RecordingExecutor {
    order: Vec<String>,
    clock: Arc<TestClock>,
    advance_ms: i64,
}

impl CoordinatorExecutor for RecordingExecutor {
    fn execute_quantum(
        &mut self,
        _transaction: &Transaction<'_>,
        request: &CoordinatorRequest,
        _context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        self.order.push(request.request_id.clone());
        self.clock.advance(self.advance_ms);
        Ok(ExecutionQuantum::Complete {
            event_kind: "complete".to_string(),
            result_json: "{}".to_string(),
        })
    }
}

fn enqueue_request(
    coordinator: &mut StoreCoordinator,
    index: usize,
    kind: RequestKind,
    created_at: i64,
) {
    coordinator
        .enqueue(CoordinatorRequest::new(
            format!("request-{index}"),
            format!("idem-{index}"),
            kind,
            "{}",
            "requester",
            10_000,
            created_at,
        ))
        .unwrap();
}

#[test]
fn drain_caps_the_interactive_burst_at_32_before_a_batch_quantum() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    let holder = LeaseHolder::new("holder", "2.30.0", std::process::id());
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        holder,
        Arc::clone(&clock),
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    for index in 0..33 {
        enqueue_request(&mut coordinator, index, RequestKind::Update, index as i64);
    }
    enqueue_request(&mut coordinator, 100, RequestKind::Import, 100);
    let mut executor = RecordingExecutor {
        order: Vec::new(),
        clock: Arc::clone(&clock),
        advance_ms: 0,
    };

    let report = coordinator
        .drain(&mut executor, &CoordinatorPolicy::default())
        .unwrap();

    assert_eq!(executor.order[32], "request-100");
    assert_eq!(report.interactive_quanta, 33);
    assert_eq!(report.batch_quanta, 1);
    assert!(coordinator.lease().unwrap().is_none());
}

#[test]
fn drain_caps_the_interactive_burst_at_250_ms_and_releases_after_success() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    clock.set(10);
    let holder = LeaseHolder::new("holder", "2.30.0", std::process::id());
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        holder,
        Arc::clone(&clock),
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    for index in 0..4 {
        enqueue_request(&mut coordinator, index, RequestKind::Update, index as i64);
    }
    enqueue_request(&mut coordinator, 100, RequestKind::Import, 100);
    let mut executor = RecordingExecutor {
        order: Vec::new(),
        clock: Arc::clone(&clock),
        advance_ms: 100,
    };

    coordinator
        .drain(&mut executor, &CoordinatorPolicy::default())
        .unwrap();

    assert_eq!(executor.order[3], "request-100");
    assert!(coordinator.lease().unwrap().is_none());
}

#[test]
fn requester_deadline_only_controls_acknowledgment_and_never_deletes_the_request() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);
    append_terminal(&layout, "request-1", "{}");
    coordinator.reconcile("request-1").unwrap();

    assert!(!coordinator.acknowledge("request-1", 10_001).unwrap());
    assert_eq!(
        coordinator.request("request-1").unwrap().state.as_str(),
        "committed"
    );
    assert!(coordinator.acknowledge("request-1", 10_000).unwrap());
    assert_eq!(
        coordinator.request("request-1").unwrap().state.as_str(),
        "acknowledged"
    );
}

struct FailingExecutor;

impl CoordinatorExecutor for FailingExecutor {
    fn execute_quantum(
        &mut self,
        _transaction: &Transaction<'_>,
        _request: &CoordinatorRequest,
        _context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        Err("boom".to_string())
    }
}

struct PanickingExecutor;

impl CoordinatorExecutor for PanickingExecutor {
    fn execute_quantum(
        &mut self,
        _transaction: &Transaction<'_>,
        _request: &CoordinatorRequest,
        _context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        panic!("boom")
    }
}

#[test]
fn drain_releases_the_lease_on_executor_error_and_panic() {
    for panic in [false, true] {
        let temp = TempDir::new();
        let layout = layout(temp.path());
        let clock = Arc::new(TestClock::default());
        let holder = LeaseHolder::new("holder", "2.30.0", std::process::id());
        let mut coordinator = StoreCoordinator::open_with_runtime(
            &layout,
            holder,
            clock,
            Arc::new(FixedLiveness(true)),
        )
        .unwrap();
        enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);

        if panic {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                coordinator.drain(&mut PanickingExecutor, &CoordinatorPolicy::default())
            }));
            assert!(outcome.is_err());
        } else {
            let error = coordinator
                .drain(&mut FailingExecutor, &CoordinatorPolicy::default())
                .unwrap_err();
            assert!(matches!(error, CoordinatorError::ExecutionFailed { .. }));
        }
        assert!(coordinator.lease().unwrap().is_none());
    }
}

struct ProducingExecutor {
    layout: StoreLayout,
    clock: Arc<TestClock>,
    order: Vec<String>,
    next_request: usize,
    advance_ms: i64,
}

impl CoordinatorExecutor for ProducingExecutor {
    fn execute_quantum(
        &mut self,
        _transaction: &Transaction<'_>,
        request: &CoordinatorRequest,
        _context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        self.order.push(request.request_id.clone());
        self.clock.advance(self.advance_ms);
        if request.kind != RequestKind::Import {
            let mut producer = StoreCoordinator::open(&self.layout).unwrap();
            enqueue_request(
                &mut producer,
                self.next_request,
                RequestKind::Update,
                self.clock.now_ms(),
            );
            self.next_request += 1;
        }
        Ok(ExecutionQuantum::Complete {
            event_kind: "complete".to_string(),
            result_json: "{}".to_string(),
        })
    }
}

#[test]
fn batch_progresses_under_a_sustained_interactive_producer() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    let holder = LeaseHolder::new("holder", "2.30.0", std::process::id());
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        holder,
        Arc::clone(&clock),
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    for index in 0..33 {
        enqueue_request(&mut coordinator, index, RequestKind::Update, index as i64);
    }
    enqueue_request(&mut coordinator, 100, RequestKind::Import, 100);
    let mut executor = ProducingExecutor {
        layout,
        clock,
        order: Vec::new(),
        next_request: 1_000,
        advance_ms: 1,
    };

    let policy = CoordinatorPolicy {
        interactive_burst_ms: 10_000,
        service_window_ms: 40,
        ..CoordinatorPolicy::default()
    };

    coordinator.drain(&mut executor, &policy).unwrap();

    assert_eq!(executor.order[32], "request-100");
}

#[test]
fn initial_backlog_drains_beyond_the_service_window_but_later_arrivals_do_not() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    let holder = LeaseHolder::new("holder", "2.30.0", std::process::id());
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        holder,
        Arc::clone(&clock),
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);
    enqueue_request(&mut coordinator, 2, RequestKind::Update, 2);
    let mut executor = ProducingExecutor {
        layout,
        clock,
        order: Vec::new(),
        next_request: 1_000,
        advance_ms: 600,
    };

    coordinator
        .drain(&mut executor, &CoordinatorPolicy::default())
        .unwrap();

    assert_eq!(executor.order, ["request-1", "request-2"]);
    assert_eq!(
        coordinator.request("request-1000").unwrap().state.as_str(),
        "queued"
    );
}

struct ChunkExecutor {
    contexts: Vec<ExecutionContext>,
}

impl CoordinatorExecutor for ChunkExecutor {
    fn execute_quantum(
        &mut self,
        _transaction: &Transaction<'_>,
        _request: &CoordinatorRequest,
        context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        self.contexts.push(context);
        match context.next_chunk_index {
            0 => Ok(ExecutionQuantum::Progress {
                event_kind: "l1".to_string(),
                payload_json: "{}".to_string(),
                level: Some(StoreLevel::L1),
            }),
            1 => Ok(ExecutionQuantum::Progress {
                event_kind: "l2".to_string(),
                payload_json: "{}".to_string(),
                level: Some(StoreLevel::L2),
            }),
            2 => Ok(ExecutionQuantum::Complete {
                event_kind: "complete".to_string(),
                result_json: "{}".to_string(),
            }),
            other => panic!("unexpected chunk index {other}"),
        }
    }
}

#[test]
fn drain_resumes_global_chunk_indices_across_level_waves_before_one_terminal() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        LeaseHolder::new("holder", "2.30.0", std::process::id()),
        Arc::clone(&clock),
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Import, 1);
    let mut executor = ChunkExecutor {
        contexts: Vec::new(),
    };

    coordinator
        .drain(&mut executor, &CoordinatorPolicy::default())
        .unwrap();

    assert_eq!(
        executor
            .contexts
            .iter()
            .map(|context| context.next_chunk_index)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    let store = Connection::open(layout.store_db()).unwrap();
    assert_eq!(
        store
            .query_row(
                "SELECT COUNT(*) FROM store_log WHERE request_id = 'request-1' AND terminal = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn terminal_store_effect_overrides_failed_coord_state_and_uses_clock_for_updated_at() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    clock.set(1_720_000_000_123);
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        LeaseHolder::new("holder", "2.30.0", std::process::id()),
        Arc::clone(&clock),
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    coord
        .execute(
            "UPDATE requests SET state = 'failed', error_json = '{}', updated_at = 2
             WHERE request_id = 'request-1'",
            [],
        )
        .unwrap();
    append_terminal(&layout, "request-1", "{}");

    coordinator.reconcile("request-1").unwrap();

    let request = coordinator.request("request-1").unwrap();
    assert_eq!(request.state.as_str(), "committed");
    assert_eq!(request.updated_at, 1_720_000_000_123);
}

#[test]
fn store_log_timestamp_maps_injected_unix_milliseconds_to_real_utc() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    clock.set(1_720_000_000_123);
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        LeaseHolder::new("holder", "2.30.0", std::process::id()),
        Arc::clone(&clock),
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);
    coordinator
        .drain(
            &mut RecordingExecutor {
                order: Vec::new(),
                clock,
                advance_ms: 0,
            },
            &CoordinatorPolicy::default(),
        )
        .unwrap();

    let store = Connection::open(layout.store_db()).unwrap();
    let created_at = store
        .query_row(
            "SELECT created_at FROM store_log WHERE request_id = 'request-1' AND terminal = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    assert_eq!(created_at, "2024-07-03T09:46:40.123Z");
}

struct SlowExecutor {
    clock: Arc<TestClock>,
}

impl CoordinatorExecutor for SlowExecutor {
    fn execute_quantum(
        &mut self,
        _transaction: &Transaction<'_>,
        _request: &CoordinatorRequest,
        _context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        self.clock.advance(4_001);
        Ok(ExecutionQuantum::Complete {
            event_kind: "complete".to_string(),
            result_json: "{}".to_string(),
        })
    }
}

#[test]
fn quantum_must_finish_inside_the_structural_lease_bound_before_store_commit() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        LeaseHolder::new("holder", "2.30.0", std::process::id()),
        Arc::clone(&clock),
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);

    let error = coordinator
        .drain(&mut SlowExecutor { clock }, &CoordinatorPolicy::default())
        .unwrap_err();

    assert!(matches!(
        error,
        CoordinatorError::QuantumDeadlineExceeded {
            elapsed_ms: 4_001,
            maximum_ms: 4_000
        }
    ));
    let store = Connection::open(layout.store_db()).unwrap();
    assert_eq!(
        store
            .query_row(
                "SELECT COUNT(*) FROM store_log WHERE request_id = 'request-1'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
}
