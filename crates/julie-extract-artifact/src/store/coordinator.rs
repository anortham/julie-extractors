use std::cmp::Ordering;
use std::error::Error;
use std::fmt;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering as AtomicOrdering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use super::connection::{
    compare_versions as compare_store_versions, extractor_downgrade_allowed,
    required_writer_version, system_now_ms,
};
use super::layout::valid_generation_name;
use super::pragmas::{WriterPragmaProfile, configure_writer_pragmas};
use super::{
    GenerationFence, StoreConnectionError, StoreConnectionFactory, StoreLayout, StoreLog,
    StoreLogEntry,
};

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
    Resolve,
    Export,
    FromArtifact,
}

impl RequestKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Resolve => "resolve",
            Self::Export => "export",
            Self::FromArtifact => "from_artifact",
        }
    }

    fn is_batch(self) -> bool {
        matches!(self, Self::Import | Self::FromArtifact)
    }

    fn permits_renewable_quantum(self) -> bool {
        matches!(self, Self::FromArtifact)
    }

    fn parse(value: &str) -> Result<Self, CoordinatorError> {
        match value {
            "import" => Ok(Self::Import),
            "update" => Ok(Self::Update),
            "delete" => Ok(Self::Delete),
            "resolve" => Ok(Self::Resolve),
            "export" => Ok(Self::Export),
            "from_artifact" => Ok(Self::FromArtifact),
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
pub struct RequestReceipt {
    pub request_id: String,
    pub idempotency_key: String,
    pub kind: RequestKind,
    pub payload_json: String,
    pub terminal_result_json: String,
    pub terminal_generation_name: String,
    pub terminal_log_sequence: i64,
    pub completed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerCursor {
    pub consumer_id: String,
    pub generation_name: String,
    pub store_log_sequence: i64,
    pub updated_at: i64,
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

/// Live maintenance_intent identity fields used for lease authority decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentIdentity {
    pub run_id: String,
    pub owner_id: String,
    pub owner_pid: u32,
    pub fencing_token: i64,
}

/// Explicit maintenance-owner proof required to acquire under a live intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceOwnerFence {
    pub run_id: String,
    pub owner_id: String,
    pub owner_pid: u32,
    pub fencing_token: i64,
}

impl MaintenanceOwnerFence {
    fn matches_intent(&self, intent: &IntentIdentity) -> bool {
        self.run_id == intent.run_id
            && self.owner_id == intent.owner_id
            && self.owner_pid == intent.owner_pid
            && self.fencing_token == intent.fencing_token
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
    CursorRegression {
        consumer_id: String,
        current: i64,
        requested: i64,
    },
    CursorAhead {
        requested: i64,
        high_water: i64,
    },
    CursorGenerationConflict {
        consumer_id: String,
        current: String,
        requested: String,
    },
    InvalidGeneration {
        generation_name: String,
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
    StoreConnection(StoreConnectionError),
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
            Self::CursorRegression {
                consumer_id,
                current,
                requested,
            } => write!(
                formatter,
                "consumer cursor {consumer_id:?} cannot regress from {current} to {requested}"
            ),
            Self::CursorAhead {
                requested,
                high_water,
            } => write!(
                formatter,
                "consumer cursor sequence {requested} exceeds store-log high-water {high_water}"
            ),
            Self::CursorGenerationConflict {
                consumer_id,
                current,
                requested,
            } => write!(
                formatter,
                "consumer cursor {consumer_id:?} belongs to generation {current:?}, not {requested:?}"
            ),
            Self::InvalidGeneration { generation_name } => {
                write!(
                    formatter,
                    "invalid or missing generation {generation_name:?}"
                )
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
            Self::StoreConnection(error) => error.fmt(formatter),
        }
    }
}

impl Error for CoordinatorError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::StoreLog(error) => Some(error),
            Self::StoreConnection(error) => Some(error),
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

impl From<StoreConnectionError> for CoordinatorError {
    fn from(error: StoreConnectionError) -> Self {
        Self::StoreConnection(error)
    }
}

pub struct StoreCoordinator {
    layout: StoreLayout,
    family_id: String,
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
        let family_id = coordinator_store_family(layout)?;
        Ok(Self {
            layout: layout.clone(),
            family_id,
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
        let family_id = coordinator_store_family(layout)?;
        Ok(Self {
            layout: layout.clone(),
            family_id,
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
        if receipt_by_id(&transaction, &request.request_id)?
            .is_some_and(|receipt| receipt.idempotency_key != request.idempotency_key)
        {
            return Err(CoordinatorError::RequestIdConflict {
                request_id: request.request_id,
            });
        }
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
        if let Some(receipt) = receipt_by_idempotency(&transaction, &request.idempotency_key)? {
            if receipt.kind == request.kind && receipt.payload_json == request.payload_json {
                transaction.commit()?;
                return Ok(EnqueueResult {
                    inserted: false,
                    request: request_from_receipt(receipt),
                });
            }
            return Err(CoordinatorError::IdempotencyConflict {
                idempotency_key: request.idempotency_key,
                existing_request_id: receipt.request_id,
            });
        }
        // In-txn recheck: refuse new inserts under foreign live maintenance intent.
        refuse_foreign_live_maintenance_intent(&transaction, self.clock.now_ms())?;
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

    pub fn claim_resolve(
        &self,
        request_id: &str,
        owner_id: &str,
        now: i64,
        stale_after_ms: i64,
    ) -> Result<bool, CoordinatorError> {
        if request_id.is_empty() || owner_id.is_empty() || now < 0 || stale_after_ms <= 0 {
            return Err(CoordinatorError::InvalidRequest);
        }
        let stale_before = now.saturating_sub(stale_after_ms);
        let mut connection = open_coordinator(&self.coordinator_db)?;
        let transaction = begin_coordinator(&mut connection)?;
        if foreign_live_maintenance_intent(&transaction, now)?.is_some() {
            return Ok(false);
        }
        let Some(request) = request_by_id(&transaction, request_id)? else {
            return Ok(false);
        };
        if request.kind != RequestKind::Resolve
            || !matches!(request.state, RequestState::Queued | RequestState::Claimed)
        {
            return Ok(false);
        }
        let other_claimed: bool = transaction.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM requests
               WHERE kind='resolve' AND state='claimed' AND request_id<>?1
             )",
            [request_id],
            |row| row.get(0),
        )?;
        if other_claimed {
            return Ok(false);
        }
        let prior_owner_dead = request
            .claim_owner
            .as_deref()
            .and_then(resolve_owner_pid)
            .is_some_and(|pid| self.pid_liveness.status(pid) == PidStatus::Dead);
        let eligible = request.state == RequestState::Queued
            || request.claim_owner.as_deref() == Some(owner_id)
            || request
                .claim_heartbeat_at
                .is_some_and(|heartbeat| heartbeat <= stale_before)
            || prior_owner_dead;
        if !eligible {
            return Ok(false);
        }
        let changed = transaction.execute(
            "UPDATE requests SET state='claimed',claim_owner=?1,
                    claim_heartbeat_at=?2,updated_at=?2
             WHERE request_id=?3 AND kind='resolve'",
            params![owner_id, now, request_id],
        )?;
        transaction.commit()?;
        Ok(changed == 1)
    }

    pub fn heartbeat_resolve(
        &self,
        request_id: &str,
        owner_id: &str,
        now: i64,
    ) -> Result<bool, CoordinatorError> {
        if request_id.is_empty() || owner_id.is_empty() || now < 0 {
            return Err(CoordinatorError::InvalidRequest);
        }
        let connection = open_coordinator(&self.coordinator_db)?;
        Ok(connection.execute(
            "UPDATE requests SET claim_heartbeat_at=?1,updated_at=?1
             WHERE request_id=?2 AND kind='resolve' AND state='claimed'
               AND claim_owner=?3",
            params![now, request_id, owner_id],
        )? == 1)
    }

    pub fn resolve_claim_is_current(
        &self,
        request_id: &str,
        owner_id: &str,
    ) -> Result<bool, CoordinatorError> {
        if request_id.is_empty() || owner_id.is_empty() {
            return Err(CoordinatorError::InvalidRequest);
        }
        let connection = open_coordinator(&self.coordinator_db)?;
        Ok(connection.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM requests
               WHERE request_id=?1 AND kind='resolve' AND state='claimed'
                 AND claim_owner=?2
             )",
            params![request_id, owner_id],
            |row| row.get(0),
        )?)
    }

    pub fn fail_resolve(
        &self,
        request_id: &str,
        owner_id: &str,
        detail: &str,
        now: i64,
    ) -> Result<bool, CoordinatorError> {
        if request_id.is_empty() || owner_id.is_empty() || detail.is_empty() || now < 0 {
            return Err(CoordinatorError::InvalidRequest);
        }
        let error_json = serde_json::json!({ "message": detail }).to_string();
        let connection = open_coordinator(&self.coordinator_db)?;
        Ok(connection.execute(
            "UPDATE requests SET state='failed',claim_owner=NULL,
                    claim_heartbeat_at=NULL,result_json=NULL,error_json=?1,updated_at=?2
             WHERE request_id=?3 AND kind='resolve' AND state='claimed'
               AND claim_owner=?4",
            params![error_json, now, request_id, owner_id],
        )? == 1)
    }

    pub fn commit_resolve(
        &mut self,
        request_id: &str,
        owner_id: &str,
    ) -> Result<ReconcileOutcome, CoordinatorError> {
        if !self.resolve_claim_is_current(request_id, owner_id)? {
            return Err(CoordinatorError::LeaseLost);
        }
        let outcome = self.reconcile(request_id)?;
        if !outcome.committed_in_fact {
            return Err(CoordinatorError::CoordinatorAheadOfStore {
                request_id: request_id.to_string(),
            });
        }
        Ok(outcome)
    }

    pub fn try_acquire_or_takeover(
        &mut self,
        holder: LeaseHolder,
        now: i64,
    ) -> Result<LeaseDisposition, CoordinatorError> {
        self.try_acquire_with_intent_policy(holder, None, now)
    }

    /// Acquires the store-writer lease under an explicit maintenance-owner fence.
    ///
    /// Ordinary [`try_acquire_or_takeover`] never accepts a maintenance bypass. This API
    /// requires the live `maintenance_intent` row to match `owner` on every field and
    /// reuses `owner.fencing_token` as the lease fencing token.
    pub fn try_acquire_for_maintenance(
        &mut self,
        holder: LeaseHolder,
        owner: MaintenanceOwnerFence,
        now: i64,
    ) -> Result<LeaseDisposition, CoordinatorError> {
        if owner.run_id.is_empty()
            || owner.owner_id.is_empty()
            || owner.owner_pid == 0
            || owner.fencing_token <= 0
        {
            return Err(CoordinatorError::InvalidRequest);
        }
        if holder.holder_id != owner.owner_id || holder.holder_pid != owner.owner_pid {
            return Err(CoordinatorError::InvalidRequest);
        }
        self.try_acquire_with_intent_policy(holder, Some(owner), now)
    }

    fn try_acquire_with_intent_policy(
        &mut self,
        holder: LeaseHolder,
        maintenance_owner: Option<MaintenanceOwnerFence>,
        now: i64,
    ) -> Result<LeaseDisposition, CoordinatorError> {
        if holder.holder_id.is_empty() || holder.holder_version.is_empty() || holder.holder_pid == 0
        {
            return Err(CoordinatorError::InvalidRequest);
        }
        self.ensure_writer_eligible(&holder.holder_version)?;
        let mut connection = open_coordinator(&self.coordinator_db)?;
        let transaction = begin_coordinator(&mut connection)?;
        let live_intent = foreign_live_maintenance_intent(&transaction, now)?;
        match (&maintenance_owner, live_intent) {
            (Some(owner), Some(intent)) if owner.matches_intent(&intent) => {}
            (Some(_), Some(intent)) | (None, Some(intent)) => {
                return Err(CoordinatorError::StoreConnection(
                    StoreConnectionError::MaintenanceInProgress {
                        run_id: intent.run_id,
                    },
                ));
            }
            // Maintenance-owner acquire requires a live matching intent. Do not
            // fall through and mint a writer lease with a caller-supplied token.
            (Some(_), None) => return Err(CoordinatorError::InvalidRequest),
            (None, None) => {}
        }
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
        let disposition = match (existing, maintenance_owner.as_ref()) {
            (None, Some(owner)) => {
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
                        owner.fencing_token,
                    ],
                )?;
                LeaseDisposition::Acquired {
                    fencing_token: owner.fencing_token,
                }
            }
            (None, None) => {
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
            (Some((holder_id, old_pid, old_expiry, fencing_token)), Some(owner))
                if holder_id == holder.holder_id
                    && old_pid == holder.holder_pid
                    && fencing_token == owner.fencing_token
                    && old_expiry > now =>
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
            (Some((holder_id, old_pid, old_expiry, fencing_token)), None)
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
            (Some((old_holder_id, old_pid, old_expiry, fencing_token)), Some(owner))
                if old_expiry <= now || self.pid_liveness.status(old_pid) == PidStatus::Dead =>
            {
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
                        owner.fencing_token,
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
                    fencing_token: owner.fencing_token,
                }
            }
            (Some((old_holder_id, old_pid, old_expiry, fencing_token)), None)
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
            (Some(_), _) => LeaseDisposition::HeldByOther,
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
        heartbeat_lease_at(
            &self.coordinator_db,
            holder,
            fencing_token,
            now,
            lease_duration_ms,
        )
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
        StoreConnectionFactory::new(
            self.layout.clone(),
            self.family_id.clone(),
            holder.holder_version.clone(),
        )
        .validate_write_fence()?;
        // Service-window scheduling stays on the injected clock; store-writer
        // lease rows always live in the wall-clock domain so open_writer fence
        // checks cannot accept wall-expired leases.
        let started_at = self.clock.now_ms();
        let wall_now = system_now_ms();
        let lease = self.try_acquire_or_takeover(holder.clone(), wall_now)?;
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
            let heartbeat = LeaseHeartbeatGuard::start(
                self.coordinator_db.clone(),
                holder.clone(),
                fencing_token,
                policy.lease_duration_ms,
            );
            let result = self.drain_acquired(executor, policy, &holder, fencing_token, started_at);
            if heartbeat.stop() {
                result
            } else {
                Err(CoordinatorError::LeaseLost)
            }
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
            let is_batch = request.kind.is_batch();
            match self.execute_request_quantum(
                executor,
                policy,
                holder,
                fencing_token,
                request.clone(),
            ) {
                Ok(()) | Err(CoordinatorError::ExecutionFailed { .. }) => {}
                Err(error @ CoordinatorError::QuantumDeadlineExceeded { .. }) => {
                    return Err(error);
                }
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
        let service_now = self.clock.now_ms();
        let wall_now = system_now_ms();
        if !self.heartbeat_lease_for(holder, fencing_token, wall_now, policy.lease_duration_ms)? {
            return Err(CoordinatorError::LeaseLost);
        }
        if !self.claim_request(
            &request.request_id,
            holder,
            fencing_token,
            service_now,
            policy.lease_duration_ms,
        )? {
            return Ok(());
        }
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("claim_before_effect");
        let factory = StoreConnectionFactory::new(
            self.layout.clone(),
            self.family_id.clone(),
            holder.holder_version.clone(),
        )
        .with_generation_fence(GenerationFence::writer(
            &self.layout,
            &holder.holder_id,
            holder.holder_pid,
            fencing_token,
            wall_now,
        ));
        let mut store = factory.open_writer()?;
        factory.advance_binary_version(&mut store)?;
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
        if elapsed_ms > policy.maximum_quantum_ms && !request.kind.permits_renewable_quantum() {
            drop(transaction);
            if !self.requeue_request(
                &request.request_id,
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
        #[cfg(feature = "test-store-crash")]
        let progress_after_commit_boundary = match &quantum {
            ExecutionQuantum::Progress { event_kind, .. }
                if event_kind.ends_with("l1_published") =>
            {
                Some("manifest_after_store_commit")
            }
            ExecutionQuantum::Progress { event_kind, .. } if event_kind.ends_with("l3_chunk") => {
                Some("nonfinal_deep_after_store_commit")
            }
            ExecutionQuantum::Progress { .. } => Some("progress_after_store_commit"),
            ExecutionQuantum::Complete { .. } => None,
        };
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
                #[cfg(feature = "test-store-crash")]
                super::test_hooks::crash_if("progress_before_store_commit");
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
                #[cfg(feature = "test-store-crash")]
                super::test_hooks::crash_if("terminal_before_store_commit");
                true
            }
        };
        let wall_now = system_now_ms();
        if !self.heartbeat_lease_for(holder, fencing_token, wall_now, policy.lease_duration_ms)? {
            return Err(CoordinatorError::LeaseLost);
        }
        transaction.commit()?;
        #[cfg(feature = "test-store-crash")]
        if let Some(boundary) = progress_after_commit_boundary {
            super::test_hooks::crash_if(boundary);
        }
        #[cfg(feature = "test-store-crash")]
        if completed {
            super::test_hooks::crash_if("terminal_after_store_commit");
        }
        let service_now = self.clock.now_ms();
        let wall_now = system_now_ms();
        if !self.heartbeat_lease_for(holder, fencing_token, wall_now, policy.lease_duration_ms)? {
            return Err(CoordinatorError::LeaseLost);
        }
        if completed {
            #[cfg(feature = "test-store-crash")]
            super::test_hooks::crash_if("post_store_pre_coord_reconcile");
            self.reconcile(&request.request_id)?;
        } else {
            let maxima = read_family_allocator_maxima(&store)?;
            let mut connection = open_coordinator(&self.coordinator_db)?;
            let transaction = begin_coordinator(&mut connection)?;
            advance_family_allocator_marks(&transaction, &maxima, service_now)?;
            let changed = transaction.execute(
                "UPDATE requests SET claim_heartbeat_at = ?1, updated_at = ?1
                 WHERE request_id = ?2 AND state = 'claimed' AND claim_owner = ?3
                   AND EXISTS (
                     SELECT 1 FROM writer_lease
                     WHERE resource = ?4 AND holder_id = ?3 AND holder_pid = ?5
                       AND fencing_token = ?6 AND expires_at > ?7
                   )",
                params![
                    service_now,
                    request.request_id,
                    holder.holder_id,
                    STORE_WRITER_RESOURCE,
                    holder.holder_pid,
                    fencing_token,
                    wall_now,
                ],
            )?;
            if changed != 1 {
                return Err(CoordinatorError::LeaseLost);
            }
            transaction.commit()?;
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
            if request.kind == RequestKind::Resolve {
                continue;
            }
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
            if request.kind.is_batch() {
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

    fn requeue_request(
        &self,
        request_id: &str,
        holder: &LeaseHolder,
        fencing_token: i64,
        now: i64,
    ) -> Result<bool, CoordinatorError> {
        let connection = open_coordinator(&self.coordinator_db)?;
        Ok(connection.execute(
            "UPDATE requests SET state = 'queued', claim_owner = NULL,
             claim_heartbeat_at = NULL, result_json = NULL, error_json = NULL, updated_at = ?1
             WHERE request_id = ?2 AND state = 'claimed' AND claim_owner = ?3
               AND EXISTS (
                 SELECT 1 FROM writer_lease
                 WHERE resource = ?4 AND holder_id = ?3 AND holder_pid = ?5
                   AND fencing_token = ?6 AND expires_at > ?1
               )",
            params![
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
        let maxima = read_family_allocator_maxima(&store)?;
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
        advance_family_allocator_marks(&transaction, &maxima, self.clock.now_ms())?;
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
        if let Some(request) = request_by_id(&connection, request_id)? {
            return Ok(request);
        }
        receipt_by_id(&connection, request_id)?
            .map(request_from_receipt)
            .ok_or_else(|| CoordinatorError::RequestNotFound {
                request_id: request_id.to_string(),
            })
    }

    pub fn request_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Option<CoordinatorRequest>, CoordinatorError> {
        let connection = open_coordinator(&self.coordinator_db)?;
        if let Some(request) = request_by_idempotency(&connection, idempotency_key)? {
            return Ok(Some(request));
        }
        Ok(receipt_by_idempotency(&connection, idempotency_key)?.map(request_from_receipt))
    }

    pub fn archive_terminal_requests(
        &mut self,
        generation_name: &str,
        completed_before: i64,
        maximum_log_sequence: i64,
        limit: usize,
    ) -> Result<Vec<RequestReceipt>, CoordinatorError> {
        validate_generation(&self.layout, generation_name)?;
        if completed_before < 0 || maximum_log_sequence < 0 {
            return Err(CoordinatorError::InvalidTime {
                field: "request_archive",
                value: completed_before.min(maximum_log_sequence),
            });
        }
        if limit == 0 {
            return Err(CoordinatorError::InvalidPolicy);
        }
        let mut connection = open_coordinator(&self.coordinator_db)?;
        let transaction = begin_coordinator(&mut connection)?;
        let candidates = {
            let mut statement = transaction.prepare(
                "SELECT request_id,idempotency_key,kind,payload_json,result_json,
                        terminal_log_sequence,updated_at
                 FROM requests
                 WHERE state IN ('committed','acknowledged')
                   AND result_json IS NOT NULL
                   AND terminal_log_sequence IS NOT NULL
                   AND updated_at<=?1 AND terminal_log_sequence<=?2
                 ORDER BY terminal_log_sequence,request_id LIMIT ?3",
            )?;
            statement
                .query_map(
                    params![completed_before, maximum_log_sequence, limit as i64],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, i64>(5)?,
                            row.get::<_, i64>(6)?,
                        ))
                    },
                )?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut archived = Vec::with_capacity(candidates.len());
        for (
            request_id,
            idempotency_key,
            kind,
            payload_json,
            result_json,
            sequence,
            completed_at,
        ) in candidates
        {
            let kind = RequestKind::parse(&kind)?;
            transaction.execute(
                "INSERT INTO request_receipts
                 (request_id,idempotency_key,kind,payload_json,terminal_result_json,
                  terminal_generation_name,terminal_log_sequence,completed_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    request_id,
                    idempotency_key,
                    kind.as_str(),
                    payload_json,
                    result_json,
                    generation_name,
                    sequence,
                    completed_at,
                ],
            )?;
            let deleted = transaction.execute(
                "DELETE FROM requests
                 WHERE request_id=?1 AND state IN ('committed','acknowledged')
                   AND terminal_log_sequence=?2",
                params![request_id, sequence],
            )?;
            if deleted != 1 {
                return Err(CoordinatorError::CorruptRequest {
                    detail: format!("request {request_id:?} changed during archival"),
                });
            }
            archived.push(RequestReceipt {
                request_id,
                idempotency_key,
                kind,
                payload_json,
                terminal_result_json: result_json,
                terminal_generation_name: generation_name.to_string(),
                terminal_log_sequence: sequence,
                completed_at,
            });
        }
        transaction.commit()?;
        Ok(archived)
    }

    pub fn advance_consumer_cursor(
        &mut self,
        consumer_id: &str,
        generation_name: &str,
        store_log_sequence: i64,
        updated_at: i64,
    ) -> Result<ConsumerCursor, CoordinatorError> {
        if consumer_id.is_empty() || consumer_id.len() > 128 {
            return Err(CoordinatorError::InvalidRequest);
        }
        validate_generation(&self.layout, generation_name)?;
        if store_log_sequence < 0 || updated_at < 0 {
            return Err(CoordinatorError::InvalidTime {
                field: "consumer_cursor",
                value: store_log_sequence.min(updated_at),
            });
        }
        let mut connection = open_coordinator(&self.coordinator_db)?;
        let transaction = begin_coordinator(&mut connection)?;
        refuse_foreign_live_maintenance_intent(&transaction, self.clock.now_ms())?;
        let high_water = transaction
            .query_row(
                "SELECT high_water FROM family_allocator_marks
                 WHERE allocator_kind='store_log' AND scope_id=''",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        if store_log_sequence > high_water {
            return Err(CoordinatorError::CursorAhead {
                requested: store_log_sequence,
                high_water,
            });
        }
        let existing = transaction
            .query_row(
                "SELECT generation_name,store_log_sequence,updated_at
                 FROM consumer_cursors WHERE consumer_id=?1",
                [consumer_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((current_generation, current_sequence, current_updated_at)) = existing {
            if current_generation != generation_name {
                return Err(CoordinatorError::CursorGenerationConflict {
                    consumer_id: consumer_id.to_string(),
                    current: current_generation,
                    requested: generation_name.to_string(),
                });
            }
            if store_log_sequence < current_sequence || updated_at < current_updated_at {
                return Err(CoordinatorError::CursorRegression {
                    consumer_id: consumer_id.to_string(),
                    current: current_sequence,
                    requested: store_log_sequence,
                });
            }
        }
        transaction.execute(
            "INSERT INTO consumer_cursors
             (consumer_id,generation_name,store_log_sequence,updated_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(consumer_id) DO UPDATE SET
               store_log_sequence=excluded.store_log_sequence,
               updated_at=excluded.updated_at",
            params![consumer_id, generation_name, store_log_sequence, updated_at],
        )?;
        transaction.commit()?;
        Ok(ConsumerCursor {
            consumer_id: consumer_id.to_string(),
            generation_name: generation_name.to_string(),
            store_log_sequence,
            updated_at,
        })
    }

    pub fn release_consumer_cursor(&mut self, consumer_id: &str) -> Result<bool, CoordinatorError> {
        if consumer_id.is_empty() || consumer_id.len() > 128 {
            return Err(CoordinatorError::InvalidRequest);
        }
        let mut connection = open_coordinator(&self.coordinator_db)?;
        let transaction = begin_coordinator(&mut connection)?;
        refuse_foreign_live_maintenance_intent(&transaction, self.clock.now_ms())?;
        let changed = transaction.execute(
            "DELETE FROM consumer_cursors WHERE consumer_id=?1",
            [consumer_id],
        )?;
        transaction.commit()?;
        Ok(changed == 1)
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
        let min_reader_version = store.query_row(
            "SELECT value FROM store_meta WHERE key = 'min_reader_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
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
        let required = required_writer_version(
            &min_reader_version,
            &min_writer_version,
            &binary_version,
            extractor_downgrade_allowed(),
        )
        .map_err(|error| match error {
            StoreConnectionError::InvalidVersion { value, .. } => {
                CoordinatorError::InvalidVersion { value }
            }
            other => CoordinatorError::CorruptRequest {
                detail: other.to_string(),
            },
        })?
        .to_string();
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
pub(crate) fn process_status(pid: u32) -> PidStatus {
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
        Ok(_) => PidStatus::Dead,
        _ => PidStatus::Unknown,
    }
}

#[cfg(not(unix))]
pub(crate) fn process_status(_pid: u32) -> PidStatus {
    PidStatus::Unknown
}

fn resolve_owner_pid(owner_id: &str) -> Option<u32> {
    owner_id.strip_prefix("cli-")?.parse().ok()
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

/// Returns the live maintenance intent identity when `expires_at > now`.
pub fn foreign_live_maintenance_intent(
    conn: &Connection,
    now: i64,
) -> Result<Option<IntentIdentity>, CoordinatorError> {
    let intent = conn
        .query_row(
            "SELECT run_id, owner_id, owner_pid, fencing_token, expires_at
             FROM maintenance_intent WHERE resource = 'store-maintenance'",
            [],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((run_id, owner_id, owner_pid, fencing_token, expires_at)) = intent else {
        return Ok(None);
    };
    if expires_at <= now {
        return Ok(None);
    }
    Ok(Some(IntentIdentity {
        run_id,
        owner_id,
        owner_pid,
        fencing_token,
    }))
}

fn refuse_foreign_live_maintenance_intent(
    conn: &Connection,
    now: i64,
) -> Result<(), CoordinatorError> {
    if let Some(intent) = foreign_live_maintenance_intent(conn, now)? {
        return Err(CoordinatorError::StoreConnection(
            StoreConnectionError::MaintenanceInProgress {
                run_id: intent.run_id,
            },
        ));
    }
    Ok(())
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

fn receipt_by_idempotency(
    connection: &Connection,
    idempotency_key: &str,
) -> Result<Option<RequestReceipt>, CoordinatorError> {
    query_receipt(
        connection,
        "SELECT request_id,idempotency_key,kind,payload_json,terminal_result_json,
                terminal_generation_name,terminal_log_sequence,completed_at
         FROM request_receipts WHERE idempotency_key=?1",
        idempotency_key,
    )
}

fn receipt_by_id(
    connection: &Connection,
    request_id: &str,
) -> Result<Option<RequestReceipt>, CoordinatorError> {
    query_receipt(
        connection,
        "SELECT request_id,idempotency_key,kind,payload_json,terminal_result_json,
                terminal_generation_name,terminal_log_sequence,completed_at
         FROM request_receipts WHERE request_id=?1",
        request_id,
    )
}

fn query_receipt(
    connection: &Connection,
    sql: &str,
    parameter: &str,
) -> Result<Option<RequestReceipt>, CoordinatorError> {
    let row = connection
        .query_row(sql, [parameter], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })
        .optional()?;
    row.map(
        |(
            request_id,
            idempotency_key,
            kind,
            payload_json,
            terminal_result_json,
            terminal_generation_name,
            terminal_log_sequence,
            completed_at,
        )| {
            Ok(RequestReceipt {
                request_id,
                idempotency_key,
                kind: RequestKind::parse(&kind)?,
                payload_json,
                terminal_result_json,
                terminal_generation_name,
                terminal_log_sequence,
                completed_at,
            })
        },
    )
    .transpose()
}

fn request_from_receipt(receipt: RequestReceipt) -> CoordinatorRequest {
    CoordinatorRequest {
        request_id: receipt.request_id,
        idempotency_key: receipt.idempotency_key,
        kind: receipt.kind,
        payload_json: receipt.payload_json,
        state: RequestState::Committed,
        requester_id: "receipt".to_string(),
        requester_deadline: None,
        claim_owner: None,
        claim_heartbeat_at: None,
        terminal_log_sequence: Some(receipt.terminal_log_sequence),
        result_json: Some(receipt.terminal_result_json),
        error_json: None,
        created_at: receipt.completed_at,
        updated_at: receipt.completed_at,
    }
}

fn validate_generation(
    layout: &StoreLayout,
    generation_name: &str,
) -> Result<(), CoordinatorError> {
    if !valid_generation_name(generation_name) {
        return Err(CoordinatorError::InvalidGeneration {
            generation_name: generation_name.to_string(),
        });
    }
    let path = layout.root().join(generation_name);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return Err(CoordinatorError::InvalidGeneration {
            generation_name: generation_name.to_string(),
        });
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CoordinatorError::InvalidGeneration {
            generation_name: generation_name.to_string(),
        });
    }
    let canonical_root =
        layout
            .root()
            .canonicalize()
            .map_err(|_| CoordinatorError::InvalidGeneration {
                generation_name: generation_name.to_string(),
            })?;
    let canonical_path = path
        .canonicalize()
        .map_err(|_| CoordinatorError::InvalidGeneration {
            generation_name: generation_name.to_string(),
        })?;
    if !canonical_path.starts_with(canonical_root) {
        return Err(CoordinatorError::InvalidGeneration {
            generation_name: generation_name.to_string(),
        });
    }
    Ok(())
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

struct LeaseHeartbeatGuard {
    stop: mpsc::Sender<()>,
    current: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl LeaseHeartbeatGuard {
    fn start(
        coordinator_db: PathBuf,
        holder: LeaseHolder,
        fencing_token: i64,
        lease_duration_ms: i64,
    ) -> Self {
        let (stop, receiver) = mpsc::channel();
        let current = Arc::new(AtomicBool::new(true));
        let worker_current = Arc::clone(&current);
        let interval_ms = u64::try_from((lease_duration_ms / 3).max(1)).unwrap_or(1);
        let worker = std::thread::spawn(move || {
            loop {
                match receiver.recv_timeout(Duration::from_millis(interval_ms)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                let now = system_now_ms();
                match heartbeat_lease_at(
                    &coordinator_db,
                    &holder,
                    fencing_token,
                    now,
                    lease_duration_ms,
                ) {
                    Ok(true) => {}
                    Ok(false) | Err(_) => {
                        worker_current.store(false, AtomicOrdering::Release);
                        return;
                    }
                }
            }
        });
        Self {
            stop,
            current,
            worker: Some(worker),
        }
    }

    fn stop(mut self) -> bool {
        let _ = self.stop.send(());
        let joined = self
            .worker
            .take()
            .is_none_or(|worker| worker.join().is_ok());
        joined && self.current.load(AtomicOrdering::Acquire)
    }
}

impl Drop for LeaseHeartbeatGuard {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
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

struct FamilyAllocatorMaximum {
    kind: &'static str,
    scope_id: String,
    high_water: i64,
}

fn read_family_allocator_maxima(
    store: &Connection,
) -> Result<Vec<FamilyAllocatorMaximum>, CoordinatorError> {
    let mut maxima = vec![
        FamilyAllocatorMaximum {
            kind: "file_version",
            scope_id: String::new(),
            high_water: store.query_row(
                "SELECT COALESCE(MAX(version_id),0) FROM file_versions",
                [],
                |row| row.get(0),
            )?,
        },
        FamilyAllocatorMaximum {
            kind: "store_log",
            scope_id: String::new(),
            high_water: store.query_row(
                "SELECT COALESCE(MAX(sequence),0) FROM store_log",
                [],
                |row| row.get(0),
            )?,
        },
    ];
    read_scoped_allocator_maxima(
        store,
        "SELECT view_id,MAX(generation) FROM manifests GROUP BY view_id ORDER BY view_id",
        "manifest_generation",
        &mut maxima,
    )?;
    read_scoped_allocator_maxima(
        store,
        "SELECT view_id,MAX(delta_generation) FROM resolution_deltas
         GROUP BY view_id ORDER BY view_id",
        "resolution_delta_generation",
        &mut maxima,
    )?;
    Ok(maxima)
}

fn read_scoped_allocator_maxima(
    store: &Connection,
    query: &str,
    kind: &'static str,
    maxima: &mut Vec<FamilyAllocatorMaximum>,
) -> Result<(), CoordinatorError> {
    let mut statement = store.prepare(query)?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        maxima.push(FamilyAllocatorMaximum {
            kind,
            scope_id: row.get(0)?,
            high_water: row.get(1)?,
        });
    }
    Ok(())
}

fn advance_family_allocator_marks(
    transaction: &Transaction<'_>,
    maxima: &[FamilyAllocatorMaximum],
    now: i64,
) -> Result<(), CoordinatorError> {
    for maximum in maxima {
        transaction.execute(
            "INSERT INTO family_allocator_marks
             (allocator_kind,scope_id,high_water,updated_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(allocator_kind,scope_id) DO UPDATE SET
               high_water=MAX(high_water,excluded.high_water),
               updated_at=MAX(updated_at,excluded.updated_at)",
            params![maximum.kind, maximum.scope_id, maximum.high_water, now],
        )?;
    }
    Ok(())
}

fn open_coordinator(path: &Path) -> Result<Connection, CoordinatorError> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(COORDINATOR_BUSY_TIMEOUT)?;
    configure_writer_pragmas(&connection, WriterPragmaProfile::Routine).map_err(|error| {
        CoordinatorError::CorruptRequest {
            detail: format!("coordinator writer pragma configuration failed: {error:?}"),
        }
    })?;
    Ok(connection)
}

fn coordinator_store_family(layout: &StoreLayout) -> Result<String, CoordinatorError> {
    let store = Connection::open_with_flags(
        layout.store_db(),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(store.query_row(
        "SELECT value FROM store_meta WHERE key = 'family_id'",
        [],
        |row| row.get(0),
    )?)
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

fn heartbeat_lease_at(
    coordinator_db: &Path,
    holder: &LeaseHolder,
    fencing_token: i64,
    now: i64,
    lease_duration_ms: i64,
) -> Result<bool, CoordinatorError> {
    let expires_at = checked_lease_expiry(now, lease_duration_ms)?;
    let connection = open_coordinator(coordinator_db)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_PATH: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn open_coordinator_configures_routine_writer_pragmas() {
        let sequence = NEXT_TEMP_PATH.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "julie-coordinator-pragmas-{}-{sequence}.db",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let connection = open_coordinator(&path).unwrap();

        assert_eq!(pragma_integer(&connection, "busy_timeout"), 5_000);
        assert_eq!(pragma_integer(&connection, "page_size"), 4096);
        assert_eq!(pragma_integer(&connection, "auto_vacuum"), 2);
        assert_eq!(pragma_text(&connection, "journal_mode"), "wal");
        assert_eq!(pragma_integer(&connection, "synchronous"), 2);
        assert_eq!(pragma_integer(&connection, "foreign_keys"), 1);
        assert_eq!(pragma_integer(&connection, "secure_delete"), 1);
        assert_eq!(pragma_integer(&connection, "wal_autocheckpoint"), 1_000);

        drop(connection);
        let _ = fs::remove_file(path);
    }

    fn pragma_integer(connection: &Connection, name: &str) -> i64 {
        connection
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
            .unwrap()
    }

    fn pragma_text(connection: &Connection, name: &str) -> String {
        connection
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
            .unwrap()
    }
}
