use julie_extract_artifact::store::{
    CapacityPlan, GenerationApplyReport, MaintenanceApplyReport, MaintenancePlan, RetentionPlan,
};
use serde::{Deserialize, Serialize};

use super::report::{
    STORE_EXIT_INCOMPATIBLE, STORE_EXIT_OPERATIONAL_FAILURE, STORE_EXIT_SUCCESS, StoreOutputFormat,
    StoreOutputPlan, StoreOutputStream,
};

pub const STORE_MAINTENANCE_REPORT_SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreMaintenanceAction {
    Inspect,
    Gc,
    Repair,
    Promote,
    CursorAdvance,
    CursorRelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreMaintenanceMode {
    Plan,
    Apply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreMaintenanceDisposition {
    Planned,
    NoChange,
    Applied,
    Checkpointed,
    Recovered,
    Rebuilt,
    Advanced,
    Released,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreMaintenanceFailureClass {
    None,
    Busy,
    StalePlan,
    CapacityInsufficient,
    IncompatibleStore,
    RecoveryRequired,
    IntegrityFailed,
    RepairUnavailable,
    InvalidArguments,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreMaintenanceErrorReport {
    pub class: StoreMaintenanceFailureClass,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoreMaintenanceCounts {
    pub versions: usize,
    pub purge_eligible_versions: usize,
    pub eligible_manifests: usize,
    pub pressure_only_manifests: usize,
    pub demotion_versions: usize,
    pub protected_bases: usize,
    pub eligible_bases: usize,
    pub protected_deltas: usize,
    pub eligible_deltas: usize,
    pub protected_pins: usize,
    pub expired_pins: usize,
    pub protected_requests: usize,
    pub protected_scratch: usize,
    pub protected_cursors: usize,
    pub protected_generations: usize,
    pub protected_failed_paths: usize,
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
    pub copied_file_versions: usize,
    pub copied_rows: usize,
    pub copied_base_files: usize,
    pub removed_generations: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreMaintenanceFingerprints {
    pub store_root: String,
    pub coordinator_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreMaintenanceRetentionReport {
    pub protected_current_bytes: u64,
    pub retained_logical_bytes: u64,
    pub eligible_bytes: u64,
    pub target_bytes: u64,
    pub ceiling_bytes: u64,
    pub pressure: bool,
    pub physical_current_bytes: u64,
    pub physical_bytes_before_gc: u64,
    pub physical_bytes_after_gc: u64,
    pub physical_baseline_bytes: u64,
    pub physical_target_bytes: u64,
    pub physical_ceiling_bytes: u64,
    pub physical_target_breached: bool,
    pub physical_ceiling_breached: bool,
    pub physical_breach_limit: u32,
    pub physical_breach_streak: u32,
    pub compaction_required: bool,
}

impl From<&RetentionPlan> for StoreMaintenanceRetentionReport {
    fn from(value: &RetentionPlan) -> Self {
        Self {
            protected_current_bytes: value.protected_current_bytes,
            retained_logical_bytes: value.retained_logical_bytes,
            eligible_bytes: value.eligible_bytes,
            target_bytes: value.target_bytes,
            ceiling_bytes: value.ceiling_bytes,
            pressure: value.pressure,
            physical_current_bytes: value.physical_current_bytes,
            physical_bytes_before_gc: value.physical_current_bytes,
            physical_bytes_after_gc: 0,
            physical_baseline_bytes: value.physical_baseline_bytes,
            physical_target_bytes: value.physical_target_bytes,
            physical_ceiling_bytes: value.physical_ceiling_bytes,
            physical_target_breached: value.physical_target_breached,
            physical_ceiling_breached: value.physical_ceiling_breached,
            physical_breach_limit: value.physical_breach_limit,
            physical_breach_streak: value.physical_breach_streak,
            compaction_required: value.compaction_required,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreMaintenanceCapacityReport {
    pub measured_bytes: u64,
    pub free_bytes: u64,
    pub store_page_bytes: u64,
    pub store_freelist_bytes: u64,
    pub store_wal_bytes: u64,
    pub base_bytes: u64,
    pub scratch_bytes: u64,
    pub staged_generation_bytes: u64,
    pub demotion_wal_headroom_bytes: u64,
    pub gc_required_bytes: u64,
    pub promotion_required_bytes: u64,
    pub gc_fits: bool,
    pub promotion_fits: bool,
}

impl From<&CapacityPlan> for StoreMaintenanceCapacityReport {
    fn from(value: &CapacityPlan) -> Self {
        Self {
            measured_bytes: value.measured_bytes,
            free_bytes: value.free_bytes,
            store_page_bytes: value.facts.store_page_bytes,
            store_freelist_bytes: value.facts.store_freelist_bytes,
            store_wal_bytes: value.facts.store_wal_bytes,
            base_bytes: value.facts.base_bytes,
            scratch_bytes: value.facts.scratch_bytes,
            staged_generation_bytes: value.facts.staged_generation_bytes,
            demotion_wal_headroom_bytes: value.demotion_wal_headroom_bytes,
            gc_required_bytes: value.gc_required_bytes,
            promotion_required_bytes: value.promotion_required_bytes,
            gc_fits: value.gc_fits,
            promotion_fits: value.promotion_fits,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreMaintenanceReport {
    pub report_schema_version: i64,
    pub action: StoreMaintenanceAction,
    pub mode: StoreMaintenanceMode,
    pub run_id: Option<String>,
    pub family_id: String,
    pub source_generation: String,
    pub destination_generation: Option<String>,
    pub selected_generation: Option<String>,
    pub disposition: StoreMaintenanceDisposition,
    pub plan_fingerprint: String,
    pub fingerprints: StoreMaintenanceFingerprints,
    pub counts: StoreMaintenanceCounts,
    pub retention: StoreMaintenanceRetentionReport,
    pub capacity: StoreMaintenanceCapacityReport,
    pub integrity_checks: Vec<String>,
    pub escalation: Option<String>,
    pub recovery_actions: Vec<String>,
    pub last_version_cursor: Option<i64>,
    pub consumer_id: Option<String>,
    pub consumer_sequence: Option<i64>,
    pub failure_class: StoreMaintenanceFailureClass,
    pub error: Option<StoreMaintenanceErrorReport>,
}

impl StoreMaintenanceReport {
    pub fn planned(action: StoreMaintenanceAction, plan: &MaintenancePlan) -> Self {
        let purge_eligible_versions = plan
            .versions
            .iter()
            .filter(|version| {
                version.l1_reasons.is_empty()
                    && version.l2_reasons.is_empty()
                    && version.l3_reasons.is_empty()
            })
            .count();
        Self {
            report_schema_version: STORE_MAINTENANCE_REPORT_SCHEMA_VERSION,
            action,
            mode: StoreMaintenanceMode::Plan,
            run_id: None,
            family_id: plan.binding.family_id.clone(),
            source_generation: plan.binding.current_generation.clone(),
            destination_generation: None,
            selected_generation: None,
            disposition: StoreMaintenanceDisposition::Planned,
            plan_fingerprint: plan.fingerprint.clone(),
            fingerprints: StoreMaintenanceFingerprints {
                store_root: plan.binding.store_root_fingerprint.clone(),
                coordinator_root: plan.binding.coordinator_root_fingerprint.clone(),
            },
            counts: StoreMaintenanceCounts {
                versions: plan.versions.len(),
                purge_eligible_versions,
                eligible_manifests: plan.eligible_manifests.len(),
                pressure_only_manifests: plan.pressure_only_manifests.len(),
                demotion_versions: plan.demotion_cohort.len(),
                protected_bases: plan.protected_bases.len(),
                eligible_bases: plan.eligible_bases.len(),
                protected_deltas: plan.protected_deltas.len(),
                eligible_deltas: plan.eligible_deltas.len(),
                protected_pins: plan.protected_pins.len(),
                expired_pins: plan.expired_pins.len(),
                protected_requests: plan.protected_requests.len(),
                protected_scratch: plan.protected_scratch.len(),
                protected_cursors: plan.protected_cursors.len(),
                protected_generations: plan.protected_generations.len(),
                protected_failed_paths: plan.protected_failed_paths.len(),
                ..StoreMaintenanceCounts::default()
            },
            retention: (&plan.retention).into(),
            capacity: (&plan.capacity).into(),
            integrity_checks: vec![
                "store_roots_validated".to_string(),
                "coordinator_roots_validated".to_string(),
            ],
            escalation: None,
            recovery_actions: Vec::new(),
            last_version_cursor: None,
            consumer_id: None,
            consumer_sequence: None,
            failure_class: StoreMaintenanceFailureClass::None,
            error: None,
        }
    }

    pub fn with_gc_apply(mut self, run_id: String, applied: &MaintenanceApplyReport) -> Self {
        self.mode = StoreMaintenanceMode::Apply;
        self.run_id = Some(run_id);
        self.disposition = if applied == &MaintenanceApplyReport::default() {
            StoreMaintenanceDisposition::NoChange
        } else {
            StoreMaintenanceDisposition::Applied
        };
        self.counts.demoted_l3 = applied.demoted_l3;
        self.counts.demoted_l2 = applied.demoted_l2;
        self.counts.purged_versions = applied.purged_versions;
        self.counts.removed_manifests = applied.removed_manifests;
        self.counts.removed_deltas = applied.removed_deltas;
        self.counts.removed_bases = applied.removed_bases;
        self.counts.removed_base_files = applied.removed_base_files;
        self.counts.removed_pins = applied.removed_pins;
        self.counts.removed_scratch_files = applied.removed_scratch_files;
        self.counts.archived_requests = applied.archived_requests;
        self.counts.pruned_log_rows = applied.pruned_log_rows;
        self.retention.physical_current_bytes = applied.physical_bytes_before;
        self.retention.physical_bytes_before_gc = applied.physical_bytes_before;
        self.retention.physical_bytes_after_gc = applied.physical_bytes_after;
        self.retention.physical_baseline_bytes = applied.physical_baseline_bytes;
        self.retention.physical_target_bytes = applied.physical_target_bytes;
        self.retention.physical_ceiling_bytes = applied.physical_ceiling_bytes;
        self.retention.physical_target_breached = applied.physical_target_breached;
        self.retention.physical_ceiling_breached = applied.physical_ceiling_breached;
        self.retention.physical_breach_streak = applied.physical_breach_streak;
        self.retention.compaction_required = applied.compaction_required;
        if applied.compaction_required {
            self.escalation = Some("compaction_required".to_string());
        }
        self.last_version_cursor = applied.last_version_cursor;
        self
    }

    pub fn with_generation_apply(
        mut self,
        run_id: String,
        applied: &GenerationApplyReport,
    ) -> Self {
        self.mode = StoreMaintenanceMode::Apply;
        self.run_id = Some(run_id);
        self.source_generation = applied.source_generation.clone();
        self.destination_generation = Some(applied.destination_generation.clone());
        self.selected_generation = applied.selected_generation.clone();
        self.disposition = match (self.action, applied.repair_disposition) {
            (StoreMaintenanceAction::Promote, None) => StoreMaintenanceDisposition::Applied,
            (_, Some(julie_extract_artifact::store::RepairDisposition::CheckpointRecovered)) => {
                StoreMaintenanceDisposition::Checkpointed
            }
            (_, Some(julie_extract_artifact::store::RepairDisposition::TornStateRecovered)) => {
                StoreMaintenanceDisposition::Recovered
            }
            (_, Some(julie_extract_artifact::store::RepairDisposition::GenerationRebuilt))
            | (_, None) => StoreMaintenanceDisposition::Rebuilt,
        };
        self.counts.copied_file_versions = applied.copied_file_versions;
        self.counts.copied_rows = applied.copied_rows;
        self.counts.copied_base_files = applied.copied_base_files;
        self.counts.removed_generations = applied.removed_generations.len();
        if applied.recovered_partial {
            self.recovery_actions.push("recovered_partial".to_string());
        }
        self.escalation = applied
            .repair_disposition
            .map(|value| format!("{value:?}").to_ascii_lowercase());
        self
    }

    pub fn with_cursor(
        mut self,
        consumer_id: &str,
        sequence: Option<i64>,
        mode: StoreMaintenanceMode,
        changed: bool,
    ) -> Self {
        self.consumer_id = Some(consumer_id.to_string());
        self.consumer_sequence = sequence;
        if mode == StoreMaintenanceMode::Apply {
            self.mode = StoreMaintenanceMode::Apply;
            self.disposition = if changed {
                match self.action {
                    StoreMaintenanceAction::CursorAdvance => StoreMaintenanceDisposition::Advanced,
                    StoreMaintenanceAction::CursorRelease => StoreMaintenanceDisposition::Released,
                    _ => self.disposition,
                }
            } else {
                StoreMaintenanceDisposition::NoChange
            };
        }
        self
    }

    pub fn with_failure(
        mut self,
        mode: StoreMaintenanceMode,
        class: StoreMaintenanceFailureClass,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        self.mode = mode;
        self.disposition = StoreMaintenanceDisposition::Failed;
        self.failure_class = class;
        self.error = Some(StoreMaintenanceErrorReport {
            class,
            code: code.into(),
            message: message.into(),
        });
        self
    }

    pub fn failed(
        action: StoreMaintenanceAction,
        mode: StoreMaintenanceMode,
        family_id: String,
        source_generation: String,
        class: StoreMaintenanceFailureClass,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let code = code.into();
        let message = message.into();
        Self {
            report_schema_version: STORE_MAINTENANCE_REPORT_SCHEMA_VERSION,
            action,
            mode,
            run_id: None,
            family_id,
            source_generation,
            destination_generation: None,
            selected_generation: None,
            disposition: StoreMaintenanceDisposition::Failed,
            plan_fingerprint: String::new(),
            fingerprints: StoreMaintenanceFingerprints {
                store_root: String::new(),
                coordinator_root: String::new(),
            },
            counts: StoreMaintenanceCounts::default(),
            retention: StoreMaintenanceRetentionReport {
                protected_current_bytes: 0,
                retained_logical_bytes: 0,
                eligible_bytes: 0,
                target_bytes: 0,
                ceiling_bytes: 0,
                pressure: false,
                physical_current_bytes: 0,
                physical_bytes_before_gc: 0,
                physical_bytes_after_gc: 0,
                physical_baseline_bytes: 0,
                physical_target_bytes: 0,
                physical_ceiling_bytes: 0,
                physical_target_breached: false,
                physical_ceiling_breached: false,
                physical_breach_limit: 0,
                physical_breach_streak: 0,
                compaction_required: false,
            },
            capacity: StoreMaintenanceCapacityReport {
                measured_bytes: 0,
                free_bytes: 0,
                store_page_bytes: 0,
                store_freelist_bytes: 0,
                store_wal_bytes: 0,
                base_bytes: 0,
                scratch_bytes: 0,
                staged_generation_bytes: 0,
                demotion_wal_headroom_bytes: 0,
                gc_required_bytes: 0,
                promotion_required_bytes: 0,
                gc_fits: false,
                promotion_fits: false,
            },
            integrity_checks: Vec::new(),
            escalation: None,
            recovery_actions: Vec::new(),
            last_version_cursor: None,
            consumer_id: None,
            consumer_sequence: None,
            failure_class: class,
            error: Some(StoreMaintenanceErrorReport {
                class,
                code,
                message,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMaintenanceCommandOutcome {
    report: StoreMaintenanceReport,
    exit_code: u8,
}

impl StoreMaintenanceCommandOutcome {
    pub fn success(report: StoreMaintenanceReport) -> Self {
        Self {
            report,
            exit_code: STORE_EXIT_SUCCESS,
        }
    }

    pub fn failure(report: StoreMaintenanceReport) -> Self {
        let exit_code = if report.failure_class == StoreMaintenanceFailureClass::IncompatibleStore {
            STORE_EXIT_INCOMPATIBLE
        } else {
            STORE_EXIT_OPERATIONAL_FAILURE
        };
        Self { report, exit_code }
    }

    pub fn exit_code(&self) -> u8 {
        self.exit_code
    }

    pub fn report(&self) -> &StoreMaintenanceReport {
        &self.report
    }

    pub fn render(&self, format: StoreOutputFormat) -> String {
        match format {
            StoreOutputFormat::Json => self.render_json(),
            StoreOutputFormat::Human => self.render_human(),
        }
    }

    pub fn render_json(&self) -> String {
        let mut rendered =
            serde_json::to_string(&self.report).expect("maintenance report serializes");
        rendered.push('\n');
        rendered
    }

    pub fn render_human(&self) -> String {
        let status = if self.exit_code == STORE_EXIT_SUCCESS {
            "ok"
        } else {
            "failed"
        };
        let code = self
            .report
            .error
            .as_ref()
            .map(|error| format!(" code={}", error.code))
            .unwrap_or_default();
        format!(
            "{status} action={:?} mode={:?} family={} source={} destination={} disposition={:?} failure={:?}{code}\n",
            self.report.action,
            self.report.mode,
            self.report.family_id,
            self.report.source_generation,
            self.report.destination_generation.as_deref().unwrap_or("none"),
            self.report.disposition,
            self.report.failure_class,
        )
        .to_ascii_lowercase()
    }

    pub fn output_plan(&self, json: bool) -> StoreOutputPlan {
        StoreOutputPlan {
            format: if json {
                StoreOutputFormat::Json
            } else {
                StoreOutputFormat::Human
            },
            stream: if json || self.exit_code == STORE_EXIT_SUCCESS {
                StoreOutputStream::Stdout
            } else {
                StoreOutputStream::Stderr
            },
        }
    }
}
