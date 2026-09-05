use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use julie_extract_artifact::store::{
    CapacityProvider, ConsumerCursor, CoordinatorError, CoordinatorRequest, MaintenanceApplyPolicy,
    MaintenanceClock, MaintenanceExecutor, MaintenanceInspector, MaintenanceRun, ManifestStore,
    ReaderAcquireRequest, ReaderRegistration, ReaderReleaseRequest, ReaderRenewRequest,
    RequestKind, StoreConnectionError, StoreConnectionFactory, StoreCoordinator, StoreLayout,
    StoreLog, StoreLogEntry,
};
use rusqlite::{Connection, OptionalExtension, params};

const FAMILY_ID: &str = "family-a";
const VIEW_ID: &str = "view-a";
const WRITER_VERSION: &str = "2.40.0";
const MAINTENANCE_NOW: i64 = 4_000_000_000_000;

#[test]
fn active_reader_leaves_cursor_monotonic_bounded_and_generation_bound() {
    let temp = TempStore::new("cursor-reader-monotonic");
    let layout = StoreLayout::create(temp.path(), FAMILY_ID, WRITER_VERSION, 7).unwrap();
    let (_, terminal_sequence) = publish_manifest(&layout, "request-manifest");
    let nonce = "01010101010101010101010101010101";
    let reader = acquire_reader(&layout, nonce);
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();

    let cursor = coordinator
        .advance_consumer_cursor(
            "consumer-a",
            layout.generation_name(),
            terminal_sequence,
            10,
        )
        .unwrap();
    assert_eq!(cursor.store_log_sequence, terminal_sequence);

    assert!(matches!(
        coordinator.advance_consumer_cursor(
            "consumer-a",
            layout.generation_name(),
            terminal_sequence - 1,
            11,
        ),
        Err(CoordinatorError::CursorRegression { .. })
    ));
    assert!(matches!(
        coordinator.advance_consumer_cursor(
            "consumer-a",
            layout.generation_name(),
            terminal_sequence + 1,
            11,
        ),
        Err(CoordinatorError::CursorAhead { .. })
    ));
    assert!(matches!(
        coordinator.advance_consumer_cursor("consumer-a", "gen-999", terminal_sequence, 11),
        Err(CoordinatorError::InvalidGeneration { .. })
    ));

    assert_eq!(cursor_row(&layout, "consumer-a"), Some(cursor));
    assert!(authenticated_reader(&layout, &reader, nonce).as_ref() == Some(&reader));
}

#[test]
fn cursor_and_reader_creation_and_release_are_independent() {
    let temp = TempStore::new("cursor-reader-release");
    let layout = StoreLayout::create(temp.path(), FAMILY_ID, WRITER_VERSION, 7).unwrap();
    let (_, terminal_sequence) = publish_manifest(&layout, "request-manifest");
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();

    let cursor: ConsumerCursor = coordinator
        .advance_consumer_cursor(
            "cursor-only",
            layout.generation_name(),
            terminal_sequence,
            10,
        )
        .unwrap();
    assert_eq!(cursor.store_log_sequence, terminal_sequence);
    assert_eq!(reader_count(&layout), 0);

    let nonce = "02020202020202020202020202020202";
    let reader = acquire_reader(&layout, nonce);
    assert!(coordinator.release_consumer_cursor("cursor-only").unwrap());
    assert_eq!(cursor_row(&layout, "cursor-only"), None);
    assert!(authenticated_reader(&layout, &reader, nonce).as_ref() == Some(&reader));

    coordinator
        .advance_consumer_cursor(
            "cursor-survivor",
            layout.generation_name(),
            terminal_sequence,
            11,
        )
        .unwrap();
    assert!(
        coordinator
            .release_reader(&release_request(&reader, nonce))
            .unwrap()
    );
    assert_eq!(reader_count(&layout), 0);
    assert_eq!(
        cursor_row(&layout, "cursor-survivor")
            .unwrap()
            .store_log_sequence,
        terminal_sequence
    );
}

#[test]
fn foreign_maintenance_intent_blocks_cursor_and_authenticated_reader_mutations() {
    let temp = TempStore::new("cursor-reader-maintenance-intent");
    let layout = StoreLayout::create(temp.path(), FAMILY_ID, WRITER_VERSION, 7).unwrap();
    let (_, terminal_sequence) = publish_manifest(&layout, "request-manifest");
    let nonce = "03030303030303030303030303030303";
    let reader = acquire_reader(&layout, nonce);
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    let cursor = coordinator
        .advance_consumer_cursor(
            "consumer-a",
            layout.generation_name(),
            terminal_sequence,
            10,
        )
        .unwrap();
    insert_live_maintenance_intent(&layout, "foreign-run");

    assert_maintenance_refusal(coordinator.advance_consumer_cursor(
        "consumer-a",
        layout.generation_name(),
        terminal_sequence,
        11,
    ));
    assert_maintenance_refusal(coordinator.release_consumer_cursor("consumer-a"));
    assert_maintenance_refusal(coordinator.renew_reader(&ReaderRenewRequest::new(
        FAMILY_ID,
        reader.identity().pin_id(),
        nonce,
        std::process::id(),
        30_000,
    )));
    assert_maintenance_refusal(coordinator.release_reader(&release_request(&reader, nonce)));

    assert_eq!(cursor_row(&layout, "consumer-a"), Some(cursor));
    assert!(authenticated_reader(&layout, &reader, nonce).as_ref() == Some(&reader));
}

#[test]
fn gc_retains_the_stricter_cursor_or_live_reader_log_floor() {
    reader_floor_below_cursor_retains_its_inclusive_boundary();
    cursor_floor_below_reader_prunes_through_its_acknowledged_sequence();
}

fn reader_floor_below_cursor_retains_its_inclusive_boundary() {
    let temp = TempStore::new("reader-floor-below-cursor");
    let layout = StoreLayout::create(temp.path(), FAMILY_ID, WRITER_VERSION, 7).unwrap();
    let (_, manifest_terminal) = publish_manifest(&layout, "request-manifest");
    prune_receipted_request_log(&layout, "request-manifest", manifest_terminal);
    let reader_floor = append_terminal_request(&layout, "request-reader-floor", 2);
    let nonce = "04040404040404040404040404040404";
    let reader = acquire_reader(&layout, nonce);
    assert_eq!(reader_floor, 3);
    assert_eq!(reader.snapshot().served_store_log_sequence(), 3);
    assert_eq!(reader.snapshot().min_retained_store_log_sequence(), 3);
    let later_terminal = append_terminal_request(&layout, "request-later", 3);
    assert_eq!(later_terminal, 4);
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    coordinator
        .advance_consumer_cursor("consumer-a", layout.generation_name(), later_terminal, 10)
        .unwrap();

    run_gc(&layout, "gc-reader-floor-first");
    assert_eq!(log_sequences(&layout), vec![3, 4]);

    assert!(
        coordinator
            .release_reader(&release_request(&reader, nonce))
            .unwrap()
    );
    run_gc(&layout, "gc-after-reader-release");
    assert!(log_sequences(&layout).is_empty());
    assert_eq!(
        cursor_row(&layout, "consumer-a")
            .unwrap()
            .store_log_sequence,
        4
    );
}

fn cursor_floor_below_reader_prunes_through_its_acknowledged_sequence() {
    let temp = TempStore::new("cursor-floor-below-reader");
    let layout = StoreLayout::create(temp.path(), FAMILY_ID, WRITER_VERSION, 7).unwrap();
    let cursor_floor = append_terminal_request(&layout, "request-before-one", 1);
    let between_floor = append_terminal_request(&layout, "request-before-two", 2);
    let (reader_floor, manifest_terminal) = publish_manifest(&layout, "request-manifest");
    let nonce = "05050505050505050505050505050505";
    let reader = acquire_reader(&layout, nonce);
    assert_eq!(
        (cursor_floor, between_floor, reader_floor, manifest_terminal),
        (1, 2, 3, 4)
    );
    assert_eq!(reader.snapshot().min_retained_store_log_sequence(), 3);
    let mut coordinator = StoreCoordinator::open(&layout).unwrap();
    coordinator
        .advance_consumer_cursor("consumer-a", layout.generation_name(), cursor_floor, 10)
        .unwrap();

    run_gc(&layout, "gc-cursor-floor-first");
    assert_eq!(log_sequences(&layout), vec![2, 3, 4]);

    assert!(coordinator.release_consumer_cursor("consumer-a").unwrap());
    run_gc(&layout, "gc-after-cursor-release");
    assert_eq!(log_sequences(&layout), vec![3, 4]);
    assert!(authenticated_reader(&layout, &reader, nonce).as_ref() == Some(&reader));
}

fn prune_receipted_request_log(layout: &StoreLayout, request_id: &str, terminal: i64) {
    let mut coordinator = StoreCoordinator::open(layout).unwrap();
    assert_eq!(
        coordinator
            .archive_terminal_requests(layout.generation_name(), i64::MAX, terminal, 10)
            .unwrap()
            .len(),
        1
    );
    let mut store = Connection::open(layout.store_db()).unwrap();
    let transaction = store.transaction().unwrap();
    assert_eq!(
        StoreLog::prune_receipted_request(&transaction, request_id, terminal).unwrap(),
        2
    );
    transaction.commit().unwrap();
    drop(store);
    assert!(log_sequences(layout).is_empty());
}

fn publish_manifest(layout: &StoreLayout, request_id: &str) -> (i64, i64) {
    let mut coordinator = StoreCoordinator::open(layout).unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            request_id,
            format!("{request_id}-key"),
            RequestKind::Import,
            "{}",
            "cursor-reader-test",
            i64::MAX,
            1,
        ))
        .unwrap();
    let mut store = Connection::open(layout.store_db()).unwrap();
    ManifestStore::new(&mut store)
        .ensure_view(VIEW_ID, "/repo")
        .unwrap();
    let published = ManifestStore::new(&mut store)
        .publish(VIEW_ID, None, std::iter::empty(), request_id)
        .unwrap();
    let reader_floor = published.effect_sequence.unwrap();
    let transaction = store.transaction().unwrap();
    let terminal = StoreLog::append_terminal(
        &transaction,
        &StoreLogEntry::new(
            request_id,
            "store_import_completed",
            "{}",
            "2026-09-04T00:00:01Z",
        )
        .with_view(VIEW_ID)
        .with_generation(published.generation),
    )
    .unwrap();
    transaction.commit().unwrap();
    drop(store);
    coordinator.reconcile(request_id).unwrap();
    (reader_floor, terminal)
}

fn append_terminal_request(layout: &StoreLayout, request_id: &str, created_at: i64) -> i64 {
    let mut coordinator = StoreCoordinator::open(layout).unwrap();
    coordinator
        .enqueue(CoordinatorRequest::new(
            request_id,
            format!("{request_id}-key"),
            RequestKind::Update,
            "{}",
            "cursor-reader-test",
            i64::MAX,
            created_at,
        ))
        .unwrap();
    let mut store = Connection::open(layout.store_db()).unwrap();
    let transaction = store.transaction().unwrap();
    let terminal = StoreLog::append_terminal(
        &transaction,
        &StoreLogEntry::new(
            request_id,
            "store_update_completed",
            "{}",
            "2026-09-04T00:00:02Z",
        ),
    )
    .unwrap();
    transaction.commit().unwrap();
    drop(store);
    coordinator.reconcile(request_id).unwrap();
    terminal
}

fn acquire_reader(layout: &StoreLayout, nonce: &str) -> ReaderRegistration {
    StoreCoordinator::open(layout)
        .unwrap()
        .acquire_reader(&ReaderAcquireRequest::new(
            FAMILY_ID,
            VIEW_ID,
            layout.generation_name(),
            "miller",
            std::process::id(),
            nonce,
            30_000,
        ))
        .unwrap()
        .registration()
        .clone()
}

fn release_request(reader: &ReaderRegistration, nonce: &str) -> ReaderReleaseRequest {
    ReaderReleaseRequest::new(FAMILY_ID, reader.identity().pin_id(), nonce)
}

fn authenticated_reader(
    layout: &StoreLayout,
    reader: &ReaderRegistration,
    nonce: &str,
) -> Option<ReaderRegistration> {
    StoreCoordinator::open(layout)
        .unwrap()
        .reader_registration(&release_request(reader, nonce))
        .unwrap()
}

fn insert_live_maintenance_intent(layout: &StoreLayout, run_id: &str) {
    Connection::open(layout.coordinator_db())
        .unwrap()
        .execute(
            "INSERT INTO maintenance_intent
             (resource,run_id,action,source_generation_name,owner_id,owner_pid,
              fencing_token,heartbeat_at,expires_at,started_at,plan_fingerprint,
              source_min_writer_version)
             VALUES ('store-maintenance',?1,'gc',?2,'foreign-owner',7,41,1,?3,1,
                     'foreign-plan','2.40.0')",
            params![run_id, layout.generation_name(), i64::MAX],
        )
        .unwrap();
}

fn assert_maintenance_refusal<T>(result: Result<T, CoordinatorError>) {
    assert!(matches!(
        result,
        Err(CoordinatorError::StoreConnection(
            StoreConnectionError::MaintenanceInProgress { run_id }
        )) if run_id == "foreign-run"
    ));
}

fn run_gc(layout: &StoreLayout, run_id: &str) {
    let plan = MaintenanceInspector::new(
        StoreConnectionFactory::new(layout.clone(), FAMILY_ID, WRITER_VERSION),
        FixedClock,
        FixedCapacity,
    )
    .inspect()
    .unwrap();
    assert_eq!(plan.protected_readers.len(), reader_count(layout) as usize);
    let mut executor = MaintenanceExecutor::acquire(
        StoreConnectionFactory::new(layout.clone(), FAMILY_ID, WRITER_VERSION),
        MaintenanceRun::new(
            run_id,
            "cursor-reader-test",
            std::process::id(),
            MAINTENANCE_NOW,
            5_000,
        ),
        &plan,
        FixedCapacity,
    )
    .unwrap();
    executor
        .apply_with_policy(
            &plan,
            &MaintenanceApplyPolicy {
                request_safety_ms: 0,
                ..MaintenanceApplyPolicy::default()
            },
        )
        .unwrap();
}

fn cursor_row(layout: &StoreLayout, consumer_id: &str) -> Option<ConsumerCursor> {
    Connection::open(layout.coordinator_db())
        .unwrap()
        .query_row(
            "SELECT consumer_id,generation_name,store_log_sequence,updated_at
             FROM consumer_cursors WHERE consumer_id=?1",
            [consumer_id],
            |row| {
                Ok(ConsumerCursor {
                    consumer_id: row.get(0)?,
                    generation_name: row.get(1)?,
                    store_log_sequence: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            },
        )
        .optional()
        .unwrap()
}

fn reader_count(layout: &StoreLayout) -> i64 {
    Connection::open(layout.coordinator_db())
        .unwrap()
        .query_row("SELECT COUNT(*) FROM reader_registrations", [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn log_sequences(layout: &StoreLayout) -> Vec<i64> {
    let store = Connection::open(layout.store_db()).unwrap();
    let mut statement = store
        .prepare("SELECT sequence FROM store_log ORDER BY sequence")
        .unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[derive(Clone, Copy)]
struct FixedClock;

impl MaintenanceClock for FixedClock {
    fn now_ms(&self) -> i64 {
        MAINTENANCE_NOW
    }
}

#[derive(Clone, Copy)]
struct FixedCapacity;

impl CapacityProvider for FixedCapacity {
    fn free_bytes(&self, _path: &Path) -> Result<u64, std::io::Error> {
        Ok(512 * 1024 * 1024)
    }

    fn staged_generation_bytes(&self, _path: &Path) -> Result<u64, std::io::Error> {
        Ok(1)
    }
}

struct TempStore {
    path: PathBuf,
}

impl TempStore {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "julie-reader-cursor-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
