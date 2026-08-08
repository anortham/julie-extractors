use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering as AtomicOrdering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use super::connection::compare_versions as compare_store_versions;
use super::pragmas::{WriterPragmaProfile, configure_writer_pragmas};
use super::{StoreConnectionError, StoreLayout, StoreLog, StoreLogEntry};

const STORE_WRITER_RESOURCE: &str = "store-writer";
const DEFAULT_LEASE_DURATION_MS: i64 = 5_000;
const DEFAULT_MAX_QUANTUM_MS: i64 = 4_000;
const DEFAULT_INTERACTIVE_BURST_COUNT: usize = 32;
const DEFAULT_INTERACTIVE_BURST_MS: i64 = 250;
const DEFAULT_SERVICE_WINDOW_MS: i64 = 1_000;
const COORDINATOR_BUSY_TIMEOUT: Duration = Duration::from_secs(5);
static LAST_FENCING_TOKEN: AtomicI64 = AtomicI64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Import,
    Update,
    Delete,
}

impl RequestKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }

    fn parse(value: &str) -> Result<Self, CoordinatorError> {
        match value {
            "import" => Ok(Self::Import),
            "update" => Ok(Self::Update),
            "delete" => Ok(Self::Delete),
            _ => Err(CoordinatorError::CorruptRequest {
                detail: format!("unknown request kind {value:?}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestState {
    Queued,
    Claimed,
    Committed,
    Acknowledged,
    Failed,
}

impl RequestState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Claimed => "claimed",
            Self::Committed => "committed",
            Self::Acknowledged => "acknowledged",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Result<Self, CoordinatorError> {
        match value {
            "queued" => Ok(Self::Queued),
            "claimed" => Ok(Self::Claimed),
            "committed" => Ok(Self::Committed),
            "acknowledged" => Ok(Self::Acknowledged),
            "failed" => Ok(Self::Failed),
            _ => Err(CoordinatorError::CorruptRequest {
                detail: format!("unknown request state {value:?}"),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorRequest {
    pub request_id: String,
    pub idempotency_key: String,
    pub kind: RequestKind,
    pub payload_json: String,
    pub state: RequestState,
    pub requester_id: String,
    pub requester_deadline: Option<i64>,
    pub claim_owner: Option<String>,
    pub claim_heartbeat_at: Option<i64>,
    pub terminal_log_sequence: Option<i64>,
    pub result_json: Option<String>,
    pub error_json: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl CoordinatorRequest {
    pub fn new(
        request_id: impl Into<String>,
        idempotency_key: impl Into<String>,
        kind: RequestKind,
        payload_json: impl Into<String>,
        requester_id: impl Into<String>,
        requester_deadline: i64,
        created_at: i64,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            idempotency_key: idempotency_key.into(),
            kind,
            payload_json: payload_json.into(),
            state: RequestState::Queued,
            requester_id: requester_id.into(),
            requester_deadline: Some(requester_deadline),
            claim_owner: None,
            claim_heartbeat_at: None,
            terminal_log_sequence: None,
            result_json: None,
            error_json: None,
            created_at,
            updated_at: created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnqueueResult {
    pub inserted: bool,
    pub request: CoordinatorRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseHolder {
    pub holder_id: String,
    pub holder_version: String,
    pub holder_pid: u32,
}

impl LeaseHolder {
    pub fn new(
        holder_id: impl Into<String>,
        holder_version: impl Into<String>,
        holder_pid: u32,
    ) -> Self {
        Self {
            holder_id: holder_id.into(),
            holder_version: holder_version.into(),
            holder_pid,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseDisposition {
    Acquired { fencing_token: i64 },
    HeldByOther,
}

impl LeaseDisposition {
    pub fn acquired(self) -> bool {
        matches!(self, Self::Acquired { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileOutcome {
    pub committed_in_fact: bool,
    pub next_chunk_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseRecord {
    pub holder: LeaseHolder,
    pub heartbeat_at: i64,
    pub expires_at: i64,
    pub fencing_token: i64,
}

pub trait UnixMillisClock: fmt::Debug + Send + Sync {
    fn now_ms(&self) -> i64;
}

#[derive(Debug)]
struct SystemClock;

impl UnixMillisClock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionContext {
    pub next_chunk_index: u64,
    pub fencing_token: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionQuantum {
    Progress {
        event_kind: String,
        payload_json: String,
        level: Option<super::StoreLevel>,
    },
    Complete {
        event_kind: String,
        result_json: String,
    },
}

pub trait CoordinatorExecutor {
    fn execute_quantum(
        &mut self,
        transaction: &Transaction<'_>,
        request: &CoordinatorRequest,
        context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorPolicy {
    pub interactive_burst_count: usize,
    pub interactive_burst_ms: i64,
    pub service_window_ms: i64,
    pub own_request_id: Option<String>,
    pub lease_duration_ms: i64,
    pub maximum_quantum_ms: i64,
}

impl Default for CoordinatorPolicy {
    fn default() -> Self {
        Self {
            interactive_burst_count: DEFAULT_INTERACTIVE_BURST_COUNT,
            interactive_burst_ms: DEFAULT_INTERACTIVE_BURST_MS,
            service_window_ms: DEFAULT_SERVICE_WINDOW_MS,
            own_request_id: None,
            lease_duration_ms: DEFAULT_LEASE_DURATION_MS,
            maximum_quantum_ms: DEFAULT_MAX_QUANTUM_MS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DrainReport {
    pub completed_requests: usize,
    pub failed_requests: usize,
    pub progress_quanta: usize,
    pub interactive_quanta: usize,
    pub batch_quanta: usize,
}

#[derive(Debug)]
pub enum CoordinatorError {
    InvalidRequest,
    IdempotencyConflict {
        idempotency_key: String,
        existing_request_id: String,
    },
    RequestIdConflict {
        request_id: String,
    },
    RequestNotFound {
        request_id: String,
    },
    CorruptRequest {
        detail: String,
    },
    CoordinatorAheadOfStore {
        request_id: String,
    },
    InvalidVersion {
        value: String,
    },
    InvalidTime {
        field: &'static str,
        value: i64,
    },
    WriterVersionTooOld {
        running: String,
        required: String,
    },
    LeaseUnavailable,
    LeaseLost,
    MissingLeaseHolder,
    ExecutionFailed {
        request_id: String,
        detail: String,
    },
    InvalidPolicy,
    QuantumDeadlineExceeded {
        elapsed_ms: i64,
        maximum_ms: i64,
    },
    Sqlite(rusqlite::Error),
    StoreLog(super::StoreLogError),
}

impl fmt::Display for CoordinatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest => write!(formatter, "coordinator request is invalid"),
            Self::IdempotencyConflict {
                idempotency_key,
                existing_request_id,
            } => write!(
                formatter,
                "idempotency key {idempotency_key:?} belongs to request {existing_request_id:?}"
            ),
            Self::RequestIdConflict { request_id } => {
                write!(formatter, "request id {request_id:?} is already in use")
            }
            Self::RequestNotFound { request_id } => {
                write!(
                    formatter,
                    "coordinator request {request_id:?} was not found"
                )
            }
            Self::CorruptRequest { detail } => {
                write!(formatter, "coordinator row is corrupt: {detail}")
            }
            Self::CoordinatorAheadOfStore { request_id } => write!(
                formatter,
                "coordinator request {request_id:?} is terminal without a terminal store effect"
            ),
            Self::InvalidVersion { value } => write!(formatter, "invalid version {value:?}"),
            Self::InvalidTime { field, value } => {
                write!(formatter, "invalid {field} timestamp {value}")
            }
            Self::WriterVersionTooOld { running, required } => write!(
                formatter,
                "writer version {running:?} is below required version {required:?}"
            ),
            Self::LeaseUnavailable => {
                write!(formatter, "store-writer lease is held by another process")
            }
            Self::LeaseLost => write!(formatter, "store-writer lease fencing check failed"),
            Self::MissingLeaseHolder => {
                write!(formatter, "coordinator has no configured lease holder")
            }
            Self::ExecutionFailed { request_id, detail } => {
                write!(formatter, "request {request_id:?} failed: {detail}")
            }
            Self::InvalidPolicy => write!(formatter, "coordinator policy is invalid"),
            Self::QuantumDeadlineExceeded {
                elapsed_ms,
                maximum_ms,
            } => write!(
                formatter,
                "coordinator quantum took {elapsed_ms} ms; maximum is {maximum_ms} ms"
            ),
            Self::Sqlite(error) => error.fmt(formatter),
            Self::StoreLog(error) => error.fmt(formatter),
        }
    }
}

impl Error for CoordinatorError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::StoreLog(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for CoordinatorError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<super::StoreLogError> for CoordinatorError {
    fn from(error: super::StoreLogError) -> Self {
        Self::StoreLog(error)
    }
}

pub struct StoreCoordinator {
    coordinator_db: PathBuf,
    store_db: PathBuf,
    pid_liveness: Arc<dyn PidLiveness>,
    clock: Arc<dyn UnixMillisClock>,
    holder: Option<LeaseHolder>,
    held_fencing_token: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PidStatus {
    Alive,
    Dead,
    Unknown,
}

pub trait PidLiveness: fmt::Debug + Send + Sync {
    fn status(&self, pid: u32) -> PidStatus;
}

#[derive(Debug)]
struct SystemPidLiveness;

impl PidLiveness for SystemPidLiveness {
    fn status(&self, pid: u32) -> PidStatus {
        if pid == std::process::id() {
            PidStatus::Alive
        } else {
            process_status(pid)
        }
    }
}

impl StoreCoordinator {
    pub fn open(layout: &StoreLayout) -> Result<Self, CoordinatorError> {
        Self::open_with_liveness(layout, SystemPidLiveness)
    }

    pub fn open_with_liveness(
        layout: &StoreLayout,
        pid_liveness: impl PidLiveness + 'static,
    ) -> Result<Self, CoordinatorError> {
        open_coordinator(layout.coordinator_db())?;
        Connection::open(layout.store_db())?;
        Ok(Self {
            coordinator_db: layout.coordinator_db().to_path_buf(),
            store_db: layout.store_db().to_path_buf(),
            pid_liveness: Arc::new(pid_liveness),
            clock: Arc::new(SystemClock),
            holder: None,
            held_fencing_token: None,
        })
    }

    pub fn open_with_runtime<C, L>(
        layout: &StoreLayout,
        holder: LeaseHolder,
        clock: Arc<C>,
        pid_liveness: Arc<L>,
    ) -> Result<Self, CoordinatorError>
    where
        C: UnixMillisClock + 'static,
        L: PidLiveness + 'static,
    {
        open_coordinator(layout.coordinator_db())?;
        Connection::open(layout.store_db())?;
        Ok(Self {
            coordinator_db: layout.coordinator_db().to_path_buf(),
            store_db: layout.store_db().to_path_buf(),
            pid_liveness,
            clock,
            holder: Some(holder),
            held_fencing_token: None,
        })
    }

    pub fn enqueue(
        &mut self,
        request: CoordinatorRequest,
    ) -> Result<EnqueueResult, CoordinatorError> {
        validate_request(&request)?;
        let mut connection = open_coordinator(&self.coordinator_db)?;
        let transaction = begin_coordinator(&mut connection)?;
        if request_by_id(&transaction, &request.request_id)?
            .is_some_and(|existing| existing.idempotency_key != request.idempotency_key)
        {
            return Err(CoordinatorError::RequestIdConflict {
                request_id: request.request_id,
            });
        }
        if let Some(existing) = request_by_idempotency(&transaction, &request.idempotency_key)? {
            if existing.kind == request.kind && existing.payload_json == request.payload_json {
                transaction.commit()?;
                return Ok(EnqueueResult {
                    inserted: false,
                    request: existing,
                });
            }
            return Err(CoordinatorError::IdempotencyConflict {
                idempotency_key: request.idempotency_key,
                existing_request_id: existing.request_id,
            });
        }
        transaction.execute(
            "INSERT INTO requests
             (request_id, idempotency_key, kind, payload_json, state, requester_id,
              requester_deadline, claim_owner, claim_heartbeat_at, terminal_log_sequence,
              result_json, error_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6, NULL, NULL, NULL, NULL, NULL, ?7, ?7)",
            params![
                request.request_id,
                request.idempotency_key,
                request.kind.as_str(),
                request.payload_json,
                request.requester_id,
                request.requester_deadline,
                request.created_at,
            ],
        )?;
        transaction.commit()?;
        Ok(EnqueueResult {
            inserted: true,
            request,
        })
    }

    pub fn try_acquire_or_takeover(
        &mut self,
        holder: LeaseHolder,
        now: i64,
    ) -> Result<LeaseDisposition, CoordinatorError> {
        if holder.holder_id.is_empty() || holder.holder_version.is_empty() || holder.holder_pid == 0
        {
            return Err(CoordinatorError::InvalidRequest);
        }
        self.ensure_writer_eligible(&holder.holder_version)?;
        let mut connection = open_coordinator(&self.coordinator_db)?;
        let transaction = begin_coordinator(&mut connection)?;
        let existing = transaction
            .query_row(
                "SELECT holder_id, holder_pid, expires_at, fencing_token FROM writer_lease
                 WHERE resource = ?1",
                [STORE_WRITER_RESOURCE],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        let expires_at = checked_lease_expiry(now, DEFAULT_LEASE_DURATION_MS)?;
        let disposition = match existing {
            None => {
                let fencing_token = allocate_fencing_token(now.max(1))?;
                transaction.execute(
                    "INSERT INTO writer_lease
                     (resource, holder_id, holder_version, holder_pid, heartbeat_at, expires_at, fencing_token)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        STORE_WRITER_RESOURCE,
                        holder.holder_id,
                        holder.holder_version,
                        holder.holder_pid,
                        now,
                        expires_at,
                        fencing_token,
                    ],
                )?;
                LeaseDisposition::Acquired { fencing_token }
            }
            Some((holder_id, old_pid, old_expiry, fencing_token))
                if holder_id == holder.holder_id
                    && old_pid == holder.holder_pid
                    && old_expiry > now
                    && self.held_fencing_token == Some(fencing_token) =>
            {
                transaction.execute(
                    "UPDATE writer_lease SET holder_version = ?1, holder_pid = ?2,
                     heartbeat_at = ?3, expires_at = ?4 WHERE resource = ?5 AND fencing_token = ?6",
                    params![
                        holder.holder_version,
                        holder.holder_pid,
                        now,
                        expires_at,
                        STORE_WRITER_RESOURCE,
                        fencing_token,
                    ],
                )?;
                LeaseDisposition::Acquired { fencing_token }
            }
            Some((old_holder_id, old_pid, old_expiry, fencing_token))
                if old_expiry <= now || self.pid_liveness.status(old_pid) == PidStatus::Dead =>
            {
                let minimum_token = fencing_token
                    .checked_add(1)
                    .map(|token| token.max(now.max(1)))
                    .ok_or_else(|| CoordinatorError::CorruptRequest {
                        detail: "writer lease fencing token overflow".to_string(),
                    })?;
                let next_token = allocate_fencing_token(minimum_token)?;
                transaction.execute(
                    "UPDATE writer_lease SET holder_id = ?1, holder_version = ?2,
                     holder_pid = ?3, heartbeat_at = ?4, expires_at = ?5, fencing_token = ?6
                     WHERE resource = ?7 AND fencing_token = ?8",
                    params![
                        holder.holder_id,
                        holder.holder_version,
                        holder.holder_pid,
                        now,
                        expires_at,
                        next_token,
                        STORE_WRITER_RESOURCE,
                        fencing_token,
                    ],
                )?;
                transaction.execute(
                    "UPDATE requests SET claim_owner = ?1, claim_heartbeat_at = ?2, updated_at = ?2
                     WHERE state = 'claimed' AND claim_owner = ?3",
                    params![holder.holder_id, now, old_holder_id],
                )?;
                LeaseDisposition::Acquired {
                    fencing_token: next_token,
                }
            }
            Some(_) => LeaseDisposition::HeldByOther,
        };
        transaction.commit()?;
        if let LeaseDisposition::Acquired { fencing_token } = disposition {
            self.held_fencing_token = Some(fencing_token);
        }
        Ok(disposition)
    }

    pub fn release_lease(
        &mut self,
        holder: &LeaseHolder,
        fencing_token: i64,
    ) -> Result<bool, CoordinatorError> {
        let connection = open_coordinator(&self.coordinator_db)?;
        let released = connection.execute(
            "DELETE FROM writer_lease
             WHERE resource = ?1 AND holder_id = ?2 AND holder_pid = ?3 AND fencing_token = ?4",
            params![
                STORE_WRITER_RESOURCE,
                holder.holder_id,
                holder.holder_pid,
                fencing_token,
            ],
        )? == 1;
        if released && self.held_fencing_token == Some(fencing_token) {
            self.held_fencing_token = None;
        }
        Ok(released)
    }

    fn release_lease_for(
        &mut self,
        holder: &LeaseHolder,
        fencing_token: i64,
    ) -> Result<bool, CoordinatorError> {
        let released = release_lease_at(&self.coordinator_db, holder, fencing_token)?;
        if released && self.held_fencing_token == Some(fencing_token) {
            self.held_fencing_token = None;
        }
        Ok(released)
    }

    pub fn heartbeat_lease(
        &mut self,
        holder: &LeaseHolder,
        fencing_token: i64,
        now: i64,
    ) -> Result<bool, CoordinatorError> {
        self.heartbeat_lease_for(holder, fencing_token, now, DEFAULT_LEASE_DURATION_MS)
    }

    fn heartbeat_lease_for(
        &mut self,
        holder: &LeaseHolder,
        fencing_token: i64,
        now: i64,
        lease_duration_ms: i64,
    ) -> Result<bool, CoordinatorError> {
        let expires_at = checked_lease_expiry(now, lease_duration_ms)?;
        let connection = open_coordinator(&self.coordinator_db)?;
        Ok(connection.execute(
            "UPDATE writer_lease SET heartbeat_at = ?1, expires_at = ?2
             WHERE resource = ?3 AND holder_id = ?4 AND holder_pid = ?5 AND fencing_token = ?6
               AND expires_at > ?1",
            params![
                now,
                expires_at,
                STORE_WRITER_RESOURCE,
                holder.holder_id,
                holder.holder_pid,
                fencing_token,
            ],
        )? == 1)
    }

    pub fn lease(&self) -> Result<Option<LeaseRecord>, CoordinatorError> {
        let connection = open_coordinator(&self.coordinator_db)?;
        Ok(connection
            .query_row(
                "SELECT holder_id, holder_version, holder_pid, heartbeat_at, expires_at,
                        fencing_token FROM writer_lease WHERE resource = ?1",
                [STORE_WRITER_RESOURCE],
                |row| {
                    Ok(LeaseRecord {
                        holder: LeaseHolder {
                            holder_id: row.get(0)?,
                            holder_version: row.get(1)?,
                            holder_pid: row.get(2)?,
                        },
                        heartbeat_at: row.get(3)?,
                        expires_at: row.get(4)?,
                        fencing_token: row.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn drain(
        &mut self,
        executor: &mut dyn CoordinatorExecutor,
        policy: &CoordinatorPolicy,
    ) -> Result<DrainReport, CoordinatorError> {
        if policy.interactive_burst_count == 0
            || policy.interactive_burst_ms <= 0
            || policy.service_window_ms < 0
            || policy.maximum_quantum_ms <= 0
            || policy.lease_duration_ms <= policy.maximum_quantum_ms
        {
            return Err(CoordinatorError::InvalidPolicy);
        }
        let holder = self
            .holder
            .clone()
            .ok_or(CoordinatorError::MissingLeaseHolder)?;
        let started_at = self.clock.now_ms();
        let lease = self.try_acquire_or_takeover(holder.clone(), started_at)?;
        let LeaseDisposition::Acquired { fencing_token } = lease else {
            return Err(CoordinatorError::LeaseUnavailable);
        };
        let mut guard = LeaseReleaseGuard {
            coordinator_db: self.coordinator_db.clone(),
            holder: holder.clone(),
            fencing_token,
            armed: true,
        };
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.drain_acquired(executor, policy, &holder, fencing_token, started_at)
        }));
        match result {
            Ok(result) => {
                let release_result = self.release_lease_for(&holder, fencing_token);
                if release_result.is_ok() {
                    guard.disarm();
                }
                release_result?;
                result
            }
            Err(payload) => {
                drop(guard);
                resume_unwind(payload)
            }
        }
    }

    fn drain_acquired(
        &mut self,
        executor: &mut dyn CoordinatorExecutor,
        policy: &CoordinatorPolicy,
        holder: &LeaseHolder,
        fencing_token: i64,
        started_at: i64,
    ) -> Result<DrainReport, CoordinatorError> {
        let own_only = policy
            .own_request_id
            .clone()
            .into_iter()
            .collect::<Vec<_>>();
        let awaiting_own_terminal = if let Some(request_id) = policy.own_request_id.as_ref() {
            let request = self.request(request_id)?;
            !matches!(
                request.state,
                RequestState::Committed | RequestState::Acknowledged | RequestState::Failed
            )
        } else {
            false
        };
        let mut awaiting_own_terminal = awaiting_own_terminal;
        let mut backlog_remaining = if awaiting_own_terminal {
            Vec::new()
        } else {
            self.pending_request_ids()?
        };
        let mut service_deadline = if awaiting_own_terminal {
            None
        } else {
            Some(checked_service_deadline(
                started_at,
                policy.service_window_ms,
            )?)
        };
        let mut report = DrainReport::default();
        let mut interactive_in_burst = 0usize;
        let mut burst_started_at = started_at;
        loop {
            backlog_remaining.retain(|request_id| {
                self.request(request_id).is_ok_and(|request| {
                    matches!(request.state, RequestState::Queued | RequestState::Claimed)
                })
            });
            let now = self.clock.now_ms();
            let allowed_ids = if awaiting_own_terminal {
                Some(&own_only)
            } else if backlog_remaining.is_empty() {
                if service_deadline.is_some_and(|deadline| now >= deadline) {
                    break;
                }
                None
            } else {
                Some(&backlog_remaining)
            };
            let force_batch = interactive_in_burst >= policy.interactive_burst_count
                || now.saturating_sub(burst_started_at) >= policy.interactive_burst_ms;
            let candidate = self.next_pending_request(
                allowed_ids,
                force_batch,
                &holder.holder_id,
                now,
                policy.lease_duration_ms,
            )?;
            let Some(request) = candidate else {
                if force_batch {
                    interactive_in_burst = 0;
                    burst_started_at = now;
                    if self
                        .next_pending_request(
                            allowed_ids,
                            false,
                            &holder.holder_id,
                            now,
                            policy.lease_duration_ms,
                        )?
                        .is_some()
                    {
                        continue;
                    }
                }
                break;
            };
            let is_batch = request.kind == RequestKind::Import;
            match self.execute_request_quantum(
                executor,
                policy,
                holder,
                fencing_token,
                request.clone(),
            ) {
                Ok(())
                | Err(CoordinatorError::ExecutionFailed { .. })
                | Err(CoordinatorError::QuantumDeadlineExceeded { .. }) => {}
                Err(error) => return Err(error),
            }
            if is_batch {
                report.batch_quanta += 1;
                interactive_in_burst = 0;
                burst_started_at = self.clock.now_ms();
            } else {
                report.interactive_quanta += 1;
                interactive_in_burst += 1;
            }
            let state = self.request(&request.request_id)?.state;
            match state {
                RequestState::Committed | RequestState::Acknowledged => {
                    report.completed_requests += 1;
                    backlog_remaining.retain(|request_id| request_id != &request.request_id);
                }
                RequestState::Failed => {
                    report.failed_requests += 1;
                    backlog_remaining.retain(|request_id| request_id != &request.request_id);
                }
                RequestState::Queued | RequestState::Claimed => report.progress_quanta += 1,
            }
            if awaiting_own_terminal
                && policy
                    .own_request_id
                    .as_ref()
                    .is_some_and(|request_id| request_id == &request.request_id)
                && matches!(
                    state,
                    RequestState::Committed | RequestState::Acknowledged | RequestState::Failed
                )
            {
                awaiting_own_terminal = false;
                backlog_remaining = self.pending_request_ids()?;
                service_deadline = Some(checked_service_deadline(
                    self.clock.now_ms(),
                    policy.service_window_ms,
                )?);
            }
        }
        Ok(report)
    }

    fn execute_request_quantum(
        &mut self,
        executor: &mut dyn CoordinatorExecutor,
        policy: &CoordinatorPolicy,
        holder: &LeaseHolder,
        fencing_token: i64,
        request: CoordinatorRequest,
    ) -> Result<(), CoordinatorError> {
        let reconciliation = self.reconcile(&request.request_id)?;
        if reconciliation.committed_in_fact {
            return Ok(());
        }
        let now = self.clock.now_ms();
        if !self.heartbeat_lease_for(holder, fencing_token, now, policy.lease_duration_ms)? {
            return Err(CoordinatorError::LeaseLost);
        }
        if !self.claim_request(
            &request.request_id,
            holder,
            fencing_token,
            now,
            policy.lease_duration_ms,
        )? {
            return Ok(());
        }
        let mut store = Connection::open(&self.store_db)?;
        configure_writer_pragmas(&store, WriterPragmaProfile::Bulk).map_err(|error| {
            CoordinatorError::CorruptRequest {
                detail: format!("store writer pragma configuration failed: {error:?}"),
            }
        })?;
        let transaction = store.transaction()?;
        let context = ExecutionContext {
            next_chunk_index: reconciliation.next_chunk_index,
            fencing_token,
        };
        let quantum_started_at = self.clock.now_ms();
        let quantum = match executor.execute_quantum(&transaction, &request, context) {
            Ok(quantum) => quantum,
            Err(detail) => {
                drop(transaction);
                if !self.fail_request(
                    &request.request_id,
                    &detail,
                    holder,
                    fencing_token,
                    self.clock.now_ms(),
                )? {
                    return Err(CoordinatorError::LeaseLost);
                }
                return Err(CoordinatorError::ExecutionFailed {
                    request_id: request.request_id,
                    detail,
                });
            }
        };
        let quantum_finished_at = self.clock.now_ms();
        let elapsed_ms = quantum_finished_at.saturating_sub(quantum_started_at);
        if elapsed_ms > policy.maximum_quantum_ms {
            drop(transaction);
            if !self.fail_request(
                &request.request_id,
                "coordinator quantum exceeded its lease-safe bound",
                holder,
                fencing_token,
                quantum_finished_at,
            )? {
                return Err(CoordinatorError::LeaseLost);
            }
            return Err(CoordinatorError::QuantumDeadlineExceeded {
                elapsed_ms,
                maximum_ms: policy.maximum_quantum_ms,
            });
        }
        let completed = match quantum {
            ExecutionQuantum::Progress {
                event_kind,
                payload_json,
                level,
            } => {
                let created_at = sqlite_rfc3339(&transaction, self.clock.now_ms())?;
                let mut entry =
                    StoreLogEntry::new(&request.request_id, event_kind, payload_json, created_at);
                if let Some(level) = level {
                    entry = entry.with_level(level);
                }
                StoreLog::append_progress(&transaction, &entry, reconciliation.next_chunk_index)?;
                false
            }
            ExecutionQuantum::Complete {
                event_kind,
                result_json,
            } => {
                let created_at = sqlite_rfc3339(&transaction, self.clock.now_ms())?;
                let entry =
                    StoreLogEntry::new(&request.request_id, event_kind, result_json, created_at);
                StoreLog::append_terminal(&transaction, &entry)?;
                true
            }
        };
        let now = self.clock.now_ms();
        if !self.heartbeat_lease_for(holder, fencing_token, now, policy.lease_duration_ms)? {
            return Err(CoordinatorError::LeaseLost);
        }
        transaction.commit()?;
        let now = self.clock.now_ms();
        if !self.heartbeat_lease_for(holder, fencing_token, now, policy.lease_duration_ms)? {
            return Err(CoordinatorError::LeaseLost);
        }
        if completed {
            self.reconcile(&request.request_id)?;
        } else {
            let connection = open_coordinator(&self.coordinator_db)?;
            let changed = connection.execute(
                "UPDATE requests SET claim_heartbeat_at = ?1, updated_at = ?1
                 WHERE request_id = ?2 AND state = 'claimed' AND claim_owner = ?3
                   AND EXISTS (
                     SELECT 1 FROM writer_lease
                     WHERE resource = ?4 AND holder_id = ?3 AND holder_pid = ?5
                       AND fencing_token = ?6 AND expires_at > ?1
                   )",
                params![
                    now,
                    request.request_id,
                    holder.holder_id,
                    STORE_WRITER_RESOURCE,
                    holder.holder_pid,
                    fencing_token,
                ],
            )?;
            if changed != 1 {
                return Err(CoordinatorError::LeaseLost);
            }
        }
        Ok(())
    }

    fn pending_request_ids(&self) -> Result<Vec<String>, CoordinatorError> {
        let connection = open_coordinator(&self.coordinator_db)?;
        let mut statement = connection.prepare(
            "SELECT request_id FROM requests WHERE state IN ('queued', 'claimed')
             ORDER BY created_at, request_id",
        )?;
        let rows = statement
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(rows)
    }

    fn next_pending_request(
        &self,
        allowed_ids: Option<&Vec<String>>,
        force_batch: bool,
        holder_id: &str,
        now: i64,
        lease_duration_ms: i64,
    ) -> Result<Option<CoordinatorRequest>, CoordinatorError> {
        let connection = open_coordinator(&self.coordinator_db)?;
        let mut statement = connection.prepare(
            "SELECT request_id
             FROM requests WHERE state IN ('queued', 'claimed') ORDER BY created_at, request_id",
        )?;
        let request_ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut interactive = None;
        let mut batch = None;
        for request_id in request_ids {
            let request = self.request(&request_id)?;
            let claim_is_eligible = request.state == RequestState::Queued
                || request.claim_owner.as_deref() == Some(holder_id)
                || request
                    .claim_heartbeat_at
                    .is_some_and(|heartbeat| heartbeat <= now.saturating_sub(lease_duration_ms));
            if !claim_is_eligible {
                continue;
            }
            if allowed_ids.is_some_and(|ids| !ids.contains(&request.request_id)) {
                continue;
            }
            if request.kind == RequestKind::Import {
                batch.get_or_insert(request);
            } else {
                interactive.get_or_insert(request);
            }
        }
        Ok(if force_batch {
            batch.or(interactive)
        } else {
            interactive.or(batch)
        })
    }

    fn claim_request(
        &self,
        request_id: &str,
        holder: &LeaseHolder,
        fencing_token: i64,
        now: i64,
        lease_duration_ms: i64,
    ) -> Result<bool, CoordinatorError> {
        let connection = open_coordinator(&self.coordinator_db)?;
        Ok(connection.execute(
            "UPDATE requests SET state = 'claimed', claim_owner = ?1,
             claim_heartbeat_at = ?2, updated_at = ?2
             WHERE request_id = ?3 AND (
               state = 'queued' OR (state = 'claimed' AND (
                 claim_owner = ?1 OR claim_heartbeat_at <= ?4
               ))
             ) AND EXISTS (
               SELECT 1 FROM writer_lease
               WHERE resource = ?5 AND holder_id = ?1 AND holder_pid = ?6
                 AND fencing_token = ?7 AND expires_at > ?2
             )",
            params![
                holder.holder_id,
                now,
                request_id,
                now.saturating_sub(lease_duration_ms),
                STORE_WRITER_RESOURCE,
                holder.holder_pid,
                fencing_token,
            ],
        )? == 1)
    }

    fn fail_request(
        &self,
        request_id: &str,
        detail: &str,
        holder: &LeaseHolder,
        fencing_token: i64,
        now: i64,
    ) -> Result<bool, CoordinatorError> {
        let error_json = serde_json::json!({ "message": detail }).to_string();
        let connection = open_coordinator(&self.coordinator_db)?;
        Ok(connection.execute(
            "UPDATE requests SET state = 'failed', claim_owner = NULL,
             claim_heartbeat_at = NULL, result_json = NULL, error_json = ?1, updated_at = ?2
             WHERE request_id = ?3 AND state = 'claimed' AND claim_owner = ?4
               AND EXISTS (
                 SELECT 1 FROM writer_lease
                 WHERE resource = ?5 AND holder_id = ?4 AND holder_pid = ?6
                   AND fencing_token = ?7 AND expires_at > ?2
               )",
            params![
                error_json,
                now,
                request_id,
                holder.holder_id,
                STORE_WRITER_RESOURCE,
                holder.holder_pid,
                fencing_token,
            ],
        )? == 1)
    }

    pub fn reconcile(&mut self, request_id: &str) -> Result<ReconcileOutcome, CoordinatorError> {
        let store = Connection::open(&self.store_db)?;
        let terminal = StoreLog::committed_in_fact(&store, request_id)?;
        let next_chunk_index = store.query_row(
            "SELECT COALESCE(MAX(chunk_index), -1) + 1 FROM request_chunks WHERE request_id = ?1",
            [request_id],
            |row| row.get::<_, i64>(0),
        )?;
        drop(store);
        let mut connection = open_coordinator(&self.coordinator_db)?;
        let transaction = begin_coordinator(&mut connection)?;
        let coordinator_request = request_by_id(&transaction, request_id)?.ok_or_else(|| {
            CoordinatorError::RequestNotFound {
                request_id: request_id.to_string(),
            }
        })?;
        if terminal.is_none()
            && matches!(
                coordinator_request.state,
                RequestState::Committed | RequestState::Acknowledged
            )
        {
            return Err(CoordinatorError::CoordinatorAheadOfStore {
                request_id: request_id.to_string(),
            });
        }
        if let Some(ref terminal) = terminal
            && matches!(
                coordinator_request.state,
                RequestState::Committed | RequestState::Acknowledged
            )
            && (coordinator_request.terminal_log_sequence != Some(terminal.sequence)
                || coordinator_request.result_json.as_deref()
                    != Some(terminal.payload_json.as_str()))
        {
            return Err(CoordinatorError::CorruptRequest {
                detail: format!(
                    "request {request_id:?} terminal coordinator fields do not match store_log"
                ),
            });
        }
        if let Some(ref terminal) = terminal {
            let changed = transaction.execute(
                "UPDATE requests SET state = 'committed', claim_owner = NULL,
                 claim_heartbeat_at = NULL, terminal_log_sequence = ?1,
                 result_json = ?2, error_json = NULL, updated_at = ?3
                 WHERE request_id = ?4 AND state IN ('queued', 'claimed', 'failed')",
                params![
                    terminal.sequence,
                    terminal.payload_json,
                    self.clock.now_ms(),
                    request_id,
                ],
            )?;
            if changed == 0
                && !matches!(
                    request_by_id(&transaction, request_id)?
                        .ok_or_else(|| CoordinatorError::RequestNotFound {
                            request_id: request_id.to_string(),
                        })?
                        .state,
                    RequestState::Committed | RequestState::Acknowledged
                )
            {
                return Err(CoordinatorError::CorruptRequest {
                    detail: format!(
                        "request {request_id:?} cannot reconcile a terminal store effect"
                    ),
                });
            }
        }
        transaction.commit()?;
        Ok(ReconcileOutcome {
            committed_in_fact: terminal.is_some(),
            next_chunk_index: u64::try_from(next_chunk_index).map_err(|_| {
                CoordinatorError::CorruptRequest {
                    detail: format!("request {request_id:?} has an invalid chunk index"),
                }
            })?,
        })
    }

    pub fn request(&self, request_id: &str) -> Result<CoordinatorRequest, CoordinatorError> {
        let connection = open_coordinator(&self.coordinator_db)?;
        request_by_id(&connection, request_id)?.ok_or_else(|| CoordinatorError::RequestNotFound {
            request_id: request_id.to_string(),
        })
    }

    pub fn acknowledge(&mut self, request_id: &str, now: i64) -> Result<bool, CoordinatorError> {
        if now < 0 {
            return Err(CoordinatorError::InvalidTime {
                field: "acknowledged_at",
                value: now,
            });
        }
        let mut connection = open_coordinator(&self.coordinator_db)?;
        let transaction = begin_coordinator(&mut connection)?;
        let changed = transaction.execute(
            "UPDATE requests SET state = 'acknowledged', updated_at = ?1
             WHERE request_id = ?2 AND state = 'committed'
               AND (requester_deadline IS NULL OR requester_deadline >= ?1)",
            params![now, request_id],
        )?;
        if changed == 1 {
            transaction.commit()?;
            Ok(true)
        } else {
            request_by_id(&transaction, request_id)?.ok_or_else(|| {
                CoordinatorError::RequestNotFound {
                    request_id: request_id.to_string(),
                }
            })?;
            transaction.commit()?;
            Ok(false)
        }
    }

    fn ensure_writer_eligible(&self, running: &str) -> Result<(), CoordinatorError> {
        let store = Connection::open(&self.store_db)?;
        let min_writer_version = store.query_row(
            "SELECT value FROM store_meta WHERE key = 'min_writer_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let binary_version = store.query_row(
            "SELECT value FROM store_meta WHERE key = 'binary_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let required = if compare_versions(&min_writer_version, &binary_version)? == Ordering::Less
        {
            binary_version
        } else {
            min_writer_version
        };
        if compare_versions(running, &required)? == Ordering::Less {
            Err(CoordinatorError::WriterVersionTooOld {
                running: running.to_string(),
                required,
            })
        } else {
            Ok(())
        }
    }
}

pub fn compare_versions(left: &str, right: &str) -> Result<Ordering, CoordinatorError> {
    compare_store_versions(left, right).map_err(|error| match error {
        StoreConnectionError::InvalidVersion { value, .. } => {
            CoordinatorError::InvalidVersion { value }
        }
        other => CoordinatorError::CorruptRequest {
            detail: other.to_string(),
        },
    })
}

#[cfg(unix)]
fn process_status(pid: u32) -> PidStatus {
    let kill_status = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match kill_status {
        Ok(status) if status.success() => return PidStatus::Alive,
        Err(_) => return PidStatus::Unknown,
        Ok(_) => {}
    }
    match std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
    {
        Ok(output)
            if output.status.success() && output.stdout.iter().all(u8::is_ascii_whitespace) =>
        {
            PidStatus::Dead
        }
        Ok(output) if output.status.success() => PidStatus::Unknown,
        _ => PidStatus::Unknown,
    }
}

#[cfg(not(unix))]
fn process_status(_pid: u32) -> PidStatus {
    PidStatus::Unknown
}

fn validate_request(request: &CoordinatorRequest) -> Result<(), CoordinatorError> {
    if request.created_at < 0 {
        return Err(CoordinatorError::InvalidTime {
            field: "created_at",
            value: request.created_at,
        });
    }
    if request.updated_at < 0 {
        return Err(CoordinatorError::InvalidTime {
            field: "updated_at",
            value: request.updated_at,
        });
    }
    if let Some(deadline) = request.requester_deadline
        && deadline < 0
    {
        return Err(CoordinatorError::InvalidTime {
            field: "requester_deadline",
            value: deadline,
        });
    }
    if request.request_id.is_empty()
        || request.idempotency_key.is_empty()
        || request.requester_id.is_empty()
        || request.state != RequestState::Queued
        || serde_json::from_str::<serde_json::Value>(&request.payload_json).is_err()
    {
        Err(CoordinatorError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn checked_lease_expiry(now: i64, lease_duration_ms: i64) -> Result<i64, CoordinatorError> {
    if now < 0 || lease_duration_ms <= 0 {
        return Err(CoordinatorError::InvalidTime {
            field: "lease_expiry",
            value: now,
        });
    }
    now.checked_add(lease_duration_ms)
        .ok_or(CoordinatorError::InvalidTime {
            field: "lease_expiry",
            value: now,
        })
}

fn allocate_fencing_token(minimum: i64) -> Result<i64, CoordinatorError> {
    let mut current = LAST_FENCING_TOKEN.load(AtomicOrdering::SeqCst);
    loop {
        let next = current
            .checked_add(1)
            .map(|candidate| candidate.max(minimum))
            .ok_or_else(|| CoordinatorError::CorruptRequest {
                detail: "writer lease fencing token overflow".to_string(),
            })?;
        match LAST_FENCING_TOKEN.compare_exchange(
            current,
            next,
            AtomicOrdering::SeqCst,
            AtomicOrdering::SeqCst,
        ) {
            Ok(_) => return Ok(next),
            Err(observed) => current = observed,
        }
    }
}

fn checked_service_deadline(now: i64, service_window_ms: i64) -> Result<i64, CoordinatorError> {
    now.checked_add(service_window_ms)
        .ok_or(CoordinatorError::InvalidTime {
            field: "service_deadline",
            value: now,
        })
}

fn request_by_idempotency(
    connection: &Connection,
    idempotency_key: &str,
) -> Result<Option<CoordinatorRequest>, CoordinatorError> {
    query_request(
        connection,
        "SELECT request_id, idempotency_key, kind, payload_json, state, requester_id,
                requester_deadline, claim_owner, claim_heartbeat_at, terminal_log_sequence,
                result_json, error_json, created_at, updated_at
         FROM requests WHERE idempotency_key = ?1",
        idempotency_key,
    )
}

fn request_by_id(
    connection: &Connection,
    request_id: &str,
) -> Result<Option<CoordinatorRequest>, CoordinatorError> {
    query_request(
        connection,
        "SELECT request_id, idempotency_key, kind, payload_json, state, requester_id,
                requester_deadline, claim_owner, claim_heartbeat_at, terminal_log_sequence,
                result_json, error_json, created_at, updated_at
         FROM requests WHERE request_id = ?1",
        request_id,
    )
}

fn query_request(
    connection: &Connection,
    sql: &str,
    parameter: &str,
) -> Result<Option<CoordinatorRequest>, CoordinatorError> {
    let row = connection
        .query_row(sql, [parameter], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, Option<i64>>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, i64>(12)?,
                row.get::<_, i64>(13)?,
            ))
        })
        .optional()?;
    row.map(
        |(
            request_id,
            idempotency_key,
            kind,
            payload_json,
            state,
            requester_id,
            requester_deadline,
            claim_owner,
            claim_heartbeat_at,
            terminal_log_sequence,
            result_json,
            error_json,
            created_at,
            updated_at,
        )| {
            Ok(CoordinatorRequest {
                request_id,
                idempotency_key,
                kind: RequestKind::parse(&kind)?,
                payload_json,
                state: RequestState::parse(&state)?,
                requester_id,
                requester_deadline,
                claim_owner,
                claim_heartbeat_at,
                terminal_log_sequence,
                result_json,
                error_json,
                created_at,
                updated_at,
            })
        },
    )
    .transpose()
}

struct LeaseReleaseGuard {
    coordinator_db: PathBuf,
    holder: LeaseHolder,
    fencing_token: i64,
    armed: bool,
}

impl LeaseReleaseGuard {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for LeaseReleaseGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = release_lease_at(&self.coordinator_db, &self.holder, self.fencing_token);
        }
    }
}

fn sqlite_rfc3339(transaction: &Transaction<'_>, unix_ms: i64) -> Result<String, CoordinatorError> {
    let seconds = unix_ms.div_euclid(1_000);
    let milliseconds = unix_ms.rem_euclid(1_000);
    let base = transaction.query_row(
        "SELECT strftime('%Y-%m-%dT%H:%M:%S', ?1, 'unixepoch')",
        [seconds],
        |row| row.get::<_, String>(0),
    )?;
    Ok(format!("{base}.{milliseconds:03}Z"))
}

fn open_coordinator(path: &Path) -> Result<Connection, CoordinatorError> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(COORDINATOR_BUSY_TIMEOUT)?;
    Ok(connection)
}

fn begin_coordinator(connection: &mut Connection) -> Result<Transaction<'_>, CoordinatorError> {
    Ok(connection.transaction_with_behavior(TransactionBehavior::Immediate)?)
}

fn release_lease_at(
    coordinator_db: &Path,
    holder: &LeaseHolder,
    fencing_token: i64,
) -> Result<bool, CoordinatorError> {
    let connection = open_coordinator(coordinator_db)?;
    Ok(connection.execute(
        "DELETE FROM writer_lease
         WHERE resource = ?1 AND holder_id = ?2 AND holder_pid = ?3 AND fencing_token = ?4",
        params![
            STORE_WRITER_RESOURCE,
            holder.holder_id,
            holder.holder_pid,
            fencing_token,
        ],
    )? == 1)
}
