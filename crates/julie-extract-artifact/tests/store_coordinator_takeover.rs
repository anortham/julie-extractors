#![cfg(feature = "test-takeover")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use julie_extract_artifact::store::{
    CoordinatorExecutor, CoordinatorPolicy, CoordinatorRequest, ExecutionContext, ExecutionQuantum,
    LeaseDisposition, LeaseHolder, PidLiveness, PidStatus, RequestKind, StoreCoordinator,
    StoreLayout, UnixMillisClock,
};
use rusqlite::{Connection, Transaction};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "julie-coordinator-takeover-{}-{suffix}",
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

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap())
        .unwrap()
}

#[derive(Debug)]
struct FixedClock(AtomicI64);

impl FixedClock {
    fn new(now: i64) -> Self {
        Self(AtomicI64::new(now))
    }
}

impl UnixMillisClock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
struct LivePid;

impl PidLiveness for LivePid {
    fn status(&self, _pid: u32) -> PidStatus {
        PidStatus::Alive
    }
}

struct CompleteExecutor;

impl CoordinatorExecutor for CompleteExecutor {
    fn execute_quantum(
        &mut self,
        _transaction: &Transaction<'_>,
        _request: &CoordinatorRequest,
        _context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        Ok(ExecutionQuantum::Complete {
            event_kind: "takeover_complete".to_string(),
            result_json: "{}".to_string(),
        })
    }
}

#[test]
fn hard_killed_holder_is_taken_over_without_a_duplicate_terminal_effect() {
    let temp = TempDir::new();
    let layout = StoreLayout::create(temp.path(), "family-a", "2.30.0").unwrap();
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            "request-b",
            "idem-b",
            RequestKind::Update,
            "{}",
            "requester-b",
            unix_ms() + 10_000,
            unix_ms(),
        ))
        .unwrap();
    let ready = temp.path().join("holder-a.ready");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "takeover_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("JULIE_COORDINATOR_WORKER_ROOT", temp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..500 {
        if ready.exists() {
            break;
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "holder process exited before ready"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "holder process did not acquire the lease");
    let first_token = fs::read_to_string(&ready).unwrap().parse::<i64>().unwrap();

    child.kill().unwrap();
    child.wait().unwrap();

    let takeover_at = unix_ms().max(first_token.saturating_add(5_000));
    let holder_b = LeaseHolder::new("holder-b", "2.30.0", std::process::id());
    let mut draining = StoreCoordinator::open_with_runtime(
        &layout,
        holder_b.clone(),
        Arc::new(FixedClock::new(takeover_at)),
        Arc::new(LivePid),
    )
    .unwrap();
    let lease = draining
        .try_acquire_or_takeover(holder_b, takeover_at)
        .unwrap();
    let LeaseDisposition::Acquired { fencing_token } = lease else {
        panic!("dead holder was not taken over");
    };
    assert!(fencing_token > first_token);
    draining
        .drain(&mut CompleteExecutor, &CoordinatorPolicy::default())
        .unwrap();

    let store = Connection::open(layout.store_db()).unwrap();
    let terminal_count = store
        .query_row(
            "SELECT COUNT(*) FROM store_log WHERE request_id = 'request-b' AND terminal = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(terminal_count, 1);
}

#[test]
fn takeover_worker() {
    let Ok(root) = std::env::var("JULIE_COORDINATOR_WORKER_ROOT") else {
        return;
    };
    let layout = StoreLayout::open(&root).unwrap();
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let lease = coordinator
        .try_acquire_or_takeover(
            LeaseHolder::new("holder-a", "2.30.0", std::process::id()),
            unix_ms(),
        )
        .unwrap();
    let LeaseDisposition::Acquired { fencing_token } = lease else {
        panic!("worker failed to acquire lease");
    };
    fs::write(
        Path::new(&root).join("holder-a.ready"),
        fencing_token.to_string(),
    )
    .unwrap();
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}
