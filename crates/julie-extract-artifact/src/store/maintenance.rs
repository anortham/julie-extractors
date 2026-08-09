use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::resolution::resolution_file_bytes;
use super::{StoreConnectionError, StoreConnectionFactory};

const DAY_MS: i64 = 86_400_000;
const DEFAULT_WINDOW_SIZE: usize = 512;
const MAX_DEMOTION_VERSIONS: usize = 100;
const MAX_DEMOTION_BYTES: u64 = 64 * 1024 * 1024;
const JOURNAL_RETENTION_BYTES: u64 = 256 * 1024 * 1024;
const CHECKPOINT_HEADROOM_BYTES: u64 = 16 * 1024 * 1024;

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
    Connection(StoreConnectionError),
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
            Self::Connection(_) => "store_connection_error",
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
            Self::Connection(error) => error.fmt(formatter),
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
    let pressure = retained_logical_bytes > target_bytes;
    let pressure_only_manifests = if pressure {
        Vec::new()
    } else {
        eligible_manifests.clone()
    };
    let demotion_cohort = demotion_cohort(&snapshot.versions, &decisions);
    let demotion_wal_headroom_bytes: u64 = demotion_cohort
        .iter()
        .map(|candidate| candidate.estimated_dirty_bytes)
        .sum();
    let measured_bytes = snapshot
        .capacity
        .store_page_bytes
        .saturating_add(snapshot.capacity.store_wal_bytes)
        .saturating_add(snapshot.capacity.base_bytes)
        .saturating_add(snapshot.capacity.scratch_bytes);
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
    Ok(MaintenanceCapacity {
        free_bytes: provider.free_bytes(factory.layout().root())?,
        store_page_bytes: page_size.saturating_mul(page_count),
        store_freelist_bytes: page_size.saturating_mul(freelist),
        store_wal_bytes: file_len(Path::new(&format!(
            "{}-wal",
            factory.layout().store_db().display()
        )))?,
        base_bytes: directory_bytes(factory.layout().bases_dir())?,
        scratch_bytes: directory_bytes(factory.layout().scratch_dir())?,
        staged_generation_bytes: provider.staged_generation_bytes(factory.layout().root())?,
    })
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
        if name.len() == 7
            && name.starts_with("gen-")
            && name[4..].bytes().all(|byte| byte.is_ascii_digit())
        {
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
