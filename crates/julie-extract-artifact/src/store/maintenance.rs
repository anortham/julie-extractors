use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::layout::valid_generation_name;
use super::pragmas::{WriterPragmaProfile, configure_writer_pragmas};
use super::resolution::{
    resolution_file_bytes, resolution_file_sha256, retire_resolution_base, retire_resolution_delta,
};
use super::connection::compare_versions;
use super::{
    CoordinatorError, GenerationFence, MaintenanceAction, PidStatus, StoreConnectionError,
    StoreConnectionFactory, StoreCoordinator, StoreLog, StoreLogError,
};

const DAY_MS: i64 = 86_400_000;
const DEFAULT_WINDOW_SIZE: usize = 512;
const MAX_DEMOTION_VERSIONS: usize = 100;
const MAX_DEMOTION_BYTES: u64 = 64 * 1024 * 1024;
const JOURNAL_RETENTION_BYTES: u64 = 256 * 1024 * 1024;
const CHECKPOINT_HEADROOM_BYTES: u64 = 16 * 1024 * 1024;
const DEFAULT_PHYSICAL_BREACH_LIMIT: u32 = 3;

const MAINTENANCE_TMP_PREFIX: &str = "maintenance_tmp_";
const META_MIN_WRITER_VERSION: &str = "min_writer_version";
const META_GENERATION_STATE: &str = "generation_state";
const TMP_RUN_ID: &str = "maintenance_tmp_run_id";
const TMP_ACTION: &str = "maintenance_tmp_action";
const TMP_SOURCE_GENERATION: &str = "maintenance_tmp_source_generation_name";
const TMP_OWNER_ID: &str = "maintenance_tmp_owner_id";
const TMP_OWNER_PID: &str = "maintenance_tmp_owner_pid";
const TMP_FENCING_TOKEN: &str = "maintenance_tmp_fencing_token";
const TMP_HEARTBEAT_AT: &str = "maintenance_tmp_heartbeat_at";
const TMP_STARTED_AT: &str = "maintenance_tmp_started_at";
const TMP_PLAN_FINGERPRINT: &str = "maintenance_tmp_plan_fingerprint";
const TMP_SOURCE_MIN_WRITER: &str = "maintenance_tmp_source_min_writer_version";

pub trait MaintenanceClock {
    fn now_ms(&self) -> i64;
}

pub trait CapacityProvider {
    fn free_bytes(&self, path: &Path) -> Result<u64, io::Error>;
    fn staged_generation_bytes(&self, path: &Path) -> Result<u64, io::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MaintenanceLevel {
    L1,
    L2,
    L3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MaintenanceRootKind {
    CurrentManifest,
    HistoricalManifest,
    RetentionWindow,
    PathCap,
    ResolutionBase,
    IdentifierDeltaSource,
    IdentifierDeltaTarget,
    PendingDeltaSource,
    PendingDeltaTarget,
    ViewBinding,
    Pin,
    Request,
    Scratch,
    ConsumerCursor,
    CurrentGeneration,
    RollbackGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProtectionReason {
    pub kind: MaintenanceRootKind,
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionFact {
    pub version_id: i64,
    pub path: String,
    pub logical_bytes: u64,
    pub complete_l1: bool,
    pub complete_l2: bool,
    pub complete_l3: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFact {
    pub view_id: String,
    pub generation: i64,
    pub created_at_ms: i64,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestVersionFact {
    pub view_id: String,
    pub generation: i64,
    pub version_id: i64,
    pub path: String,
    pub failed_preserved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedPathFact {
    pub view_id: String,
    pub generation: i64,
    pub path: String,
    pub language: String,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseVersionFact {
    pub base_id: String,
    pub version_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaVersionFact {
    pub view_id: String,
    pub delta_generation: i64,
    pub source_version_id: i64,
    pub target_version_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRootFact {
    pub version_id: i64,
    pub max_level: MaintenanceLevel,
    pub kind: MaintenanceRootKind,
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocatorMark {
    pub kind: String,
    pub scope_id: String,
    pub high_water: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinatorRequestFact {
    pub request_id: String,
    pub state: String,
    pub claim_owner: Option<String>,
    pub terminal_log_sequence: Option<i64>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsumerCursorFact {
    pub consumer_id: String,
    pub generation_name: String,
    pub store_log_sequence: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlanBinding {
    pub family_id: String,
    pub current_generation: String,
    pub store_root_fingerprint: String,
    pub coordinator_root_fingerprint: String,
    pub store_log_max: i64,
    pub request_watermark: i64,
    pub allocator_marks: Vec<AllocatorMark>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MaintenanceCapacity {
    pub free_bytes: u64,
    pub store_page_bytes: u64,
    pub store_freelist_bytes: u64,
    pub store_wal_bytes: u64,
    pub base_bytes: u64,
    pub scratch_bytes: u64,
    pub staged_generation_bytes: u64,
    pub retention_baseline_bytes: u64,
    pub retention_breach_streak: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MaintenanceSnapshot {
    pub binding: PlanBinding,
    pub now_ms: i64,
    pub capacity: MaintenanceCapacity,
    pub versions: Vec<VersionFact>,
    pub manifests: Vec<ManifestFact>,
    pub manifest_versions: Vec<ManifestVersionFact>,
    pub failed_paths: Vec<FailedPathFact>,
    pub base_versions: Vec<BaseVersionFact>,
    pub identifier_delta_versions: Vec<DeltaVersionFact>,
    pub pending_delta_versions: Vec<DeltaVersionFact>,
    pub additional_version_roots: Vec<VersionRootFact>,
    pub protected_bases: Vec<String>,
    pub eligible_bases: Vec<String>,
    pub protected_deltas: Vec<String>,
    pub eligible_deltas: Vec<String>,
    pub protected_pins: Vec<String>,
    pub expired_pins: Vec<String>,
    pub protected_requests: Vec<String>,
    pub request_facts: Vec<CoordinatorRequestFact>,
    pub protected_scratch: Vec<String>,
    pub protected_cursors: Vec<String>,
    pub cursor_facts: Vec<ConsumerCursorFact>,
    pub protected_generations: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaintenancePolicy {
    pub retention_window_days: i64,
    pub retention_path_cap: usize,
    pub target_numerator: u64,
    pub target_denominator: u64,
    pub ceiling_numerator: u64,
    pub ceiling_denominator: u64,
    pub physical_breach_limit: u32,
}

impl Default for MaintenancePolicy {
    fn default() -> Self {
        Self {
            retention_window_days: 7,
            retention_path_cap: 24,
            target_numerator: 120,
            target_denominator: 100,
            ceiling_numerator: 125,
            ceiling_denominator: 100,
            physical_breach_limit: DEFAULT_PHYSICAL_BREACH_LIMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionDecision {
    pub version_id: i64,
    pub path: String,
    pub logical_bytes: u64,
    pub l1_reasons: Vec<ProtectionReason>,
    pub l2_reasons: Vec<ProtectionReason>,
    pub l3_reasons: Vec<ProtectionReason>,
}

impl VersionDecision {
    pub fn reasons(&self, level: MaintenanceLevel) -> &[ProtectionReason] {
        match level {
            MaintenanceLevel::L1 => &self.l1_reasons,
            MaintenanceLevel::L2 => &self.l2_reasons,
            MaintenanceLevel::L3 => &self.l3_reasons,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DemotionCandidate {
    pub version_id: i64,
    pub estimated_dirty_bytes: u64,
    pub drop_l3: bool,
    pub drop_l2: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPlan {
    pub protected_current_bytes: u64,
    pub retained_logical_bytes: u64,
    pub eligible_bytes: u64,
    pub target_bytes: u64,
    pub ceiling_bytes: u64,
    pub pressure: bool,
    pub physical_current_bytes: u64,
    pub physical_baseline_bytes: u64,
    pub physical_target_bytes: u64,
    pub physical_ceiling_bytes: u64,
    pub physical_target_breached: bool,
    pub physical_ceiling_breached: bool,
    pub physical_breach_limit: u32,
    pub physical_breach_streak: u32,
    pub compaction_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapacityPlan {
    pub facts: MaintenanceCapacity,
    pub measured_bytes: u64,
    pub free_bytes: u64,
    pub demotion_wal_headroom_bytes: u64,
    pub gc_required_bytes: u64,
    pub promotion_required_bytes: u64,
    pub gc_fits: bool,
    pub promotion_fits: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaintenancePlan {
    pub binding: PlanBinding,
    pub fingerprint: String,
    pub versions: Vec<VersionDecision>,
    pub eligible_manifests: Vec<(String, i64)>,
    pub pressure_only_manifests: Vec<(String, i64)>,
    pub demotion_cohort: Vec<DemotionCandidate>,
    pub protected_bases: Vec<String>,
    pub eligible_bases: Vec<String>,
    pub protected_deltas: Vec<String>,
    pub eligible_deltas: Vec<String>,
    pub protected_pins: Vec<String>,
    pub expired_pins: Vec<String>,
    pub protected_requests: Vec<String>,
    pub protected_scratch: Vec<String>,
    pub protected_cursors: Vec<String>,
    pub protected_generations: Vec<String>,
    pub protected_failed_paths: Vec<String>,
    pub retention: RetentionPlan,
    pub capacity: CapacityPlan,
    pub max_observed_window: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceRun {
    pub run_id: String,
    pub owner_id: String,
    pub owner_pid: u32,
    pub now_ms: i64,
    pub lease_duration_ms: i64,
}

impl MaintenanceRun {
    pub fn new(
        run_id: impl Into<String>,
        owner_id: impl Into<String>,
        owner_pid: u32,
        now_ms: i64,
        lease_duration_ms: i64,
    ) -> Self {
        Self {
            run_id: run_id.into(),
            owner_id: owner_id.into(),
            owner_pid,
            now_ms,
            lease_duration_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MaintenanceApplyReport {
    pub demoted_l3: usize,
    pub demoted_l2: usize,
    pub purged_versions: usize,
    pub removed_manifests: usize,
    pub removed_deltas: usize,
    pub removed_bases: usize,
    pub removed_base_files: usize,
    pub removed_pins: usize,
    pub removed_scratch_files: usize,
    pub archived_requests: usize,
    pub pruned_log_rows: usize,
    pub last_version_cursor: Option<i64>,
    pub checkpoint_order: Vec<String>,
    pub store_bytes_before_vacuum: u64,
    pub store_bytes_after_vacuum: u64,
    pub freelist_pages_before_vacuum: u64,
    pub freelist_pages_after_vacuum: u64,
    pub vacuum_pages: u64,
    pub physical_bytes_before: u64,
    pub physical_bytes_after: u64,
    pub physical_baseline_bytes: u64,
    pub physical_target_bytes: u64,
    pub physical_ceiling_bytes: u64,
    pub physical_target_breached: bool,
    pub physical_ceiling_breached: bool,
    pub physical_breach_streak: u32,
    pub compaction_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaintenanceApplyPolicy {
    pub request_safety_ms: i64,
    pub receipt_limit: usize,
    pub incremental_vacuum_pages: usize,
}

impl Default for MaintenanceApplyPolicy {
    fn default() -> Self {
        Self {
            request_safety_ms: 7 * DAY_MS,
            receipt_limit: 100,
            incremental_vacuum_pages: 64,
        }
    }
}

pub struct MaintenanceExecutor {
    factory: StoreConnectionFactory,
    run: MaintenanceRun,
    fencing_token: i64,
    source_min_writer_version: String,
    capacity: Box<dyn CapacityProvider + Send + Sync>,
}

impl MaintenancePlan {
    pub fn version(&self, version_id: i64) -> Option<&VersionDecision> {
        self.versions
            .binary_search_by_key(&version_id, |version| version.version_id)
            .ok()
            .map(|index| &self.versions[index])
    }

    pub fn eligible_manifest(&self, view_id: &str, generation: i64) -> bool {
        self.eligible_manifests
            .binary_search(&(view_id.to_string(), generation))
            .is_ok()
    }
}

#[derive(Debug)]
pub enum MaintenanceError {
    InspectionRaced { database: &'static str },
    UnknownRoot { kind: &'static str, id: String },
    InvalidPolicy { field: &'static str },
    InvalidMetadata { field: &'static str, value: String },
    StalePlan,
    MaintenanceBusy,
    CapacityInsufficient,
    MaintenanceFenceLost,
    Connection(StoreConnectionError),
    Coordinator(CoordinatorError),
    Log(StoreLogError),
    Sqlite(rusqlite::Error),
    Io(io::Error),
    Serialization(serde_json::Error),
}

impl MaintenanceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InspectionRaced { .. } => "maintenance_inspection_raced",
            Self::UnknownRoot { .. } => "unknown_maintenance_root",
            Self::InvalidPolicy { .. } => "invalid_maintenance_policy",
            Self::InvalidMetadata { .. } => "invalid_maintenance_metadata",
            Self::StalePlan => "maintenance_plan_stale",
            Self::MaintenanceBusy => "maintenance_busy",
            Self::CapacityInsufficient => "capacity_insufficient",
            Self::MaintenanceFenceLost => "maintenance_fence_lost",
            Self::Connection(_) => "store_connection_error",
            Self::Coordinator(_) => "maintenance_coordinator_error",
            Self::Log(_) => "maintenance_store_log_error",
            Self::Sqlite(_) => "maintenance_sqlite_error",
            Self::Io(_) => "maintenance_io_error",
            Self::Serialization(_) => "maintenance_serialization_error",
        }
    }
}

impl fmt::Display for MaintenanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InspectionRaced { database } => {
                write!(
                    formatter,
                    "{database} changed during maintenance inspection"
                )
            }
            Self::UnknownRoot { kind, id } => write!(formatter, "unknown {kind} root {id}"),
            Self::InvalidPolicy { field } => {
                write!(formatter, "invalid maintenance policy {field}")
            }
            Self::InvalidMetadata { field, value } => {
                write!(formatter, "invalid maintenance metadata {field}={value:?}")
            }
            Self::StalePlan => write!(formatter, "maintenance plan no longer matches store roots"),
            Self::MaintenanceBusy => {
                write!(formatter, "store maintenance cannot acquire ownership")
            }
            Self::CapacityInsufficient => {
                write!(
                    formatter,
                    "insufficient capacity for the planned maintenance cohort"
                )
            }
            Self::MaintenanceFenceLost => write!(formatter, "maintenance ownership was lost"),
            Self::Connection(error) => error.fmt(formatter),
            Self::Coordinator(error) => error.fmt(formatter),
            Self::Log(error) => error.fmt(formatter),
            Self::Sqlite(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
            Self::Serialization(error) => error.fmt(formatter),
        }
    }
}

impl Error for MaintenanceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Connection(error) => Some(error),
            Self::Coordinator(error) => Some(error),
            Self::Log(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Serialization(error) => Some(error),
            _ => None,
        }
    }
}

impl From<StoreConnectionError> for MaintenanceError {
    fn from(error: StoreConnectionError) -> Self {
        Self::Connection(error)
    }
}

impl From<CoordinatorError> for MaintenanceError {
    fn from(error: CoordinatorError) -> Self {
        Self::Coordinator(error)
    }
}

impl From<StoreLogError> for MaintenanceError {
    fn from(error: StoreLogError) -> Self {
        Self::Log(error)
    }
}

impl From<rusqlite::Error> for MaintenanceError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<io::Error> for MaintenanceError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for MaintenanceError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

pub fn plan_maintenance(
    snapshot: &MaintenanceSnapshot,
    policy: &MaintenancePolicy,
) -> Result<MaintenancePlan, MaintenanceError> {
    validate_policy(policy)?;
    let versions: BTreeMap<i64, &VersionFact> = snapshot
        .versions
        .iter()
        .map(|version| (version.version_id, version))
        .collect();
    if versions.len() != snapshot.versions.len() {
        return Err(MaintenanceError::InvalidMetadata {
            field: "version_id",
            value: "duplicate".to_string(),
        });
    }
    let manifests: BTreeMap<(&str, i64), &ManifestFact> = snapshot
        .manifests
        .iter()
        .map(|manifest| ((manifest.view_id.as_str(), manifest.generation), manifest))
        .collect();
    let mut reasons: BTreeMap<i64, [BTreeSet<ProtectionReason>; 3]> = versions
        .keys()
        .map(|version_id| (*version_id, std::array::from_fn(|_| BTreeSet::new())))
        .collect();
    let cutoff = snapshot
        .now_ms
        .saturating_sub(policy.retention_window_days.saturating_mul(DAY_MS));
    let historical_ranks = historical_path_ranks(snapshot, &manifests);
    let mut manifest_entries: BTreeMap<(&str, i64), Vec<&ManifestVersionFact>> = BTreeMap::new();
    for entry in &snapshot.manifest_versions {
        require_version(&versions, entry.version_id, "manifest version")?;
        if !manifests.contains_key(&(entry.view_id.as_str(), entry.generation)) {
            return Err(MaintenanceError::UnknownRoot {
                kind: "manifest",
                id: format!("{}:{}", entry.view_id, entry.generation),
            });
        }
        manifest_entries
            .entry((entry.view_id.as_str(), entry.generation))
            .or_default()
            .push(entry);
    }
    let mut eligible_manifests = Vec::new();
    for (key, manifest) in &manifests {
        let entries = manifest_entries.get(key).cloned().unwrap_or_default();
        if manifest.current {
            for entry in entries {
                let version = versions[&entry.version_id];
                add_completed_reasons(
                    &mut reasons,
                    version,
                    MaintenanceRootKind::CurrentManifest,
                    format!("{}:{}", manifest.view_id, manifest.generation),
                );
            }
            continue;
        }
        let inside_window = manifest.created_at_ms > cutoff;
        let beyond_cap = !entries.is_empty()
            && entries.iter().all(|entry| {
                historical_ranks
                    .get(&(
                        entry.path.as_str(),
                        manifest.view_id.as_str(),
                        manifest.generation,
                    ))
                    .is_some_and(|rank| *rank >= policy.retention_path_cap)
            });
        if !inside_window && beyond_cap {
            eligible_manifests.push((manifest.view_id.clone(), manifest.generation));
            continue;
        }
        let kind = if inside_window {
            MaintenanceRootKind::RetentionWindow
        } else {
            MaintenanceRootKind::PathCap
        };
        for entry in entries {
            add_reason(
                &mut reasons,
                entry.version_id,
                MaintenanceLevel::L1,
                MaintenanceRootKind::HistoricalManifest,
                format!("{}:{}", manifest.view_id, manifest.generation),
            );
            add_reason(
                &mut reasons,
                entry.version_id,
                MaintenanceLevel::L1,
                kind,
                format!("{}:{}", manifest.view_id, manifest.generation),
            );
        }
    }
    for root in &snapshot.base_versions {
        require_version(&versions, root.version_id, "resolution base")?;
        for level in [MaintenanceLevel::L1, MaintenanceLevel::L2] {
            add_reason(
                &mut reasons,
                root.version_id,
                level,
                MaintenanceRootKind::ResolutionBase,
                root.base_id.clone(),
            );
        }
    }
    apply_delta_roots(
        &versions,
        &mut reasons,
        &snapshot.identifier_delta_versions,
        MaintenanceRootKind::IdentifierDeltaSource,
        MaintenanceRootKind::IdentifierDeltaTarget,
    )?;
    apply_delta_roots(
        &versions,
        &mut reasons,
        &snapshot.pending_delta_versions,
        MaintenanceRootKind::PendingDeltaSource,
        MaintenanceRootKind::PendingDeltaTarget,
    )?;
    for root in &snapshot.additional_version_roots {
        require_version(&versions, root.version_id, "version protection")?;
        for level in levels_through(root.max_level) {
            add_reason(
                &mut reasons,
                root.version_id,
                level,
                root.kind,
                root.reference.clone(),
            );
        }
    }

    let decisions: Vec<_> = versions
        .values()
        .map(|version| {
            let level_reasons = &reasons[&version.version_id];
            VersionDecision {
                version_id: version.version_id,
                path: version.path.clone(),
                logical_bytes: version.logical_bytes,
                l1_reasons: level_reasons[0].iter().cloned().collect(),
                l2_reasons: level_reasons[1].iter().cloned().collect(),
                l3_reasons: level_reasons[2].iter().cloned().collect(),
            }
        })
        .collect();
    let protected_current_bytes = decisions
        .iter()
        .filter(|decision| {
            decision
                .l1_reasons
                .iter()
                .any(|reason| !retention_only(reason.kind))
        })
        .try_fold(0_u64, |total, decision| {
            total.checked_add(decision.logical_bytes)
        })
        .ok_or(MaintenanceError::InvalidMetadata {
            field: "protected_current_bytes",
            value: "overflow".to_string(),
        })?;
    let retained_logical_bytes = snapshot
        .versions
        .iter()
        .try_fold(0_u64, |total, version| {
            total.checked_add(version.logical_bytes)
        })
        .ok_or(MaintenanceError::InvalidMetadata {
            field: "retained_logical_bytes",
            value: "overflow".to_string(),
        })?;
    let target_bytes = ratio_bytes(
        protected_current_bytes,
        policy.target_numerator,
        policy.target_denominator,
    )?;
    let ceiling_bytes = ratio_bytes(
        protected_current_bytes,
        policy.ceiling_numerator,
        policy.ceiling_denominator,
    )?;
    eligible_manifests.sort();
    let eligible_versions: BTreeSet<i64> = snapshot
        .manifest_versions
        .iter()
        .filter(|entry| {
            eligible_manifests
                .binary_search(&(entry.view_id.clone(), entry.generation))
                .is_ok()
        })
        .map(|entry| entry.version_id)
        .collect();
    let eligible_bytes = eligible_versions
        .iter()
        .filter_map(|version_id| versions.get(version_id))
        .try_fold(0_u64, |total, version| {
            total.checked_add(version.logical_bytes)
        })
        .ok_or(MaintenanceError::InvalidMetadata {
            field: "eligible_bytes",
            value: "overflow".to_string(),
        })?;
    let measured_bytes = snapshot
        .capacity
        .store_page_bytes
        .saturating_add(snapshot.capacity.store_wal_bytes)
        .saturating_add(snapshot.capacity.base_bytes)
        .saturating_add(snapshot.capacity.scratch_bytes);
    let physical_baseline_bytes = if snapshot.capacity.retention_baseline_bytes == 0 {
        measured_bytes
    } else {
        snapshot.capacity.retention_baseline_bytes
    };
    let physical_target_bytes = ratio_bytes(
        physical_baseline_bytes,
        policy.target_numerator,
        policy.target_denominator,
    )?;
    let physical_ceiling_bytes = ratio_bytes(
        physical_baseline_bytes,
        policy.ceiling_numerator,
        policy.ceiling_denominator,
    )?;
    let physical_target_breached = measured_bytes > physical_target_bytes;
    let physical_ceiling_breached = measured_bytes > physical_ceiling_bytes;
    let pressure = retained_logical_bytes > target_bytes;
    let pressure_only_manifests = if pressure || physical_ceiling_breached {
        Vec::new()
    } else {
        eligible_manifests.clone()
    };
    let demotion_cohort = demotion_cohort(&snapshot.versions, &decisions);
    let demotion_wal_headroom_bytes: u64 = demotion_cohort
        .iter()
        .map(|candidate| candidate.estimated_dirty_bytes)
        .sum();
    let gc_required_bytes = demotion_wal_headroom_bytes
        .saturating_add(snapshot.capacity.store_wal_bytes)
        .saturating_add(CHECKPOINT_HEADROOM_BYTES);
    let promotion_required_bytes = snapshot
        .capacity
        .staged_generation_bytes
        .saturating_add(snapshot.capacity.store_wal_bytes)
        .saturating_add(JOURNAL_RETENTION_BYTES)
        .saturating_add(CHECKPOINT_HEADROOM_BYTES);
    let capacity = CapacityPlan {
        facts: snapshot.capacity.clone(),
        measured_bytes,
        free_bytes: snapshot.capacity.free_bytes,
        demotion_wal_headroom_bytes,
        gc_required_bytes,
        promotion_required_bytes,
        gc_fits: snapshot.capacity.free_bytes >= gc_required_bytes,
        promotion_fits: snapshot.capacity.free_bytes >= promotion_required_bytes,
    };
    let mut plan = MaintenancePlan {
        binding: canonical_binding(&snapshot.binding),
        fingerprint: String::new(),
        versions: decisions,
        eligible_manifests,
        pressure_only_manifests,
        demotion_cohort,
        protected_bases: sorted_unique(&snapshot.protected_bases),
        eligible_bases: sorted_unique(&snapshot.eligible_bases),
        protected_deltas: sorted_unique(&snapshot.protected_deltas),
        eligible_deltas: sorted_unique(&snapshot.eligible_deltas),
        protected_pins: sorted_unique(&snapshot.protected_pins),
        expired_pins: sorted_unique(&snapshot.expired_pins),
        protected_requests: sorted_unique(&snapshot.protected_requests),
        protected_scratch: sorted_unique(&snapshot.protected_scratch),
        protected_cursors: sorted_unique(&snapshot.protected_cursors),
        protected_generations: sorted_unique(&snapshot.protected_generations),
        protected_failed_paths: sorted_unique(
            &snapshot
                .failed_paths
                .iter()
                .filter(|fact| fact.current)
                .map(|fact| {
                    format!(
                        "{}:{}:{}:{}",
                        fact.view_id, fact.generation, fact.language, fact.path
                    )
                })
                .collect::<Vec<_>>(),
        ),
        retention: RetentionPlan {
            protected_current_bytes,
            retained_logical_bytes,
            eligible_bytes,
            target_bytes,
            ceiling_bytes,
            pressure,
            physical_current_bytes: measured_bytes,
            physical_baseline_bytes,
            physical_target_bytes,
            physical_ceiling_bytes,
            physical_target_breached,
            physical_ceiling_breached,
            physical_breach_limit: policy.physical_breach_limit,
            physical_breach_streak: snapshot.capacity.retention_breach_streak,
            compaction_required: snapshot.capacity.retention_breach_streak
                >= policy.physical_breach_limit,
        },
        capacity,
        max_observed_window: 0,
    };
    plan.fingerprint = plan_fingerprint(&plan)?;
    Ok(plan)
}

pub struct MaintenanceInspector<C, P> {
    factory: StoreConnectionFactory,
    clock: C,
    capacity: P,
    window_size: usize,
}

impl<C: MaintenanceClock, P: CapacityProvider> MaintenanceInspector<C, P> {
    pub fn new(factory: StoreConnectionFactory, clock: C, capacity: P) -> Self {
        Self {
            factory,
            clock,
            capacity,
            window_size: DEFAULT_WINDOW_SIZE,
        }
    }

    pub fn with_window_size(mut self, window_size: usize) -> Self {
        self.window_size = window_size.clamp(1, 1000);
        self
    }

    pub fn inspect(&self) -> Result<MaintenancePlan, MaintenanceError> {
        let store = self.factory.open_reader()?;
        let coord = Connection::open_with_flags(
            self.factory.layout().coordinator_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        coord.pragma_update(None, "query_only", true)?;
        let store_data_version = data_version(&store)?;
        let coordinator_data_version = data_version(&coord)?;
        let mut max_observed_window = 0;
        let mut snapshot = MaintenanceSnapshot {
            binding: read_binding(&store, &coord, &self.factory)?,
            now_ms: self.clock.now_ms(),
            capacity: read_capacity(&store, &self.factory, &self.capacity)?,
            ..MaintenanceSnapshot::default()
        };
        read_versions(
            &store,
            self.window_size,
            &mut max_observed_window,
            &mut snapshot.versions,
        )?;
        read_manifests(
            &store,
            self.window_size,
            &mut max_observed_window,
            &mut snapshot.manifests,
        )?;
        read_manifest_versions(
            &store,
            self.window_size,
            &mut max_observed_window,
            &mut snapshot.manifest_versions,
        )?;
        read_failed_paths(
            &store,
            self.window_size,
            &mut max_observed_window,
            &mut snapshot.failed_paths,
        )?;
        read_base_versions(
            &store,
            self.window_size,
            &mut max_observed_window,
            &mut snapshot,
        )?;
        read_delta_versions(
            &store,
            self.window_size,
            &mut max_observed_window,
            &mut snapshot,
        )?;
        read_additional_version_roots(
            &store,
            snapshot.now_ms,
            self.window_size,
            &mut max_observed_window,
            &mut snapshot.additional_version_roots,
        )?;
        read_store_objects(
            &store,
            snapshot.now_ms,
            self.window_size,
            &mut max_observed_window,
            &mut snapshot,
        )?;
        read_coordinator_objects(
            &coord,
            self.window_size,
            &mut max_observed_window,
            &mut snapshot,
        )?;
        snapshot.protected_generations = named_generations(self.factory.layout().root())?;
        snapshot.protected_scratch = named_files(self.factory.layout().scratch_dir())?;
        snapshot.binding.store_root_fingerprint = store_root_fingerprint(&snapshot)?;
        snapshot.binding.coordinator_root_fingerprint = coordinator_root_fingerprint(&snapshot)?;
        if data_version(&store)? != store_data_version {
            return Err(MaintenanceError::InspectionRaced {
                database: "store.db",
            });
        }
        if data_version(&coord)? != coordinator_data_version {
            return Err(MaintenanceError::InspectionRaced {
                database: "coord.db",
            });
        }
        let mut plan = plan_maintenance(&snapshot, &read_policy(&store)?)?;
        plan.max_observed_window = max_observed_window;
        plan.fingerprint = plan_fingerprint(&plan)?;
        Ok(plan)
    }
}

impl MaintenanceExecutor {
    pub fn acquire(
        factory: StoreConnectionFactory,
        run: MaintenanceRun,
        plan: &MaintenancePlan,
        capacity: impl CapacityProvider + Send + Sync + 'static,
    ) -> Result<Self, MaintenanceError> {
        Self::acquire_for_action(factory, run, plan, MaintenanceAction::Gc, capacity)
    }

    pub fn acquire_for_action(
        factory: StoreConnectionFactory,
        run: MaintenanceRun,
        plan: &MaintenancePlan,
        action: MaintenanceAction,
        capacity: impl CapacityProvider + Send + Sync + 'static,
    ) -> Result<Self, MaintenanceError> {
        validate_run(&run)?;
        let capacity: Box<dyn CapacityProvider + Send + Sync> = Box::new(capacity);
        let capacity_fits = match action {
            MaintenanceAction::Gc | MaintenanceAction::Repair => plan.capacity.gc_fits,
            MaintenanceAction::Promote | MaintenanceAction::Rollback => {
                plan.capacity.promotion_fits
            }
        };
        if !capacity_fits {
            return Err(MaintenanceError::CapacityInsufficient);
        }
        ensure_live_capacity(
            capacity.as_ref(),
            factory.layout().root(),
            required_bytes_for_action(plan, action),
        )?;
        factory.validate_writer_compatibility()?;
        let observed = MaintenanceInspector::new(
            factory.clone(),
            RevalidationClock(run.now_ms),
            LiveCapacityProbe {
                provider: capacity.as_ref(),
            },
        )
        .inspect()?;
        if observed.binding != plan.binding {
            return Err(MaintenanceError::StalePlan);
        }
        let wall_now = wall_now_ms()?;
        let expires_at = wall_now.checked_add(run.lease_duration_ms).ok_or(
            MaintenanceError::InvalidMetadata {
                field: "maintenance_expiry",
                value: "overflow".to_string(),
            },
        )?;
        let fencing_token = wall_now
            .checked_add(i64::from(run.owner_pid))
            .map(|value| value.max(1))
            .ok_or(MaintenanceError::InvalidMetadata {
                field: "maintenance_fencing_token",
                value: "overflow".to_string(),
            })?;
        let store = factory.open_reader()?;
        let store_min_writer_version = store.query_row(
            "SELECT value FROM store_meta WHERE key='min_writer_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        let holder_version = factory.binary_version().to_string();
        drop(store);
        let mut coord = open_maintenance_coordinator(factory.layout().coordinator_db())?;
        let transaction = coord.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active_intent = transaction
            .query_row(
                "SELECT run_id,owner_id,owner_pid,expires_at,source_min_writer_version
                 FROM maintenance_intent
                 WHERE resource='store-maintenance'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        // Preserve the pre-maintenance floor across dead/expired takeover when a prior intent
        // already recorded it. Store floor alone may already be temporarily raised.
        let source_min_writer_version = active_intent
            .as_ref()
            .map(|(_, _, _, _, prior)| prior.clone())
            .unwrap_or_else(|| store_min_writer_version.clone());
        let active_owner_dead = active_intent.as_ref().is_some_and(|(_, _, owner_pid, _, _)| {
            match u32::try_from(*owner_pid) {
                Ok(owner_pid) => super::coordinator::process_status(owner_pid) == PidStatus::Dead,
                Err(_) => false,
            }
        });
        if active_intent
            .as_ref()
            .is_some_and(|(run_id, owner_id, owner_pid, expiry, _)| {
                *expiry > wall_now
                    && (run_id != &run.run_id
                        || owner_id != &run.owner_id
                        || *owner_pid != i64::from(run.owner_pid))
                    && !active_owner_dead
            })
        {
            return Err(MaintenanceError::MaintenanceBusy);
        }
        if transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM writer_lease WHERE expires_at>?1)",
            [wall_now],
            |row| row.get::<_, bool>(0),
        )? || transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM requests WHERE state='claimed')",
            [],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(MaintenanceError::MaintenanceBusy);
        }
        transaction.execute("DELETE FROM writer_lease WHERE expires_at<=?1", [wall_now])?;
        if active_owner_dead {
            transaction.execute(
                "DELETE FROM maintenance_intent WHERE resource='store-maintenance'",
                [],
            )?;
        } else {
            transaction.execute(
                "DELETE FROM maintenance_intent WHERE expires_at<=?1",
                [wall_now],
            )?;
        }
        // M1: durable intent + writer lease under one coordinator transaction.
        transaction.execute(
            "INSERT INTO maintenance_intent
             (resource,run_id,action,source_generation_name,owner_id,owner_pid,fencing_token,
              heartbeat_at,expires_at,started_at,plan_fingerprint,source_min_writer_version)
             VALUES ('store-maintenance',?1,?2,?3,?4,?5,?6,?7,?8,?7,?9,?10)",
            params![
                run.run_id,
                action.as_str(),
                factory.layout().generation_name(),
                run.owner_id,
                run.owner_pid,
                fencing_token,
                wall_now,
                expires_at,
                plan.fingerprint,
                source_min_writer_version,
            ],
        )?;
        transaction.execute(
            "INSERT INTO writer_lease
             (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,fencing_token)
             VALUES ('store-writer',?1,?2,?3,?4,?5,?6)",
            params![
                run.owner_id,
                holder_version,
                run.owner_pid,
                wall_now,
                expires_at,
                fencing_token,
            ],
        )?;
        transaction.commit()?;
        let executor = Self {
            factory,
            run,
            fencing_token,
            source_min_writer_version,
            capacity,
        };
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("maintenance_after_intent_before_floor");
        // M2: raise frozen source floor and mirror intent into store_meta.
        if let Err(error) = executor.raise_source_floor_and_mirror(action, wall_now) {
            let _ = executor.restore_serving_source_floor_and_clear_coord();
            return Err(error);
        }
        Ok(executor)
    }

    pub(crate) fn ensure_gc_capacity(&self, plan: &MaintenancePlan) -> Result<(), MaintenanceError> {
        ensure_live_capacity(
            self.capacity.as_ref(),
            self.factory.layout().root(),
            plan.capacity.gc_required_bytes,
        )
    }

    pub(crate) fn ensure_promotion_capacity(
        &self,
        plan: &MaintenancePlan,
    ) -> Result<(), MaintenanceError> {
        ensure_live_capacity(
            self.capacity.as_ref(),
            self.factory.layout().root(),
            plan.capacity.promotion_required_bytes,
        )
    }

    pub(crate) fn factory(&self) -> &StoreConnectionFactory {
        &self.factory
    }

    pub(crate) fn run(&self) -> &MaintenanceRun {
        &self.run
    }

    pub(crate) fn fencing_token(&self) -> i64 {
        self.fencing_token
    }

    pub(crate) fn source_min_writer_version(&self) -> &str {
        &self.source_min_writer_version
    }

    pub(crate) fn release_writer_for_generation_build(
        &self,
        plan: &MaintenancePlan,
    ) -> Result<(), MaintenanceError> {
        self.validate_ownership(plan)?;
        let coord = open_maintenance_coordinator(self.factory.layout().coordinator_db())?;
        let changed = coord.execute(
            "DELETE FROM writer_lease
             WHERE resource='store-writer' AND holder_id=?1 AND holder_pid=?2
               AND fencing_token=?3",
            params![self.run.owner_id, self.run.owner_pid, self.fencing_token],
        )?;
        if changed != 1 {
            return Err(MaintenanceError::MaintenanceFenceLost);
        }
        Ok(())
    }

    pub(crate) fn heartbeat_generation_build(&self) -> Result<(), MaintenanceError> {
        let wall_now = wall_now_ms()?;
        let expires_at = wall_now.checked_add(self.run.lease_duration_ms).ok_or(
            MaintenanceError::InvalidMetadata {
                field: "maintenance_expiry",
                value: "overflow".to_string(),
            },
        )?;
        let coord = open_maintenance_coordinator(self.factory.layout().coordinator_db())?;
        let changed = coord.execute(
            "UPDATE maintenance_intent SET heartbeat_at=?5,expires_at=?6
             WHERE resource='store-maintenance' AND run_id=?1 AND owner_id=?2
               AND owner_pid=?3 AND fencing_token=?4 AND expires_at>?5",
            params![
                self.run.run_id,
                self.run.owner_id,
                self.run.owner_pid,
                self.fencing_token,
                wall_now,
                expires_at,
            ],
        )?;
        if changed != 1 {
            return Err(MaintenanceError::MaintenanceFenceLost);
        }
        Ok(())
    }

    pub(crate) fn reacquire_writer_for_generation_publish(
        &self,
        plan: &MaintenancePlan,
    ) -> Result<(), MaintenanceError> {
        self.validate_generation_publish_binding(plan)?;
        let wall_now = wall_now_ms()?;
        let expires_at = wall_now.checked_add(self.run.lease_duration_ms).ok_or(
            MaintenanceError::InvalidMetadata {
                field: "maintenance_expiry",
                value: "overflow".to_string(),
            },
        )?;
        let store = self.factory.open_reader()?;
        let holder_version = store.query_row(
            "SELECT value FROM store_meta WHERE key='binary_version'",
            [],
            |row| row.get::<_, String>(0),
        )?;
        drop(store);
        let mut coord = open_maintenance_coordinator(self.factory.layout().coordinator_db())?;
        let transaction = coord.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let owns_intent = transaction.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM maintenance_intent
               WHERE resource='store-maintenance' AND run_id=?1 AND owner_id=?2
                 AND owner_pid=?3 AND fencing_token=?4 AND plan_fingerprint=?5
                 AND source_generation_name=?6 AND expires_at>?7
             )",
            params![
                self.run.run_id,
                self.run.owner_id,
                self.run.owner_pid,
                self.fencing_token,
                plan.fingerprint,
                self.factory.layout().generation_name(),
                wall_now,
            ],
            |row| row.get::<_, bool>(0),
        )?;
        if !owns_intent
            || transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM requests WHERE state='claimed')",
                [],
                |row| row.get::<_, bool>(0),
            )?
            || transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM writer_lease WHERE expires_at>?1)",
                [wall_now],
                |row| row.get::<_, bool>(0),
            )?
        {
            return Err(MaintenanceError::MaintenanceBusy);
        }
        transaction.execute(
            "INSERT INTO writer_lease
             (resource,holder_id,holder_version,holder_pid,heartbeat_at,expires_at,fencing_token)
             VALUES ('store-writer',?1,?2,?3,?4,?5,?6)",
            params![
                self.run.owner_id,
                holder_version,
                self.run.owner_pid,
                wall_now,
                expires_at,
                self.fencing_token,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn finish_generation_action(&self) -> Result<(), MaintenanceError> {
        self.restore_serving_source_floor_and_clear_coord()
    }

    pub fn apply(
        &mut self,
        plan: &MaintenancePlan,
    ) -> Result<MaintenanceApplyReport, MaintenanceError> {
        self.apply_with_policy(plan, &MaintenanceApplyPolicy::default())
    }

    pub fn apply_with_policy(
        &mut self,
        plan: &MaintenancePlan,
        policy: &MaintenanceApplyPolicy,
    ) -> Result<MaintenanceApplyReport, MaintenanceError> {
        if policy.request_safety_ms < 0
            || policy.receipt_limit == 0
            || policy.incremental_vacuum_pages == 0
        {
            return Err(MaintenanceError::InvalidPolicy {
                field: "maintenance_apply_policy",
            });
        }
        self.validate_ownership(plan)?;
        self.validate_plan_binding(plan)?;
        // Live free-bytes re-probe before first mutative step (scratch purge).
        self.ensure_gc_capacity(plan)?;
        let scratch_files = terminal_request_scratch_files(self.factory.layout())?;
        let mut report = MaintenanceApplyReport::default();
        for path in scratch_files {
            #[cfg(feature = "test-store-crash")]
            super::test_hooks::crash_if("maintenance_scratch_before_remove");
            fs::remove_file(path)?;
            report.removed_scratch_files += 1;
            #[cfg(feature = "test-store-crash")]
            super::test_hooks::crash_if("maintenance_scratch_after_remove");
        }
        let fence = GenerationFence::maintenance(
            self.factory.layout(),
            &self.run.run_id,
            &self.run.owner_id,
            self.run.owner_pid,
            self.fencing_token,
            wall_now_ms()?,
        );
        let mut writer = self
            .factory
            .clone()
            .with_generation_fence(fence)
            .open_writer()?;
        let transaction = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let durable_cursor = transaction
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM store_meta
                 WHERE key='maintenance_gc_version_cursor'
                   AND EXISTS(SELECT 1 FROM store_meta
                              WHERE key='maintenance_gc_plan_fingerprint' AND value=?1)",
                [&plan.fingerprint],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        // Re-probe again immediately before first GC delete/demotion cohort.
        self.ensure_gc_capacity(plan)?;
        report.removed_pins = transaction.execute(
            "DELETE FROM resolution_pins
             WHERE CAST(strftime('%s',expires_at) AS INTEGER)<=?1",
            [self.run.now_ms.div_euclid(1000)],
        )?;
        for delta in &plan.eligible_deltas {
            let (view_id, generation) = parse_scoped_generation(delta)?;
            report.removed_deltas += retire_resolution_delta(&transaction, view_id, generation)?;
        }
        let mut base_files = Vec::new();
        for base_id in &plan.eligible_bases {
            let candidate = transaction
                .query_row(
                    "SELECT relative_path,file_bytes,file_sha256 FROM resolution_bases
                     WHERE base_id=?1 AND state='ready'
                       AND NOT EXISTS(SELECT 1 FROM views WHERE resolution_base_id=?1)
                       AND NOT EXISTS(SELECT 1 FROM resolution_pins WHERE base_id=?1)
                       AND NOT EXISTS(SELECT 1 FROM resolution_deltas WHERE base_id=?1)",
                    [base_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?;
            let Some((relative_path, recorded_bytes, recorded_sha256)) = candidate else {
                continue;
            };
            let path = checked_base_path(self.factory.layout(), &relative_path)?;
            let actual_bytes = resolution_file_bytes(&path)?;
            if actual_bytes
                != u64::try_from(recorded_bytes).map_err(|_| MaintenanceError::InvalidMetadata {
                    field: "resolution_base_bytes",
                    value: recorded_bytes.to_string(),
                })?
            {
                return Err(MaintenanceError::InvalidMetadata {
                    field: "resolution_base_bytes",
                    value: base_id.clone(),
                });
            }
            if resolution_file_sha256(&path)? != recorded_sha256 {
                return Err(MaintenanceError::InvalidMetadata {
                    field: "resolution_base_sha256",
                    value: base_id.clone(),
                });
            }
            report.removed_bases += retire_resolution_base(&transaction, base_id)?;
            base_files.push(path);
        }
        for (view_id, generation) in &plan.eligible_manifests {
            transaction.execute(
                "DELETE FROM manifest_entries
                 WHERE view_id=?1 AND generation=?2
                   AND EXISTS(SELECT 1 FROM manifests
                              WHERE manifests.view_id=manifest_entries.view_id
                                AND manifests.generation=manifest_entries.generation)
                   AND NOT EXISTS(SELECT 1 FROM views
                                  WHERE views.view_id=manifest_entries.view_id
                                    AND views.current_generation=manifest_entries.generation)
                   AND NOT EXISTS(SELECT 1 FROM resolution_pins
                                  WHERE resolution_pins.view_id=manifest_entries.view_id
                                    AND resolution_pins.manifest_generation=manifest_entries.generation)",
                params![view_id, generation],
            )?;
            report.removed_manifests += transaction.execute(
                "DELETE FROM manifests
                 WHERE view_id=?1 AND generation=?2
                   AND NOT EXISTS(SELECT 1 FROM views
                                  WHERE views.view_id=manifests.view_id
                                    AND views.current_generation=manifests.generation)
                   AND NOT EXISTS(SELECT 1 FROM resolution_pins
                                  WHERE resolution_pins.view_id=manifests.view_id
                                    AND resolution_pins.manifest_generation=manifests.generation)",
                params![view_id, generation],
            )?;
        }
        for candidate in &plan.demotion_cohort {
            if candidate.version_id <= durable_cursor {
                continue;
            }
            if candidate.drop_l3 {
                delete_level_rows(&transaction, candidate.version_id, MaintenanceLevel::L3)?;
                let changed = transaction.execute(
                    "UPDATE file_versions SET complete_l3=NULL
                     WHERE version_id=?1 AND complete_l3 IS NOT NULL",
                    [candidate.version_id],
                )?;
                report.demoted_l3 += changed;
            } else if candidate.drop_l2 {
                delete_level_rows(&transaction, candidate.version_id, MaintenanceLevel::L2)?;
                let changed = transaction.execute(
                    "UPDATE file_versions SET complete_l2=NULL,complete_l3=NULL
                     WHERE version_id=?1 AND complete_l2 IS NOT NULL AND complete_l3 IS NULL",
                    [candidate.version_id],
                )?;
                report.demoted_l2 += changed;
            }
            report.last_version_cursor = Some(candidate.version_id);
        }
        if plan.demotion_cohort.is_empty() {
            for decision in &plan.versions {
                if decision.version_id <= durable_cursor {
                    continue;
                }
                if !decision.l1_reasons.is_empty()
                    || !decision.l2_reasons.is_empty()
                    || !decision.l3_reasons.is_empty()
                {
                    continue;
                }
                let changed = transaction.execute(
                    "DELETE FROM file_versions
                     WHERE version_id=?1 AND complete_l2 IS NULL AND complete_l3 IS NULL
                       AND NOT EXISTS(SELECT 1 FROM manifest_entries WHERE version_id=?1)
                       AND NOT EXISTS(SELECT 1 FROM resolution_base_versions WHERE version_id=?1)
                       AND NOT EXISTS(SELECT 1 FROM resolution_identifier_deltas
                                      WHERE version_id=?1 OR target_version_id=?1)
                       AND NOT EXISTS(SELECT 1 FROM resolution_pending_deltas
                                      WHERE version_id=?1 OR target_version_id=?1)",
                    [decision.version_id],
                )?;
                report.purged_versions += changed;
                if changed == 1 {
                    report.last_version_cursor = Some(decision.version_id);
                }
            }
        }
        if let Some(cursor) = report.last_version_cursor {
            transaction.execute(
                "INSERT INTO store_meta(key,value)
                 VALUES ('maintenance_gc_plan_fingerprint',?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [&plan.fingerprint],
            )?;
            transaction.execute(
                "INSERT INTO store_meta(key,value)
                 VALUES ('maintenance_gc_version_cursor',?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [cursor.to_string()],
            )?;
        }
        self.validate_ownership(plan)?;
        self.validate_coordinator_binding(plan)?;
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("maintenance_store_before_commit");
        transaction.commit()?;
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("maintenance_store_after_commit");
        for path in base_files {
            #[cfg(feature = "test-store-crash")]
            super::test_hooks::crash_if("maintenance_base_before_remove");
            fs::remove_file(&path)?;
            report.removed_base_files += 1;
            #[cfg(feature = "test-store-crash")]
            super::test_hooks::crash_if("maintenance_base_after_remove");
        }
        for path in orphan_base_files(self.factory.layout(), &writer)? {
            #[cfg(feature = "test-store-crash")]
            super::test_hooks::crash_if("maintenance_base_before_remove");
            fs::remove_file(&path)?;
            report.removed_base_files += 1;
            #[cfg(feature = "test-store-crash")]
            super::test_hooks::crash_if("maintenance_base_after_remove");
        }
        let (safe_sequence, _high_water) = self.safe_log_sequence(plan)?;
        let completed_before = self.run.now_ms.saturating_sub(policy.request_safety_ms);
        let mut coordinator = StoreCoordinator::open(self.factory.layout())?;
        let archived = coordinator.archive_terminal_requests(
            self.factory.layout().generation_name(),
            completed_before,
            safe_sequence,
            policy.receipt_limit,
        )?;
        report.archived_requests = archived.len();
        #[cfg(feature = "test-store-crash")]
        super::test_hooks::crash_if("maintenance_after_coordinator_archive");
        let receipt_ids = eligible_receipt_ids(
            self.factory.layout().coordinator_db(),
            completed_before,
            safe_sequence,
            policy.receipt_limit,
        )?;
        if !receipt_ids.is_empty() {
            self.validate_ownership(plan)?;
            let fence = GenerationFence::maintenance(
                self.factory.layout(),
                &self.run.run_id,
                &self.run.owner_id,
                self.run.owner_pid,
                self.fencing_token,
                wall_now_ms()?,
            );
            let mut log_writer = self
                .factory
                .clone()
                .with_generation_fence(fence)
                .open_writer()?;
            let log_transaction =
                log_writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
            for request_id in receipt_ids {
                report.pruned_log_rows += StoreLog::prune_receipted_request(
                    &log_transaction,
                    &request_id,
                    safe_sequence,
                )?;
            }
            #[cfg(feature = "test-store-crash")]
            super::test_hooks::crash_if("maintenance_log_before_commit");
            log_transaction.commit()?;
            #[cfg(feature = "test-store-crash")]
            super::test_hooks::crash_if("maintenance_log_after_commit");
            drop(log_writer);
        }
        report.checkpoint_order.push("checkpoint".to_string());
        writer.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;
        let store_bytes_before_vacuum = file_len(self.factory.layout().store_db())?;
        let freelist_pages_before_vacuum =
            sqlite_u64(&writer, "PRAGMA freelist_count", "freelist_count")?;
        report
            .checkpoint_order
            .push("incremental_vacuum".to_string());
        let vacuum_pages = Self::step_incremental_vacuum(&writer, policy.incremental_vacuum_pages)?;
        let freelist_pages_after_vacuum =
            sqlite_u64(&writer, "PRAGMA freelist_count", "freelist_count")?;
        report
            .checkpoint_order
            .push("truncate_checkpoint".to_string());
        writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        report.store_bytes_before_vacuum = store_bytes_before_vacuum;
        report.store_bytes_after_vacuum = file_len(self.factory.layout().store_db())?;
        report.freelist_pages_before_vacuum = freelist_pages_before_vacuum;
        report.freelist_pages_after_vacuum = freelist_pages_after_vacuum;
        report.vacuum_pages = vacuum_pages;
        let physical_bytes_after = physical_bytes(&writer, self.factory.layout())?;
        let physical_target_breached = physical_bytes_after > plan.retention.physical_target_bytes;
        let physical_ceiling_breached =
            physical_bytes_after > plan.retention.physical_ceiling_bytes;
        let previous_streak = writer
            .query_row(
                "SELECT value FROM store_meta
                 WHERE key='retention_physical_breach_streak'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|value| {
                value
                    .parse::<u32>()
                    .map_err(|_| MaintenanceError::InvalidMetadata {
                        field: "retention_physical_breach_streak",
                        value,
                    })
            })
            .transpose()?
            .unwrap_or(0);
        let physical_breach_streak = if physical_target_breached {
            previous_streak.saturating_add(1)
        } else {
            0
        };
        let compaction_required = physical_breach_streak >= plan.retention.physical_breach_limit;
        let metadata = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
        metadata.execute(
            "INSERT INTO store_meta(key,value)
             VALUES ('retention_physical_baseline_bytes',?1)
             ON CONFLICT(key) DO NOTHING",
            [plan.retention.physical_baseline_bytes.to_string()],
        )?;
        metadata.execute(
            "INSERT INTO store_meta(key,value)
             VALUES ('retention_physical_breach_streak',?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [physical_breach_streak.to_string()],
        )?;
        metadata.commit()?;
        report.physical_bytes_before = plan.retention.physical_current_bytes;
        report.physical_bytes_after = physical_bytes_after;
        report.physical_baseline_bytes = plan.retention.physical_baseline_bytes;
        report.physical_target_bytes = plan.retention.physical_target_bytes;
        report.physical_ceiling_bytes = plan.retention.physical_ceiling_bytes;
        report.physical_target_breached = physical_target_breached;
        report.physical_ceiling_breached = physical_ceiling_breached;
        report.physical_breach_streak = physical_breach_streak;
        report.compaction_required = compaction_required;
        drop(writer);
        self.finish()?;
        Ok(report)
    }

    pub(crate) fn step_incremental_vacuum(
        connection: &Connection,
        pages_per_step: usize,
    ) -> Result<u64, MaintenanceError> {
        let page_budget =
            u64::try_from(pages_per_step).map_err(|_| MaintenanceError::InvalidPolicy {
                field: "incremental_vacuum_pages",
            })?;
        let mut vacuum_pages = 0_u64;
        loop {
            let before = sqlite_u64(connection, "PRAGMA freelist_count", "freelist_count")?;
            if before == 0 || vacuum_pages >= page_budget {
                return Ok(vacuum_pages);
            }
            let requested = before.min(page_budget - vacuum_pages);
            connection.execute_batch(&format!("PRAGMA incremental_vacuum({requested});"))?;
            let after = sqlite_u64(connection, "PRAGMA freelist_count", "freelist_count")?;
            if after >= before {
                return Err(MaintenanceError::InvalidMetadata {
                    field: "incremental_vacuum",
                    value: format!("freelist did not decrease from {before}"),
                });
            }
            vacuum_pages = vacuum_pages.saturating_add(before - after);
        }
    }

    fn safe_log_sequence(&self, plan: &MaintenancePlan) -> Result<(i64, i64), MaintenanceError> {
        let coord = Connection::open_with_flags(
            self.factory.layout().coordinator_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let allocator_high_water = coord
            .query_row(
                "SELECT high_water FROM family_allocator_marks
                 WHERE allocator_kind='store_log' AND scope_id=''",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        let high_water = allocator_high_water.max(plan.binding.store_log_max);
        let mut safe = high_water;
        let mut statement = coord.prepare(
            "SELECT consumer_id,generation_name,store_log_sequence
             FROM consumer_cursors ORDER BY consumer_id",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let consumer_id: String = row.get(0)?;
            let generation_name: String = row.get(1)?;
            let sequence: i64 = row.get(2)?;
            checked_generation_path(self.factory.layout(), &generation_name)?;
            if sequence < 0 || sequence > high_water {
                return Err(MaintenanceError::InvalidMetadata {
                    field: "consumer_cursor",
                    value: format!("{consumer_id}:{sequence}:{high_water}"),
                });
            }
            safe = safe.min(sequence);
        }
        Ok((safe, high_water))
    }

    fn validate_ownership(&self, plan: &MaintenancePlan) -> Result<(), MaintenanceError> {
        let coord = Connection::open_with_flags(
            self.factory.layout().coordinator_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let valid = coord.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM maintenance_intent i JOIN writer_lease l
                 ON l.resource='store-writer'
               WHERE i.resource='store-maintenance' AND i.run_id=?1 AND i.owner_id=?2
                 AND i.owner_pid=?3 AND i.fencing_token=?4 AND i.plan_fingerprint=?5
                 AND i.source_generation_name=?6 AND l.holder_id=i.owner_id
                 AND l.holder_pid=i.owner_pid AND l.fencing_token=i.fencing_token
                 AND i.expires_at>?7 AND l.expires_at>?7)",
            params![
                self.run.run_id,
                self.run.owner_id,
                self.run.owner_pid,
                self.fencing_token,
                plan.fingerprint,
                self.factory.layout().generation_name(),
                wall_now_ms()?,
            ],
            |row| row.get::<_, bool>(0),
        )?;
        if valid {
            Ok(())
        } else {
            Err(MaintenanceError::MaintenanceFenceLost)
        }
    }

    fn validate_plan_binding(&self, plan: &MaintenancePlan) -> Result<(), MaintenanceError> {
        let observed = MaintenanceInspector::new(
            self.factory.clone(),
            RevalidationClock(self.run.now_ms),
            LiveCapacityProbe {
                provider: self.capacity.as_ref(),
            },
        )
        .inspect()?;
        if observed.binding == plan.binding {
            Ok(())
        } else {
            Err(MaintenanceError::StalePlan)
        }
    }

    fn validate_coordinator_binding(&self, plan: &MaintenancePlan) -> Result<(), MaintenanceError> {
        let observed = MaintenanceInspector::new(
            self.factory.clone(),
            RevalidationClock(self.run.now_ms),
            LiveCapacityProbe {
                provider: self.capacity.as_ref(),
            },
        )
        .inspect()?;
        let expected = &plan.binding;
        let actual = &observed.binding;
        if actual.family_id == expected.family_id
            && actual.current_generation == expected.current_generation
            && actual.coordinator_root_fingerprint == expected.coordinator_root_fingerprint
            && actual.store_log_max == expected.store_log_max
            && actual.request_watermark == expected.request_watermark
            && actual.allocator_marks == expected.allocator_marks
        {
            Ok(())
        } else {
            Err(MaintenanceError::StalePlan)
        }
    }

    fn validate_generation_publish_binding(
        &self,
        plan: &MaintenancePlan,
    ) -> Result<(), MaintenanceError> {
        let observed = MaintenanceInspector::new(
            self.factory.clone(),
            RevalidationClock(self.run.now_ms),
            LiveCapacityProbe {
                provider: self.capacity.as_ref(),
            },
        )
        .inspect()?;
        let expected = &plan.binding;
        let actual = &observed.binding;
        if actual.family_id == expected.family_id
            && actual.current_generation == expected.current_generation
            && actual.store_log_max == expected.store_log_max
            && actual.request_watermark == expected.request_watermark
        {
            Ok(())
        } else {
            Err(MaintenanceError::StalePlan)
        }
    }

    fn finish(&self) -> Result<(), MaintenanceError> {
        self.restore_serving_source_floor_and_clear_coord()
    }

    fn raise_source_floor_and_mirror(
        &self,
        action: MaintenanceAction,
        wall_now: i64,
    ) -> Result<(), MaintenanceError> {
        let generation_state = Connection::open_with_flags(
            self.factory.layout().store_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?
        .query_row(
            "SELECT value FROM store_meta WHERE key=?1",
            [META_GENERATION_STATE],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .unwrap_or_default();
        // Mid-publish recovery can open a retired CURRENT generation. Floor raise applies only
        // while the bound generation is still serving writers.
        if generation_state != "serving" {
            return Ok(());
        }
        let binary = self.factory.binary_version();
        let raised = match compare_versions(binary, &self.source_min_writer_version)? {
            Ordering::Greater => binary.to_string(),
            _ => self.source_min_writer_version.clone(),
        };
        let plan_fingerprint: String = {
            let coord = Connection::open_with_flags(
                self.factory.layout().coordinator_db(),
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            coord.query_row(
                "SELECT plan_fingerprint FROM maintenance_intent
                 WHERE resource='store-maintenance' AND run_id=?1 AND fencing_token=?2",
                params![self.run.run_id, self.fencing_token],
                |row| row.get(0),
            )?
        };
        let fence = GenerationFence::maintenance(
            self.factory.layout(),
            &self.run.run_id,
            &self.run.owner_id,
            self.run.owner_pid,
            self.fencing_token,
            wall_now,
        );
        let mut writer = self
            .factory
            .clone()
            .with_generation_fence(fence)
            .open_writer()?;
        let transaction = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE store_meta SET value=?1 WHERE key=?2",
            params![raised, META_MIN_WRITER_VERSION],
        )?;
        let owner_pid = self.run.owner_pid.to_string();
        let fencing_token = self.fencing_token.to_string();
        let heartbeat = wall_now.to_string();
        let mirror_rows = [
            (TMP_RUN_ID, self.run.run_id.as_str()),
            (TMP_ACTION, action.as_str()),
            (TMP_SOURCE_GENERATION, self.factory.layout().generation_name()),
            (TMP_OWNER_ID, self.run.owner_id.as_str()),
            (TMP_OWNER_PID, owner_pid.as_str()),
            (TMP_FENCING_TOKEN, fencing_token.as_str()),
            (TMP_HEARTBEAT_AT, heartbeat.as_str()),
            (TMP_STARTED_AT, heartbeat.as_str()),
            (TMP_PLAN_FINGERPRINT, plan_fingerprint.as_str()),
            (TMP_SOURCE_MIN_WRITER, self.source_min_writer_version.as_str()),
        ];
        for (key, value) in mirror_rows {
            transaction.execute(
                "INSERT INTO store_meta(key,value) VALUES (?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn restore_serving_source_floor_and_clear_coord(&self) -> Result<(), MaintenanceError> {
        let generation_state = Connection::open_with_flags(
            self.factory.layout().store_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?
        .query_row(
            "SELECT value FROM store_meta WHERE key=?1",
            [META_GENERATION_STATE],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .unwrap_or_default();
        if generation_state == "serving" {
            let wall_now = wall_now_ms()?;
            let fence = GenerationFence::maintenance(
                self.factory.layout(),
                &self.run.run_id,
                &self.run.owner_id,
                self.run.owner_pid,
                self.fencing_token,
                wall_now,
            );
            let mut writer = self
                .factory
                .clone()
                .with_generation_fence(fence)
                .open_writer()?;
            let transaction = writer.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute(
                "UPDATE store_meta SET value=?1 WHERE key=?2",
                params![self.source_min_writer_version, META_MIN_WRITER_VERSION],
            )?;
            transaction.execute(
                "DELETE FROM store_meta WHERE key LIKE ?1",
                [format!("{}%", MAINTENANCE_TMP_PREFIX)],
            )?;
            transaction.commit()?;
            drop(writer);
        }
        let mut coord = open_maintenance_coordinator(self.factory.layout().coordinator_db())?;
        let transaction = coord.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let lease_deleted = transaction.execute(
            "DELETE FROM writer_lease
             WHERE resource='store-writer' AND holder_id=?1 AND holder_pid=?2
               AND fencing_token=?3",
            params![self.run.owner_id, self.run.owner_pid, self.fencing_token],
        )?;
        let intent_deleted = transaction.execute(
            "DELETE FROM maintenance_intent
             WHERE resource='store-maintenance' AND run_id=?1 AND owner_id=?2
               AND owner_pid=?3 AND fencing_token=?4",
            params![
                self.run.run_id,
                self.run.owner_id,
                self.run.owner_pid,
                self.fencing_token,
            ],
        )?;
        // After M3 generation build, the writer lease may already be absent; intent must clear.
        if intent_deleted != 1 || lease_deleted > 1 {
            return Err(MaintenanceError::MaintenanceFenceLost);
        }
        transaction.commit()?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct RevalidationClock(i64);

impl MaintenanceClock for RevalidationClock {
    fn now_ms(&self) -> i64 {
        self.0
    }
}

struct LiveCapacityProbe<'a> {
    provider: &'a dyn CapacityProvider,
}

impl CapacityProvider for LiveCapacityProbe<'_> {
    fn free_bytes(&self, path: &Path) -> Result<u64, io::Error> {
        self.provider.free_bytes(path)
    }

    fn staged_generation_bytes(&self, path: &Path) -> Result<u64, io::Error> {
        self.provider.staged_generation_bytes(path)
    }
}

fn required_bytes_for_action(plan: &MaintenancePlan, action: MaintenanceAction) -> u64 {
    match action {
        MaintenanceAction::Gc | MaintenanceAction::Repair => plan.capacity.gc_required_bytes,
        MaintenanceAction::Promote | MaintenanceAction::Rollback => {
            plan.capacity.promotion_required_bytes
        }
    }
}

fn ensure_live_capacity(
    provider: &dyn CapacityProvider,
    root: &Path,
    required_bytes: u64,
) -> Result<(), MaintenanceError> {
    let free_bytes = provider.free_bytes(root)?;
    if free_bytes < required_bytes {
        return Err(MaintenanceError::CapacityInsufficient);
    }
    Ok(())
}

fn validate_run(run: &MaintenanceRun) -> Result<(), MaintenanceError> {
    if run.run_id.is_empty()
        || run.run_id.len() > 128
        || run.owner_id.is_empty()
        || run.owner_id.len() > 128
        || run.owner_pid == 0
        || run.now_ms < 0
        || run.lease_duration_ms <= 0
    {
        return Err(MaintenanceError::InvalidMetadata {
            field: "maintenance_run",
            value: run.run_id.clone(),
        });
    }
    Ok(())
}

fn wall_now_ms() -> Result<i64, MaintenanceError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| MaintenanceError::InvalidMetadata {
            field: "system_clock",
            value: error.to_string(),
        })?
        .as_millis();
    i64::try_from(millis).map_err(|_| MaintenanceError::InvalidMetadata {
        field: "system_clock",
        value: "overflow".to_string(),
    })
}

fn open_maintenance_coordinator(path: &Path) -> Result<Connection, MaintenanceError> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    configure_writer_pragmas(&connection, WriterPragmaProfile::Routine).map_err(|error| {
        MaintenanceError::InvalidMetadata {
            field: "coordinator_pragmas",
            value: format!("{error:?}"),
        }
    })?;
    Ok(connection)
}

fn parse_scoped_generation(value: &str) -> Result<(&str, i64), MaintenanceError> {
    let (view_id, generation) =
        value
            .rsplit_once(':')
            .ok_or_else(|| MaintenanceError::InvalidMetadata {
                field: "resolution_delta",
                value: value.to_string(),
            })?;
    let generation = generation
        .parse::<i64>()
        .map_err(|_| MaintenanceError::InvalidMetadata {
            field: "resolution_delta",
            value: value.to_string(),
        })?;
    if view_id.is_empty() || generation <= 0 {
        return Err(MaintenanceError::InvalidMetadata {
            field: "resolution_delta",
            value: value.to_string(),
        });
    }
    Ok((view_id, generation))
}

fn checked_base_path(
    layout: &super::StoreLayout,
    relative_path: &str,
) -> Result<PathBuf, MaintenanceError> {
    let relative = Path::new(relative_path);
    let mut components = relative.components();
    if relative_path.contains(['\0', ':'])
        || relative.is_absolute()
        || components.next() != Some(Component::Normal("bases".as_ref()))
        || components.any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(MaintenanceError::InvalidMetadata {
            field: "resolution_base_path",
            value: relative_path.to_string(),
        });
    }
    let path = layout.generation_dir().join(relative);
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(MaintenanceError::InvalidMetadata {
            field: "resolution_base_path",
            value: path.display().to_string(),
        });
    }
    let canonical = path.canonicalize()?;
    let bases = layout.bases_dir().canonicalize()?;
    if !canonical.starts_with(&bases) {
        return Err(MaintenanceError::InvalidMetadata {
            field: "resolution_base_path",
            value: canonical.display().to_string(),
        });
    }
    Ok(canonical)
}

fn checked_generation_path(
    layout: &super::StoreLayout,
    generation_name: &str,
) -> Result<PathBuf, MaintenanceError> {
    if !valid_generation_name(generation_name) {
        return Err(MaintenanceError::InvalidMetadata {
            field: "consumer_cursor_generation",
            value: generation_name.to_string(),
        });
    }
    let path = layout.root().join(generation_name);
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(MaintenanceError::InvalidMetadata {
            field: "consumer_cursor_generation",
            value: path.display().to_string(),
        });
    }
    let canonical = path.canonicalize()?;
    let root = layout.root().canonicalize()?;
    if !canonical.starts_with(&root) {
        return Err(MaintenanceError::InvalidMetadata {
            field: "consumer_cursor_generation",
            value: canonical.display().to_string(),
        });
    }
    Ok(canonical)
}

fn eligible_receipt_ids(
    coordinator_db: &Path,
    completed_before: i64,
    maximum_log_sequence: i64,
    limit: usize,
) -> Result<Vec<String>, MaintenanceError> {
    let connection = Connection::open_with_flags(
        coordinator_db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.pragma_update(None, "query_only", "ON")?;
    let mut statement = connection.prepare(
        "SELECT request_id FROM request_receipts
         WHERE completed_at<=?1 AND terminal_log_sequence<=?2
         ORDER BY terminal_log_sequence,request_id LIMIT ?3",
    )?;
    Ok(statement
        .query_map(
            params![completed_before, maximum_log_sequence, limit as i64],
            |row| row.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?)
}

fn terminal_request_scratch_files(
    layout: &super::StoreLayout,
) -> Result<Vec<PathBuf>, MaintenanceError> {
    let coordinator = Connection::open_with_flags(
        layout.coordinator_db(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    coordinator.pragma_update(None, "query_only", "ON")?;
    let mut files = Vec::new();
    for entry in fs::read_dir(layout.scratch_dir())? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Some(request_id) = request_id_from_scratch_name(&name) else {
            continue;
        };
        let terminal = coordinator.query_row(
            "SELECT EXISTS(SELECT 1 FROM requests
                           WHERE request_id=?1 AND state IN ('failed','committed','acknowledged'))
                    OR EXISTS(SELECT 1 FROM request_receipts WHERE request_id=?1)",
            [request_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !terminal {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(MaintenanceError::InvalidMetadata {
                field: "scratch_path",
                value: entry.path().display().to_string(),
            });
        }
        let canonical = entry.path().canonicalize()?;
        if !canonical.starts_with(layout.scratch_dir().canonicalize()?) {
            return Err(MaintenanceError::InvalidMetadata {
                field: "scratch_path",
                value: canonical.display().to_string(),
            });
        }
        files.push(canonical);
    }
    files.sort();
    Ok(files)
}

fn request_id_from_scratch_name(name: &str) -> Option<&str> {
    let base = name
        .strip_suffix("-wal")
        .or_else(|| name.strip_suffix("-shm"))
        .unwrap_or(name);
    let base = base.strip_suffix(".work").unwrap_or(base);
    let request = base
        .strip_prefix("resolve-exact-")
        .or_else(|| base.strip_prefix("resolve-delta-"))?
        .strip_suffix(".db")?;
    (!request.is_empty()).then_some(request)
}

fn orphan_base_files(
    layout: &super::StoreLayout,
    store: &Connection,
) -> Result<Vec<PathBuf>, MaintenanceError> {
    let mut statement =
        store.prepare("SELECT relative_path FROM resolution_bases ORDER BY base_id")?;
    let registered = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut orphaned = Vec::new();
    for entry in fs::read_dir(layout.bases_dir())? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(MaintenanceError::InvalidMetadata {
                field: "resolution_base_path",
                value: entry.path().display().to_string(),
            });
        }
        let relative = format!("bases/{}", entry.file_name().to_string_lossy());
        if !registered.contains(&relative) {
            let canonical = entry.path().canonicalize()?;
            if !canonical.starts_with(layout.bases_dir().canonicalize()?) {
                return Err(MaintenanceError::InvalidMetadata {
                    field: "resolution_base_path",
                    value: canonical.display().to_string(),
                });
            }
            orphaned.push(canonical);
        }
    }
    orphaned.sort();
    Ok(orphaned)
}

fn delete_level_rows(
    transaction: &rusqlite::Transaction<'_>,
    version_id: i64,
    level: MaintenanceLevel,
) -> Result<(), MaintenanceError> {
    match level {
        MaintenanceLevel::L3 => {
            for table in [
                "type_arguments",
                "type_argument_usages",
                "literals",
                "source_regions",
                "structural_facts",
            ] {
                transaction.execute(
                    &format!("DELETE FROM {table} WHERE version_id=?1"),
                    [version_id],
                )?;
            }
        }
        MaintenanceLevel::L2 => {
            transaction.execute("DELETE FROM identifiers WHERE version_id=?1", [version_id])?;
            transaction.execute(
                "DELETE FROM reference_sites WHERE version_id=?1 AND level=2",
                [version_id],
            )?;
        }
        MaintenanceLevel::L1 => {
            return Err(MaintenanceError::InvalidMetadata {
                field: "demotion_level",
                value: "l1".to_string(),
            });
        }
    }
    Ok(())
}

fn data_version(connection: &Connection) -> Result<i64, MaintenanceError> {
    Ok(connection.query_row("PRAGMA data_version", [], |row| row.get(0))?)
}

fn validate_policy(policy: &MaintenancePolicy) -> Result<(), MaintenanceError> {
    if policy.retention_window_days < 0 {
        return Err(MaintenanceError::InvalidPolicy {
            field: "retention_window_days",
        });
    }
    if policy.retention_path_cap == 0 {
        return Err(MaintenanceError::InvalidPolicy {
            field: "retention_path_cap",
        });
    }
    if policy.physical_breach_limit == 0 {
        return Err(MaintenanceError::InvalidPolicy {
            field: "physical_breach_limit",
        });
    }
    if policy.target_denominator == 0 || policy.ceiling_denominator == 0 {
        return Err(MaintenanceError::InvalidPolicy {
            field: "ratio_denominator",
        });
    }
    let target = policy
        .target_numerator
        .saturating_mul(policy.ceiling_denominator);
    let ceiling = policy
        .ceiling_numerator
        .saturating_mul(policy.target_denominator);
    if target > ceiling {
        return Err(MaintenanceError::InvalidPolicy {
            field: "retention_ratios",
        });
    }
    Ok(())
}

fn require_version(
    versions: &BTreeMap<i64, &VersionFact>,
    version_id: i64,
    kind: &'static str,
) -> Result<(), MaintenanceError> {
    if versions.contains_key(&version_id) {
        Ok(())
    } else {
        Err(MaintenanceError::UnknownRoot {
            kind,
            id: version_id.to_string(),
        })
    }
}

fn historical_path_ranks<'a>(
    snapshot: &'a MaintenanceSnapshot,
    manifests: &BTreeMap<(&'a str, i64), &'a ManifestFact>,
) -> BTreeMap<(&'a str, &'a str, i64), usize> {
    let mut by_path: BTreeMap<&str, Vec<(&str, i64, i64)>> = BTreeMap::new();
    for entry in &snapshot.manifest_versions {
        if let Some(manifest) = manifests.get(&(entry.view_id.as_str(), entry.generation))
            && !manifest.current
        {
            by_path.entry(&entry.path).or_default().push((
                &entry.view_id,
                entry.generation,
                manifest.created_at_ms,
            ));
        }
    }
    let mut ranks = BTreeMap::new();
    for (path, mut rows) in by_path {
        rows.sort_by(|left, right| (right.2, right.1, right.0).cmp(&(left.2, left.1, left.0)));
        rows.dedup();
        for (rank, (view_id, generation, _)) in rows.into_iter().enumerate() {
            ranks.insert((path, view_id, generation), rank);
        }
    }
    ranks
}

fn add_completed_reasons(
    reasons: &mut BTreeMap<i64, [BTreeSet<ProtectionReason>; 3]>,
    version: &VersionFact,
    kind: MaintenanceRootKind,
    reference: String,
) {
    if version.complete_l1 {
        add_reason(
            reasons,
            version.version_id,
            MaintenanceLevel::L1,
            kind,
            reference.clone(),
        );
    }
    if version.complete_l2 {
        add_reason(
            reasons,
            version.version_id,
            MaintenanceLevel::L2,
            kind,
            reference.clone(),
        );
    }
    if version.complete_l3 {
        add_reason(
            reasons,
            version.version_id,
            MaintenanceLevel::L3,
            kind,
            reference,
        );
    }
}

fn add_reason(
    reasons: &mut BTreeMap<i64, [BTreeSet<ProtectionReason>; 3]>,
    version_id: i64,
    level: MaintenanceLevel,
    kind: MaintenanceRootKind,
    reference: String,
) {
    let index = match level {
        MaintenanceLevel::L1 => 0,
        MaintenanceLevel::L2 => 1,
        MaintenanceLevel::L3 => 2,
    };
    reasons.get_mut(&version_id).expect("validated root")[index]
        .insert(ProtectionReason { kind, reference });
}

fn apply_delta_roots(
    versions: &BTreeMap<i64, &VersionFact>,
    reasons: &mut BTreeMap<i64, [BTreeSet<ProtectionReason>; 3]>,
    roots: &[DeltaVersionFact],
    source_kind: MaintenanceRootKind,
    target_kind: MaintenanceRootKind,
) -> Result<(), MaintenanceError> {
    for root in roots {
        require_version(versions, root.source_version_id, "delta source version")?;
        let reference = format!("{}:{}", root.view_id, root.delta_generation);
        for level in [MaintenanceLevel::L1, MaintenanceLevel::L2] {
            add_reason(
                reasons,
                root.source_version_id,
                level,
                source_kind,
                reference.clone(),
            );
        }
        if let Some(target) = root.target_version_id {
            require_version(versions, target, "delta target version")?;
            for level in [MaintenanceLevel::L1, MaintenanceLevel::L2] {
                add_reason(reasons, target, level, target_kind, reference.clone());
            }
        }
    }
    Ok(())
}

fn levels_through(max_level: MaintenanceLevel) -> impl Iterator<Item = MaintenanceLevel> {
    [
        MaintenanceLevel::L1,
        MaintenanceLevel::L2,
        MaintenanceLevel::L3,
    ]
    .into_iter()
    .take(match max_level {
        MaintenanceLevel::L1 => 1,
        MaintenanceLevel::L2 => 2,
        MaintenanceLevel::L3 => 3,
    })
}

fn ratio_bytes(bytes: u64, numerator: u64, denominator: u64) -> Result<u64, MaintenanceError> {
    bytes
        .checked_mul(numerator)
        .map(|value| value / denominator)
        .ok_or(MaintenanceError::InvalidMetadata {
            field: "retention_ratio",
            value: "overflow".to_string(),
        })
}

fn retention_only(kind: MaintenanceRootKind) -> bool {
    matches!(
        kind,
        MaintenanceRootKind::HistoricalManifest
            | MaintenanceRootKind::RetentionWindow
            | MaintenanceRootKind::PathCap
    )
}

fn demotion_cohort(
    versions: &[VersionFact],
    decisions: &[VersionDecision],
) -> Vec<DemotionCandidate> {
    let decision_by_id: BTreeMap<_, _> = decisions
        .iter()
        .map(|decision| (decision.version_id, decision))
        .collect();
    let mut candidates = Vec::new();
    let mut ordered_versions: Vec<_> = versions.iter().collect();
    ordered_versions.sort_by_key(|version| version.version_id);
    for l3_pass in [true, false] {
        let mut bytes = 0_u64;
        for version in &ordered_versions {
            let decision = decision_by_id[&version.version_id];
            let eligible = if l3_pass {
                version.complete_l3 && decision.l3_reasons.is_empty()
            } else {
                !version.complete_l3 && version.complete_l2 && decision.l2_reasons.is_empty()
            };
            if !eligible {
                continue;
            }
            let estimated = version.logical_bytes.clamp(4096, MAX_DEMOTION_BYTES);
            if candidates.len() >= MAX_DEMOTION_VERSIONS
                || bytes.saturating_add(estimated) > MAX_DEMOTION_BYTES
            {
                break;
            }
            bytes += estimated;
            candidates.push(DemotionCandidate {
                version_id: version.version_id,
                estimated_dirty_bytes: estimated,
                drop_l3: l3_pass,
                drop_l2: !l3_pass,
            });
        }
        if !candidates.is_empty() {
            break;
        }
    }
    candidates.sort_by_key(|candidate| candidate.version_id);
    candidates
}

fn canonical_binding(binding: &PlanBinding) -> PlanBinding {
    let mut binding = binding.clone();
    binding.allocator_marks.sort_by(|left, right| {
        (left.kind.as_str(), left.scope_id.as_str())
            .cmp(&(right.kind.as_str(), right.scope_id.as_str()))
    });
    binding
}

fn sorted_unique(values: &[String]) -> Vec<String> {
    values
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn plan_fingerprint(plan: &MaintenancePlan) -> Result<String, MaintenanceError> {
    let mut normalized = plan.clone();
    normalized.fingerprint.clear();
    normalized.max_observed_window = 0;
    let bytes = serde_json::to_vec(&normalized)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn read_binding(
    store: &Connection,
    coord: &Connection,
    factory: &StoreConnectionFactory,
) -> Result<PlanBinding, MaintenanceError> {
    let family_id = store.query_row(
        "SELECT value FROM store_meta WHERE key='family_id'",
        [],
        |row| row.get(0),
    )?;
    let store_log_max = store.query_row(
        "SELECT COALESCE(MAX(sequence),0) FROM store_log",
        [],
        |row| row.get(0),
    )?;
    let request_watermark = coord.query_row(
        "SELECT COALESCE(MAX(watermark),0) FROM (
           SELECT updated_at AS watermark FROM requests
           UNION ALL
           SELECT completed_at AS watermark FROM request_receipts
         )",
        [],
        |row| row.get(0),
    )?;
    let mut allocator_marks = Vec::new();
    let mut statement = coord.prepare("SELECT allocator_kind,scope_id,high_water FROM family_allocator_marks ORDER BY allocator_kind,scope_id")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        allocator_marks.push(AllocatorMark {
            kind: row.get(0)?,
            scope_id: row.get(1)?,
            high_water: row.get(2)?,
        });
    }
    Ok(PlanBinding {
        family_id,
        current_generation: factory.layout().generation_name().to_string(),
        store_root_fingerprint: root_fingerprint(factory.layout().store_db())?,
        coordinator_root_fingerprint: root_fingerprint(factory.layout().coordinator_db())?,
        store_log_max,
        request_watermark,
        allocator_marks,
    })
}

fn root_fingerprint(path: &Path) -> Result<String, MaintenanceError> {
    let canonical = path.canonicalize()?;
    let metadata = fs::metadata(&canonical)?;
    let mut digest = Sha256::new();
    digest.update(canonical.as_os_str().as_encoded_bytes());
    digest.update(metadata.len().to_le_bytes());
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn store_root_fingerprint(snapshot: &MaintenanceSnapshot) -> Result<String, MaintenanceError> {
    #[derive(Serialize)]
    struct StoreRoots<'a> {
        physical: &'a str,
        versions: &'a [VersionFact],
        manifests: &'a [ManifestFact],
        manifest_versions: &'a [ManifestVersionFact],
        failed_paths: &'a [FailedPathFact],
        base_versions: &'a [BaseVersionFact],
        identifier_delta_versions: &'a [DeltaVersionFact],
        pending_delta_versions: &'a [DeltaVersionFact],
        additional_version_roots: &'a [VersionRootFact],
        bases: &'a [String],
        eligible_bases: &'a [String],
        deltas: &'a [String],
        eligible_deltas: &'a [String],
        pins: &'a [String],
        expired_pins: &'a [String],
        scratch: &'a [String],
        generations: &'a [String],
    }
    let roots = StoreRoots {
        physical: &snapshot.binding.store_root_fingerprint,
        versions: &snapshot.versions,
        manifests: &snapshot.manifests,
        manifest_versions: &snapshot.manifest_versions,
        failed_paths: &snapshot.failed_paths,
        base_versions: &snapshot.base_versions,
        identifier_delta_versions: &snapshot.identifier_delta_versions,
        pending_delta_versions: &snapshot.pending_delta_versions,
        additional_version_roots: &snapshot.additional_version_roots,
        bases: &snapshot.protected_bases,
        eligible_bases: &snapshot.eligible_bases,
        deltas: &snapshot.protected_deltas,
        eligible_deltas: &snapshot.eligible_deltas,
        pins: &snapshot.protected_pins,
        expired_pins: &snapshot.expired_pins,
        scratch: &snapshot.protected_scratch,
        generations: &snapshot.protected_generations,
    };
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&roots)?)
    ))
}

fn coordinator_root_fingerprint(
    snapshot: &MaintenanceSnapshot,
) -> Result<String, MaintenanceError> {
    #[derive(Serialize)]
    struct CoordinatorRoots<'a> {
        physical: &'a str,
        requests: &'a [String],
        cursors: &'a [String],
        request_facts: &'a [CoordinatorRequestFact],
        cursor_facts: &'a [ConsumerCursorFact],
        allocator_marks: &'a [AllocatorMark],
        request_watermark: i64,
    }
    let roots = CoordinatorRoots {
        physical: &snapshot.binding.coordinator_root_fingerprint,
        requests: &snapshot.protected_requests,
        cursors: &snapshot.protected_cursors,
        request_facts: &snapshot.request_facts,
        cursor_facts: &snapshot.cursor_facts,
        allocator_marks: &snapshot.binding.allocator_marks,
        request_watermark: snapshot.binding.request_watermark,
    };
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&roots)?)
    ))
}

fn read_policy(store: &Connection) -> Result<MaintenancePolicy, MaintenanceError> {
    let integer = |key: &'static str| -> Result<i64, MaintenanceError> {
        let value: String =
            store.query_row("SELECT value FROM store_meta WHERE key=?1", [key], |row| {
                row.get(0)
            })?;
        value
            .parse()
            .map_err(|_| MaintenanceError::InvalidMetadata { field: key, value })
    };
    let ratio = |key: &'static str| -> Result<(u64, u64), MaintenanceError> {
        let value: String =
            store.query_row("SELECT value FROM store_meta WHERE key=?1", [key], |row| {
                row.get(0)
            })?;
        let Some((whole, fraction)) = value.split_once('.') else {
            return Err(MaintenanceError::InvalidMetadata { field: key, value });
        };
        let denominator = 10_u64.checked_pow(fraction.len() as u32).ok_or_else(|| {
            MaintenanceError::InvalidMetadata {
                field: key,
                value: value.clone(),
            }
        })?;
        let whole: u64 = whole
            .parse()
            .map_err(|_| MaintenanceError::InvalidMetadata {
                field: key,
                value: value.clone(),
            })?;
        let fraction: u64 = fraction
            .parse()
            .map_err(|_| MaintenanceError::InvalidMetadata {
                field: key,
                value: value.clone(),
            })?;
        Ok((
            whole.saturating_mul(denominator).saturating_add(fraction),
            denominator,
        ))
    };
    let (target_numerator, target_denominator) = ratio("retention_byte_target")?;
    let (ceiling_numerator, ceiling_denominator) = ratio("retention_byte_ceiling")?;
    let physical_breach_limit = store
        .query_row(
            "SELECT value FROM store_meta WHERE key='retention_physical_breach_limit'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|_| MaintenanceError::InvalidMetadata {
                    field: "retention_physical_breach_limit",
                    value,
                })
        })
        .transpose()?
        .unwrap_or(DEFAULT_PHYSICAL_BREACH_LIMIT);
    Ok(MaintenancePolicy {
        retention_window_days: integer("retention_window_days")?,
        retention_path_cap: usize::try_from(integer("retention_path_cap")?).map_err(|_| {
            MaintenanceError::InvalidMetadata {
                field: "retention_path_cap",
                value: "out_of_range".to_string(),
            }
        })?,
        target_numerator,
        target_denominator,
        ceiling_numerator,
        ceiling_denominator,
        physical_breach_limit,
    })
}

fn read_capacity<P: CapacityProvider>(
    store: &Connection,
    factory: &StoreConnectionFactory,
    provider: &P,
) -> Result<MaintenanceCapacity, MaintenanceError> {
    let page_size = sqlite_u64(store, "PRAGMA page_size", "page_size")?;
    let page_count = sqlite_u64(store, "PRAGMA page_count", "page_count")?;
    let freelist = sqlite_u64(store, "PRAGMA freelist_count", "freelist_count")?;
    let store_wal_bytes = file_len(Path::new(&format!(
        "{}-wal",
        factory.layout().store_db().display()
    )))?;
    let base_bytes = directory_bytes(factory.layout().bases_dir())?;
    let scratch_bytes = directory_bytes(factory.layout().scratch_dir())?;
    let measured_bytes = page_size
        .saturating_mul(page_count)
        .saturating_add(store_wal_bytes)
        .saturating_add(base_bytes)
        .saturating_add(scratch_bytes);
    let retention_baseline_bytes = store
        .query_row(
            "SELECT value FROM store_meta
             WHERE key='retention_physical_baseline_bytes'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| MaintenanceError::InvalidMetadata {
                    field: "retention_physical_baseline_bytes",
                    value,
                })
        })
        .transpose()?
        .unwrap_or(measured_bytes);
    let retention_breach_streak = store
        .query_row(
            "SELECT value FROM store_meta
             WHERE key='retention_physical_breach_streak'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|_| MaintenanceError::InvalidMetadata {
                    field: "retention_physical_breach_streak",
                    value,
                })
        })
        .transpose()?
        .unwrap_or(0);
    Ok(MaintenanceCapacity {
        free_bytes: provider.free_bytes(factory.layout().root())?,
        store_page_bytes: page_size.saturating_mul(page_count),
        store_freelist_bytes: page_size.saturating_mul(freelist),
        store_wal_bytes,
        base_bytes,
        scratch_bytes,
        staged_generation_bytes: provider.staged_generation_bytes(factory.layout().root())?,
        retention_baseline_bytes,
        retention_breach_streak,
    })
}

fn physical_bytes(
    store: &Connection,
    layout: &super::layout::StoreLayout,
) -> Result<u64, MaintenanceError> {
    let page_size = sqlite_u64(store, "PRAGMA page_size", "page_size")?;
    let page_count = sqlite_u64(store, "PRAGMA page_count", "page_count")?;
    Ok(page_size
        .saturating_mul(page_count)
        .saturating_add(file_len(Path::new(&format!(
            "{}-wal",
            layout.store_db().display()
        )))?)
        .saturating_add(directory_bytes(layout.bases_dir())?)
        .saturating_add(directory_bytes(layout.scratch_dir())?))
}

fn sqlite_u64(
    connection: &Connection,
    sql: &str,
    field: &'static str,
) -> Result<u64, MaintenanceError> {
    let value: i64 = connection.query_row(sql, [], |row| row.get(0))?;
    u64::try_from(value).map_err(|_| MaintenanceError::InvalidMetadata {
        field,
        value: value.to_string(),
    })
}

fn file_len(path: &Path) -> Result<u64, MaintenanceError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(MaintenanceError::InvalidMetadata {
                field: "file_path",
                value: path.display().to_string(),
            })
        }
        Ok(metadata) if metadata.is_file() => Ok(metadata.len()),
        Ok(_) => Err(MaintenanceError::InvalidMetadata {
            field: "file_path",
            value: path.display().to_string(),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

fn directory_bytes(path: &Path) -> Result<u64, MaintenanceError> {
    let mut total = 0_u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            return Err(MaintenanceError::InvalidMetadata {
                field: "directory_entry",
                value: entry.path().display().to_string(),
            });
        }
        if metadata.is_file() {
            total = total.saturating_add(resolution_file_bytes(&entry.path())?);
        }
    }
    Ok(total)
}

fn read_versions(
    connection: &Connection,
    limit: usize,
    peak: &mut usize,
    output: &mut Vec<VersionFact>,
) -> Result<(), MaintenanceError> {
    let mut after = 0_i64;
    loop {
        let mut statement = connection.prepare("SELECT version_id,path,content_bytes,complete_l1 IS NOT NULL,complete_l2 IS NOT NULL,complete_l3 IS NOT NULL FROM file_versions WHERE version_id>?1 ORDER BY version_id LIMIT ?2")?;
        let page: Vec<_> = statement
            .query_map(params![after, limit as i64], |row| {
                let logical_bytes: i64 = row.get(2)?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    logical_bytes,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .collect::<Result<Vec<(i64, String, i64, bool, bool, bool)>, _>>()?
            .into_iter()
            .map(
                |(version_id, path, logical_bytes, complete_l1, complete_l2, complete_l3)| {
                    Ok(VersionFact {
                        version_id,
                        path,
                        logical_bytes: u64::try_from(logical_bytes).map_err(|_| {
                            MaintenanceError::InvalidMetadata {
                                field: "content_bytes",
                                value: logical_bytes.to_string(),
                            }
                        })?,
                        complete_l1,
                        complete_l2,
                        complete_l3,
                    })
                },
            )
            .collect::<Result<_, MaintenanceError>>()?;
        observe_page(peak, page.len(), limit)?;
        let Some(last) = page.last() else {
            break;
        };
        after = last.version_id;
        output.extend(page);
    }
    Ok(())
}

fn read_manifests(
    connection: &Connection,
    limit: usize,
    peak: &mut usize,
    output: &mut Vec<ManifestFact>,
) -> Result<(), MaintenanceError> {
    let mut after_view = String::new();
    let mut after_generation = 0_i64;
    loop {
        let mut statement = connection.prepare("SELECT m.view_id,m.generation,CAST(strftime('%s',m.created_at) AS INTEGER)*1000,m.generation IS v.current_generation FROM manifests m JOIN views v ON v.view_id=m.view_id WHERE (m.view_id,m.generation)>(?1,?2) ORDER BY m.view_id,m.generation LIMIT ?3")?;
        let page: Vec<_> = statement
            .query_map(params![after_view, after_generation, limit as i64], |row| {
                Ok(ManifestFact {
                    view_id: row.get(0)?,
                    generation: row.get(1)?,
                    created_at_ms: row.get(2)?,
                    current: row.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        observe_page(peak, page.len(), limit)?;
        let Some(last) = page.last() else {
            break;
        };
        after_view = last.view_id.clone();
        after_generation = last.generation;
        output.extend(page);
    }
    Ok(())
}

fn read_manifest_versions(
    connection: &Connection,
    limit: usize,
    peak: &mut usize,
    output: &mut Vec<ManifestVersionFact>,
) -> Result<(), MaintenanceError> {
    let mut key = (String::new(), 0_i64, String::new());
    loop {
        let mut statement = connection.prepare("SELECT view_id,generation,version_id,path,status='failed_preserved' FROM manifest_entries WHERE version_id IS NOT NULL AND (view_id,generation,path)>(?1,?2,?3) ORDER BY view_id,generation,path LIMIT ?4")?;
        let page: Vec<_> = statement
            .query_map(params![key.0, key.1, key.2, limit as i64], |row| {
                Ok(ManifestVersionFact {
                    view_id: row.get(0)?,
                    generation: row.get(1)?,
                    version_id: row.get(2)?,
                    path: row.get(3)?,
                    failed_preserved: row.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        observe_page(peak, page.len(), limit)?;
        let Some(last) = page.last() else {
            break;
        };
        key = (last.view_id.clone(), last.generation, last.path.clone());
        output.extend(page);
    }
    Ok(())
}

fn read_failed_paths(
    connection: &Connection,
    limit: usize,
    peak: &mut usize,
    output: &mut Vec<FailedPathFact>,
) -> Result<(), MaintenanceError> {
    let mut key = (String::new(), 0_i64, String::new());
    loop {
        let mut statement = connection.prepare("SELECT e.view_id,e.generation,e.path,e.language,e.generation IS v.current_generation FROM manifest_entries e JOIN views v ON v.view_id=e.view_id WHERE e.version_id IS NULL AND (e.view_id,e.generation,e.path)>(?1,?2,?3) ORDER BY e.view_id,e.generation,e.path LIMIT ?4")?;
        let page: Vec<_> = statement
            .query_map(params![key.0, key.1, key.2, limit as i64], |row| {
                Ok(FailedPathFact {
                    view_id: row.get(0)?,
                    generation: row.get(1)?,
                    path: row.get(2)?,
                    language: row.get(3)?,
                    current: row.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        observe_page(peak, page.len(), limit)?;
        let Some(last) = page.last() else {
            break;
        };
        key = (last.view_id.clone(), last.generation, last.path.clone());
        output.extend(page);
    }
    Ok(())
}

fn read_base_versions(
    connection: &Connection,
    limit: usize,
    peak: &mut usize,
    snapshot: &mut MaintenanceSnapshot,
) -> Result<(), MaintenanceError> {
    let mut key = (String::new(), 0_i64);
    loop {
        let mut statement = connection.prepare("SELECT bv.base_id,bv.version_id FROM resolution_base_versions bv JOIN resolution_bases b ON b.base_id=bv.base_id WHERE (bv.base_id,bv.version_id)>(?1,?2) ORDER BY bv.base_id,bv.version_id LIMIT ?3")?;
        let page: Vec<_> = statement
            .query_map(params![key.0, key.1, limit as i64], |row| {
                Ok(BaseVersionFact {
                    base_id: row.get(0)?,
                    version_id: row.get(1)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        observe_page(peak, page.len(), limit)?;
        let Some(last) = page.last() else {
            break;
        };
        key = (last.base_id.clone(), last.version_id);
        snapshot.base_versions.extend(page);
    }
    Ok(())
}

fn read_delta_versions(
    connection: &Connection,
    limit: usize,
    peak: &mut usize,
    snapshot: &mut MaintenanceSnapshot,
) -> Result<(), MaintenanceError> {
    read_one_delta_table(
        connection,
        "resolution_identifier_deltas",
        "identifier_id",
        limit,
        peak,
        &mut snapshot.identifier_delta_versions,
    )?;
    read_one_delta_table(
        connection,
        "resolution_pending_deltas",
        "pending_relationship_id",
        limit,
        peak,
        &mut snapshot.pending_delta_versions,
    )
}

fn read_additional_version_roots(
    connection: &Connection,
    now_ms: i64,
    limit: usize,
    peak: &mut usize,
    output: &mut Vec<VersionRootFact>,
) -> Result<(), MaintenanceError> {
    let mut after = String::new();
    let now_seconds = now_ms.div_euclid(1000);
    loop {
        let mut statement = connection.prepare(
            "WITH roots(root_key,version_id,max_level,kind,reference) AS (
               SELECT 'view-base:'||v.view_id||':'||printf('%020d',bv.version_id),
                      bv.version_id,2,'view_binding',v.view_id||':'||v.resolution_base_id
               FROM views v JOIN resolution_base_versions bv ON bv.base_id=v.resolution_base_id
               WHERE v.resolution_state<>'unbound'
               UNION ALL
               SELECT 'pin-base:'||p.pin_id||':'||printf('%020d',bv.version_id),
                      bv.version_id,2,'pin',p.pin_id
               FROM resolution_pins p JOIN resolution_base_versions bv ON bv.base_id=p.base_id
               WHERE CAST(strftime('%s',p.expires_at) AS INTEGER)>?2
               UNION ALL
               SELECT 'pin-manifest:'||p.pin_id||':'||printf('%020d',e.version_id),
                      e.version_id,1,'pin',p.pin_id
               FROM resolution_pins p JOIN manifest_entries e
                 ON e.view_id=p.view_id AND e.generation=p.manifest_generation
               WHERE e.version_id IS NOT NULL
                 AND CAST(strftime('%s',p.expires_at) AS INTEGER)>?2
               UNION ALL
               SELECT 'view-id-source:'||v.view_id||':'||printf('%020d',d.version_id),
                      d.version_id,2,'view_binding',v.view_id||':'||v.resolution_delta_generation
               FROM views v JOIN resolution_identifier_deltas d
                 ON d.view_id=v.view_id AND d.delta_generation=v.resolution_delta_generation
               WHERE v.resolution_state<>'unbound'
               UNION ALL
               SELECT 'view-id-target:'||v.view_id||':'||printf('%020d',d.target_version_id),
                      d.target_version_id,2,'view_binding',v.view_id||':'||v.resolution_delta_generation
               FROM views v JOIN resolution_identifier_deltas d
                 ON d.view_id=v.view_id AND d.delta_generation=v.resolution_delta_generation
               WHERE v.resolution_state<>'unbound' AND d.target_version_id IS NOT NULL
               UNION ALL
               SELECT 'view-pending-source:'||v.view_id||':'||printf('%020d',d.version_id),
                      d.version_id,2,'view_binding',v.view_id||':'||v.resolution_delta_generation
               FROM views v JOIN resolution_pending_deltas d
                 ON d.view_id=v.view_id AND d.delta_generation=v.resolution_delta_generation
               WHERE v.resolution_state<>'unbound'
               UNION ALL
               SELECT 'view-pending-target:'||v.view_id||':'||printf('%020d',d.target_version_id),
                      d.target_version_id,2,'view_binding',v.view_id||':'||v.resolution_delta_generation
               FROM views v JOIN resolution_pending_deltas d
                 ON d.view_id=v.view_id AND d.delta_generation=v.resolution_delta_generation
               WHERE v.resolution_state<>'unbound' AND d.target_version_id IS NOT NULL
             )
             SELECT root_key,version_id,max_level,kind,reference FROM roots
             WHERE root_key>?1 ORDER BY root_key LIMIT ?3",
        )?;
        let page = statement
            .query_map(params![after, now_seconds, limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        observe_page(peak, page.len(), limit)?;
        let Some(last) = page.last() else {
            break;
        };
        after = last.0.clone();
        for (_, version_id, level, kind, reference) in page {
            output.push(VersionRootFact {
                version_id,
                max_level: match level {
                    1 => MaintenanceLevel::L1,
                    2 => MaintenanceLevel::L2,
                    3 => MaintenanceLevel::L3,
                    _ => {
                        return Err(MaintenanceError::InvalidMetadata {
                            field: "protection_level",
                            value: level.to_string(),
                        });
                    }
                },
                kind: match kind.as_str() {
                    "view_binding" => MaintenanceRootKind::ViewBinding,
                    "pin" => MaintenanceRootKind::Pin,
                    _ => {
                        return Err(MaintenanceError::InvalidMetadata {
                            field: "protection_kind",
                            value: kind,
                        });
                    }
                },
                reference,
            });
        }
    }
    output.sort_by(|left, right| {
        (
            left.version_id,
            left.max_level,
            left.kind,
            left.reference.as_str(),
        )
            .cmp(&(
                right.version_id,
                right.max_level,
                right.kind,
                right.reference.as_str(),
            ))
    });
    output.dedup();
    Ok(())
}

fn read_one_delta_table(
    connection: &Connection,
    table: &str,
    local_key: &str,
    limit: usize,
    peak: &mut usize,
    output: &mut Vec<DeltaVersionFact>,
) -> Result<(), MaintenanceError> {
    let mut key = (String::new(), 0_i64, 0_i64, String::new());
    loop {
        let sql = format!(
            "SELECT view_id,delta_generation,version_id,target_version_id,{local_key} FROM {table} WHERE (view_id,delta_generation,version_id,{local_key})>(?1,?2,?3,?4) ORDER BY view_id,delta_generation,version_id,{local_key} LIMIT ?5"
        );
        let mut statement = connection.prepare(&sql)?;
        let page: Vec<_> = statement
            .query_map(params![key.0, key.1, key.2, key.3, limit as i64], |row| {
                Ok((
                    DeltaVersionFact {
                        view_id: row.get(0)?,
                        delta_generation: row.get(1)?,
                        source_version_id: row.get(2)?,
                        target_version_id: row.get(3)?,
                    },
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<_, _>>()?;
        observe_page(peak, page.len(), limit)?;
        let Some((last, local)) = page.last() else {
            break;
        };
        key = (
            last.view_id.clone(),
            last.delta_generation,
            last.source_version_id,
            local.clone(),
        );
        output.extend(page.into_iter().map(|(fact, _)| fact));
    }
    Ok(())
}

fn read_store_objects(
    connection: &Connection,
    now_ms: i64,
    limit: usize,
    peak: &mut usize,
    snapshot: &mut MaintenanceSnapshot,
) -> Result<(), MaintenanceError> {
    let now_seconds = now_ms.div_euclid(1000);
    snapshot.protected_bases = read_string_pages_with_extra(
        connection,
        "SELECT base_id FROM resolution_bases WHERE (state='building' OR EXISTS(SELECT 1 FROM views WHERE resolution_base_id=resolution_bases.base_id) OR EXISTS(SELECT 1 FROM resolution_pins WHERE base_id=resolution_bases.base_id AND CAST(strftime('%s',expires_at) AS INTEGER)>?3)) AND base_id>?1 ORDER BY base_id LIMIT ?2",
        limit,
        peak,
        now_seconds,
    )?;
    snapshot.eligible_bases = read_string_pages_with_extra(
        connection,
        "SELECT base_id FROM resolution_bases WHERE state='ready' AND NOT EXISTS(SELECT 1 FROM views WHERE resolution_base_id=resolution_bases.base_id) AND NOT EXISTS(SELECT 1 FROM resolution_pins WHERE base_id=resolution_bases.base_id AND CAST(strftime('%s',expires_at) AS INTEGER)>?3) AND NOT EXISTS(SELECT 1 FROM resolution_deltas WHERE base_id=resolution_bases.base_id) AND base_id>?1 ORDER BY base_id LIMIT ?2",
        limit,
        peak,
        now_seconds,
    )?;
    snapshot.protected_deltas = read_string_pages_with_extra(
        connection,
        "SELECT view_id||':'||printf('%020d',delta_generation) FROM resolution_deltas WHERE (EXISTS(SELECT 1 FROM views WHERE views.view_id=resolution_deltas.view_id AND views.resolution_delta_generation=resolution_deltas.delta_generation) OR EXISTS(SELECT 1 FROM resolution_pins WHERE resolution_pins.view_id=resolution_deltas.view_id AND resolution_pins.delta_generation=resolution_deltas.delta_generation AND CAST(strftime('%s',expires_at) AS INTEGER)>?3)) AND view_id||':'||printf('%020d',delta_generation)>?1 ORDER BY view_id,delta_generation LIMIT ?2",
        limit,
        peak,
        now_seconds,
    )?;
    snapshot.eligible_deltas = read_string_pages_with_extra(
        connection,
        "SELECT view_id||':'||printf('%020d',delta_generation) FROM resolution_deltas WHERE NOT EXISTS(SELECT 1 FROM views WHERE views.view_id=resolution_deltas.view_id AND views.resolution_delta_generation=resolution_deltas.delta_generation) AND NOT EXISTS(SELECT 1 FROM resolution_pins WHERE resolution_pins.view_id=resolution_deltas.view_id AND resolution_pins.delta_generation=resolution_deltas.delta_generation AND CAST(strftime('%s',expires_at) AS INTEGER)>?3) AND view_id||':'||printf('%020d',delta_generation)>?1 ORDER BY view_id,delta_generation LIMIT ?2",
        limit,
        peak,
        now_seconds,
    )?;
    snapshot.protected_pins = read_string_pages_with_extra(
        connection,
        "SELECT pin_id FROM resolution_pins WHERE CAST(strftime('%s',expires_at) AS INTEGER)>?3 AND pin_id>?1 ORDER BY pin_id LIMIT ?2",
        limit,
        peak,
        now_seconds,
    )?;
    snapshot.expired_pins = read_string_pages_with_extra(
        connection,
        "SELECT pin_id FROM resolution_pins WHERE CAST(strftime('%s',expires_at) AS INTEGER)<=?3 AND pin_id>?1 ORDER BY pin_id LIMIT ?2",
        limit,
        peak,
        now_seconds,
    )?;
    Ok(())
}

fn read_coordinator_objects(
    connection: &Connection,
    limit: usize,
    peak: &mut usize,
    snapshot: &mut MaintenanceSnapshot,
) -> Result<(), MaintenanceError> {
    let mut after = String::new();
    loop {
        let mut statement = connection.prepare(
            "SELECT request_id,state,claim_owner,terminal_log_sequence,updated_at
             FROM requests WHERE request_id>?1 ORDER BY request_id LIMIT ?2",
        )?;
        let page: Vec<_> = statement
            .query_map(params![after, limit as i64], |row| {
                Ok(CoordinatorRequestFact {
                    request_id: row.get(0)?,
                    state: row.get(1)?,
                    claim_owner: row.get(2)?,
                    terminal_log_sequence: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        observe_page(peak, page.len(), limit)?;
        let Some(last) = page.last() else {
            break;
        };
        after = last.request_id.clone();
        snapshot.request_facts.extend(page);
    }
    after.clear();
    loop {
        let mut statement = connection.prepare(
            "SELECT consumer_id,generation_name,store_log_sequence,updated_at
             FROM consumer_cursors WHERE consumer_id>?1 ORDER BY consumer_id LIMIT ?2",
        )?;
        let page: Vec<_> = statement
            .query_map(params![after, limit as i64], |row| {
                Ok(ConsumerCursorFact {
                    consumer_id: row.get(0)?,
                    generation_name: row.get(1)?,
                    store_log_sequence: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        observe_page(peak, page.len(), limit)?;
        let Some(last) = page.last() else {
            break;
        };
        after = last.consumer_id.clone();
        snapshot.cursor_facts.extend(page);
    }
    snapshot.protected_requests = snapshot
        .request_facts
        .iter()
        .map(|fact| fact.request_id.clone())
        .collect();
    snapshot.protected_cursors = snapshot
        .cursor_facts
        .iter()
        .map(|fact| fact.consumer_id.clone())
        .collect();
    Ok(())
}

fn read_string_pages_with_extra(
    connection: &Connection,
    sql: &str,
    limit: usize,
    peak: &mut usize,
    extra: i64,
) -> Result<Vec<String>, MaintenanceError> {
    let mut output = Vec::new();
    let mut after = String::new();
    loop {
        let mut statement = connection.prepare(sql)?;
        let page: Vec<String> = statement
            .query_map(params![after, limit as i64, extra], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        observe_page(peak, page.len(), limit)?;
        let Some(last) = page.last() else {
            break;
        };
        after = last.clone();
        output.extend(page);
    }
    Ok(output)
}

fn observe_page(peak: &mut usize, rows: usize, limit: usize) -> Result<(), MaintenanceError> {
    if rows > limit {
        return Err(MaintenanceError::InvalidMetadata {
            field: "window_size",
            value: rows.to_string(),
        });
    }
    *peak = (*peak).max(rows);
    Ok(())
}

fn named_generations(root: &Path) -> Result<Vec<String>, MaintenanceError> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if valid_generation_name(&name) {
            let metadata = fs::symlink_metadata(entry.path())?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(MaintenanceError::InvalidMetadata {
                    field: "generation_path",
                    value: entry.path().display().to_string(),
                });
            }
            let canonical = entry.path().canonicalize()?;
            if !canonical.starts_with(root) {
                return Err(MaintenanceError::InvalidMetadata {
                    field: "generation_path",
                    value: canonical.display().to_string(),
                });
            }
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn named_files(root: &Path) -> Result<Vec<String>, MaintenanceError> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            return Err(MaintenanceError::InvalidMetadata {
                field: "scratch_path",
                value: entry.path().display().to_string(),
            });
        }
        if metadata.is_file() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
}
