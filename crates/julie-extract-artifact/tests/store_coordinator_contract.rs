use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::store::{
    ConsumerCursor, CoordinatorError, CoordinatorExecutor, CoordinatorPolicy, CoordinatorRequest,
    ExecutionContext, ExecutionQuantum, LeaseDisposition, LeaseHolder, PidLiveness, PidStatus,
    RequestKind, RequestReceipt, StoreCoordinator, StoreLayout, StoreLevel, StoreLog,
    StoreLogEntry, UnixMillisClock, compare_versions,
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
fn coordinator_store_transactions_obey_the_generation_write_fence() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    clock.set(10);
    let holder = LeaseHolder::new("holder-a", "2.30.0", std::process::id());
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        holder,
        clock.clone(),
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);
    Connection::open(layout.store_db())
        .unwrap()
        .execute(
            "UPDATE store_meta SET value = 'retired' WHERE key = 'generation_state'",
            [],
        )
        .unwrap();
    let mut executor = RecordingExecutor {
        order: Vec::new(),
        clock,
        advance_ms: 0,
    };

    let error = coordinator
        .drain(&mut executor, &CoordinatorPolicy::default())
        .unwrap_err();

    assert!(matches!(
        error,
        CoordinatorError::StoreConnection(
            julie_extract_artifact::store::StoreConnectionError::GenerationNotServing { state }
        ) if state == "retired"
    ));
    assert!(executor.order.is_empty());
    assert_eq!(
        coordinator.request("request-1").unwrap().state.as_str(),
        "queued"
    );
    assert_eq!(
        Connection::open(layout.store_db())
            .unwrap()
            .query_row("SELECT COUNT(*) FROM store_log", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn schema_v2_request_kinds_roundtrip_and_only_one_resolve_may_be_claimed() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    for (index, kind, expected) in [
        (1, RequestKind::Resolve, "resolve"),
        (2, RequestKind::Export, "export"),
        (3, RequestKind::FromArtifact, "from_artifact"),
        (4, RequestKind::Resolve, "resolve"),
    ] {
        let request_id = format!("request-{index}");
        coordinator
            .enqueue(CoordinatorRequest::new(
                &request_id,
                format!("idem-{index}"),
                kind,
                "{}",
                "requester",
                1_000,
                index,
            ))
            .unwrap();
        assert_eq!(
            coordinator.request(&request_id).unwrap().kind.as_str(),
            expected
        );
    }

    let connection = Connection::open(layout.coordinator_db()).unwrap();
    connection
        .execute(
            "UPDATE requests SET state='claimed', claim_owner='owner-a', claim_heartbeat_at=10
             WHERE request_id='request-1'",
            [],
        )
        .unwrap();
    assert!(
        connection
            .execute(
                "UPDATE requests SET state='claimed', claim_owner='owner-b', claim_heartbeat_at=10
                 WHERE request_id='request-4'",
                [],
            )
            .is_err()
    );
    connection
        .execute(
            "UPDATE requests SET state='claimed', claim_owner='owner-b', claim_heartbeat_at=10
             WHERE request_id='request-2'",
            [],
        )
        .unwrap();
}

#[test]
fn resolve_claim_heartbeats_and_stale_takeover_are_fenced_without_a_writer_lease() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            "resolve-1",
            "resolve-key-1",
            RequestKind::Resolve,
            "{}",
            "requester",
            30_000,
            1,
        ))
        .unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            "resolve-2",
            "resolve-key-2",
            RequestKind::Resolve,
            "{}",
            "requester",
            30_000,
            2,
        ))
        .unwrap();

    assert!(
        coordinator
            .claim_resolve("resolve-1", "resolver-a", 10, 5_000)
            .unwrap()
    );
    assert!(
        !coordinator
            .claim_resolve("resolve-2", "resolver-b", 20, 5_000)
            .unwrap()
    );
    assert!(
        coordinator
            .heartbeat_resolve("resolve-1", "resolver-a", 30)
            .unwrap()
    );
    assert!(
        coordinator
            .resolve_claim_is_current("resolve-1", "resolver-a")
            .unwrap()
    );
    assert!(coordinator.lease().unwrap().is_none());

    assert!(
        coordinator
            .claim_resolve("resolve-1", "resolver-b", 5_031, 5_000)
            .unwrap()
    );
    assert!(
        !coordinator
            .heartbeat_resolve("resolve-1", "resolver-a", 5_032)
            .unwrap()
    );
    assert!(
        !coordinator
            .fail_resolve("resolve-1", "resolver-a", "stale", 5_033)
            .unwrap()
    );
    assert!(
        coordinator
            .resolve_claim_is_current("resolve-1", "resolver-b")
            .unwrap()
    );
    assert!(coordinator.lease().unwrap().is_none());
    append_terminal(&layout, "resolve-1", "{\"resolved\":true}");
    assert!(matches!(
        coordinator.commit_resolve("resolve-1", "resolver-a"),
        Err(CoordinatorError::LeaseLost)
    ));
    assert!(
        coordinator
            .commit_resolve("resolve-1", "resolver-b")
            .unwrap()
            .committed_in_fact
    );
}

#[test]
fn dead_resolve_claimant_is_taken_over_before_the_heartbeat_stales() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut live = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    live.enqueue(CoordinatorRequest::new(
        "resolve-1",
        "resolve-key-1",
        RequestKind::Resolve,
        "{}",
        "requester",
        30_000,
        1,
    ))
    .unwrap();
    assert!(
        live.claim_resolve("resolve-1", "cli-41", 10, 5_000)
            .unwrap()
    );

    let dead = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(false)).unwrap();
    assert!(
        dead.claim_resolve("resolve-1", "cli-42", 11, 5_000)
            .unwrap()
    );
    assert!(!dead.heartbeat_resolve("resolve-1", "cli-41", 12).unwrap());
    assert!(
        dead.resolve_claim_is_current("resolve-1", "cli-42")
            .unwrap()
    );
    assert!(dead.lease().unwrap().is_none());
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
fn request_id_reuse_with_a_different_key_is_a_typed_conflict() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);

    let error = coordinator
        .enqueue(CoordinatorRequest::new(
            "request-1",
            "different-key",
            RequestKind::Update,
            "{}",
            "requester",
            10_000,
            2,
        ))
        .unwrap_err();

    assert!(matches!(
        error,
        CoordinatorError::RequestIdConflict { request_id } if request_id == "request-1"
    ));
}

#[test]
fn invalid_public_request_and_lease_times_are_typed_errors() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let request_error = coordinator
        .enqueue(CoordinatorRequest::new(
            "request-1",
            "idem-1",
            RequestKind::Update,
            "{}",
            "requester",
            10,
            -1,
        ))
        .unwrap_err();
    assert!(matches!(
        request_error,
        CoordinatorError::InvalidTime {
            field: "created_at",
            value: -1
        }
    ));

    let lease_error = coordinator
        .try_acquire_or_takeover(LeaseHolder::new("holder", "2.30.0", 41), i64::MAX)
        .unwrap_err();
    assert!(matches!(
        lease_error,
        CoordinatorError::InvalidTime {
            field: "lease_expiry",
            value: i64::MAX
        }
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
    fn status(&self, _pid: u32) -> PidStatus {
        if self.0 {
            PidStatus::Alive
        } else {
            PidStatus::Dead
        }
    }
}

#[derive(Debug)]
struct UnknownLiveness;

impl PidLiveness for UnknownLiveness {
    fn status(&self, _pid: u32) -> PidStatus {
        PidStatus::Unknown
    }
}

#[test]
fn unknown_pid_liveness_never_authorizes_takeover() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut owner = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    owner
        .try_acquire_or_takeover(LeaseHolder::new("holder-a", "2.30.0", 41), 10)
        .unwrap();
    let mut contender = StoreCoordinator::open_with_liveness(&layout, UnknownLiveness).unwrap();

    assert_eq!(
        contender
            .try_acquire_or_takeover(LeaseHolder::new("holder-b", "2.30.0", 42), 11)
            .unwrap(),
        LeaseDisposition::HeldByOther
    );
}

#[test]
fn live_holder_is_not_displaced_but_dead_or_expired_holder_is_fenced() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut live = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    let first = live
        .try_acquire_or_takeover(LeaseHolder::new("holder-a", "2.30.0", 41), 10)
        .unwrap();
    let LeaseDisposition::Acquired {
        fencing_token: first_token,
    } = first
    else {
        panic!("first lease was not acquired");
    };
    assert_eq!(
        live.try_acquire_or_takeover(LeaseHolder::new("holder-b", "2.31.0", 42), 11)
            .unwrap(),
        LeaseDisposition::HeldByOther
    );
    let LeaseDisposition::Acquired {
        fencing_token: takeover_token,
    } = live
        .try_acquire_or_takeover(LeaseHolder::new("holder-b", "2.31.0", 42), 5_010)
        .unwrap()
    else {
        panic!("expired lease was not taken over");
    };
    assert!(takeover_token > first_token);

    live.release_lease(&LeaseHolder::new("holder-b", "2.31.0", 42), takeover_token)
        .unwrap();
    live.try_acquire_or_takeover(LeaseHolder::new("holder-a", "2.30.0", 41), 6_000)
        .unwrap();
    let mut dead = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(false)).unwrap();
    let LeaseDisposition::Acquired {
        fencing_token: dead_takeover_token,
    } = dead
        .try_acquire_or_takeover(LeaseHolder::new("holder-b", "2.31.0", 42), 6_001)
        .unwrap()
    else {
        panic!("dead lease was not taken over");
    };
    assert!(dead_takeover_token > takeover_token);
}

#[test]
fn dead_lease_takeover_transfers_only_the_prior_holders_claims() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut owner = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    for (request_id, idempotency_key) in [("request-a", "idem-a"), ("request-x", "idem-x")] {
        owner
            .enqueue(CoordinatorRequest::new(
                request_id,
                idempotency_key,
                RequestKind::Import,
                "{}",
                "requester",
                10_000,
                1,
            ))
            .unwrap();
    }
    owner
        .try_acquire_or_takeover(LeaseHolder::new("holder-a", "2.30.0", 41), 10)
        .unwrap();
    let connection = Connection::open(layout.coordinator_db()).unwrap();
    connection
        .execute(
            "UPDATE requests SET state = 'claimed', claim_owner = 'holder-a', claim_heartbeat_at = 10
             WHERE request_id = 'request-a'",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE requests SET state = 'claimed', claim_owner = 'holder-x', claim_heartbeat_at = 10
             WHERE request_id = 'request-x'",
            [],
        )
        .unwrap();

    let mut live = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    assert_eq!(
        live.try_acquire_or_takeover(LeaseHolder::new("holder-b", "2.30.0", 42), 11)
            .unwrap(),
        LeaseDisposition::HeldByOther
    );
    assert_eq!(
        owner.request("request-a").unwrap().claim_owner.as_deref(),
        Some("holder-a")
    );

    let mut dead = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(false)).unwrap();
    assert!(
        dead.try_acquire_or_takeover(LeaseHolder::new("holder-b", "2.30.0", 42), 11)
            .unwrap()
            .acquired()
    );
    let transferred = dead.request("request-a").unwrap();
    let preserved = dead.request("request-x").unwrap();
    assert_eq!(transferred.claim_owner.as_deref(), Some("holder-b"));
    assert_eq!(transferred.claim_heartbeat_at, Some(11));
    assert_eq!(preserved.claim_owner.as_deref(), Some("holder-x"));
    assert_eq!(preserved.claim_heartbeat_at, Some(10));
}

#[test]
fn same_holder_id_with_a_different_live_pid_cannot_renew_or_overwrite() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator =
        StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    assert!(
        coordinator
            .try_acquire_or_takeover(LeaseHolder::new("holder", "2.30.0", 41), 10)
            .unwrap()
            .acquired()
    );

    assert_eq!(
        coordinator
            .try_acquire_or_takeover(LeaseHolder::new("holder", "2.31.0", 42), 11)
            .unwrap(),
        LeaseDisposition::HeldByOther
    );
    let lease = coordinator.lease().unwrap().unwrap();
    assert_eq!(lease.holder.holder_pid, 41);
    assert_eq!(lease.holder.holder_version, "2.30.0");
}

#[test]
fn a_separate_instance_with_the_same_holder_and_pid_cannot_renew_without_the_token() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let holder = LeaseHolder::new("holder", "2.30.0", 41);
    let mut owner = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    owner.try_acquire_or_takeover(holder.clone(), 10).unwrap();
    let mut contender = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();

    assert_eq!(
        contender.try_acquire_or_takeover(holder, 11).unwrap(),
        LeaseDisposition::HeldByOther
    );
}

#[test]
fn heartbeat_and_release_require_the_current_owner_and_fencing_token() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator =
        StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    let holder_a = LeaseHolder::new("holder-a", "2.30.0", 41);
    let holder_b = LeaseHolder::new("holder-b", "2.30.0", 42);
    let lease = coordinator
        .try_acquire_or_takeover(holder_a.clone(), 10)
        .unwrap();
    let LeaseDisposition::Acquired { fencing_token } = lease else {
        panic!("lease was not acquired");
    };

    assert!(
        !coordinator
            .heartbeat_lease(&holder_b, fencing_token, 20)
            .unwrap()
    );
    assert!(
        !coordinator
            .heartbeat_lease(&holder_a, fencing_token + 1, 20)
            .unwrap()
    );
    assert!(
        coordinator
            .heartbeat_lease(&holder_a, fencing_token, 20)
            .unwrap()
    );
    assert!(
        !coordinator
            .release_lease(&holder_a, fencing_token + 1)
            .unwrap()
    );
    assert!(coordinator.release_lease(&holder_a, fencing_token).unwrap());
}

#[test]
fn released_lease_reacquires_with_a_new_token_that_rejects_stale_owner_calls() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator =
        StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    let holder = LeaseHolder::new("holder-a", "2.30.0", 41);
    let LeaseDisposition::Acquired {
        fencing_token: first_token,
    } = coordinator
        .try_acquire_or_takeover(holder.clone(), 10)
        .unwrap()
    else {
        panic!("first lease was not acquired");
    };
    assert!(coordinator.release_lease(&holder, first_token).unwrap());
    let LeaseDisposition::Acquired {
        fencing_token: second_token,
    } = coordinator
        .try_acquire_or_takeover(holder.clone(), 10)
        .unwrap()
    else {
        panic!("second lease was not acquired");
    };

    assert!(second_token > first_token);
    assert!(
        !coordinator
            .heartbeat_lease(&holder, first_token, 11)
            .unwrap()
    );
    assert!(!coordinator.release_lease(&holder, first_token).unwrap());
    assert!(coordinator.lease().unwrap().is_some());
}

#[test]
fn reused_cross_process_token_requires_the_current_holder_pid() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator =
        StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    let holder_a = LeaseHolder::new("shared-holder", "2.30.0", 41);
    let holder_b = LeaseHolder::new("shared-holder", "2.30.0", 42);
    let LeaseDisposition::Acquired { fencing_token } = coordinator
        .try_acquire_or_takeover(holder_a.clone(), 10)
        .unwrap()
    else {
        panic!("lease was not acquired");
    };
    assert!(coordinator.release_lease(&holder_a, fencing_token).unwrap());
    let connection = Connection::open(layout.coordinator_db()).unwrap();
    connection
        .execute(
            "INSERT INTO writer_lease
             (resource, holder_id, holder_version, holder_pid, heartbeat_at, expires_at, fencing_token)
             VALUES ('store-writer', ?1, ?2, ?3, 10, 5010, ?4)",
            (
                &holder_b.holder_id,
                &holder_b.holder_version,
                holder_b.holder_pid,
                fencing_token,
            ),
        )
        .unwrap();

    assert!(
        !coordinator
            .heartbeat_lease(&holder_a, fencing_token, 11)
            .unwrap()
    );
    assert!(!coordinator.release_lease(&holder_a, fencing_token).unwrap());
    assert!(
        coordinator
            .heartbeat_lease(&holder_b, fencing_token, 11)
            .unwrap()
    );
    assert!(coordinator.release_lease(&holder_b, fencing_token).unwrap());
}

#[test]
fn expired_holder_cannot_resurrect_its_lease_with_a_heartbeat() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator =
        StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    let holder = LeaseHolder::new("holder-a", "2.30.0", 41);
    let lease = coordinator
        .try_acquire_or_takeover(holder.clone(), 10)
        .unwrap();
    let LeaseDisposition::Acquired { fencing_token } = lease else {
        panic!("lease was not acquired");
    };

    assert!(
        !coordinator
            .heartbeat_lease(&holder, fencing_token, 5_010)
            .unwrap()
    );
    assert_eq!(coordinator.lease().unwrap().unwrap().expires_at, 5_010);
}

#[test]
fn expired_holder_reacquisition_advances_the_fencing_token() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator =
        StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    let holder = LeaseHolder::new("holder-a", "2.30.0", 41);
    let LeaseDisposition::Acquired {
        fencing_token: first_token,
    } = coordinator
        .try_acquire_or_takeover(holder.clone(), 10)
        .unwrap()
    else {
        panic!("first lease was not acquired");
    };

    let LeaseDisposition::Acquired {
        fencing_token: second_token,
    } = coordinator.try_acquire_or_takeover(holder, 5_010).unwrap()
    else {
        panic!("expired lease was not reacquired");
    };

    assert!(second_token > first_token);
}

#[test]
fn coordinator_operations_use_a_bounded_busy_policy_instead_of_reporting_lease_loss() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut owner = StoreCoordinator::open_with_liveness(&layout, FixedLiveness(true)).unwrap();
    let LeaseDisposition::Acquired { fencing_token } = owner
        .try_acquire_or_takeover(LeaseHolder::new("holder-a", "2.30.0", 41), 10)
        .unwrap()
    else {
        panic!("lease was not acquired");
    };
    let locker = Connection::open(layout.coordinator_db()).unwrap();
    locker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let worker_layout = layout.clone();
    let worker = std::thread::spawn(move || {
        let mut coordinator =
            StoreCoordinator::open_with_liveness(&worker_layout, FixedLiveness(true)).unwrap();
        coordinator.heartbeat_lease(
            &LeaseHolder::new("holder-a", "2.30.0", 41),
            fencing_token,
            20,
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(50));
    locker.execute_batch("COMMIT").unwrap();

    assert!(worker.join().unwrap().unwrap());
}

#[test]
fn coordinator_database_connections_are_centralized_through_one_busy_policy() {
    let source = include_str!("../src/store/coordinator.rs");

    assert!(source.contains("fn open_coordinator("));
    assert_eq!(
        source
            .matches("Connection::open(&self.coordinator_db)")
            .count(),
        0
    );
    assert!(source.contains("connection.busy_timeout(COORDINATOR_BUSY_TIMEOUT)?"));
    assert!(source.contains("fn begin_coordinator("));
    assert_eq!(
        source
            .matches("transaction_with_behavior(TransactionBehavior::Immediate)")
            .count(),
        1
    );
}

struct PragmaReadbackExecutor;

impl CoordinatorExecutor for PragmaReadbackExecutor {
    fn execute_quantum(
        &mut self,
        transaction: &Transaction<'_>,
        _request: &CoordinatorRequest,
        _context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        let text = |name: &str| {
            transaction
                .query_row(&format!("PRAGMA {name}"), [], |row| row.get::<_, String>(0))
                .unwrap()
        };
        let integer = |name: &str| {
            transaction
                .query_row(&format!("PRAGMA {name}"), [], |row| row.get::<_, i64>(0))
                .unwrap()
        };
        assert_eq!(text("journal_mode"), "wal");
        assert_eq!(integer("page_size"), 4096);
        assert_eq!(integer("auto_vacuum"), 2);
        assert_eq!(integer("synchronous"), 2);
        assert_eq!(integer("foreign_keys"), 1);
        assert_eq!(integer("secure_delete"), 1);
        assert_eq!(integer("wal_autocheckpoint"), 8_000);
        Ok(ExecutionQuantum::Complete {
            event_kind: "pragma_readback_completed".to_string(),
            result_json: "{}".to_string(),
        })
    }
}

#[test]
fn import_quantum_receives_the_bulk_writer_pragma_profile_before_begin() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    clock.set(10);
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        LeaseHolder::new("holder", "2.30.0", 41),
        clock,
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            "request-pragmas",
            "idem-pragmas",
            RequestKind::Import,
            "{}",
            "requester",
            1_000,
            1,
        ))
        .unwrap();
    coordinator
        .drain(
            &mut PragmaReadbackExecutor,
            &CoordinatorPolicy {
                own_request_id: Some("request-pragmas".to_string()),
                ..CoordinatorPolicy::default()
            },
        )
        .unwrap();
}

#[test]
fn normal_drain_paths_explicitly_release_and_only_panic_relies_on_drop() {
    let source = include_str!("../src/store/coordinator.rs");

    assert!(source.contains("let release_result = self.release_lease_for("));
    assert!(source.contains("guard.disarm();"));
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

#[test]
fn coordinator_terminal_state_must_exactly_match_the_store_terminal() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);
    let terminal_sequence = append_terminal(&layout, "request-1", "{\"ok\":true}");
    let connection = Connection::open(layout.coordinator_db()).unwrap();
    connection
        .execute(
            "UPDATE requests SET state = 'committed', terminal_log_sequence = ?1,
             result_json = '{\"ok\":false}', updated_at = 2 WHERE request_id = 'request-1'",
            [terminal_sequence],
        )
        .unwrap();

    let error = coordinator.reconcile("request-1").unwrap_err();

    assert!(matches!(error, CoordinatorError::CorruptRequest { .. }));
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

struct TakeoverThenFailExecutor {
    coordinator_db: PathBuf,
}

struct SelectiveFailExecutor {
    fail_request_ids: Vec<String>,
    order: Vec<String>,
}

impl CoordinatorExecutor for SelectiveFailExecutor {
    fn execute_quantum(
        &mut self,
        _transaction: &Transaction<'_>,
        request: &CoordinatorRequest,
        _context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        self.order.push(request.request_id.clone());
        if self.fail_request_ids.contains(&request.request_id) {
            Err(format!("{} failed", request.request_id))
        } else {
            Ok(ExecutionQuantum::Complete {
                event_kind: "complete".to_string(),
                result_json: "{}".to_string(),
            })
        }
    }
}

impl CoordinatorExecutor for TakeoverThenFailExecutor {
    fn execute_quantum(
        &mut self,
        _transaction: &Transaction<'_>,
        request: &CoordinatorRequest,
        context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        let connection = Connection::open(&self.coordinator_db).unwrap();
        connection
            .execute(
                "UPDATE writer_lease SET holder_id = 'holder-b', holder_pid = 42,
                 heartbeat_at = 20, expires_at = 5020, fencing_token = ?1
                 WHERE resource = 'store-writer'",
                [context.fencing_token + 1],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE requests SET claim_owner = 'holder-b', claim_heartbeat_at = 20,
                 updated_at = 20 WHERE request_id = ?1",
                [&request.request_id],
            )
            .unwrap();
        Err("holder-a failed after takeover".to_string())
    }
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
fn stale_holder_cannot_fail_the_successors_claim_after_takeover() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    clock.set(10);
    let holder = LeaseHolder::new("holder-a", "2.30.0", 41);
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        holder,
        Arc::clone(&clock),
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);
    let mut executor = TakeoverThenFailExecutor {
        coordinator_db: layout.coordinator_db().to_path_buf(),
    };

    let error = coordinator
        .drain(&mut executor, &CoordinatorPolicy::default())
        .unwrap_err();

    assert!(matches!(error, CoordinatorError::LeaseLost));
    let request = coordinator.request("request-1").unwrap();
    assert_eq!(request.state.as_str(), "claimed");
    assert_eq!(request.claim_owner.as_deref(), Some("holder-b"));
}

#[test]
fn stale_holder_cannot_claim_after_takeover_between_heartbeat_and_claim() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let connection = Connection::open(layout.coordinator_db()).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER takeover_after_heartbeat
             AFTER UPDATE OF heartbeat_at ON writer_lease
             WHEN NEW.holder_id = 'holder-a'
             BEGIN
               UPDATE writer_lease SET holder_id = 'holder-b', holder_pid = 42,
                 heartbeat_at = NEW.heartbeat_at, expires_at = NEW.expires_at,
                 fencing_token = NEW.fencing_token + 1
               WHERE resource = NEW.resource;
             END;",
        )
        .unwrap();
    let clock = Arc::new(TestClock::default());
    clock.set(10);
    let holder = LeaseHolder::new("holder-a", "2.30.0", 41);
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        holder,
        Arc::clone(&clock),
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);
    let mut executor = RecordingExecutor {
        order: Vec::new(),
        clock,
        advance_ms: 0,
    };

    let error = coordinator
        .drain(&mut executor, &CoordinatorPolicy::default())
        .unwrap_err();

    assert!(matches!(error, CoordinatorError::LeaseLost));
    assert!(executor.order.is_empty());
    assert_eq!(
        coordinator.request("request-1").unwrap().state.as_str(),
        "queued"
    );
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
fn own_request_runs_exclusively_until_terminal_before_backlog_snapshot() {
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
    let mut executor = RecordingExecutor {
        order: Vec::new(),
        clock,
        advance_ms: 0,
    };
    let policy = CoordinatorPolicy {
        own_request_id: Some("request-2".to_string()),
        ..CoordinatorPolicy::default()
    };

    coordinator.drain(&mut executor, &policy).unwrap();

    assert_eq!(executor.order, ["request-2", "request-1"]);
}

#[test]
fn generic_backlog_drain_leaves_resolves_to_the_off_lease_resolver() {
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
    enqueue_request(&mut coordinator, 1, RequestKind::Resolve, 1);
    enqueue_request(&mut coordinator, 2, RequestKind::Resolve, 2);
    enqueue_request(&mut coordinator, 3, RequestKind::Update, 3);
    assert!(
        coordinator
            .claim_resolve("request-1", "resolver-a", 10, 5_000)
            .unwrap()
    );
    let mut executor = RecordingExecutor {
        order: Vec::new(),
        clock,
        advance_ms: 0,
    };
    let policy = CoordinatorPolicy {
        own_request_id: Some("request-3".to_string()),
        ..CoordinatorPolicy::default()
    };

    coordinator.drain(&mut executor, &policy).unwrap();

    assert_eq!(executor.order, ["request-3"]);
    assert_eq!(
        coordinator.request("request-1").unwrap().state.as_str(),
        "claimed"
    );
    assert_eq!(
        coordinator.request("request-2").unwrap().state.as_str(),
        "queued"
    );
}

#[test]
fn failed_own_request_transitions_to_backlog_and_drain_continues() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    let holder = LeaseHolder::new("holder", "2.30.0", std::process::id());
    let mut coordinator =
        StoreCoordinator::open_with_runtime(&layout, holder, clock, Arc::new(FixedLiveness(true)))
            .unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);
    enqueue_request(&mut coordinator, 2, RequestKind::Update, 2);
    let mut executor = SelectiveFailExecutor {
        fail_request_ids: vec!["request-2".to_string()],
        order: Vec::new(),
    };
    let policy = CoordinatorPolicy {
        own_request_id: Some("request-2".to_string()),
        ..CoordinatorPolicy::default()
    };

    let report = coordinator.drain(&mut executor, &policy).unwrap();

    assert_eq!(executor.order, ["request-2", "request-1"]);
    assert_eq!(report.failed_requests, 1);
    assert_eq!(report.completed_requests, 1);
    assert_eq!(
        coordinator.request("request-2").unwrap().state.as_str(),
        "failed"
    );
    assert_eq!(
        coordinator.request("request-1").unwrap().state.as_str(),
        "committed"
    );
}

#[test]
fn failed_backlog_request_does_not_block_the_following_snapshot_request() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    let holder = LeaseHolder::new("holder", "2.30.0", std::process::id());
    let mut coordinator =
        StoreCoordinator::open_with_runtime(&layout, holder, clock, Arc::new(FixedLiveness(true)))
            .unwrap();
    enqueue_request(&mut coordinator, 1, RequestKind::Update, 1);
    enqueue_request(&mut coordinator, 2, RequestKind::Update, 2);
    let mut executor = SelectiveFailExecutor {
        fail_request_ids: vec!["request-1".to_string()],
        order: Vec::new(),
    };

    let report = coordinator
        .drain(&mut executor, &CoordinatorPolicy::default())
        .unwrap();

    assert_eq!(executor.order, ["request-1", "request-2"]);
    assert_eq!(report.failed_requests, 1);
    assert_eq!(report.completed_requests, 1);
    assert_eq!(
        coordinator.request("request-1").unwrap().state.as_str(),
        "failed"
    );
    assert_eq!(
        coordinator.request("request-2").unwrap().state.as_str(),
        "committed"
    );
}

#[test]
fn missing_own_request_id_is_a_typed_error() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    let holder = LeaseHolder::new("holder", "2.30.0", std::process::id());
    let mut coordinator =
        StoreCoordinator::open_with_runtime(&layout, holder, clock, Arc::new(FixedLiveness(true)))
            .unwrap();
    let policy = CoordinatorPolicy {
        own_request_id: Some("missing-request".to_string()),
        ..CoordinatorPolicy::default()
    };

    let error = coordinator
        .drain(
            &mut RecordingExecutor {
                order: Vec::new(),
                clock: Arc::new(TestClock::default()),
                advance_ms: 0,
            },
            &policy,
        )
        .unwrap_err();

    assert!(matches!(
        error,
        CoordinatorError::RequestNotFound { request_id } if request_id == "missing-request"
    ));
}

#[test]
fn overflowing_service_deadline_is_a_typed_time_error() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let clock = Arc::new(TestClock::default());
    clock.set(1);
    let holder = LeaseHolder::new("holder", "2.30.0", std::process::id());
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        holder,
        Arc::clone(&clock),
        Arc::new(FixedLiveness(true)),
    )
    .unwrap();
    let policy = CoordinatorPolicy {
        service_window_ms: i64::MAX,
        ..CoordinatorPolicy::default()
    };

    let error = coordinator
        .drain(
            &mut RecordingExecutor {
                order: Vec::new(),
                clock,
                advance_ms: 0,
            },
            &policy,
        )
        .unwrap_err();

    assert!(matches!(
        error,
        CoordinatorError::InvalidTime {
            field: "service_deadline",
            value: 1
        }
    ));
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
            let report = coordinator
                .drain(&mut FailingExecutor, &CoordinatorPolicy::default())
                .unwrap();
            assert_eq!(report.failed_requests, 1);
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

#[test]
fn request_arriving_exactly_at_the_service_deadline_is_not_accepted() {
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
    let mut executor = ProducingExecutor {
        layout,
        clock,
        order: Vec::new(),
        next_request: 1_000,
        advance_ms: 1_000,
    };

    coordinator
        .drain(&mut executor, &CoordinatorPolicy::default())
        .unwrap();

    assert_eq!(executor.order, ["request-1"]);
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
        .drain(
            &mut SlowExecutor {
                clock: Arc::clone(&clock),
            },
            &CoordinatorPolicy::default(),
        )
        .unwrap_err();

    assert!(matches!(
        error,
        CoordinatorError::QuantumDeadlineExceeded { .. }
    ));
    assert_eq!(
        coordinator.request("request-1").unwrap().state.as_str(),
        "queued"
    );
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

    let report = coordinator
        .drain(
            &mut RecordingExecutor {
                order: Vec::new(),
                clock,
                advance_ms: 0,
            },
            &CoordinatorPolicy::default(),
        )
        .unwrap();
    assert_eq!(report.completed_requests, 1);
    assert_eq!(
        coordinator.request("request-1").unwrap().state.as_str(),
        "committed"
    );
}

#[test]
fn terminal_request_archival_creates_one_receipt_and_preserves_replay_conflicts() {
    let temp = TempDir::new();
    let layout = layout(temp.path());
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let request = CoordinatorRequest::new(
        "request-archive",
        "idem-archive",
        RequestKind::Update,
        r#"{"path":"src/lib.rs"}"#,
        "requester",
        1_000,
        1,
    );
    coordinator.enqueue(request.clone()).unwrap();
    let sequence = append_terminal(&layout, &request.request_id, r#"{"generation":7}"#);
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "UPDATE requests SET state='committed',terminal_log_sequence=?1,
             result_json=?2,updated_at=10 WHERE request_id=?3",
            rusqlite::params![sequence, r#"{"generation":7}"#, request.request_id],
        )
        .unwrap();

    let archived = coordinator
        .archive_terminal_requests("gen-001", 10, sequence, 10)
        .unwrap();

    assert_eq!(
        archived,
        vec![RequestReceipt {
            request_id: "request-archive".to_string(),
            idempotency_key: "idem-archive".to_string(),
            kind: RequestKind::Update,
            payload_json: r#"{"path":"src/lib.rs"}"#.to_string(),
            terminal_result_json: r#"{"generation":7}"#.to_string(),
            terminal_generation_name: "gen-001".to_string(),
            terminal_log_sequence: sequence,
            completed_at: 10,
        }]
    );
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    assert_eq!(
        coord
            .query_row(
                "SELECT COUNT(*) FROM requests WHERE request_id='request-archive'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        coord
            .query_row("SELECT COUNT(*) FROM request_receipts", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        1
    );

    let replay = coordinator
        .enqueue(CoordinatorRequest::new(
            "request-retry",
            "idem-archive",
            RequestKind::Update,
            r#"{"path":"src/lib.rs"}"#,
            "requester",
            2_000,
            12,
        ))
        .unwrap();
    assert!(!replay.inserted);
    assert_eq!(replay.request.request_id, "request-archive");
    assert_eq!(
        replay.request.result_json.as_deref(),
        Some(r#"{"generation":7}"#)
    );
    assert!(matches!(
        coordinator.enqueue(CoordinatorRequest::new(
            "request-conflict",
            "idem-archive",
            RequestKind::Delete,
            r#"{"path":"src/lib.rs"}"#,
            "requester",
            2_000,
            12,
        )),
        Err(CoordinatorError::IdempotencyConflict { .. })
    ));
    assert!(matches!(
        coordinator.enqueue(CoordinatorRequest::new(
            "request-archive",
            "different-idem",
            RequestKind::Update,
            r#"{"path":"src/lib.rs"}"#,
            "requester",
            2_000,
            12,
        )),
        Err(CoordinatorError::RequestIdConflict { .. })
    ));
}

#[test]
fn consumer_cursor_advance_is_monotonic_bounded_and_releasable() {
    let temp = TempDir::new();
    let initial = layout(temp.path());
    fs::rename(initial.generation_dir(), temp.path().join("gen-1000")).unwrap();
    fs::write(temp.path().join("CURRENT"), "gen-1000\n").unwrap();
    let layout = StoreLayout::open(temp.path()).unwrap();
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let coord = Connection::open(layout.coordinator_db()).unwrap();
    coord
        .execute(
            "INSERT INTO family_allocator_marks
             (allocator_kind,scope_id,high_water,updated_at)
             VALUES ('store_log','',10,1)",
            [],
        )
        .unwrap();

    let cursor = coordinator
        .advance_consumer_cursor("miller", "gen-1000", 5, 2)
        .unwrap();
    assert_eq!(
        cursor,
        ConsumerCursor {
            consumer_id: "miller".to_string(),
            generation_name: "gen-1000".to_string(),
            store_log_sequence: 5,
            updated_at: 2,
        }
    );
    assert!(matches!(
        coordinator.advance_consumer_cursor("miller", "gen-1000", 4, 3),
        Err(CoordinatorError::CursorRegression { .. })
    ));
    assert!(matches!(
        coordinator.advance_consumer_cursor("miller", "gen-1000", 11, 3),
        Err(CoordinatorError::CursorAhead { .. })
    ));
    assert!(matches!(
        coordinator.advance_consumer_cursor("miller", "missing", 6, 3),
        Err(CoordinatorError::InvalidGeneration { .. })
    ));
    assert!(coordinator.release_consumer_cursor("miller").unwrap());
    assert!(!coordinator.release_consumer_cursor("miller").unwrap());
}
