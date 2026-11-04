use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::compat::{ArtifactDump, TableDump};
use crate::coverage::{CoverageControl, CoverageManifest, CoverageSample, FactAssertion};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SUMMARY_FORMAT_VERSION: u32 = 1;
const ALLOWLIST_FORMAT_VERSION: u32 = 1;
const WARMUP_RUNS_PER_BINARY: usize = 1;
const SAMPLE_COUNT: usize = 5;
const MIN_BATCH_DURATION_MS: u128 = 2_000;
const MAX_BATCH_OPERATIONS: usize = 4_096;
const MAX_SAMPLE_SPREAD_PERCENT: u128 = 10;
const MIN_SAMPLE_SPREAD_MS: u128 = 20;
const ELAPSED_GROWTH_PERCENT: u128 = 10;
const ELAPSED_GROWTH_MS: u128 = 100;
const PEAK_RSS_GROWTH_PERCENT: u128 = 15;
const PEAK_RSS_GROWTH_BYTES: u64 = 32 * 1024 * 1024;
const ARTIFACT_GROWTH_PERCENT: u128 = 10;
const ARTIFACT_GROWTH_BYTES: u64 = 1024 * 1024;
const SOURCE_TREE_HASH_ALGORITHM: &str = "git-ls-tree-r-v1-sha256";
const OUTPUT_LEDGER: &str = "docs/contracts/extraction-output-changes.md";
const TRANSIENT_TABLES: &[&str] = &[
    "artifact_metadata",
    "extraction_revisions",
    "revision_file_changes",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparisonPlan {
    pub baseline: PathBuf,
    pub candidate: PathBuf,
    pub baseline_expectations: PathBuf,
    pub baseline_expectations_sha256: String,
    pub baseline_build_metadata: Option<PathBuf>,
    pub baseline_build_metadata_sha256: String,
    pub candidate_build_metadata: Option<PathBuf>,
    pub candidate_build_metadata_sha256: String,
    pub budget_approvals: Option<PathBuf>,
    pub budget_approvals_sha256: Option<String>,
    pub manifest: PathBuf,
    pub allowlist: Option<PathBuf>,
    pub out_dir: PathBuf,
    pub language: Option<String>,
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BuildSourceIdentity {
    pub commit: String,
    pub tree_hash_algorithm: String,
    pub tree_sha256: String,
    pub dirty: bool,
    pub dirty_state_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BuildEvidence {
    pub kind: String,
    pub reference: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExtractorBuildMetadata {
    pub rustc: String,
    pub cargo: String,
    pub host_target: String,
    pub target: String,
    pub profile: String,
    pub features: Vec<String>,
    pub options: BTreeMap<String, String>,
    pub recipe: Vec<String>,
    pub recipe_sha256: String,
    pub evidence: BuildEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BuildMetadata {
    pub format_version: u32,
    pub binary_sha256: String,
    pub source: BuildSourceIdentity,
    pub build: ExtractorBuildMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BaselineSnapshotExpectation {
    pub language: String,
    pub level: String,
    pub workload: String,
    pub source_state_sha256: String,
    pub semantic_dump_sha256: String,
    pub diagnostic_counts_by_file: BTreeMap<String, u64>,
    pub health: ComparisonHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BaselineExpectationReceipt {
    pub format_version: u32,
    pub baseline_binary_sha256: String,
    pub manifest_sha256: String,
    pub source: BuildSourceIdentity,
    pub snapshots: Vec<BaselineSnapshotExpectation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaselineSnapshotPin<'a> {
    pub binary_sha256: &'a str,
    pub manifest_sha256: &'a str,
    pub source: &'a BuildSourceIdentity,
    pub language: &'a str,
    pub level: &'a str,
    pub workload: &'a str,
    pub source_state_sha256: &'a str,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum BudgetMetric {
    ElapsedMs,
    PeakRssBytes,
    ArtifactBytes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct ApprovedBudgetDelta {
    pub table: String,
    pub action: RowChangeAction,
    pub canonical_row: String,
    pub row_sha256: String,
    pub count: usize,
    pub allowlist_id: String,
    pub ledger_marker: String,
    pub source_state_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BudgetApproval {
    pub id: String,
    pub owner: String,
    pub approval_reference: String,
    pub approval_evidence_sha256: String,
    pub baseline_binary_sha256: String,
    pub candidate_binary_sha256: String,
    pub manifest_sha256: String,
    pub language: String,
    pub level: String,
    pub workload: String,
    pub baseline_profile: String,
    pub candidate_profile: String,
    pub metric: BudgetMetric,
    pub maximum_growth_percent: u128,
    pub maximum_growth_absolute: u64,
    pub approved_deltas: Vec<ApprovedBudgetDelta>,
    pub approved_deltas_sha256: String,
    pub table_row_counts: BTreeMap<String, u64>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BudgetApprovalFile {
    pub format_version: u32,
    pub approvals: Vec<BudgetApproval>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetApprovalContext<'a> {
    pub baseline_binary_sha256: &'a str,
    pub candidate_binary_sha256: &'a str,
    pub manifest_sha256: &'a str,
    pub language: &'a str,
    pub level: &'a str,
    pub workload: &'a str,
    pub baseline_profile: &'a str,
    pub candidate_profile: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetEvaluation {
    pub violations: Vec<BudgetViolation>,
    pub used_approval_ids: Vec<String>,
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NormalizedMetrics {
    pub operation_count: usize,
    pub elapsed_nanos: u128,
    pub elapsed_nanos_per_operation: Option<u128>,
    pub file_count: u64,
    pub elapsed_nanos_per_file: Option<u128>,
    pub semantic_row_total: u64,
    pub artifact_bytes: u64,
    pub bytes_per_row_numerator: u64,
    pub bytes_per_row_denominator: u64,
    pub state_sizes: Vec<ArtifactStateMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactStateMetrics {
    pub source_state_sha256: String,
    pub semantic_row_total: u64,
    pub artifact_bytes: u64,
    pub bytes_per_row_numerator: u64,
    pub bytes_per_row_denominator: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComparisonProvenance {
    pub manifest_sha256: String,
    pub binary_path: PathBuf,
    pub binary_sha256: String,
    pub binary_version: String,
    pub build_metadata_path: PathBuf,
    pub build_metadata_sha256: String,
    pub build_metadata_format_version: u32,
    pub source: BuildSourceIdentity,
    pub build: ExtractorBuildMetadata,
    pub toolchain: String,
    pub profile: String,
    pub level: String,
    pub jobs: usize,
    pub os: String,
    pub cpu: String,
    pub memory_bytes: u64,
    pub storage: String,
    pub cache_policy: String,
    pub peak_rss_method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComparisonHarnessProvenance {
    pub binary_path: PathBuf,
    pub binary_sha256: String,
    pub version: String,
    pub toolchain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComparisonPolicy {
    pub warmup_runs_per_binary: usize,
    pub paired_samples_per_workload: usize,
    pub minimum_baseline_batch_ms: u128,
    pub maximum_calibration_operations: usize,
    pub maximum_sample_spread_percent: u128,
    pub minimum_sample_spread_ms: u128,
    pub budget_growth_thresholds_both_required: bool,
    pub elapsed_growth_percent: u128,
    pub elapsed_growth_ms: u128,
    pub peak_rss_growth_percent: u128,
    pub peak_rss_growth_bytes: u64,
    pub artifact_growth_percent: u128,
    pub artifact_growth_bytes: u64,
    pub transient_tables: Vec<String>,
    pub excluded_volatile_columns: BTreeMap<String, Vec<String>>,
    pub timed_interval: String,
    pub workload_preparation: String,
}

pub fn run_from_args(args: &[String]) -> ExitCode {
    let result = plan_from_args(args).and_then(run);
    match result {
        Ok(summary) => {
            println!(
                "performance comparison {}: {} workload(s); summary {}",
                if summary.passed { "passed" } else { "failed" },
                summary.workloads.len(),
                summary.summary_path.display()
            );
            if summary.passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(error) => {
            eprintln!("{}", error.message);
            ExitCode::from(error.exit_code)
        }
    }
}

pub fn run_capture_baseline_receipt_from_args(args: &[String]) -> ExitCode {
    let result = capture_baseline_receipt_from_args(args);
    match result {
        Ok((path, sha256, snapshots)) => {
            println!(
                "baseline expectation receipt captured: {} ({} snapshot(s), sha256 {})",
                path.display(),
                snapshots,
                sha256
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}", error.message);
            ExitCode::from(error.exit_code)
        }
    }
}

#[derive(Debug)]
struct ComparisonError {
    exit_code: u8,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BaselineReceiptCapturePlan {
    binary: PathBuf,
    build_metadata: Option<PathBuf>,
    build_metadata_sha256: String,
    manifest: PathBuf,
    manifest_sha256: String,
    out_dir: PathBuf,
    language: Option<String>,
}

struct BaselineSnapshotWriter<'a> {
    out_dir: &'a Path,
    language: &'a str,
    level: &'a str,
    written_dumps: &'a mut BTreeSet<(String, String)>,
    snapshots: &'a mut Vec<BaselineSnapshotExpectation>,
}

impl BaselineSnapshotWriter<'_> {
    fn record(
        &mut self,
        workload: &str,
        source_state_sha256: &str,
        health: &ComparisonHealth,
        dump: &ArtifactDump,
    ) -> Result<(), ComparisonError> {
        let semantic_dump_sha256 =
            artifact_dump_sha256(dump).map_err(ComparisonError::environment)?;
        let diagnostic_counts_by_file =
            diagnostic_counts_by_file(dump).map_err(ComparisonError::environment)?;
        let dump_key = (self.level.to_string(), source_state_sha256.to_string());
        if self.written_dumps.insert(dump_key) {
            write_semantic_dump(
                self.out_dir,
                self.language,
                self.level,
                source_state_sha256,
                dump,
            )?;
        }
        self.snapshots.push(BaselineSnapshotExpectation {
            language: self.language.to_string(),
            level: self.level.to_string(),
            workload: workload.to_string(),
            source_state_sha256: source_state_sha256.to_string(),
            semantic_dump_sha256,
            diagnostic_counts_by_file,
            health: health.clone(),
        });
        Ok(())
    }
}

impl ComparisonError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            exit_code: 2,
            message: message.into(),
        }
    }

    fn environment(message: impl Into<String>) -> Self {
        Self {
            exit_code: 2,
            message: message.into(),
        }
    }
}

fn plan_from_args(args: &[String]) -> Result<ComparisonPlan, ComparisonError> {
    let mut baseline = None;
    let mut candidate = None;
    let mut baseline_expectations = None;
    let mut baseline_expectations_sha256 = None;
    let mut baseline_build_metadata = None;
    let mut baseline_build_metadata_sha256 = None;
    let mut candidate_build_metadata = None;
    let mut candidate_build_metadata_sha256 = None;
    let mut budget_approvals = None;
    let mut budget_approvals_sha256 = None;
    let mut manifest = None;
    let mut allowlist = None;
    let mut out_dir = None;
    let mut language = None;
    let mut profile = None;
    let mut index = 0;
    while index < args.len() {
        let (slot, option) = match args[index].as_str() {
            "--baseline" => (&mut baseline, "--baseline"),
            "--candidate" => (&mut candidate, "--candidate"),
            "--baseline-expectations" => (&mut baseline_expectations, "--baseline-expectations"),
            "--baseline-expectations-sha256" => (
                &mut baseline_expectations_sha256,
                "--baseline-expectations-sha256",
            ),
            "--baseline-build-metadata" => {
                (&mut baseline_build_metadata, "--baseline-build-metadata")
            }
            "--baseline-build-metadata-sha256" => (
                &mut baseline_build_metadata_sha256,
                "--baseline-build-metadata-sha256",
            ),
            "--candidate-build-metadata" => {
                (&mut candidate_build_metadata, "--candidate-build-metadata")
            }
            "--candidate-build-metadata-sha256" => (
                &mut candidate_build_metadata_sha256,
                "--candidate-build-metadata-sha256",
            ),
            "--budget-approvals" => (&mut budget_approvals, "--budget-approvals"),
            "--budget-approvals-sha256" => {
                (&mut budget_approvals_sha256, "--budget-approvals-sha256")
            }
            "--manifest" => (&mut manifest, "--manifest"),
            "--allowlist" => (&mut allowlist, "--allowlist"),
            "--out-dir" => (&mut out_dir, "--out-dir"),
            "--language" => (&mut language, "--language"),
            "--profile" => (&mut profile, "--profile"),
            other => {
                return Err(ComparisonError::usage(format!(
                    "unexpected performance compare argument `{other}`"
                )));
            }
        };
        index += 1;
        let value = args
            .get(index)
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(|| ComparisonError::usage(format!("missing value for {option}")))?;
        if slot.replace(value.clone()).is_some() {
            return Err(ComparisonError::usage(format!("duplicate option {option}")));
        }
        index += 1;
    }
    if budget_approvals.is_some() != budget_approvals_sha256.is_some() {
        return Err(ComparisonError::usage(
            "--budget-approvals and --budget-approvals-sha256 must be supplied together",
        ));
    }
    Ok(ComparisonPlan {
        baseline: PathBuf::from(
            baseline.ok_or_else(|| ComparisonError::usage("missing --baseline <path>"))?,
        ),
        candidate: PathBuf::from(
            candidate.ok_or_else(|| ComparisonError::usage("missing --candidate <path>"))?,
        ),
        baseline_expectations: PathBuf::from(
            baseline_expectations
                .ok_or_else(|| ComparisonError::usage("missing --baseline-expectations <path>"))?,
        ),
        baseline_expectations_sha256: baseline_expectations_sha256.ok_or_else(|| {
            ComparisonError::usage("missing --baseline-expectations-sha256 <sha256>")
        })?,
        baseline_build_metadata: baseline_build_metadata.map(PathBuf::from),
        baseline_build_metadata_sha256: baseline_build_metadata_sha256.ok_or_else(|| {
            ComparisonError::usage("missing --baseline-build-metadata-sha256 <sha256>")
        })?,
        candidate_build_metadata: candidate_build_metadata.map(PathBuf::from),
        candidate_build_metadata_sha256: candidate_build_metadata_sha256.ok_or_else(|| {
            ComparisonError::usage("missing --candidate-build-metadata-sha256 <sha256>")
        })?,
        budget_approvals: budget_approvals.map(PathBuf::from),
        budget_approvals_sha256,
        manifest: PathBuf::from(
            manifest.ok_or_else(|| ComparisonError::usage("missing --manifest <path>"))?,
        ),
        allowlist: allowlist.map(PathBuf::from),
        out_dir: PathBuf::from(
            out_dir.ok_or_else(|| ComparisonError::usage("missing --out-dir <new-dir>"))?,
        ),
        language,
        profile,
    })
}

fn capture_plan_from_args(args: &[String]) -> Result<BaselineReceiptCapturePlan, ComparisonError> {
    let mut binary = None;
    let mut build_metadata = None;
    let mut build_metadata_sha256 = None;
    let mut manifest = None;
    let mut manifest_sha256 = None;
    let mut out_dir = None;
    let mut language = None;
    let mut index = 0;
    while index < args.len() {
        let (slot, option) = match args[index].as_str() {
            "--binary" => (&mut binary, "--binary"),
            "--build-metadata" => (&mut build_metadata, "--build-metadata"),
            "--build-metadata-sha256" => (&mut build_metadata_sha256, "--build-metadata-sha256"),
            "--manifest" => (&mut manifest, "--manifest"),
            "--manifest-sha256" => (&mut manifest_sha256, "--manifest-sha256"),
            "--out-dir" => (&mut out_dir, "--out-dir"),
            "--language" => (&mut language, "--language"),
            other => {
                return Err(ComparisonError::usage(format!(
                    "unexpected baseline-receipt argument `{other}`"
                )));
            }
        };
        index += 1;
        let value = args
            .get(index)
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(|| ComparisonError::usage(format!("missing value for {option}")))?;
        if slot.replace(value.clone()).is_some() {
            return Err(ComparisonError::usage(format!("duplicate option {option}")));
        }
        index += 1;
    }
    Ok(BaselineReceiptCapturePlan {
        binary: PathBuf::from(
            binary.ok_or_else(|| ComparisonError::usage("missing --binary <path>"))?,
        ),
        build_metadata: build_metadata.map(PathBuf::from),
        build_metadata_sha256: build_metadata_sha256
            .ok_or_else(|| ComparisonError::usage("missing --build-metadata-sha256 <sha256>"))?,
        manifest: PathBuf::from(
            manifest.ok_or_else(|| ComparisonError::usage("missing --manifest <path>"))?,
        ),
        manifest_sha256: manifest_sha256
            .ok_or_else(|| ComparisonError::usage("missing --manifest-sha256 <sha256>"))?,
        out_dir: PathBuf::from(
            out_dir.ok_or_else(|| ComparisonError::usage("missing --out-dir <new-dir>"))?,
        ),
        language,
    })
}

fn capture_baseline_receipt_from_args(
    args: &[String],
) -> Result<(PathBuf, String, usize), ComparisonError> {
    let plan = capture_plan_from_args(args)?;
    let repo_root = repository_root();
    let binary = require_binary(&plan.binary)?;
    let build = load_build_metadata(
        &repo_root,
        &binary,
        plan.build_metadata.as_deref(),
        &plan.build_metadata_sha256,
    )?;
    if build.record.build.profile != "release" {
        return Err(ComparisonError::environment(
            "baseline receipt capture requires release build metadata",
        ));
    }
    validate_sha256(&plan.manifest_sha256).map_err(|_| {
        ComparisonError::usage("manifest SHA-256 must contain 64 hexadecimal digits")
    })?;
    let manifest_path =
        fs::canonicalize(resolve_path(&repo_root, &plan.manifest)).map_err(|error| {
            ComparisonError::environment(format!(
                "cannot resolve coverage manifest {}: {error}",
                plan.manifest.display()
            ))
        })?;
    let manifest_bytes = fs::read(&manifest_path).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to read coverage manifest {}: {error}",
            manifest_path.display()
        ))
    })?;
    let manifest_sha256 = sha256(&manifest_bytes);
    if manifest_sha256 != plan.manifest_sha256.to_ascii_lowercase() {
        return Err(ComparisonError::environment(format!(
            "coverage manifest SHA-256 mismatch: expected {}, found {}",
            plan.manifest_sha256, manifest_sha256
        )));
    }
    let manifest: CoverageManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        ComparisonError::environment(format!("invalid coverage manifest: {error}"))
    })?;
    crate::coverage::validate_manifest_shape(&manifest)
        .map_err(|error| ComparisonError::environment(error.to_string()))?;
    let inputs = select_inputs(&repo_root, &manifest, plan.language.as_deref())?;
    if inputs.is_empty() {
        return Err(ComparisonError::environment(
            "baseline receipt capture selected an empty corpus",
        ));
    }
    let original_state =
        comparison_input_state_sha256(&inputs, None).map_err(ComparisonError::environment)?;
    let updated_state = comparison_input_state_sha256(&inputs, Some(inputs.len() - 1))
        .map_err(ComparisonError::environment)?;
    let out_dir = resolve_path(&repo_root, &plan.out_dir);
    fs::create_dir(&out_dir).map_err(|error| {
        ComparisonError::environment(format!(
            "baseline receipt output directory must be new: {}: {error}",
            out_dir.display()
        ))
    })?;
    let out_dir = fs::canonicalize(&out_dir).map_err(|error| {
        ComparisonError::environment(format!(
            "cannot canonicalize receipt output directory: {error}"
        ))
    })?;
    let language_scope = plan.language.as_deref().unwrap_or("all");
    let mut snapshots = Vec::new();
    let mut written_dumps = BTreeSet::new();
    for level in ["references", "full"] {
        let label = format!("{language_scope}-{level}-workloads");
        let mut snapshot_writer = BaselineSnapshotWriter {
            out_dir: &out_dir,
            language: language_scope,
            level,
            written_dumps: &mut written_dumps,
            snapshots: &mut snapshots,
        };
        let stage = stage_corpus(&out_dir, &label, "baseline", &inputs)?;
        let validation_stage = StagedCorpus {
            root: stage.root.clone(),
            artifact: stage.validation_artifact.clone(),
            validation_artifact: stage.validation_artifact.clone(),
            inputs: stage.inputs.clone(),
            update_index: stage.update_index,
        };
        clear_artifact(&stage.artifact)?;
        let (fresh_health, original_dump) =
            capture_operation(&binary, &stage, level, WorkloadKind::FreshScan)?;
        require_capture_state(&stage, &original_state, &label)?;
        snapshot_writer.record("fresh_scan", &original_state, &fresh_health, &original_dump)?;

        let (no_change_health, no_change_dump) =
            capture_operation(&binary, &stage, level, WorkloadKind::NoChangeScan)?;
        validate_operation_fresh_equivalence(&no_change_dump, &original_dump, &original_state)
            .map_err(ComparisonError::environment)?;
        snapshot_writer.record(
            "no_change_scan",
            &original_state,
            &no_change_health,
            &no_change_dump,
        )?;

        clear_artifact(&stage.artifact)?;
        let (_, update_original_dump) =
            capture_operation(&binary, &stage, level, WorkloadKind::FreshScan)?;
        validate_operation_fresh_equivalence(
            &update_original_dump,
            &original_dump,
            &original_state,
        )
        .map_err(ComparisonError::environment)?;
        write_update_variant(&stage, true)?;
        let (updated_health, updated_dump) =
            capture_operation(&binary, &stage, level, WorkloadKind::SingleFileUpdate)?;
        require_capture_state(&stage, &updated_state, &label)?;
        let (fresh_updated_health, fresh_updated_dump) =
            capture_fresh_equivalent(&binary, &validation_stage, level)?;
        validate_operation_fresh_equivalence(&updated_dump, &fresh_updated_dump, &updated_state)
            .map_err(ComparisonError::environment)?;
        snapshot_writer.record(
            "single_file_update",
            &updated_state,
            &updated_health,
            &updated_dump,
        )?;
        snapshot_writer.record(
            "fresh_scan",
            &updated_state,
            &fresh_updated_health,
            &fresh_updated_dump,
        )?;

        write_update_variant(&stage, false)?;
        let (restored_health, restored_dump) =
            capture_operation(&binary, &stage, level, WorkloadKind::SingleFileUpdate)?;
        require_capture_state(&stage, &original_state, &label)?;
        let (fresh_restored_health, fresh_restored_dump) =
            capture_fresh_equivalent(&binary, &validation_stage, level)?;
        if fresh_restored_health != fresh_health {
            return Err(ComparisonError::environment(format!(
                "baseline fresh-scan health changed for restored source state {original_state}"
            )));
        }
        validate_operation_fresh_equivalence(&restored_dump, &fresh_restored_dump, &original_state)
            .map_err(ComparisonError::environment)?;
        snapshot_writer.record(
            "single_file_update",
            &original_state,
            &restored_health,
            &restored_dump,
        )?;
    }
    let receipt = BaselineExpectationReceipt {
        format_version: 1,
        baseline_binary_sha256: build.record.binary_sha256.clone(),
        manifest_sha256,
        source: build.record.source,
        snapshots,
    };
    validate_baseline_receipt(&receipt).map_err(ComparisonError::environment)?;
    let receipt_path = out_dir.join("baseline-expectations.json");
    let receipt_bytes = serde_json::to_vec_pretty(&receipt)
        .map_err(|error| ComparisonError::environment(error.to_string()))?;
    let mut receipt_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&receipt_path)
        .map_err(|error| {
            ComparisonError::environment(format!(
                "cannot create baseline expectation receipt {}: {error}",
                receipt_path.display()
            ))
        })?;
    receipt_file.write_all(&receipt_bytes).map_err(|error| {
        ComparisonError::environment(format!(
            "cannot write baseline expectation receipt: {error}"
        ))
    })?;
    let snapshot_count = receipt.snapshots.len();
    Ok((receipt_path, sha256(&receipt_bytes), snapshot_count))
}

fn capture_operation(
    binary: &Path,
    stage: &StagedCorpus,
    level: &str,
    workload: WorkloadKind,
) -> Result<(ComparisonHealth, ArtifactDump), ComparisonError> {
    let report = run_scan_unmeasured(binary, stage, level)?;
    let (health, failures) = report_health(&report)?;
    if let Some(failure) = failures.first() {
        return Err(ComparisonError::environment(format!(
            "baseline {} reports {failure}",
            workload.name()
        )));
    }
    validate_operation(&report, workload, stage.inputs.len(), &health).map_err(|error| {
        ComparisonError::environment(format!("baseline {}: {error}", workload.name()))
    })?;
    let dump = dump_comparison_artifact(&stage.artifact).map_err(ComparisonError::environment)?;
    Ok((health, dump))
}

fn capture_fresh_equivalent(
    binary: &Path,
    stage: &StagedCorpus,
    level: &str,
) -> Result<(ComparisonHealth, ArtifactDump), ComparisonError> {
    clear_artifact(&stage.artifact)?;
    capture_operation(binary, stage, level, WorkloadKind::FreshScan)
}

fn require_capture_state(
    stage: &StagedCorpus,
    expected_sha256: &str,
    label: &str,
) -> Result<(), ComparisonError> {
    let actual_sha256 = staged_source_state_sha256(stage)?;
    if actual_sha256 != expected_sha256 {
        return Err(ComparisonError::environment(format!(
            "baseline snapshot {label} source-state hash differed from preflight"
        )));
    }
    Ok(())
}

fn write_semantic_dump(
    out_dir: &Path,
    language: &str,
    level: &str,
    source_state_sha256: &str,
    dump: &ArtifactDump,
) -> Result<(), ComparisonError> {
    let mut text = String::new();
    for table in &dump.tables {
        let _ = writeln!(text, "#table\t{}", table.table);
        text.push_str(&table.text);
        if !table.text.ends_with('\n') {
            text.push('\n');
        }
    }
    let path = out_dir.join(format!(
        "semantic-dump-{language}-{level}-{source_state_sha256}.tsv"
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| {
            ComparisonError::environment(format!(
                "cannot create baseline semantic dump {}: {error}",
                path.display()
            ))
        })?;
    file.write_all(text.as_bytes()).map_err(|error| {
        ComparisonError::environment(format!("cannot write baseline semantic dump: {error}"))
    })
}

fn run_scan_unmeasured(
    binary: &Path,
    stage: &StagedCorpus,
    level: &str,
) -> Result<Value, ComparisonError> {
    let output = Command::new(binary)
        .args(["scan", "--root"])
        .arg(&stage.root)
        .args(["--db"])
        .arg(&stage.artifact)
        .args(["--json", "--level", level, "--jobs", "1"])
        .output()
        .map_err(|error| {
            ComparisonError::environment(format!(
                "failed to run baseline extractor {}: {error}",
                binary.display()
            ))
        })?;
    if !output.status.success() {
        return Err(ComparisonError::environment(format!(
            "baseline extractor {} exited with {}: {}",
            binary.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        ComparisonError::environment(format!("baseline extractor returned invalid JSON: {error}"))
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RowChangeAction {
    Add,
    Remove,
}

impl RowChangeAction {
    fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Remove => "remove",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AllowedRowChange {
    pub id: String,
    pub ledger_marker: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub workload: Option<String>,
    pub table: String,
    pub action: RowChangeAction,
    pub canonical_row: String,
    pub row_sha256: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RowDelta {
    pub table: String,
    pub action: RowChangeAction,
    pub canonical_row: String,
    pub row_sha256: String,
    pub count: usize,
    pub allowlist_id: String,
    pub ledger_marker: String,
    pub allowed: bool,
    pub source_state_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DumpComparison {
    pub row_deltas: Vec<RowDelta>,
    pub used_allowlist_ids: Vec<String>,
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComparisonHealth {
    pub files_scanned: u64,
    pub files_failed: u64,
    pub files_unsupported: u64,
    pub errors: u64,
    pub skipped: BTreeMap<String, u64>,
    pub warnings: BTreeMap<String, u64>,
    pub parse_diagnostics: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SampleMetrics {
    pub elapsed_ms: u128,
    pub peak_rss_bytes: u64,
    pub artifact_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationSample {
    pub order: String,
    pub elapsed_ms: u128,
    pub peak_rss_bytes: u64,
    pub artifact_bytes: u64,
    pub normalized: NormalizedMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MetricRange {
    pub min: u128,
    pub median: u128,
    pub max: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SampleSeries {
    pub calibration_operations: usize,
    pub baseline_calibration_elapsed_ms: u128,
    pub samples: Vec<OperationSample>,
    pub elapsed_ms: MetricRange,
    pub peak_rss_bytes: MetricRange,
    pub artifact_bytes: MetricRange,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkloadResult {
    pub language: String,
    pub level: String,
    pub workload: String,
    pub baseline: SampleSeries,
    pub candidate: SampleSeries,
    pub baseline_health: ComparisonHealth,
    pub candidate_health: ComparisonHealth,
    pub row_deltas: Vec<RowDelta>,
    pub gate_failures: Vec<String>,
    pub repeat_passed: Option<bool>,
    pub previous_attempt: Option<Box<PreviousAttempt>>,
    pub operation_state_sha256: Vec<String>,
    pub approved_budget_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreviousAttempt {
    pub baseline: SampleSeries,
    pub candidate: SampleSeries,
    pub gate_failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComparisonSummary {
    pub format_version: u32,
    pub started_unix_seconds: u64,
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub allowlist: Option<PathBuf>,
    pub allowlist_sha256: Option<String>,
    pub baseline_expectations: PathBuf,
    pub baseline_expectations_sha256: String,
    pub budget_approvals: Option<PathBuf>,
    pub budget_approvals_sha256: Option<String>,
    pub output_ledger: PathBuf,
    pub output_ledger_sha256: String,
    pub harness: ComparisonHarnessProvenance,
    pub measurement_policy: ComparisonPolicy,
    pub summary_path: PathBuf,
    pub language_filter: Option<String>,
    pub baseline: ComparisonProvenance,
    pub candidate: ComparisonProvenance,
    pub workloads: Vec<WorkloadResult>,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetViolation {
    pub metric: &'static str,
    pub baseline: u64,
    pub candidate: u64,
}

pub fn budget_violations(
    baseline: &SampleMetrics,
    candidate: &SampleMetrics,
) -> Vec<BudgetViolation> {
    let mut violations = Vec::new();
    let (elapsed_percent, elapsed_absolute) = default_budget_thresholds(BudgetMetric::ElapsedMs);
    let time_delta = candidate.elapsed_ms.saturating_sub(baseline.elapsed_ms);
    if time_delta > u128::from(elapsed_absolute)
        && time_delta.saturating_mul(100) > baseline.elapsed_ms.saturating_mul(elapsed_percent)
    {
        violations.push(BudgetViolation {
            metric: "elapsed_ms",
            baseline: clamp_u128(baseline.elapsed_ms),
            candidate: clamp_u128(candidate.elapsed_ms),
        });
    }
    let (memory_percent, memory_absolute) = default_budget_thresholds(BudgetMetric::PeakRssBytes);
    let memory_delta = candidate
        .peak_rss_bytes
        .saturating_sub(baseline.peak_rss_bytes);
    if memory_delta > memory_absolute
        && u128::from(memory_delta).saturating_mul(100)
            > u128::from(baseline.peak_rss_bytes).saturating_mul(memory_percent)
    {
        violations.push(BudgetViolation {
            metric: "peak_rss_bytes",
            baseline: baseline.peak_rss_bytes,
            candidate: candidate.peak_rss_bytes,
        });
    }
    let (size_percent, size_absolute) = default_budget_thresholds(BudgetMetric::ArtifactBytes);
    let size_delta = candidate
        .artifact_bytes
        .saturating_sub(baseline.artifact_bytes);
    if size_delta > size_absolute
        && u128::from(size_delta).saturating_mul(100)
            > u128::from(baseline.artifact_bytes).saturating_mul(size_percent)
    {
        violations.push(BudgetViolation {
            metric: "artifact_bytes",
            baseline: baseline.artifact_bytes,
            candidate: candidate.artifact_bytes,
        });
    }
    violations
}

fn default_budget_thresholds(metric: BudgetMetric) -> (u128, u64) {
    match metric {
        BudgetMetric::ElapsedMs => (ELAPSED_GROWTH_PERCENT, ELAPSED_GROWTH_MS as u64),
        BudgetMetric::PeakRssBytes => (PEAK_RSS_GROWTH_PERCENT, PEAK_RSS_GROWTH_BYTES),
        BudgetMetric::ArtifactBytes => (ARTIFACT_GROWTH_PERCENT, ARTIFACT_GROWTH_BYTES),
    }
}

pub fn approved_budget_deltas_sha256(deltas: &[ApprovedBudgetDelta]) -> String {
    let mut sorted = deltas.to_vec();
    sorted.sort();
    sha256(&serde_json::to_vec(&sorted).expect("budget delta records serialize to JSON"))
}

pub fn validate_budget_approval_file(file: &BudgetApprovalFile) -> Result<(), String> {
    if file.format_version != 1 {
        return Err(format!(
            "budget approval format version {} is unsupported",
            file.format_version
        ));
    }
    if file.approvals.is_empty() {
        return Err("budget approval file has no approval entries".to_string());
    }
    let mut ids = BTreeSet::new();
    let mut scopes = BTreeSet::new();
    for approval in &file.approvals {
        if approval.id.trim().is_empty() || !ids.insert(approval.id.as_str()) {
            return Err(format!(
                "duplicate or empty budget approval id `{}`",
                approval.id
            ));
        }
        for (label, value) in [
            ("owner", approval.owner.as_str()),
            ("approval reference", approval.approval_reference.as_str()),
            ("language", approval.language.as_str()),
            ("baseline profile", approval.baseline_profile.as_str()),
            ("candidate profile", approval.candidate_profile.as_str()),
            ("reason", approval.reason.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(format!(
                    "budget approval `{}` has an empty {label}",
                    approval.id
                ));
            }
        }
        for (label, hash) in [
            ("approval evidence", &approval.approval_evidence_sha256),
            ("baseline binary", &approval.baseline_binary_sha256),
            ("candidate binary", &approval.candidate_binary_sha256),
            ("manifest", &approval.manifest_sha256),
            ("approved row deltas", &approval.approved_deltas_sha256),
        ] {
            validate_sha256(hash).map_err(|_| {
                format!(
                    "budget approval `{}` has invalid {label} SHA-256",
                    approval.id
                )
            })?;
        }
        if !["references", "full"].contains(&approval.level.as_str())
            || !["fresh_scan", "no_change_scan", "single_file_update"]
                .contains(&approval.workload.as_str())
        {
            return Err(format!(
                "budget approval `{}` has an invalid scope",
                approval.id
            ));
        }
        let scope = (
            approval.language.as_str(),
            approval.level.as_str(),
            approval.workload.as_str(),
            approval.metric,
        );
        if !scopes.insert(scope) {
            return Err(format!(
                "duplicate budget approval scope for `{}`",
                approval.id
            ));
        }
        let (default_percent, default_absolute) = default_budget_thresholds(approval.metric);
        if approval.maximum_growth_percent <= default_percent
            || approval.maximum_growth_absolute <= default_absolute
        {
            return Err(format!(
                "budget approval `{}` must raise both default growth thresholds",
                approval.id
            ));
        }
        if approval.approved_deltas.is_empty() {
            return Err(format!(
                "budget approval `{}` has no approved row additions",
                approval.id
            ));
        }
        if approval
            .approved_deltas
            .iter()
            .any(|delta| delta.action != RowChangeAction::Add)
        {
            return Err(format!(
                "budget approval `{}` may bind only exact added rows",
                approval.id
            ));
        }
        let mut computed_allocation = BTreeMap::new();
        for delta in &approval.approved_deltas {
            if delta.table.trim().is_empty()
                || delta.count == 0
                || delta.allowlist_id.trim().is_empty()
                || delta.ledger_marker.trim().is_empty()
            {
                return Err(format!(
                    "budget approval `{}` has an incomplete row delta",
                    approval.id
                ));
            }
            if sha256(delta.canonical_row.as_bytes()) != delta.row_sha256 {
                return Err(format!(
                    "budget approval `{}` row hash mismatch",
                    approval.id
                ));
            }
            validate_sha256(&delta.source_state_sha256).map_err(|_| {
                format!(
                    "budget approval `{}` has an invalid source-state SHA-256",
                    approval.id
                )
            })?;
            *computed_allocation.entry(delta.table.clone()).or_insert(0) += delta.count as u64;
        }
        if computed_allocation != approval.table_row_counts {
            return Err(format!(
                "budget approval `{}` table allocation differs from exact approved rows",
                approval.id
            ));
        }
        if approved_budget_deltas_sha256(&approval.approved_deltas)
            != approval.approved_deltas_sha256
        {
            return Err(format!(
                "budget approval `{}` row-delta SHA-256 mismatch",
                approval.id
            ));
        }
    }
    Ok(())
}

pub fn validate_budget_approval_context(
    approvals: &[BudgetApproval],
    context: BudgetApprovalContext<'_>,
) -> Result<(), String> {
    for approval in approvals {
        if approval.language != context.language
            || approval.level != context.level
            || approval.workload != context.workload
        {
            return Err(format!(
                "budget approval `{}` is outside the selected language, level, or workload scope",
                approval.id
            ));
        }
        if approval.baseline_binary_sha256 != context.baseline_binary_sha256
            || approval.candidate_binary_sha256 != context.candidate_binary_sha256
            || approval.manifest_sha256 != context.manifest_sha256
            || approval.baseline_profile != context.baseline_profile
            || approval.candidate_profile != context.candidate_profile
        {
            return Err(format!(
                "budget approval `{}` is pinned to different binaries, manifest, or profiles",
                approval.id
            ));
        }
    }
    Ok(())
}

pub fn evaluate_budgets(
    baseline: &SampleMetrics,
    candidate: &SampleMetrics,
    row_deltas: &[RowDelta],
    approvals: &[BudgetApproval],
    context: BudgetApprovalContext<'_>,
) -> BudgetEvaluation {
    let default_violations = budget_violations(baseline, candidate);
    let allowed_additions = row_deltas
        .iter()
        .filter(|delta| delta.allowed && delta.action == RowChangeAction::Add)
        .filter_map(|delta| {
            Some(ApprovedBudgetDelta {
                table: delta.table.clone(),
                action: delta.action,
                canonical_row: delta.canonical_row.clone(),
                row_sha256: delta.row_sha256.clone(),
                count: delta.count,
                allowlist_id: delta.allowlist_id.clone(),
                ledger_marker: delta.ledger_marker.clone(),
                source_state_sha256: delta.source_state_sha256.clone()?,
            })
        })
        .collect::<Vec<_>>();
    let mut violations = Vec::new();
    let mut used_approval_ids = BTreeSet::new();
    let mut failures = Vec::new();
    let mut matched_ids = BTreeSet::new();
    for violation in default_violations {
        let metric = match violation.metric {
            "elapsed_ms" => BudgetMetric::ElapsedMs,
            "peak_rss_bytes" => BudgetMetric::PeakRssBytes,
            "artifact_bytes" => BudgetMetric::ArtifactBytes,
            _ => continue,
        };
        let matches = approvals
            .iter()
            .filter(|approval| {
                approval.language == context.language
                    && approval.level == context.level
                    && approval.workload == context.workload
                    && approval.metric == metric
            })
            .collect::<Vec<_>>();
        let Some(approval) = matches.first().copied() else {
            violations.push(violation);
            continue;
        };
        matched_ids.insert(approval.id.as_str());
        if matches.len() != 1 {
            failures.push(format!(
                "multiple owner approvals match {}/{} /{}",
                context.language, context.level, context.workload
            ));
            violations.push(violation);
            continue;
        }
        if let Err(error) =
            validate_budget_approval_context(std::slice::from_ref(approval), context)
        {
            failures.push(error);
            violations.push(violation);
            continue;
        }
        let mut actual = allowed_additions.clone();
        actual.sort();
        let mut expected = approval.approved_deltas.clone();
        expected.sort();
        if actual.is_empty() || actual != expected {
            failures.push(format!(
                "budget approval `{}` does not match the exact added-row set for {}/{}/{}",
                approval.id, context.language, context.level, context.workload
            ));
            violations.push(violation);
            continue;
        }
        let (growth, baseline_value) = match metric {
            BudgetMetric::ElapsedMs => (
                candidate.elapsed_ms.saturating_sub(baseline.elapsed_ms),
                baseline.elapsed_ms,
            ),
            BudgetMetric::PeakRssBytes => (
                u128::from(
                    candidate
                        .peak_rss_bytes
                        .saturating_sub(baseline.peak_rss_bytes),
                ),
                u128::from(baseline.peak_rss_bytes),
            ),
            BudgetMetric::ArtifactBytes => (
                u128::from(
                    candidate
                        .artifact_bytes
                        .saturating_sub(baseline.artifact_bytes),
                ),
                u128::from(baseline.artifact_bytes),
            ),
        };
        if growth > u128::from(approval.maximum_growth_absolute)
            && growth.saturating_mul(100)
                > baseline_value.saturating_mul(approval.maximum_growth_percent)
        {
            violations.push(violation);
        } else {
            used_approval_ids.insert(approval.id.clone());
        }
    }
    for approval in approvals.iter().filter(|approval| {
        approval.language == context.language
            && approval.level == context.level
            && approval.workload == context.workload
    }) {
        if !matched_ids.contains(approval.id.as_str()) {
            failures.push(format!(
                "unused budget approval `{}` in {}/{}/{} scope",
                approval.id, context.language, context.level, context.workload
            ));
        }
    }
    BudgetEvaluation {
        violations,
        used_approval_ids: used_approval_ids.into_iter().collect(),
        failures,
    }
}

pub fn sample_spread_is_conclusive(samples: &[u128]) -> bool {
    if samples.len() != SAMPLE_COUNT {
        return false;
    }
    let mut ordered = samples.to_vec();
    ordered.sort_unstable();
    let median = ordered[ordered.len() / 2];
    ordered[ordered.len() - 1].saturating_sub(ordered[0])
        <= (median.saturating_mul(MAX_SAMPLE_SPREAD_PERCENT) / 100).max(MIN_SAMPLE_SPREAD_MS)
}

pub fn validate_provenance_pair(
    baseline: &ComparisonProvenance,
    candidate: &ComparisonProvenance,
) -> Result<(), String> {
    for (name, provenance) in [("baseline", baseline), ("candidate", candidate)] {
        validate_sha256(&provenance.manifest_sha256)
            .map_err(|_| format!("{name} manifest SHA-256 is invalid"))?;
        validate_sha256(&provenance.binary_sha256)
            .map_err(|_| format!("{name} binary SHA-256 is invalid"))?;
        validate_sha256(&provenance.build_metadata_sha256)
            .map_err(|_| format!("{name} build metadata SHA-256 is invalid"))?;
        if provenance.build_metadata_format_version != 1 {
            return Err(format!(
                "{name} build metadata format version is unsupported"
            ));
        }
        if provenance.binary_path.as_os_str().is_empty()
            || provenance.build_metadata_path.as_os_str().is_empty()
            || provenance.binary_version.trim().is_empty()
        {
            return Err(format!("{name} binary identity provenance is incomplete"));
        }
        validate_build_metadata(
            &BuildMetadata {
                format_version: 1,
                binary_sha256: provenance.binary_sha256.clone(),
                source: provenance.source.clone(),
                build: provenance.build.clone(),
            },
            &provenance.binary_sha256,
        )
        .map_err(|error| format!("{name} build provenance is invalid: {error}"))?;
        if provenance.profile != provenance.build.profile
            || provenance.toolchain
                != format!("{}; {}", provenance.build.rustc, provenance.build.cargo)
        {
            return Err(format!(
                "{name} provenance disagrees with its build metadata"
            ));
        }
        for (field, value) in [
            ("toolchain", provenance.toolchain.as_str()),
            ("profile", provenance.profile.as_str()),
            ("level", provenance.level.as_str()),
            ("OS", provenance.os.as_str()),
            ("CPU", provenance.cpu.as_str()),
            ("storage", provenance.storage.as_str()),
            ("cache policy", provenance.cache_policy.as_str()),
            ("peak RSS method", provenance.peak_rss_method.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{name} {field} provenance is empty"));
            }
        }
        if provenance.jobs == 0 || provenance.memory_bytes == 0 {
            return Err(format!("{name} job or memory provenance is invalid"));
        }
    }
    if baseline.manifest_sha256 != candidate.manifest_sha256 {
        return Err("baseline and candidate manifest hashes differ".to_string());
    }
    for (field, baseline_value, candidate_value) in [
        ("toolchain", &baseline.toolchain, &candidate.toolchain),
        ("profile", &baseline.profile, &candidate.profile),
        ("level", &baseline.level, &candidate.level),
        ("OS", &baseline.os, &candidate.os),
        ("CPU", &baseline.cpu, &candidate.cpu),
        ("storage", &baseline.storage, &candidate.storage),
        (
            "cache policy",
            &baseline.cache_policy,
            &candidate.cache_policy,
        ),
    ] {
        if baseline_value != candidate_value {
            return Err(format!("baseline and candidate {field} provenance differs"));
        }
    }
    if baseline.peak_rss_method != candidate.peak_rss_method {
        return Err("baseline and candidate peak RSS method provenance differs".to_string());
    }
    if baseline.jobs != candidate.jobs {
        return Err("baseline and candidate jobs provenance differs".to_string());
    }
    if baseline.build.rustc != candidate.build.rustc
        || baseline.build.cargo != candidate.build.cargo
        || baseline.build.host_target != candidate.build.host_target
        || baseline.build.target != candidate.build.target
        || baseline.build.profile != candidate.build.profile
        || baseline.build.features != candidate.build.features
        || baseline.build.options != candidate.build.options
    {
        return Err("baseline and candidate build metadata is incompatible".to_string());
    }
    if baseline.memory_bytes != candidate.memory_bytes {
        return Err("baseline and candidate memory provenance differs".to_string());
    }
    Ok(())
}

pub fn build_recipe_sha256(recipe: &[String]) -> String {
    sha256(&serde_json::to_vec(recipe).expect("string arrays serialize to JSON"))
}

pub fn normalized_metrics(
    elapsed_nanos: u128,
    operation_count: usize,
    file_count: u64,
    semantic_row_total: u64,
    artifact_bytes: u64,
) -> NormalizedMetrics {
    NormalizedMetrics {
        operation_count,
        elapsed_nanos,
        elapsed_nanos_per_operation: (operation_count > 0)
            .then(|| elapsed_nanos / operation_count as u128),
        file_count,
        elapsed_nanos_per_file: (file_count > 0).then(|| elapsed_nanos / u128::from(file_count)),
        semantic_row_total,
        artifact_bytes,
        bytes_per_row_numerator: artifact_bytes,
        bytes_per_row_denominator: semantic_row_total,
        state_sizes: Vec::new(),
    }
}

pub fn validate_operation_fresh_equivalence(
    operation_dump: &ArtifactDump,
    fresh_dump: &ArtifactDump,
    source_state_sha256: &str,
) -> Result<(), String> {
    if operation_dump == fresh_dump {
        Ok(())
    } else {
        Err(format!(
            "semantic output differs from a fresh scan for source state {source_state_sha256}"
        ))
    }
}

pub fn validate_build_metadata_pair(
    baseline: &BuildMetadata,
    candidate: &BuildMetadata,
    baseline_binary_sha256: &str,
    candidate_binary_sha256: &str,
) -> Result<(), String> {
    validate_build_metadata(baseline, baseline_binary_sha256)
        .map_err(|error| format!("baseline build metadata {error}"))?;
    validate_build_metadata(candidate, candidate_binary_sha256)
        .map_err(|error| format!("candidate build metadata {error}"))?;
    let left = &baseline.build;
    let right = &candidate.build;
    for (label, a, b) in [
        ("rustc", &left.rustc, &right.rustc),
        ("Cargo", &left.cargo, &right.cargo),
        ("host target", &left.host_target, &right.host_target),
        ("target", &left.target, &right.target),
        ("profile", &left.profile, &right.profile),
    ] {
        if a != b {
            return Err(format!("baseline and candidate {label} differ"));
        }
    }
    if left.features != right.features {
        return Err("baseline and candidate features differ".to_string());
    }
    if left.options != right.options {
        return Err("baseline and candidate build options differ".to_string());
    }
    Ok(())
}

fn validate_build_metadata(metadata: &BuildMetadata, binary_sha256: &str) -> Result<(), String> {
    if metadata.format_version != 1 {
        return Err(format!(
            "format version {} is unsupported",
            metadata.format_version
        ));
    }
    validate_sha256(&metadata.binary_sha256)
        .map_err(|_| "binary SHA-256 is invalid".to_string())?;
    if metadata.binary_sha256 != binary_sha256 {
        return Err("binary SHA-256 does not match the build metadata".to_string());
    }
    if metadata.source.commit.len() != 40
        || !metadata
            .source
            .commit
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err("source commit must be a full 40-digit commit ID".to_string());
    }
    if metadata.source.tree_hash_algorithm != SOURCE_TREE_HASH_ALGORITHM {
        return Err(format!(
            "source tree hash algorithm must be `{SOURCE_TREE_HASH_ALGORITHM}`"
        ));
    }
    validate_sha256(&metadata.source.tree_sha256)
        .map_err(|_| "source tree SHA-256 is invalid".to_string())?;
    if metadata.source.dirty {
        let state_hash = metadata
            .source
            .dirty_state_sha256
            .as_deref()
            .ok_or_else(|| "dirty source requires a dirty-state SHA-256".to_string())?;
        validate_sha256(state_hash)
            .map_err(|_| "dirty source state SHA-256 is invalid".to_string())?;
    } else if metadata.source.dirty_state_sha256.is_some() {
        return Err("clean source cannot carry a dirty-state SHA-256".to_string());
    }
    for (label, value) in [
        ("rustc", metadata.build.rustc.as_str()),
        ("Cargo", metadata.build.cargo.as_str()),
        ("host target", metadata.build.host_target.as_str()),
        ("target", metadata.build.target.as_str()),
        ("profile", metadata.build.profile.as_str()),
        ("build evidence kind", metadata.build.evidence.kind.as_str()),
        (
            "build evidence reference",
            metadata.build.evidence.reference.as_str(),
        ),
    ] {
        if value.trim().is_empty() {
            return Err(format!("{label} is empty"));
        }
    }
    if metadata.build.recipe.is_empty() || metadata.build.recipe.iter().any(|part| part.is_empty())
    {
        return Err("build recipe is missing or contains an empty argument".to_string());
    }
    validate_sha256(&metadata.build.recipe_sha256)
        .map_err(|_| "build recipe SHA-256 is invalid".to_string())?;
    if metadata.build.recipe_sha256 != build_recipe_sha256(&metadata.build.recipe) {
        return Err("build recipe SHA-256 does not match the recorded recipe".to_string());
    }
    validate_sha256(&metadata.build.evidence.sha256)
        .map_err(|_| "build evidence SHA-256 is invalid".to_string())?;
    let mut features = BTreeSet::new();
    if metadata
        .build
        .features
        .iter()
        .any(|feature| feature.trim().is_empty() || !features.insert(feature))
    {
        return Err("build features are empty or duplicated".to_string());
    }
    if metadata
        .build
        .options
        .keys()
        .any(|option| option.trim().is_empty())
    {
        return Err("build options contain an empty key".to_string());
    }
    Ok(())
}

pub fn compare_health(
    baseline: &ComparisonHealth,
    candidate: &ComparisonHealth,
) -> Result<(), String> {
    if baseline.files_scanned != candidate.files_scanned {
        return Err(format!(
            "scanned file count changed from {} to {}",
            baseline.files_scanned, candidate.files_scanned
        ));
    }
    compare_nonincreasing(
        "failed file",
        &baseline.files_failed,
        &candidate.files_failed,
    )?;
    compare_nonincreasing(
        "unsupported file",
        &baseline.files_unsupported,
        &candidate.files_unsupported,
    )?;
    compare_nonincreasing("error", &baseline.errors, &candidate.errors)?;
    compare_health_map_exact("skipped", &baseline.skipped, &candidate.skipped)?;
    compare_health_map_exact("warning", &baseline.warnings, &candidate.warnings)?;
    compare_health_map(
        "parse diagnostic",
        &baseline.parse_diagnostics,
        &candidate.parse_diagnostics,
    )?;
    Ok(())
}

pub fn compare_artifact_dumps(
    baseline: &ArtifactDump,
    candidate: &ArtifactDump,
    allowlist: &[AllowedRowChange],
    ledger: &str,
) -> Result<Vec<RowDelta>, String> {
    let comparison = inspect_artifact_dumps_scoped(baseline, candidate, allowlist, ledger, None)?;
    if let Some(failure) = comparison.failures.first() {
        return Err(failure.clone());
    }
    if let Some(stale) = allowlist
        .iter()
        .find(|entry| !comparison.used_allowlist_ids.contains(&entry.id))
    {
        return Err(format!(
            "unused allowlist entry `{}` for {} {} row {}",
            stale.id,
            stale.action.name(),
            stale.table,
            stale.canonical_row
        ));
    }
    Ok(comparison.row_deltas)
}

pub fn compare_artifact_dumps_for_scope(
    baseline: &ArtifactDump,
    candidate: &ArtifactDump,
    allowlist: &[AllowedRowChange],
    ledger: &str,
    language: &str,
    level: &str,
    workload: &str,
) -> Result<Vec<RowDelta>, String> {
    let scope = AllowlistScope {
        language,
        level,
        workload,
    };
    let comparison =
        inspect_artifact_dumps_scoped(baseline, candidate, allowlist, ledger, Some(scope))?;
    if let Some(failure) = comparison.failures.first() {
        return Err(failure.clone());
    }
    validate_allowlist_scope_usage(
        allowlist,
        &comparison.used_allowlist_ids,
        language,
        level,
        workload,
    )?;
    Ok(comparison.row_deltas)
}

pub fn inspect_artifact_dumps(
    baseline: &ArtifactDump,
    candidate: &ArtifactDump,
    allowlist: &[AllowedRowChange],
    ledger: &str,
) -> Result<DumpComparison, String> {
    inspect_artifact_dumps_scoped(baseline, candidate, allowlist, ledger, None)
}

#[derive(Clone, Copy)]
struct AllowlistScope<'a> {
    language: &'a str,
    level: &'a str,
    workload: &'a str,
}

fn inspect_artifact_dumps_scoped(
    baseline: &ArtifactDump,
    candidate: &ArtifactDump,
    allowlist: &[AllowedRowChange],
    ledger: &str,
    scope: Option<AllowlistScope<'_>>,
) -> Result<DumpComparison, String> {
    let allowances = validated_allowlist(allowlist, ledger)?;
    let baseline_tables = dump_tables(baseline)?;
    let candidate_tables = dump_tables(candidate)?;
    if baseline_tables.keys().collect::<Vec<_>>() != candidate_tables.keys().collect::<Vec<_>>() {
        return Err("durable SQLite table sets differ".to_string());
    }
    let tables = baseline_tables
        .keys()
        .chain(candidate_tables.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut deltas = Vec::new();
    let mut failures = Vec::new();
    let mut used = BTreeSet::new();
    for table in tables {
        let (baseline_header, baseline_rows) = baseline_tables
            .get(&table)
            .expect("table keys were compared");
        let (candidate_header, candidate_rows) = candidate_tables
            .get(&table)
            .expect("table keys were compared");
        if baseline_header != candidate_header {
            return Err(format!("column contract differs for table `{table}`"));
        }
        let all_rows = baseline_rows
            .keys()
            .chain(candidate_rows.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        for canonical_row in all_rows {
            let baseline_count = baseline_rows.get(&canonical_row).copied().unwrap_or(0);
            let candidate_count = candidate_rows.get(&canonical_row).copied().unwrap_or(0);
            let differences = [
                (
                    RowChangeAction::Remove,
                    baseline_count.saturating_sub(candidate_count),
                ),
                (
                    RowChangeAction::Add,
                    candidate_count.saturating_sub(baseline_count),
                ),
            ];
            for (action, count) in differences {
                if count == 0 {
                    continue;
                }
                let row_sha256 = sha256(canonical_row.as_bytes());
                let matching_allowances = allowances
                    .iter()
                    .filter(|allowance| {
                        allowance.table == table
                            && allowance.action == action
                            && allowance.row_sha256 == row_sha256
                            && allowance.canonical_row == canonical_row
                            && scope.is_none_or(|scope| {
                                allowance
                                    .level
                                    .as_deref()
                                    .is_none_or(|value| value == scope.level)
                                    && allowance
                                        .workload
                                        .as_deref()
                                        .is_none_or(|value| value == scope.workload)
                                    && allowance.language.as_deref().is_none_or(|value| {
                                        scope.language == "all" || value == scope.language
                                    })
                            })
                            && allowance.language.as_deref().is_none_or(|language| {
                                dump_row_language(baseline_header, &canonical_row) == Some(language)
                            })
                    })
                    .collect::<Vec<_>>();
                if matching_allowances.len() > 1 {
                    return Err(format!(
                        "overlapping allowlist entries for {} in table `{table}`: {}",
                        action.name(),
                        canonical_row
                    ));
                }
                let allowance = matching_allowances.first().copied();
                let allowed = allowance.is_some_and(|entry| entry.count == count);
                let (allowlist_id, ledger_marker) = match allowance {
                    Some(entry) if allowed => {
                        used.insert(entry.id.as_str());
                        (entry.id.clone(), entry.ledger_marker.clone())
                    }
                    Some(entry) => {
                        failures.push(format!(
                            "unallowed {} in table `{table}`: allowlist count {} did not match observed count {} for {}",
                            action.name(), entry.count, count, canonical_row
                        ));
                        (String::new(), String::new())
                    }
                    None => {
                        failures.push(format!(
                            "unallowed {} in table `{table}`: {} ({} row(s))",
                            action.name(),
                            canonical_row,
                            count
                        ));
                        (String::new(), String::new())
                    }
                };
                deltas.push(RowDelta {
                    table: table.clone(),
                    action,
                    canonical_row: canonical_row.clone(),
                    row_sha256,
                    count,
                    allowlist_id,
                    ledger_marker,
                    allowed,
                    source_state_sha256: None,
                });
            }
        }
    }
    Ok(DumpComparison {
        row_deltas: deltas,
        used_allowlist_ids: used.into_iter().map(str::to_string).collect(),
        failures,
    })
}

pub fn validate_allowlist_scope_usage(
    allowlist: &[AllowedRowChange],
    used_ids: &[String],
    language: &str,
    level: &str,
    workload: &str,
) -> Result<(), String> {
    let used = used_ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
    for entry in allowlist.iter().filter(|entry| {
        entry
            .language
            .as_deref()
            .is_none_or(|scope| language == "all" || scope == language)
            && entry.level.as_deref().is_none_or(|scope| scope == level)
            && entry
                .workload
                .as_deref()
                .is_none_or(|scope| scope == workload)
    }) {
        if !used.contains(entry.id.as_str()) {
            return Err(format!(
                "unused allowlist entry `{}` in {language}/{level}/{workload} scope for {} {} row {}",
                entry.id,
                entry.action.name(),
                entry.table,
                entry.canonical_row
            ));
        }
    }
    Ok(())
}

fn compare_nonincreasing(label: &str, baseline: &u64, candidate: &u64) -> Result<(), String> {
    if candidate > baseline {
        return Err(format!(
            "{label} count increased from {baseline} to {candidate}"
        ));
    }
    Ok(())
}

fn compare_health_map(
    label: &str,
    baseline: &BTreeMap<String, u64>,
    candidate: &BTreeMap<String, u64>,
) -> Result<(), String> {
    for (key, count) in candidate {
        let old_count = baseline.get(key).copied().unwrap_or(0);
        if *count > old_count {
            return Err(format!(
                "{label} `{key}` count increased from {old_count} to {count}"
            ));
        }
    }
    Ok(())
}

fn compare_health_map_exact(
    label: &str,
    baseline: &BTreeMap<String, u64>,
    candidate: &BTreeMap<String, u64>,
) -> Result<(), String> {
    if baseline != candidate {
        return Err(format!(
            "{label} category/file counts changed from {baseline:?} to {candidate:?}"
        ));
    }
    Ok(())
}

fn validated_allowlist<'a>(
    allowlist: &'a [AllowedRowChange],
    ledger: &str,
) -> Result<Vec<&'a AllowedRowChange>, String> {
    let mut ids = BTreeSet::new();
    let mut validated = Vec::with_capacity(allowlist.len());
    for entry in allowlist {
        if entry.id.trim().is_empty() || !ids.insert(entry.id.as_str()) {
            return Err(format!("duplicate allowlist id `{}`", entry.id));
        }
        if entry.count == 0 || entry.table.trim().is_empty() {
            return Err(format!("invalid allowlist entry `{}`", entry.id));
        }
        if sha256(entry.canonical_row.as_bytes()) != entry.row_sha256 {
            return Err(format!(
                "row hash mismatch for allowlist entry `{}`",
                entry.id
            ));
        }
        if entry.ledger_marker.trim().is_empty()
            || ledger_marker_count(ledger, &entry.ledger_marker) != 1
        {
            return Err(format!(
                "allowlist entry `{}` requires exactly one output ledger marker `{}`",
                entry.id, entry.ledger_marker
            ));
        }
        validated.push(entry);
    }
    for (index, left) in validated.iter().enumerate() {
        for right in &validated[index + 1..] {
            if left.table == right.table
                && left.action == right.action
                && left.row_sha256 == right.row_sha256
                && left.canonical_row == right.canonical_row
                && allowlist_scopes_overlap(left, right)
            {
                return Err(format!(
                    "overlapping allowlist entries `{}` and `{}` for `{}`",
                    left.id, right.id, left.table
                ));
            }
        }
    }
    Ok(validated)
}

fn allowlist_scopes_overlap(left: &AllowedRowChange, right: &AllowedRowChange) -> bool {
    scope_values_overlap(left.language.as_deref(), right.language.as_deref())
        && scope_values_overlap(left.level.as_deref(), right.level.as_deref())
        && scope_values_overlap(left.workload.as_deref(), right.workload.as_deref())
}

fn scope_values_overlap(left: Option<&str>, right: Option<&str>) -> bool {
    left.is_none() || right.is_none() || left == right
}

fn ledger_marker_count(ledger: &str, marker: &str) -> usize {
    ledger
        .lines()
        .filter(|line| {
            line.trim_matches(|character: char| {
                character.is_whitespace() || matches!(character, '#' | '`' | '*')
            }) == marker
        })
        .count()
}

type CanonicalTableRows = BTreeMap<String, usize>;
type CanonicalTableDump = (String, CanonicalTableRows);
type CanonicalDumpTables = BTreeMap<String, CanonicalTableDump>;

fn dump_tables(dump: &ArtifactDump) -> Result<CanonicalDumpTables, String> {
    let mut tables = BTreeMap::new();
    for table in &dump.tables {
        if tables.contains_key(&table.table) {
            return Err(format!("duplicate table dump `{}`", table.table));
        }
        let mut lines = table.text.lines();
        let header = lines
            .next()
            .ok_or_else(|| format!("table dump `{}` has no column header", table.table))?;
        if !header.starts_with("#columns\t") {
            return Err(format!(
                "table dump `{}` has an invalid header",
                table.table
            ));
        }
        let mut rows = BTreeMap::new();
        for line in lines {
            *rows.entry(line.to_string()).or_insert(0) += 1;
        }
        tables.insert(table.table.clone(), (header.to_string(), rows));
    }
    Ok(tables)
}

fn dump_row_language<'a>(header: &str, row: &'a str) -> Option<&'a str> {
    let language_index = header
        .strip_prefix("#columns\t")?
        .split('\t')
        .position(|column| column == "language")?;
    row.split('\t').nth(language_index)?.strip_prefix("text:")
}

fn clamp_u128(value: u128) -> u64 {
    value.min(u64::MAX as u128) as u64
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn artifact_dump_sha256(dump: &ArtifactDump) -> Result<String, String> {
    let tables = dump_tables(dump)?;
    let mut canonical = String::new();
    for (table, (header, rows)) in tables {
        append_hash_field(&mut canonical, &table);
        append_hash_field(&mut canonical, &header);
        for (row, count) in rows {
            append_hash_field(&mut canonical, &row);
            append_hash_field(&mut canonical, &count.to_string());
        }
        append_hash_field(&mut canonical, "table-end");
    }
    Ok(sha256(canonical.as_bytes()))
}

fn append_hash_field(target: &mut String, value: &str) {
    let _ = write!(target, "{}:", value.len());
    target.push_str(value);
}

pub fn validate_baseline_snapshot(
    receipt: &BaselineExpectationReceipt,
    pin: BaselineSnapshotPin<'_>,
    health: &ComparisonHealth,
    dump: &ArtifactDump,
) -> Result<(), String> {
    validate_baseline_receipt(receipt)?;
    if receipt.baseline_binary_sha256 != pin.binary_sha256 {
        return Err("baseline receipt binary SHA-256 does not match the pinned binary".to_string());
    }
    if receipt.manifest_sha256 != pin.manifest_sha256 {
        return Err(
            "baseline receipt manifest SHA-256 does not match the current manifest".to_string(),
        );
    }
    if &receipt.source != pin.source {
        return Err("baseline receipt source identity does not match build metadata".to_string());
    }
    let snapshots = receipt
        .snapshots
        .iter()
        .filter(|snapshot| {
            snapshot.language == pin.language
                && snapshot.level == pin.level
                && snapshot.workload == pin.workload
                && snapshot.source_state_sha256 == pin.source_state_sha256
        })
        .collect::<Vec<_>>();
    let snapshot = match snapshots.as_slice() {
        [snapshot] => *snapshot,
        [] => {
            return Err(format!(
                "no baseline expectation for {}/{}/{}/{}",
                pin.language, pin.level, pin.workload, pin.source_state_sha256
            ));
        }
        _ => return Err("duplicate baseline expectation snapshot".to_string()),
    };
    let actual_dump_hash = artifact_dump_sha256(dump)?;
    if snapshot.semantic_dump_sha256 != actual_dump_hash {
        return Err(format!(
            "baseline semantic dump SHA-256 differs for {}/{}/{}/{}",
            pin.language, pin.level, pin.workload, pin.source_state_sha256
        ));
    }
    let diagnostic_counts = diagnostic_counts_by_file(dump)?;
    if snapshot.diagnostic_counts_by_file != diagnostic_counts {
        return Err(format!(
            "baseline per-file diagnostic counts differ for {}/{}/{}/{}",
            pin.language, pin.level, pin.workload, pin.source_state_sha256
        ));
    }
    if &snapshot.health != health {
        return Err(format!(
            "baseline warning/health counts differ for {}/{}/{}/{}",
            pin.language, pin.level, pin.workload, pin.source_state_sha256
        ));
    }
    Ok(())
}

fn validate_baseline_receipt(receipt: &BaselineExpectationReceipt) -> Result<(), String> {
    if receipt.format_version != 1 {
        return Err(format!(
            "baseline expectation receipt format version {} is unsupported",
            receipt.format_version
        ));
    }
    validate_sha256(&receipt.baseline_binary_sha256)
        .map_err(|_| "baseline receipt binary SHA-256 is invalid".to_string())?;
    validate_sha256(&receipt.manifest_sha256)
        .map_err(|_| "baseline receipt manifest SHA-256 is invalid".to_string())?;
    if receipt.source.commit.len() != 40
        || !receipt
            .source
            .commit
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err("baseline receipt source commit must be a full 40-digit commit ID".to_string());
    }
    if receipt.source.tree_hash_algorithm != SOURCE_TREE_HASH_ALGORITHM {
        return Err(format!(
            "baseline receipt source tree hash algorithm must be `{SOURCE_TREE_HASH_ALGORITHM}`"
        ));
    }
    validate_sha256(&receipt.source.tree_sha256)
        .map_err(|_| "baseline receipt source tree SHA-256 is invalid".to_string())?;
    if receipt.source.dirty {
        let dirty_hash = receipt
            .source
            .dirty_state_sha256
            .as_deref()
            .ok_or_else(|| {
                "dirty baseline receipt source requires a dirty-state SHA-256".to_string()
            })?;
        validate_sha256(dirty_hash)
            .map_err(|_| "baseline receipt dirty-state SHA-256 is invalid".to_string())?;
    } else if receipt.source.dirty_state_sha256.is_some() {
        return Err("clean baseline receipt source cannot carry a dirty-state SHA-256".to_string());
    }
    let mut keys = BTreeSet::new();
    for snapshot in &receipt.snapshots {
        if snapshot.language.trim().is_empty()
            || !["references", "full"].contains(&snapshot.level.as_str())
            || !["fresh_scan", "no_change_scan", "single_file_update"]
                .contains(&snapshot.workload.as_str())
        {
            return Err("baseline receipt snapshot scope is incomplete or invalid".to_string());
        }
        validate_sha256(&snapshot.source_state_sha256)
            .map_err(|_| "baseline receipt source-state SHA-256 is invalid".to_string())?;
        validate_sha256(&snapshot.semantic_dump_sha256)
            .map_err(|_| "baseline receipt semantic dump SHA-256 is invalid".to_string())?;
        let key = (
            snapshot.language.as_str(),
            snapshot.level.as_str(),
            snapshot.workload.as_str(),
            snapshot.source_state_sha256.as_str(),
        );
        if !keys.insert(key) {
            return Err("duplicate baseline receipt snapshot scope".to_string());
        }
        if snapshot
            .diagnostic_counts_by_file
            .keys()
            .any(|path| path.trim().is_empty())
        {
            return Err("baseline receipt contains an empty diagnostic source path".to_string());
        }
    }
    if receipt.snapshots.is_empty() {
        return Err("baseline receipt has no snapshots".to_string());
    }
    Ok(())
}

fn diagnostic_counts_by_file(dump: &ArtifactDump) -> Result<BTreeMap<String, u64>, String> {
    let Some(table) = dump
        .tables
        .iter()
        .find(|table| table.table == "parse_diagnostics")
    else {
        return Ok(BTreeMap::new());
    };
    let mut lines = table.text.lines();
    let header = lines
        .next()
        .ok_or_else(|| "parse_diagnostics table has no header".to_string())?;
    let columns = header
        .strip_prefix("#columns\t")
        .ok_or_else(|| "parse_diagnostics table has an invalid header".to_string())?
        .split('\t')
        .collect::<Vec<_>>();
    let path_index = columns
        .iter()
        .position(|column| *column == "path")
        .ok_or_else(|| "parse_diagnostics table has no path column".to_string())?;
    let mut counts = BTreeMap::new();
    for line in lines {
        let path = line
            .split('\t')
            .nth(path_index)
            .and_then(|value| value.strip_prefix("text:"))
            .ok_or_else(|| "parse_diagnostics row has no text path".to_string())?;
        *counts.entry(path.to_string()).or_insert(0) += 1;
    }
    Ok(counts)
}

fn validate_sha256(value: &str) -> Result<(), ()> {
    if value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AllowlistFile {
    format_version: u32,
    entries: Vec<AllowedRowChange>,
}

struct LoadedAllowlist {
    path: Option<PathBuf>,
    sha256: Option<String>,
    entries: Vec<AllowedRowChange>,
}

#[derive(Debug)]
struct LoadedBuildMetadata {
    path: PathBuf,
    sha256: String,
    record: BuildMetadata,
}

#[derive(Debug)]
struct LoadedBaselineExpectations {
    path: PathBuf,
    sha256: String,
    record: BaselineExpectationReceipt,
}

struct LoadedBudgetApprovals {
    path: Option<PathBuf>,
    sha256: Option<String>,
    entries: Vec<BudgetApproval>,
}

#[derive(Debug, Clone)]
struct ComparisonInput {
    language: String,
    repo_source: PathBuf,
    relative_path: String,
    source_sha256: String,
    expected_facts: Vec<FactAssertion>,
    negative_controls: Vec<FactAssertion>,
    expected_parse_diagnostics: u64,
}

#[derive(Debug, Clone)]
struct StagedInput {
    language: String,
    relative_path: String,
    path: PathBuf,
    original_bytes: Vec<u8>,
    expected_facts: Vec<FactAssertion>,
    negative_controls: Vec<FactAssertion>,
    expected_parse_diagnostics: u64,
}

#[derive(Debug, Clone)]
struct StagedCorpus {
    root: PathBuf,
    artifact: PathBuf,
    validation_artifact: PathBuf,
    inputs: Vec<StagedInput>,
    update_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkloadKind {
    FreshScan,
    NoChangeScan,
    SingleFileUpdate,
}

impl WorkloadKind {
    fn name(self) -> &'static str {
        match self {
            Self::FreshScan => "fresh_scan",
            Self::NoChangeScan => "no_change_scan",
            Self::SingleFileUpdate => "single_file_update",
        }
    }
}

#[derive(Debug)]
struct BatchOutput {
    metrics: SampleMetrics,
    health: ComparisonHealth,
    runtime_failures: Vec<String>,
    operation_states: BTreeMap<String, OperationStateOutput>,
    normalized: NormalizedMetrics,
}

#[derive(Debug, Clone)]
struct OperationStateOutput {
    dump: ArtifactDump,
    artifact_bytes: u64,
    semantic_row_total: u64,
}

#[derive(Debug)]
struct MeasurementAttempt {
    baseline: SampleSeries,
    candidate: SampleSeries,
    baseline_output: BatchOutput,
    candidate_output: BatchOutput,
    baseline_preflight_failures: Vec<String>,
    candidate_preflight_failures: Vec<String>,
}

#[derive(Debug)]
struct AttemptEvaluation {
    gate_failures: Vec<String>,
    approved_budget_ids: Vec<String>,
}

#[derive(Clone, Copy)]
enum RunExpectation<'a> {
    Baseline {
        receipt: &'a BaselineExpectationReceipt,
        binary_sha256: &'a str,
        manifest_sha256: &'a str,
        source: &'a BuildSourceIdentity,
        language: &'a str,
    },
    Candidate {
        language: &'a str,
    },
}

#[derive(Clone, Copy)]
struct WorkloadIdentity<'a> {
    language: &'a str,
    language_scope: &'a str,
    level: &'a str,
    workload: WorkloadKind,
    expected_files: usize,
}

#[derive(Clone, Copy)]
struct WorkloadSide<'a> {
    binary: &'a Path,
    stage: &'a StagedCorpus,
}

#[derive(Clone, Copy)]
struct WorkloadVerification<'a> {
    baseline_receipt: &'a BaselineExpectationReceipt,
    baseline_provenance: &'a ComparisonProvenance,
    candidate_provenance: &'a ComparisonProvenance,
    manifest_sha256: &'a str,
    allowlist: &'a [AllowedRowChange],
    ledger: &'a str,
    budget_approvals: &'a [BudgetApproval],
}

#[derive(Clone, Copy)]
struct WorkloadComparison<'a> {
    identity: WorkloadIdentity<'a>,
    baseline: WorkloadSide<'a>,
    candidate: WorkloadSide<'a>,
    verification: WorkloadVerification<'a>,
}

impl<'a> WorkloadComparison<'a> {
    fn baseline_expectation(self) -> RunExpectation<'a> {
        RunExpectation::Baseline {
            receipt: self.verification.baseline_receipt,
            binary_sha256: &self.verification.baseline_provenance.binary_sha256,
            manifest_sha256: self.verification.manifest_sha256,
            source: &self.verification.baseline_provenance.source,
            language: self.identity.language_scope,
        }
    }

    fn candidate_expectation(self) -> RunExpectation<'a> {
        RunExpectation::Candidate {
            language: self.identity.language_scope,
        }
    }

    fn budget_context(self) -> BudgetApprovalContext<'a> {
        BudgetApprovalContext {
            baseline_binary_sha256: &self.verification.baseline_provenance.binary_sha256,
            candidate_binary_sha256: &self.verification.candidate_provenance.binary_sha256,
            manifest_sha256: self.verification.manifest_sha256,
            language: self.identity.language,
            level: self.identity.level,
            workload: self.identity.workload.name(),
            baseline_profile: &self.verification.baseline_provenance.profile,
            candidate_profile: &self.verification.candidate_provenance.profile,
        }
    }
}

fn run(plan: ComparisonPlan) -> Result<ComparisonSummary, ComparisonError> {
    ensure_peak_rss_supported()?;
    let started_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| ComparisonError::environment(error.to_string()))?
        .as_secs();
    let repo_root = repository_root();
    let baseline_binary = require_binary(&plan.baseline)?;
    let candidate_binary = require_binary(&plan.candidate)?;
    let baseline_binary_sha256 = sha256(&fs::read(&baseline_binary).map_err(|error| {
        ComparisonError::environment(format!("failed to hash baseline binary: {error}"))
    })?);
    let candidate_binary_sha256 = sha256(&fs::read(&candidate_binary).map_err(|error| {
        ComparisonError::environment(format!("failed to hash candidate binary: {error}"))
    })?);
    let baseline_build = load_build_metadata(
        &repo_root,
        &baseline_binary,
        plan.baseline_build_metadata.as_deref(),
        &plan.baseline_build_metadata_sha256,
    )?;
    let candidate_build = load_build_metadata(
        &repo_root,
        &candidate_binary,
        plan.candidate_build_metadata.as_deref(),
        &plan.candidate_build_metadata_sha256,
    )?;
    validate_build_metadata_pair(
        &baseline_build.record,
        &candidate_build.record,
        &baseline_binary_sha256,
        &candidate_binary_sha256,
    )
    .map_err(ComparisonError::environment)?;
    if let Some(profile) = plan.profile.as_deref() {
        if profile.trim().is_empty() {
            return Err(ComparisonError::usage("--profile cannot be empty"));
        }
        if profile != baseline_build.record.build.profile
            || profile != candidate_build.record.build.profile
        {
            return Err(ComparisonError::environment(format!(
                "requested profile `{profile}` differs from pinned build metadata"
            )));
        }
    }
    let manifest_path = resolve_path(&repo_root, &plan.manifest);
    let manifest_path = fs::canonicalize(&manifest_path).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to resolve coverage manifest {}: {error}",
            manifest_path.display()
        ))
    })?;
    let manifest_bytes = fs::read(&manifest_path).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to read coverage manifest {}: {error}",
            manifest_path.display()
        ))
    })?;
    let manifest_sha256 = sha256(&manifest_bytes);
    let manifest: CoverageManifest = serde_json::from_slice(&manifest_bytes).map_err(|error| {
        ComparisonError::environment(format!("invalid coverage manifest: {error}"))
    })?;
    crate::coverage::validate_manifest_shape(&manifest)
        .map_err(|error| ComparisonError::environment(error.to_string()))?;
    let all_inputs = select_inputs(&repo_root, &manifest, plan.language.as_deref())?;
    let baseline_expectations = load_baseline_expectations(
        &repo_root,
        &plan.baseline_expectations,
        &plan.baseline_expectations_sha256,
    )?;
    let budget_approvals = load_budget_approvals(
        &repo_root,
        plan.budget_approvals.as_deref(),
        plan.budget_approvals_sha256.as_deref(),
    )?;
    for approval in &budget_approvals.entries {
        validate_budget_approval_context(
            std::slice::from_ref(approval),
            BudgetApprovalContext {
                baseline_binary_sha256: &baseline_binary_sha256,
                candidate_binary_sha256: &candidate_binary_sha256,
                manifest_sha256: &manifest_sha256,
                language: plan.language.as_deref().unwrap_or("all"),
                level: &approval.level,
                workload: &approval.workload,
                baseline_profile: &baseline_build.record.build.profile,
                candidate_profile: &candidate_build.record.build.profile,
            },
        )
        .map_err(ComparisonError::environment)?;
    }
    validate_baseline_receipt_context(
        &baseline_expectations.record,
        &baseline_binary_sha256,
        &manifest_sha256,
        &baseline_build.record.source,
        plan.language.as_deref().unwrap_or("all"),
        &all_inputs,
    )
    .map_err(ComparisonError::environment)?;
    let LoadedAllowlist {
        path: allowlist_path,
        sha256: allowlist_sha256,
        entries: allowlist,
    } = load_allowlist(&repo_root, plan.allowlist.as_deref())?;
    validate_language_scope(&allowlist, plan.language.as_deref())
        .map_err(ComparisonError::environment)?;
    let ledger_path = repo_root.join(OUTPUT_LEDGER);
    let ledger = fs::read_to_string(&ledger_path).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to read output change ledger {}: {error}",
            ledger_path.display()
        ))
    })?;
    let output_ledger = fs::canonicalize(&ledger_path).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to resolve output change ledger {}: {error}",
            ledger_path.display()
        ))
    })?;
    let output_ledger_sha256 = sha256(ledger.as_bytes());
    validated_allowlist(&allowlist, &ledger).map_err(ComparisonError::environment)?;
    let harness = collect_harness_provenance()?;

    let out_dir = resolve_path(&repo_root, &plan.out_dir);
    fs::create_dir(&out_dir).map_err(|error| {
        ComparisonError::environment(format!(
            "comparison output directory must be new: {}: {error}",
            out_dir.display()
        ))
    })?;
    let summary_path = out_dir.join("comparison.json");
    let baseline_provenance = collect_provenance(
        &baseline_binary,
        &out_dir,
        &manifest_sha256,
        &baseline_build,
    )?;
    let candidate_provenance = collect_provenance(
        &candidate_binary,
        &out_dir,
        &manifest_sha256,
        &candidate_build,
    )?;
    validate_provenance_pair(&baseline_provenance, &candidate_provenance)
        .map_err(ComparisonError::environment)?;

    let mut workload_results = Vec::new();
    let aggregate_language = plan.language.clone().unwrap_or_else(|| "all".to_string());
    for level in ["references", "full"] {
        for workload in [
            WorkloadKind::FreshScan,
            WorkloadKind::NoChangeScan,
            WorkloadKind::SingleFileUpdate,
        ] {
            let label = format!("{}-{level}-{}", aggregate_language, workload.name());
            let baseline_stage = stage_corpus(&out_dir, &label, "baseline", &all_inputs)?;
            let candidate_stage = stage_corpus(&out_dir, &label, "candidate", &all_inputs)?;
            let matching_allowlist = allowlist
                .iter()
                .filter(|entry| scope_matches(entry, &aggregate_language, level, workload.name()))
                .cloned()
                .collect::<Vec<_>>();
            let result = measure_workload(WorkloadComparison {
                identity: WorkloadIdentity {
                    language: &aggregate_language,
                    language_scope: &aggregate_language,
                    level,
                    workload,
                    expected_files: all_inputs.len(),
                },
                baseline: WorkloadSide {
                    binary: &baseline_binary,
                    stage: &baseline_stage,
                },
                candidate: WorkloadSide {
                    binary: &candidate_binary,
                    stage: &candidate_stage,
                },
                verification: WorkloadVerification {
                    baseline_receipt: &baseline_expectations.record,
                    baseline_provenance: &baseline_provenance,
                    candidate_provenance: &candidate_provenance,
                    manifest_sha256: &manifest_sha256,
                    allowlist: &matching_allowlist,
                    ledger: &ledger,
                    budget_approvals: &budget_approvals.entries,
                },
            })?;
            workload_results.push(result);
        }
    }

    let mut summary = ComparisonSummary {
        format_version: SUMMARY_FORMAT_VERSION,
        started_unix_seconds,
        manifest: manifest_path,
        manifest_sha256,
        allowlist: allowlist_path,
        allowlist_sha256,
        baseline_expectations: baseline_expectations.path.clone(),
        baseline_expectations_sha256: baseline_expectations.sha256.clone(),
        budget_approvals: budget_approvals.path,
        budget_approvals_sha256: budget_approvals.sha256,
        output_ledger,
        output_ledger_sha256,
        harness,
        measurement_policy: comparison_policy(),
        summary_path,
        language_filter: plan.language,
        baseline: baseline_provenance,
        candidate: candidate_provenance,
        passed: false,
        workloads: workload_results,
    };
    summary.passed = summary
        .workloads
        .iter()
        .all(|workload| workload.gate_failures.is_empty());
    let serialized = serde_json::to_vec_pretty(&summary)
        .map_err(|error| ComparisonError::environment(error.to_string()))?;
    fs::write(&summary.summary_path, serialized).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to write comparison summary {}: {error}",
            summary.summary_path.display()
        ))
    })?;
    Ok(summary)
}

fn select_inputs(
    repo_root: &Path,
    manifest: &CoverageManifest,
    language_filter: Option<&str>,
) -> Result<Vec<ComparisonInput>, ComparisonError> {
    let registry = registry_languages(repo_root)?;
    let manifest_languages = manifest
        .languages
        .iter()
        .map(|sample| sample.language.as_str())
        .collect::<BTreeSet<_>>();
    if manifest_languages != registry.iter().map(String::as_str).collect() {
        return Err(ComparisonError::environment(
            "coverage manifest language rows do not match the extractor registry",
        ));
    }
    if let Some(language) = language_filter {
        if !registry.contains(language) {
            return Err(ComparisonError::usage(format!(
                "unknown language `{language}`"
            )));
        }
        if !manifest_languages.contains(language) {
            return Err(ComparisonError::environment(format!(
                "coverage manifest has no `{language}` sample"
            )));
        }
    }

    let mut inputs = manifest
        .languages
        .iter()
        .filter(|sample| language_filter.is_none_or(|language| sample.language == language))
        .map(|sample| input_from_sample(repo_root, sample))
        .collect::<Result<Vec<_>, _>>()?;
    inputs.extend(
        manifest
            .controls
            .iter()
            .filter(|control| language_filter.is_none_or(|language| control.language == language))
            .map(|control| input_from_control(repo_root, control))
            .collect::<Result<Vec<_>, _>>()?,
    );
    inputs.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    if inputs.is_empty() {
        return Err(ComparisonError::usage(
            "the selected language has no corpus sources",
        ));
    }
    let mut paths = BTreeSet::new();
    if let Some(duplicate) = inputs
        .iter()
        .map(|input| input.relative_path.as_str())
        .find(|path| !paths.insert(*path))
    {
        return Err(ComparisonError::environment(format!(
            "multiple corpus inputs map to `{duplicate}`"
        )));
    }
    Ok(inputs)
}

fn input_from_sample(
    repo_root: &Path,
    sample: &CoverageSample,
) -> Result<ComparisonInput, ComparisonError> {
    make_input(
        repo_root,
        &sample.language,
        &sample.source_path,
        &sample.source_sha256,
        sample.expected_facts.clone(),
        sample.negative_controls.clone(),
        sample.expected_parse_diagnostics,
    )
}

fn input_from_control(
    repo_root: &Path,
    control: &CoverageControl,
) -> Result<ComparisonInput, ComparisonError> {
    make_input(
        repo_root,
        &control.language,
        &control.source_path,
        &control.source_sha256,
        control.expected_facts.clone(),
        control.negative_controls.clone(),
        control.expected_parse_diagnostics,
    )
}

fn make_input(
    repo_root: &Path,
    language: &str,
    source_path: &str,
    source_sha256: &str,
    expected_facts: Vec<FactAssertion>,
    negative_controls: Vec<FactAssertion>,
    expected_parse_diagnostics: u64,
) -> Result<ComparisonInput, ComparisonError> {
    let repo_source = repo_root.join(source_path);
    crate::coverage::validate_source_hash(&repo_source, source_sha256)
        .map_err(|error| ComparisonError::environment(error.to_string()))?;
    let file_name = Path::new(source_path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            ComparisonError::environment(format!("invalid corpus source path `{source_path}`"))
        })?;
    Ok(ComparisonInput {
        language: language.to_string(),
        repo_source,
        relative_path: format!("{language}/{file_name}"),
        source_sha256: source_sha256.to_string(),
        expected_facts,
        negative_controls,
        expected_parse_diagnostics,
    })
}

fn registry_languages(repo_root: &Path) -> Result<BTreeSet<String>, ComparisonError> {
    let registry_path = repo_root.join("fixtures/extraction/capabilities.json");
    let registry: Value = serde_json::from_slice(&fs::read(&registry_path).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to read language registry {}: {error}",
            registry_path.display()
        ))
    })?)
    .map_err(|error| ComparisonError::environment(error.to_string()))?;
    let languages = registry
        .get("languages")
        .and_then(Value::as_array)
        .ok_or_else(|| ComparisonError::environment("language registry has no languages array"))?;
    languages
        .iter()
        .map(|row| {
            row.get("language")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    ComparisonError::environment("language registry row has no language name")
                })
        })
        .collect()
}

fn load_allowlist(
    repo_root: &Path,
    path: Option<&Path>,
) -> Result<LoadedAllowlist, ComparisonError> {
    let Some(path) = path else {
        return Ok(LoadedAllowlist {
            path: None,
            sha256: None,
            entries: Vec::new(),
        });
    };
    let path = fs::canonicalize(resolve_path(repo_root, path)).map_err(|error| {
        ComparisonError::environment(format!("cannot resolve output row allowlist: {error}"))
    })?;
    let bytes = fs::read(&path).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to read output row allowlist {}: {error}",
            path.display()
        ))
    })?;
    let allowlist: AllowlistFile = serde_json::from_slice(&bytes).map_err(|error| {
        ComparisonError::environment(format!("invalid output row allowlist: {error}"))
    })?;
    if allowlist.format_version != ALLOWLIST_FORMAT_VERSION {
        return Err(ComparisonError::environment(format!(
            "unsupported output row allowlist format version {}",
            allowlist.format_version
        )));
    }
    Ok(LoadedAllowlist {
        path: Some(path),
        sha256: Some(sha256(&bytes)),
        entries: allowlist.entries,
    })
}

fn load_build_metadata(
    repo_root: &Path,
    binary: &Path,
    explicit_path: Option<&Path>,
    expected_sha256: &str,
) -> Result<LoadedBuildMetadata, ComparisonError> {
    let default_path = || {
        let mut path = binary.as_os_str().to_os_string();
        path.push(".build.json");
        PathBuf::from(path)
    };
    let path = explicit_path
        .map(|path| resolve_path(repo_root, path))
        .unwrap_or_else(default_path);
    let path = fs::canonicalize(&path).map_err(|error| {
        ComparisonError::environment(format!(
            "cannot resolve build metadata for {} at {}: {error}",
            binary.display(),
            path.display()
        ))
    })?;
    let bytes = fs::read(&path).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to read build metadata {}: {error}",
            path.display()
        ))
    })?;
    let record: BuildMetadata = serde_json::from_slice(&bytes).map_err(|error| {
        ComparisonError::environment(format!(
            "invalid build metadata {}: {error}",
            path.display()
        ))
    })?;
    validate_sha256(expected_sha256).map_err(|_| {
        ComparisonError::usage("build metadata SHA-256 must contain 64 hexadecimal digits")
    })?;
    let actual_metadata_sha256 = sha256(&bytes);
    if actual_metadata_sha256 != expected_sha256.to_ascii_lowercase() {
        return Err(ComparisonError::environment(format!(
            "build metadata SHA-256 mismatch for {}: expected {}, found {}",
            path.display(),
            expected_sha256,
            actual_metadata_sha256
        )));
    }
    let binary_sha256 = sha256(&fs::read(binary).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to hash binary {}: {error}",
            binary.display()
        ))
    })?);
    validate_build_metadata(&record, &binary_sha256)
        .map_err(|error| ComparisonError::environment(format!("{}: {error}", path.display())))?;
    Ok(LoadedBuildMetadata {
        path,
        sha256: actual_metadata_sha256,
        record,
    })
}

fn load_baseline_expectations(
    repo_root: &Path,
    path: &Path,
    expected_sha256: &str,
) -> Result<LoadedBaselineExpectations, ComparisonError> {
    let path = fs::canonicalize(resolve_path(repo_root, path)).map_err(|error| {
        ComparisonError::environment(format!(
            "cannot resolve baseline expectations {}: {error}",
            path.display()
        ))
    })?;
    let bytes = fs::read(&path).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to read baseline expectations {}: {error}",
            path.display()
        ))
    })?;
    let record: BaselineExpectationReceipt = serde_json::from_slice(&bytes).map_err(|error| {
        ComparisonError::environment(format!(
            "invalid baseline expectations {}: {error}",
            path.display()
        ))
    })?;
    validate_sha256(expected_sha256).map_err(|_| {
        ComparisonError::usage("baseline expectations SHA-256 must contain 64 hexadecimal digits")
    })?;
    let actual_sha256 = sha256(&bytes);
    if actual_sha256 != expected_sha256.to_ascii_lowercase() {
        return Err(ComparisonError::environment(format!(
            "baseline expectations SHA-256 mismatch: expected {}, found {}",
            expected_sha256, actual_sha256
        )));
    }
    validate_baseline_receipt(&record).map_err(ComparisonError::environment)?;
    Ok(LoadedBaselineExpectations {
        path,
        sha256: actual_sha256,
        record,
    })
}

fn load_budget_approvals(
    repo_root: &Path,
    path: Option<&Path>,
    expected_sha256: Option<&str>,
) -> Result<LoadedBudgetApprovals, ComparisonError> {
    let (path, expected_sha256) = match (path, expected_sha256) {
        (None, None) => {
            return Ok(LoadedBudgetApprovals {
                path: None,
                sha256: None,
                entries: Vec::new(),
            });
        }
        (Some(path), Some(expected_sha256)) => (path, expected_sha256),
        _ => {
            return Err(ComparisonError::usage(
                "--budget-approvals and --budget-approvals-sha256 must be supplied together",
            ));
        }
    };
    validate_sha256(expected_sha256).map_err(|_| {
        ComparisonError::usage("budget approvals SHA-256 must contain 64 hexadecimal digits")
    })?;
    let path = fs::canonicalize(resolve_path(repo_root, path)).map_err(|error| {
        ComparisonError::environment(format!(
            "cannot resolve budget approvals {}: {error}",
            path.display()
        ))
    })?;
    let bytes = fs::read(&path).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to read budget approvals {}: {error}",
            path.display()
        ))
    })?;
    let actual_sha256 = sha256(&bytes);
    if actual_sha256 != expected_sha256.to_ascii_lowercase() {
        return Err(ComparisonError::environment(format!(
            "budget approvals SHA-256 mismatch: expected {}, found {}",
            expected_sha256, actual_sha256
        )));
    }
    let file: BudgetApprovalFile = serde_json::from_slice(&bytes).map_err(|error| {
        ComparisonError::environment(format!("invalid budget approvals: {error}"))
    })?;
    validate_budget_approval_file(&file).map_err(ComparisonError::environment)?;
    Ok(LoadedBudgetApprovals {
        path: Some(path),
        sha256: Some(actual_sha256),
        entries: file.approvals,
    })
}

fn validate_baseline_receipt_context(
    receipt: &BaselineExpectationReceipt,
    baseline_binary_sha256: &str,
    manifest_sha256: &str,
    source: &BuildSourceIdentity,
    language: &str,
    inputs: &[ComparisonInput],
) -> Result<(), String> {
    validate_baseline_receipt(receipt)?;
    if receipt.baseline_binary_sha256 != baseline_binary_sha256 {
        return Err("baseline expectation receipt is pinned to another binary".to_string());
    }
    if receipt.manifest_sha256 != manifest_sha256 {
        return Err("baseline expectation receipt is pinned to another manifest".to_string());
    }
    if &receipt.source != source {
        return Err("baseline expectation source identity differs from build metadata".to_string());
    }
    if inputs.is_empty() {
        return Err(
            "baseline expectation receipt cannot be checked against an empty corpus".to_string(),
        );
    }
    let update_index = inputs.len() - 1;
    let original_state = comparison_input_state_sha256(inputs, None)?;
    let updated_state = comparison_input_state_sha256(inputs, Some(update_index))?;
    let mut required = BTreeSet::new();
    for level in ["references", "full"] {
        required.insert((language, level, "fresh_scan", original_state.as_str()));
        required.insert((language, level, "fresh_scan", updated_state.as_str()));
        required.insert((language, level, "no_change_scan", original_state.as_str()));
        required.insert((
            language,
            level,
            "single_file_update",
            original_state.as_str(),
        ));
        required.insert((
            language,
            level,
            "single_file_update",
            updated_state.as_str(),
        ));
    }
    let pinned = receipt
        .snapshots
        .iter()
        .map(|snapshot| {
            (
                snapshot.language.as_str(),
                snapshot.level.as_str(),
                snapshot.workload.as_str(),
                snapshot.source_state_sha256.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    if let Some(missing) = required.difference(&pinned).next() {
        return Err(format!(
            "baseline expectation receipt is missing the {} / {} / {} / {} source state",
            missing.0, missing.1, missing.2, missing.3
        ));
    }
    Ok(())
}

fn comparison_input_state_sha256(
    inputs: &[ComparisonInput],
    append_newline_at: Option<usize>,
) -> Result<String, String> {
    let mut bytes = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        crate::coverage::validate_source_hash(&input.repo_source, &input.source_sha256)
            .map_err(|error| error.to_string())?;
        let mut source = fs::read(&input.repo_source).map_err(|error| error.to_string())?;
        if append_newline_at == Some(index) {
            source.push(b'\n');
        }
        append_bytes_field(&mut bytes, input.language.as_bytes());
        append_bytes_field(&mut bytes, input.relative_path.as_bytes());
        append_bytes_field(&mut bytes, &source);
    }
    Ok(sha256(&bytes))
}

fn staged_source_state_sha256(stage: &StagedCorpus) -> Result<String, ComparisonError> {
    let mut bytes = Vec::new();
    for input in &stage.inputs {
        let source = fs::read(&input.path).map_err(|error| {
            ComparisonError::environment(format!(
                "failed to hash staged source {}: {error}",
                input.path.display()
            ))
        })?;
        append_bytes_field(&mut bytes, input.language.as_bytes());
        append_bytes_field(&mut bytes, input.relative_path.as_bytes());
        append_bytes_field(&mut bytes, &source);
    }
    Ok(sha256(&bytes))
}

fn append_bytes_field(target: &mut Vec<u8>, value: &[u8]) {
    target.extend_from_slice(&(value.len() as u64).to_be_bytes());
    target.extend_from_slice(value);
}

fn scope_matches(entry: &AllowedRowChange, language: &str, level: &str, workload: &str) -> bool {
    entry
        .language
        .as_deref()
        .is_none_or(|scope| language == "all" || scope == language)
        && entry.level.as_deref().is_none_or(|scope| scope == level)
        && entry
            .workload
            .as_deref()
            .is_none_or(|scope| scope == workload)
}

fn validate_language_scope(
    entries: &[AllowedRowChange],
    language_filter: Option<&str>,
) -> Result<(), String> {
    if language_filter.is_some() && entries.iter().any(|entry| entry.language.is_none()) {
        return Err(
            "per-language comparison requires a language scope on every allowlist entry; run all languages to verify unscoped entries".to_string(),
        );
    }
    Ok(())
}

fn stage_corpus(
    out_dir: &Path,
    label: &str,
    engine: &str,
    inputs: &[ComparisonInput],
) -> Result<StagedCorpus, ComparisonError> {
    let stage_dir = out_dir.join("runs").join(label).join(engine);
    let root = stage_dir.join("source");
    let artifact_dir = stage_dir.join("artifact");
    fs::create_dir_all(&root).map_err(|error| {
        ComparisonError::environment(format!("failed to create source root: {error}"))
    })?;
    fs::create_dir_all(&artifact_dir).map_err(|error| {
        ComparisonError::environment(format!("failed to create artifact directory: {error}"))
    })?;
    let artifact = artifact_dir.join("artifact.sqlite");
    let validation_artifact = artifact_dir.join("artifact-validation.sqlite");
    let mut staged = Vec::with_capacity(inputs.len());
    for input in inputs {
        crate::coverage::validate_source_hash(&input.repo_source, &input.source_sha256)
            .map_err(|error| ComparisonError::environment(error.to_string()))?;
        let original_bytes = fs::read(&input.repo_source).map_err(|error| {
            ComparisonError::environment(format!(
                "failed to read pinned source {}: {error}",
                input.repo_source.display()
            ))
        })?;
        let path = root.join(&input.language).join(
            Path::new(&input.relative_path)
                .file_name()
                .expect("relative path includes the source filename"),
        );
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                ComparisonError::environment(format!("failed to create source parent: {error}"))
            })?;
        }
        fs::write(&path, &original_bytes).map_err(|error| {
            ComparisonError::environment(format!(
                "failed to stage source {}: {error}",
                path.display()
            ))
        })?;
        staged.push(StagedInput {
            language: input.language.clone(),
            relative_path: input.relative_path.clone(),
            path,
            original_bytes,
            expected_facts: input.expected_facts.clone(),
            negative_controls: input.negative_controls.clone(),
            expected_parse_diagnostics: input.expected_parse_diagnostics,
        });
    }
    let update_index = staged.len() - 1;
    Ok(StagedCorpus {
        root,
        artifact,
        validation_artifact,
        inputs: staged,
        update_index,
    })
}

fn collect_provenance(
    binary: &Path,
    artifact_directory: &Path,
    manifest_sha256: &str,
    build: &LoadedBuildMetadata,
) -> Result<ComparisonProvenance, ComparisonError> {
    let binary_sha256 = sha256(&fs::read(binary).map_err(|error| {
        ComparisonError::environment(format!(
            "failed to hash binary {}: {error}",
            binary.display()
        ))
    })?);
    let binary_version = command_output(binary, &["--version"])?;
    let memory_bytes = host_memory_bytes()?;
    let storage = host_storage(artifact_directory)?;
    let kernel = command_output(Path::new("uname"), &["-srmo"])?;
    let rss_tool_version = command_output(Path::new("/usr/bin/time"), &["--version"])?
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    Ok(ComparisonProvenance {
        manifest_sha256: manifest_sha256.to_string(),
        binary_path: binary.to_path_buf(),
        binary_sha256,
        binary_version,
        build_metadata_path: build.path.clone(),
        build_metadata_sha256: build.sha256.clone(),
        build_metadata_format_version: build.record.format_version,
        source: build.record.source.clone(),
        toolchain: format!("{}; {}", build.record.build.rustc, build.record.build.cargo),
        profile: build.record.build.profile.clone(),
        build: build.record.build.clone(),
        level: "references+full".to_string(),
        jobs: 1,
        os: format!(
            "{}-{}; {kernel}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        cpu: host_cpu()?,
        memory_bytes,
        storage,
        cache_policy: "operating-system cache is not dropped; each binary gets one unmeasured warm-up and one unmeasured fixed-operation calibration batch before paired samples; fresh-equivalence scans run outside timing on both sides"
            .to_string(),
        peak_rss_method: format!(
            "{rss_tool_version}; /usr/bin/time -f %M; max resident set size in KiB per scan process"
        ),
    })
}

fn collect_harness_provenance() -> Result<ComparisonHarnessProvenance, ComparisonError> {
    let binary_path = std::env::current_exe().map_err(|error| {
        ComparisonError::environment(format!("cannot resolve comparison harness binary: {error}"))
    })?;
    let binary_path = fs::canonicalize(binary_path).map_err(|error| {
        ComparisonError::environment(format!(
            "cannot canonicalize comparison harness binary: {error}"
        ))
    })?;
    let binary_sha256 = sha256(&fs::read(&binary_path).map_err(|error| {
        ComparisonError::environment(format!(
            "cannot hash comparison harness binary {}: {error}",
            binary_path.display()
        ))
    })?);
    Ok(ComparisonHarnessProvenance {
        binary_path,
        binary_sha256,
        version: format!("xtask {}", env!("CARGO_PKG_VERSION")),
        toolchain: format!(
            "{}; {}",
            command_output(Path::new("rustc"), &["--version"])?,
            command_output(Path::new("cargo"), &["--version"])?
        ),
    })
}

fn comparison_policy() -> ComparisonPolicy {
    ComparisonPolicy {
        warmup_runs_per_binary: WARMUP_RUNS_PER_BINARY,
        paired_samples_per_workload: SAMPLE_COUNT,
        minimum_baseline_batch_ms: MIN_BATCH_DURATION_MS,
        maximum_calibration_operations: MAX_BATCH_OPERATIONS,
        maximum_sample_spread_percent: MAX_SAMPLE_SPREAD_PERCENT,
        minimum_sample_spread_ms: MIN_SAMPLE_SPREAD_MS,
        budget_growth_thresholds_both_required: true,
        elapsed_growth_percent: ELAPSED_GROWTH_PERCENT,
        elapsed_growth_ms: ELAPSED_GROWTH_MS,
        peak_rss_growth_percent: PEAK_RSS_GROWTH_PERCENT,
        peak_rss_growth_bytes: PEAK_RSS_GROWTH_BYTES,
        artifact_growth_percent: ARTIFACT_GROWTH_PERCENT,
        artifact_growth_bytes: ARTIFACT_GROWTH_BYTES,
        transient_tables: TRANSIENT_TABLES
            .iter()
            .map(|table| (*table).to_string())
            .collect(),
        excluded_volatile_columns: BTreeMap::from([(
            "files".to_string(),
            vec!["indexed_at".to_string(), "last_revision_id".to_string()],
        )]),
        timed_interval: "sum of wall-clock durations around each /usr/bin/time-wrapped scan process; process startup and collector overhead included; source staging, fresh-equivalence scans, semantic checks, and artifact preparation are outside the timed intervals".to_string(),
        workload_preparation: "fresh_scan clears the database before each timed scan; no_change_scan prepares the database before timing; single_file_update prepares the original state and changes one staged source before each timed scan. Both binaries receive the same warm-up and validation-scan cache treatment; operating-system page cache is not dropped".to_string(),
    }
}

fn command_output(program: &Path, args: &[&str]) -> Result<String, ComparisonError> {
    let output = Command::new(program).args(args).output().map_err(|error| {
        ComparisonError::environment(format!("failed to run {}: {error}", program.display()))
    })?;
    if !output.status.success() {
        return Err(ComparisonError::environment(format!(
            "{} {:?} exited with {}: {}",
            program.display(),
            args,
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|error| ComparisonError::environment(error.to_string()))
}

fn require_binary(path: &Path) -> Result<PathBuf, ComparisonError> {
    let binary = fs::canonicalize(path).map_err(|error| {
        ComparisonError::usage(format!("cannot resolve binary {}: {error}", path.display()))
    })?;
    if !binary.is_file() {
        return Err(ComparisonError::usage(format!(
            "extractor binary is not a file: {}",
            binary.display()
        )));
    }
    Ok(binary)
}

fn ensure_peak_rss_supported() -> Result<(), ComparisonError> {
    if cfg!(target_os = "linux") && Path::new("/usr/bin/time").is_file() {
        Ok(())
    } else {
        Err(ComparisonError::environment(
            "native peak RSS collection is unsupported on this host; comparison is incomplete",
        ))
    }
}

fn host_cpu() -> Result<String, ComparisonError> {
    let cpu_info = fs::read_to_string("/proc/cpuinfo").map_err(|error| {
        ComparisonError::environment(format!("failed to read CPU model: {error}"))
    })?;
    cpu_info
        .lines()
        .find_map(|line| line.strip_prefix("model name\t: "))
        .map(str::to_string)
        .ok_or_else(|| ComparisonError::environment("CPU model is unavailable"))
}

fn host_memory_bytes() -> Result<u64, ComparisonError> {
    let memory_info = fs::read_to_string("/proc/meminfo").map_err(|error| {
        ComparisonError::environment(format!("failed to read host memory: {error}"))
    })?;
    let kilobytes = memory_info
        .lines()
        .find_map(|line| {
            line.strip_prefix("MemTotal:")
                .and_then(|value| value.split_whitespace().next())
                .and_then(|value| value.parse::<u64>().ok())
        })
        .ok_or_else(|| ComparisonError::environment("total host memory is unavailable"))?;
    Ok(kilobytes.saturating_mul(1024))
}

fn host_storage(path: &Path) -> Result<String, ComparisonError> {
    let output = Command::new("df")
        .args(["-P", "-T", "--"])
        .arg(path)
        .output()
        .map_err(|error| {
            ComparisonError::environment(format!("failed to inspect storage: {error}"))
        })?;
    if !output.status.success() {
        return Err(ComparisonError::environment(format!(
            "df -P -T failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .nth(1)
        .ok_or_else(|| ComparisonError::environment("storage information is unavailable"))?;
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 7 {
        return Err(ComparisonError::environment(
            "storage information is incomplete",
        ));
    }
    Ok(format!(
        "device={} filesystem={} mount={}",
        fields[0],
        fields[1],
        fields[6..].join(" ")
    ))
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under repo root")
        .to_path_buf()
}

fn resolve_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn measure_workload(comparison: WorkloadComparison<'_>) -> Result<WorkloadResult, ComparisonError> {
    let language = comparison.identity.language;
    let level = comparison.identity.level;
    let workload = comparison.identity.workload;
    let allowlist = comparison.verification.allowlist;
    let ledger = comparison.verification.ledger;
    let first_attempt = measure_attempt(&comparison)?;
    let first_evaluation = evaluate_attempt(&first_attempt, &comparison);
    let repeatable = should_retry_measurement(&first_evaluation.gate_failures);

    let (final_attempt, gate_failures, previous_attempt, repeat_attempted, approved_budget_ids) =
        if repeatable {
            let repeated = measure_attempt(&comparison)?;
            let repeated_evaluation = evaluate_attempt(&repeated, &comparison);
            let prior = PreviousAttempt {
                baseline: first_attempt.baseline.clone(),
                candidate: first_attempt.candidate.clone(),
                gate_failures: first_evaluation.gate_failures,
            };
            (
                repeated,
                repeated_evaluation.gate_failures,
                Some(Box::new(prior)),
                true,
                repeated_evaluation.approved_budget_ids,
            )
        } else {
            (
                first_attempt,
                first_evaluation.gate_failures,
                None,
                false,
                first_evaluation.approved_budget_ids,
            )
        };

    let mut failures = gate_failures;
    let (row_deltas, dump_failures) = compare_operation_states(
        &final_attempt.baseline_output,
        &final_attempt.candidate_output,
        allowlist,
        ledger,
        language,
        level,
        workload.name(),
    );
    failures.extend(dump_failures);
    failures.extend(final_attempt.baseline_output.runtime_failures.clone());
    failures.extend(final_attempt.candidate_output.runtime_failures.clone());
    failures.extend(final_attempt.baseline_preflight_failures.clone());
    failures.extend(final_attempt.candidate_preflight_failures.clone());
    compare_health(
        &final_attempt.baseline_output.health,
        &final_attempt.candidate_output.health,
    )
    .unwrap_or_else(|error| failures.push(format!("correctness: health {error}")));
    let repeat_passed = repeat_attempted.then_some(failures.is_empty());
    let operation_state_sha256 = final_attempt
        .baseline_output
        .operation_states
        .keys()
        .chain(final_attempt.candidate_output.operation_states.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    Ok(WorkloadResult {
        language: language.to_string(),
        level: level.to_string(),
        workload: workload.name().to_string(),
        baseline: final_attempt.baseline,
        candidate: final_attempt.candidate,
        baseline_health: final_attempt.baseline_output.health,
        candidate_health: final_attempt.candidate_output.health,
        row_deltas,
        gate_failures: failures,
        repeat_passed,
        previous_attempt,
        operation_state_sha256,
        approved_budget_ids,
    })
}

fn measure_attempt(
    comparison: &WorkloadComparison<'_>,
) -> Result<MeasurementAttempt, ComparisonError> {
    let level = comparison.identity.level;
    let workload = comparison.identity.workload;
    let baseline_binary = comparison.baseline.binary;
    let candidate_binary = comparison.candidate.binary;
    let baseline_stage = comparison.baseline.stage;
    let candidate_stage = comparison.candidate.stage;
    let baseline_expectation = comparison.baseline_expectation();
    let candidate_expectation = comparison.candidate_expectation();
    let mut baseline_preflight_failures = Vec::new();
    let mut candidate_preflight_failures = Vec::new();
    for _ in 0..WARMUP_RUNS_PER_BINARY {
        baseline_preflight_failures.extend(
            run_batch(
                baseline_binary,
                baseline_stage,
                level,
                workload,
                1,
                baseline_expectation,
            )?
            .runtime_failures,
        );
        candidate_preflight_failures.extend(
            run_batch(
                candidate_binary,
                candidate_stage,
                level,
                workload,
                1,
                candidate_expectation,
            )?
            .runtime_failures,
        );
    }

    let mut calibration_operations = 1usize;
    let calibration = loop {
        let output = run_batch(
            baseline_binary,
            baseline_stage,
            level,
            workload,
            calibration_operations,
            baseline_expectation,
        )?;
        baseline_preflight_failures.extend(output.runtime_failures.iter().cloned());
        if output.metrics.elapsed_ms >= MIN_BATCH_DURATION_MS {
            break output;
        }
        calibration_operations = calibration_operations.saturating_mul(2);
        if calibration_operations > MAX_BATCH_OPERATIONS {
            return Err(ComparisonError::environment(format!(
                "{workload:?} baseline batch did not reach two seconds by {MAX_BATCH_OPERATIONS} operations"
            )));
        }
    };
    candidate_preflight_failures.extend(
        run_batch(
            candidate_binary,
            candidate_stage,
            level,
            workload,
            calibration_operations,
            candidate_expectation,
        )?
        .runtime_failures,
    );

    let mut baseline_samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut candidate_samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut baseline_normalized = Vec::with_capacity(SAMPLE_COUNT);
    let mut candidate_normalized = Vec::with_capacity(SAMPLE_COUNT);
    let mut baseline_outputs = Vec::with_capacity(SAMPLE_COUNT);
    let mut candidate_outputs = Vec::with_capacity(SAMPLE_COUNT);
    for pair in 0..SAMPLE_COUNT {
        if pair % 2 == 0 {
            let baseline = run_batch(
                baseline_binary,
                baseline_stage,
                level,
                workload,
                calibration_operations,
                baseline_expectation,
            )?;
            let candidate = run_batch(
                candidate_binary,
                candidate_stage,
                level,
                workload,
                calibration_operations,
                candidate_expectation,
            )?;
            baseline_samples.push(baseline.metrics);
            candidate_samples.push(candidate.metrics);
            baseline_normalized.push(baseline.normalized.clone());
            candidate_normalized.push(candidate.normalized.clone());
            baseline_outputs.push(baseline);
            candidate_outputs.push(candidate);
        } else {
            let candidate = run_batch(
                candidate_binary,
                candidate_stage,
                level,
                workload,
                calibration_operations,
                candidate_expectation,
            )?;
            let baseline = run_batch(
                baseline_binary,
                baseline_stage,
                level,
                workload,
                calibration_operations,
                baseline_expectation,
            )?;
            baseline_samples.push(baseline.metrics);
            candidate_samples.push(candidate.metrics);
            baseline_normalized.push(baseline.normalized.clone());
            candidate_normalized.push(candidate.normalized.clone());
            baseline_outputs.push(baseline);
            candidate_outputs.push(candidate);
        }
    }
    let baseline_series = sample_series(
        calibration_operations,
        calibration.metrics.elapsed_ms,
        &baseline_samples,
        &baseline_normalized,
    );
    let candidate_series = sample_series(
        calibration_operations,
        calibration.metrics.elapsed_ms,
        &candidate_samples,
        &candidate_normalized,
    );
    Ok(MeasurementAttempt {
        baseline: baseline_series,
        candidate: candidate_series,
        baseline_output: combine_batch_outputs(baseline_outputs)?,
        candidate_output: combine_batch_outputs(candidate_outputs)?,
        baseline_preflight_failures,
        candidate_preflight_failures,
    })
}

fn sample_series(
    calibration_operations: usize,
    baseline_calibration_elapsed_ms: u128,
    samples: &[SampleMetrics],
    normalized: &[NormalizedMetrics],
) -> SampleSeries {
    let times = samples
        .iter()
        .map(|sample| sample.elapsed_ms)
        .collect::<Vec<_>>();
    let memories = samples
        .iter()
        .map(|sample| u128::from(sample.peak_rss_bytes))
        .collect::<Vec<_>>();
    let sizes = samples
        .iter()
        .map(|sample| u128::from(sample.artifact_bytes))
        .collect::<Vec<_>>();
    SampleSeries {
        calibration_operations,
        baseline_calibration_elapsed_ms,
        samples: samples
            .iter()
            .enumerate()
            .zip(normalized)
            .map(|((index, sample), normalized)| OperationSample {
                order: if index % 2 == 0 {
                    "baseline_then_candidate".to_string()
                } else {
                    "candidate_then_baseline".to_string()
                },
                elapsed_ms: sample.elapsed_ms,
                peak_rss_bytes: sample.peak_rss_bytes,
                artifact_bytes: sample.artifact_bytes,
                normalized: normalized.clone(),
            })
            .collect(),
        elapsed_ms: metric_range(&times),
        peak_rss_bytes: metric_range(&memories),
        artifact_bytes: metric_range(&sizes),
    }
}

fn should_retry_measurement(failures: &[String]) -> bool {
    !failures.is_empty()
        && failures
            .iter()
            .all(|failure| failure.starts_with("timing:") || failure.starts_with("budget:"))
}

fn metric_range(values: &[u128]) -> MetricRange {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    MetricRange {
        min: sorted[0],
        median: sorted[sorted.len() / 2],
        max: sorted[sorted.len() - 1],
    }
}

fn combine_batch_outputs(mut outputs: Vec<BatchOutput>) -> Result<BatchOutput, ComparisonError> {
    let mut final_output = outputs.pop().ok_or_else(|| {
        ComparisonError::environment("comparison produced no measurement samples")
    })?;
    for output in outputs {
        final_output.health.add(&output.health);
        final_output
            .runtime_failures
            .extend(output.runtime_failures);
        for (state, sample) in output.operation_states {
            match final_output.operation_states.entry(state.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(sample);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    if entry.get().dump != sample.dump {
                        final_output.runtime_failures.push(format!(
                            "semantic output changed across paired samples for source state {state}"
                        ));
                    }
                    let current = entry.get_mut();
                    current.artifact_bytes = current.artifact_bytes.max(sample.artifact_bytes);
                }
            }
        }
    }
    Ok(final_output)
}

fn evaluate_attempt(
    attempt: &MeasurementAttempt,
    comparison: &WorkloadComparison<'_>,
) -> AttemptEvaluation {
    let identity = comparison.identity;
    let verification = comparison.verification;
    let language = identity.language;
    let level = identity.level;
    let workload = identity.workload;
    let expected_files = identity.expected_files;
    let allowlist = verification.allowlist;
    let ledger = verification.ledger;
    let budget_approvals = verification.budget_approvals;
    let budget_context = comparison.budget_context();
    let mut failures = Vec::new();
    let baseline_times = attempt
        .baseline
        .samples
        .iter()
        .map(|sample| sample.elapsed_ms)
        .collect::<Vec<_>>();
    let candidate_times = attempt
        .candidate
        .samples
        .iter()
        .map(|sample| sample.elapsed_ms)
        .collect::<Vec<_>>();
    if !sample_spread_is_conclusive(&baseline_times) {
        failures.push("timing: baseline sample spread is inconclusive".to_string());
    }
    if !sample_spread_is_conclusive(&candidate_times) {
        failures.push("timing: candidate sample spread is inconclusive".to_string());
    }
    if attempt
        .baseline
        .samples
        .iter()
        .any(|sample| sample.elapsed_ms < MIN_BATCH_DURATION_MS)
    {
        failures.push("timing: a baseline batch sample was shorter than two seconds".to_string());
    }
    let baseline_metrics = SampleMetrics {
        elapsed_ms: attempt.baseline.elapsed_ms.median,
        peak_rss_bytes: clamp_u128(attempt.baseline.peak_rss_bytes.median),
        artifact_bytes: clamp_u128(attempt.baseline.artifact_bytes.median),
    };
    let candidate_metrics = SampleMetrics {
        elapsed_ms: attempt.candidate.elapsed_ms.median,
        peak_rss_bytes: clamp_u128(attempt.candidate.peak_rss_bytes.median),
        artifact_bytes: clamp_u128(attempt.candidate.artifact_bytes.median),
    };
    for (side, output) in [
        ("baseline", &attempt.baseline_output),
        ("candidate", &attempt.candidate_output),
    ] {
        for failure in &output.runtime_failures {
            failures.push(format!("correctness: {side} {failure}"));
        }
        if workload == WorkloadKind::FreshScan
            && output.health.files_scanned
                != (expected_files as u64).saturating_mul(
                    SAMPLE_COUNT as u64 * attempt.baseline.calibration_operations as u64,
                )
        {
            failures.push(format!(
                "correctness: {side} {} reported {} scanned files for {expected_files} expected source(s)",
                workload.name(),
                output.health.files_scanned
            ));
        }
    }
    if let Err(error) = compare_health(
        &attempt.baseline_output.health,
        &attempt.candidate_output.health,
    ) {
        failures.push(format!("correctness: health {error}"));
    }
    let (row_deltas, state_failures) = compare_operation_states(
        &attempt.baseline_output,
        &attempt.candidate_output,
        allowlist,
        ledger,
        language,
        level,
        workload.name(),
    );
    failures.extend(state_failures);
    let budget_evaluation = evaluate_budgets(
        &baseline_metrics,
        &candidate_metrics,
        &row_deltas,
        budget_approvals,
        budget_context,
    );
    for violation in budget_evaluation.violations {
        failures.push(format!(
            "budget: {} grew from {} to {}",
            violation.metric, violation.baseline, violation.candidate
        ));
    }
    failures.extend(budget_evaluation.failures);
    failures.extend(
        attempt
            .baseline_preflight_failures
            .iter()
            .map(|failure| format!("correctness: baseline preflight {failure}")),
    );
    failures.extend(
        attempt
            .candidate_preflight_failures
            .iter()
            .map(|failure| format!("correctness: candidate preflight {failure}")),
    );
    AttemptEvaluation {
        gate_failures: failures,
        approved_budget_ids: budget_evaluation.used_approval_ids,
    }
}

fn compare_operation_states(
    baseline: &BatchOutput,
    candidate: &BatchOutput,
    allowlist: &[AllowedRowChange],
    ledger: &str,
    language: &str,
    level: &str,
    workload: &str,
) -> (Vec<RowDelta>, Vec<String>) {
    let scope = AllowlistScope {
        language,
        level,
        workload,
    };
    let states = baseline
        .operation_states
        .keys()
        .chain(candidate.operation_states.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut deltas = Vec::new();
    let mut failures = Vec::new();
    let mut used = BTreeSet::new();
    for state in states {
        let (Some(old), Some(new)) = (
            baseline.operation_states.get(&state),
            candidate.operation_states.get(&state),
        ) else {
            failures.push(format!(
                "correctness: {language}/{level}/{workload} source state {state} is missing from one binary"
            ));
            continue;
        };
        match inspect_artifact_dumps_scoped(&old.dump, &new.dump, allowlist, ledger, Some(scope)) {
            Ok(comparison) => {
                used.extend(comparison.used_allowlist_ids);
                failures.extend(comparison.failures);
                deltas.extend(comparison.row_deltas.into_iter().map(|mut delta| {
                    delta.source_state_sha256 = Some(state.clone());
                    delta
                }));
            }
            Err(error) => failures.push(format!("correctness: artifact comparison {error}")),
        }
    }
    let used = used.into_iter().collect::<Vec<_>>();
    validate_allowlist_scope_usage(allowlist, &used, language, level, workload)
        .unwrap_or_else(|error| failures.push(format!("correctness: {error}")));
    (deltas, failures)
}

impl ComparisonHealth {
    fn add(&mut self, other: &Self) {
        self.files_scanned = self.files_scanned.saturating_add(other.files_scanned);
        self.files_failed = self.files_failed.saturating_add(other.files_failed);
        self.files_unsupported = self
            .files_unsupported
            .saturating_add(other.files_unsupported);
        self.errors = self.errors.saturating_add(other.errors);
        add_counts(&mut self.skipped, &other.skipped);
        add_counts(&mut self.warnings, &other.warnings);
        add_counts(&mut self.parse_diagnostics, &other.parse_diagnostics);
    }
}

fn add_counts(target: &mut BTreeMap<String, u64>, source: &BTreeMap<String, u64>) {
    for (key, count) in source {
        *target.entry(key.clone()).or_default() = target
            .get(key)
            .copied()
            .unwrap_or_default()
            .saturating_add(*count);
    }
}

fn run_batch(
    binary: &Path,
    stage: &StagedCorpus,
    level: &str,
    workload: WorkloadKind,
    operations: usize,
    expectation: RunExpectation<'_>,
) -> Result<BatchOutput, ComparisonError> {
    if operations == 0 {
        return Err(ComparisonError::environment(
            "comparison batch requires at least one operation",
        ));
    }
    restore_sources(stage)?;
    clear_artifact(&stage.artifact)?;
    clear_artifact(&stage.validation_artifact)?;
    let validation_stage = StagedCorpus {
        root: stage.root.clone(),
        artifact: stage.validation_artifact.clone(),
        validation_artifact: stage.validation_artifact.clone(),
        inputs: stage.inputs.clone(),
        update_index: stage.update_index,
    };
    let mut fresh_states = BTreeMap::new();
    let mut operation_states = BTreeMap::new();
    let mut runtime_failures = Vec::new();
    let mut max_artifact_bytes = 0u64;
    if workload != WorkloadKind::FreshScan {
        let (_, _, report) = run_scan(binary, stage, level)?;
        let (health, mut failures) = report_health(&report)?;
        if let Err(error) = validate_operation(
            &report,
            WorkloadKind::FreshScan,
            stage.inputs.len(),
            &health,
        ) {
            failures.push(error);
        }
        let state = staged_source_state_sha256(stage)?;
        let dump =
            dump_comparison_artifact(&stage.artifact).map_err(ComparisonError::environment)?;
        if let Err(error) = validate_run_expectation(
            expectation,
            stage,
            level,
            WorkloadKind::FreshScan,
            &state,
            &health,
            &dump,
        ) {
            failures.push(error);
        }
        runtime_failures.extend(
            failures
                .into_iter()
                .map(|failure| format!("prepared source state {state}: {failure}")),
        );
        fresh_states.insert(state.clone(), dump.clone());
        let artifact_bytes = sqlite_artifact_bytes(&stage.artifact)?;
        max_artifact_bytes = artifact_bytes;
        operation_states.insert(
            state,
            OperationStateOutput {
                artifact_bytes,
                semantic_row_total: semantic_row_total(&dump)
                    .map_err(ComparisonError::environment)?,
                dump,
            },
        );
    }
    let mut update_written = false;
    let mut elapsed_nanos = 0u128;
    let mut peak_rss_bytes = 0u64;
    let mut health = empty_health();
    for _ in 0..operations {
        if workload == WorkloadKind::FreshScan {
            clear_artifact(&stage.artifact)?;
        } else if workload == WorkloadKind::SingleFileUpdate {
            update_written = !update_written;
            write_update_variant(stage, update_written)?;
        }
        let (elapsed, rss, report) = run_scan(binary, stage, level)?;
        elapsed_nanos = elapsed_nanos.saturating_add(elapsed.as_nanos());
        peak_rss_bytes = peak_rss_bytes.max(rss);
        let (observed_health, mut operation_failures) = report_health(&report)?;
        if let Err(error) =
            validate_operation(&report, workload, stage.inputs.len(), &observed_health)
        {
            operation_failures.push(error);
        }
        let state = staged_source_state_sha256(stage)?;
        let dump =
            dump_comparison_artifact(&stage.artifact).map_err(ComparisonError::environment)?;
        if let Err(error) = validate_run_expectation(
            expectation,
            stage,
            level,
            workload,
            &state,
            &observed_health,
            &dump,
        ) {
            operation_failures.push(error);
        }
        let bytes = sqlite_artifact_bytes(&stage.artifact)?;
        max_artifact_bytes = max_artifact_bytes.max(bytes);
        if let Some(fresh_dump) = fresh_states.get(&state) {
            if let Err(error) = validate_operation_fresh_equivalence(&dump, fresh_dump, &state) {
                operation_failures.push(format!("incremental {error}"));
            }
        } else {
            clear_artifact(&validation_stage.artifact)?;
            let (_, _, fresh_report) = run_scan(binary, &validation_stage, level)?;
            let (fresh_health, mut fresh_failures) = report_health(&fresh_report)?;
            if let Err(error) = validate_operation(
                &fresh_report,
                WorkloadKind::FreshScan,
                stage.inputs.len(),
                &fresh_health,
            ) {
                fresh_failures.push(error);
            }
            let fresh_dump = dump_comparison_artifact(&validation_stage.artifact)
                .map_err(ComparisonError::environment)?;
            if let Err(error) = validate_run_expectation(
                expectation,
                &validation_stage,
                level,
                WorkloadKind::FreshScan,
                &state,
                &fresh_health,
                &fresh_dump,
            ) {
                fresh_failures.push(error);
            }
            if let Err(error) = validate_operation_fresh_equivalence(&dump, &fresh_dump, &state) {
                operation_failures.push(format!("operation {error}"));
            }
            operation_failures.extend(
                fresh_failures
                    .into_iter()
                    .map(|failure| format!("fresh-equivalence source state {state}: {failure}")),
            );
            fresh_states.insert(state.clone(), fresh_dump);
        }
        let rows = semantic_row_total(&dump).map_err(ComparisonError::environment)?;
        match operation_states.entry(state) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(OperationStateOutput {
                    dump: dump.clone(),
                    artifact_bytes: bytes,
                    semantic_row_total: rows,
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().artifact_bytes = entry.get().artifact_bytes.max(bytes);
            }
        }
        runtime_failures.append(&mut operation_failures);
        health.add(&observed_health);
    }
    let largest_state = operation_states
        .values()
        .max_by_key(|state| state.artifact_bytes)
        .expect("every batch has a prepared or measured state");
    let row_total = largest_state.semantic_row_total;
    let mut normalized = normalized_metrics(
        elapsed_nanos,
        operations,
        health.files_scanned,
        row_total,
        max_artifact_bytes,
    );
    normalized.semantic_row_total = operation_states
        .values()
        .map(|state| state.semantic_row_total)
        .sum();
    normalized.bytes_per_row_numerator = largest_state.artifact_bytes;
    normalized.bytes_per_row_denominator = row_total;
    normalized.state_sizes = operation_states
        .iter()
        .map(|(source_state_sha256, state)| ArtifactStateMetrics {
            source_state_sha256: source_state_sha256.clone(),
            semantic_row_total: state.semantic_row_total,
            artifact_bytes: state.artifact_bytes,
            bytes_per_row_numerator: state.artifact_bytes,
            bytes_per_row_denominator: state.semantic_row_total,
        })
        .collect();
    Ok(BatchOutput {
        metrics: SampleMetrics {
            elapsed_ms: elapsed_nanos / 1_000_000,
            peak_rss_bytes,
            artifact_bytes: max_artifact_bytes,
        },
        health,
        runtime_failures,
        operation_states,
        normalized,
    })
}

fn semantic_row_total(dump: &ArtifactDump) -> Result<u64, String> {
    let tables = dump_tables(dump)?;
    Ok(tables
        .values()
        .flat_map(|(_, rows)| rows.values())
        .map(|count| *count as u64)
        .sum())
}

fn validate_run_expectation(
    expectation: RunExpectation<'_>,
    stage: &StagedCorpus,
    level: &str,
    workload: WorkloadKind,
    source_state_sha256: &str,
    health: &ComparisonHealth,
    dump: &ArtifactDump,
) -> Result<(), String> {
    match expectation {
        RunExpectation::Baseline {
            receipt,
            binary_sha256,
            manifest_sha256,
            source,
            language,
        } => validate_baseline_snapshot(
            receipt,
            BaselineSnapshotPin {
                binary_sha256,
                manifest_sha256,
                source,
                language,
                level,
                workload: workload.name(),
                source_state_sha256,
            },
            health,
            dump,
        ),
        RunExpectation::Candidate { language } => {
            validate_expected_output(&stage.artifact, &stage.inputs, true, level)
                .map_err(|error| format!("{language}: {error}"))
        }
    }
}

fn run_scan(
    binary: &Path,
    stage: &StagedCorpus,
    level: &str,
) -> Result<(Duration, u64, Value), ComparisonError> {
    let rss_path = stage.artifact.with_extension("peak-rss-kib");
    let started = Instant::now();
    let output = Command::new("/usr/bin/time")
        .args(["-f", "%M", "-o"])
        .arg(&rss_path)
        .arg(binary)
        .args(["scan", "--root"])
        .arg(&stage.root)
        .args(["--db"])
        .arg(&stage.artifact)
        .args(["--json", "--level", level, "--jobs", "1"])
        .output()
        .map_err(|error| {
            ComparisonError::environment(format!(
                "failed to run extractor {}: {error}",
                binary.display()
            ))
        })?;
    let elapsed = started.elapsed();
    if !output.status.success() {
        return Err(ComparisonError::environment(format!(
            "extractor {} exited with {}: {}",
            binary.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let rss_kib = fs::read_to_string(&rss_path)
        .map_err(|error| ComparisonError::environment(format!("missing peak RSS result: {error}")))?
        .trim()
        .parse::<u64>()
        .map_err(|error| {
            ComparisonError::environment(format!("invalid peak RSS result: {error}"))
        })?;
    let report: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        ComparisonError::environment(format!("extractor returned invalid JSON: {error}"))
    })?;
    Ok((elapsed, rss_kib.saturating_mul(1024), report))
}

fn report_health(report: &Value) -> Result<(ComparisonHealth, Vec<String>), ComparisonError> {
    let counts = report
        .get("counts")
        .ok_or_else(|| ComparisonError::environment("scan report has no counts"))?;
    let mut warnings = BTreeMap::new();
    let mut skipped = BTreeMap::new();
    let mut operation_failures = Vec::new();
    for warning in report
        .get("warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let code = warning
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("unknown_warning");
        let path = warning
            .get("root_relative_path")
            .or_else(|| warning.get("path"))
            .and_then(Value::as_str)
            .unwrap_or("<unattributed>");
        let key = format!("{code}:{path}");
        if code == "slow_file_skipped" {
            *skipped.entry(key).or_insert(0) += 1;
        } else {
            *warnings.entry(key).or_insert(0) += 1;
        }
    }
    let errors = report
        .get("errors")
        .and_then(Value::as_array)
        .map_or(0, |values| values.len() as u64);
    if errors > 0 {
        operation_failures.push(format!("report contains {errors} errors"));
    }
    let parse_diagnostics = counts
        .get("totals")
        .and_then(|value| value.get("parse_diagnostics"))
        .and_then(Value::as_u64)
        .ok_or_else(|| ComparisonError::environment("scan report has no parse diagnostic total"))?;
    let health = ComparisonHealth {
        files_scanned: report_number(counts, "files_scanned")?,
        files_failed: report_number(counts, "files_failed")?,
        files_unsupported: report_number(counts, "files_unsupported")?,
        errors,
        skipped,
        warnings,
        parse_diagnostics: BTreeMap::from([("reported_total".to_string(), parse_diagnostics)]),
    };
    Ok((health, operation_failures))
}

fn report_number(object: &Value, key: &str) -> Result<u64, ComparisonError> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| ComparisonError::environment(format!("scan report has no unsigned {key}")))
}

fn validate_operation(
    report: &Value,
    workload: WorkloadKind,
    expected_files: usize,
    health: &ComparisonHealth,
) -> Result<(), String> {
    let status = report
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("missing");
    let counts = report
        .get("counts")
        .ok_or_else(|| "scan report has no counts".to_string())?;
    let changed = report_number(counts, "files_changed").map_err(|error| error.message)?;
    let unchanged = counts
        .get("files_unchanged")
        .and_then(Value::as_u64)
        .ok_or_else(|| "scan report has no files_unchanged count".to_string())?;
    let expected_unchanged = (expected_files as u64).saturating_sub(1);
    match workload {
        WorkloadKind::FreshScan
            if status != "ok" || changed != expected_files as u64 || unchanged != 0 =>
        {
            return Err(format!(
                "fresh scan status/counts were status={status}, changed={changed}, unchanged={unchanged}; expected {} changed and 0 unchanged",
                expected_files
            ));
        }
        WorkloadKind::NoChangeScan
            if status != "no_change" || changed != 0 || unchanged != expected_files as u64 =>
        {
            return Err(format!(
                "no-change scan status/counts were status={status}, changed={changed}, unchanged={unchanged}; expected 0 changed and {} unchanged",
                expected_files
            ));
        }
        WorkloadKind::SingleFileUpdate
            if status != "ok" || changed != 1 || unchanged != expected_unchanged =>
        {
            return Err(format!(
                "single-file update status/counts were status={status}, changed={changed}, unchanged={unchanged}; expected 1 changed and {expected_unchanged} unchanged"
            ));
        }
        _ => {}
    }
    if health.files_scanned != expected_files as u64 {
        return Err(format!(
            "{} discovered {} files; expected {expected_files}",
            workload.name(),
            health.files_scanned
        ));
    }
    if health.files_failed > 0
        || health.files_unsupported > 0
        || health.errors > 0
        || !health.skipped.is_empty()
    {
        return Err(format!(
            "scan health failed: failed={} unsupported={} errors={} skipped={:?}",
            health.files_failed, health.files_unsupported, health.errors, health.skipped
        ));
    }
    Ok(())
}

fn empty_health() -> ComparisonHealth {
    ComparisonHealth {
        files_scanned: 0,
        files_failed: 0,
        files_unsupported: 0,
        errors: 0,
        skipped: BTreeMap::new(),
        warnings: BTreeMap::new(),
        parse_diagnostics: BTreeMap::new(),
    }
}

fn restore_sources(stage: &StagedCorpus) -> Result<(), ComparisonError> {
    for input in &stage.inputs {
        fs::write(&input.path, &input.original_bytes).map_err(|error| {
            ComparisonError::environment(format!(
                "failed to restore {}: {error}",
                input.path.display()
            ))
        })?;
    }
    Ok(())
}

fn write_update_variant(stage: &StagedCorpus, updated: bool) -> Result<(), ComparisonError> {
    let input = &stage.inputs[stage.update_index];
    if updated {
        let mut bytes = input.original_bytes.clone();
        bytes.push(b'\n');
        fs::write(&input.path, bytes).map_err(|error| {
            ComparisonError::environment(format!(
                "failed to update {}: {error}",
                input.path.display()
            ))
        })?;
    } else {
        fs::write(&input.path, &input.original_bytes).map_err(|error| {
            ComparisonError::environment(format!(
                "failed to restore {}: {error}",
                input.path.display()
            ))
        })?;
    }
    Ok(())
}

fn clear_artifact(path: &Path) -> Result<(), ComparisonError> {
    for sidecar in [
        path.to_path_buf(),
        path.with_extension("sqlite-wal"),
        path.with_extension("sqlite-shm"),
    ] {
        match fs::remove_file(&sidecar) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(ComparisonError::environment(format!(
                    "failed to remove prior comparison artifact {}: {error}",
                    sidecar.display()
                )));
            }
        }
    }
    Ok(())
}

fn sqlite_artifact_bytes(path: &Path) -> Result<u64, ComparisonError> {
    let mut total = 0u64;
    for file in [
        path.to_path_buf(),
        path.with_extension("sqlite-wal"),
        path.with_extension("sqlite-shm"),
    ] {
        match fs::metadata(&file) {
            Ok(metadata) => total = total.saturating_add(metadata.len()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(ComparisonError::environment(format!(
                    "failed to stat SQLite artifact {}: {error}",
                    file.display()
                )));
            }
        }
    }
    if total == 0 {
        return Err(ComparisonError::environment(format!(
            "SQLite artifact is missing or empty: {}",
            path.display()
        )));
    }
    Ok(total)
}

fn dump_comparison_artifact(path: &Path) -> Result<ArtifactDump, String> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("failed to open SQLite artifact {}: {error}", path.display()))?;
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .map_err(|error| format!("failed to list SQLite tables: {error}"))?;
    let tables = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("failed to enumerate SQLite tables: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to read SQLite table names: {error}"))?;
    let mut result = Vec::new();
    for table in tables
        .into_iter()
        .filter(|table| !TRANSIENT_TABLES.contains(&table.as_str()))
    {
        let mut columns_statement = connection
            .prepare(&format!("PRAGMA table_info({})", quote_identifier(&table)))
            .map_err(|error| format!("failed to inspect table `{table}`: {error}"))?;
        let mut columns = columns_statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
            })
            .map_err(|error| format!("failed to read columns for `{table}`: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to read columns for `{table}`: {error}"))?;
        if table == "files" {
            columns
                .retain(|(name, _)| !["indexed_at", "last_revision_id"].contains(&name.as_str()));
        }
        let mut order = columns
            .iter()
            .filter(|(_, primary_key_position)| *primary_key_position > 0)
            .collect::<Vec<_>>();
        order.sort_by_key(|(_, primary_key_position)| *primary_key_position);
        if order.is_empty() {
            order = columns.iter().collect();
        }
        let query = format!(
            "SELECT {} FROM {} ORDER BY {}",
            columns
                .iter()
                .map(|(name, _)| quote_identifier(name))
                .collect::<Vec<_>>()
                .join(", "),
            quote_identifier(&table),
            order
                .into_iter()
                .map(|(name, _)| quote_identifier(name))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut rows_statement = connection
            .prepare(&query)
            .map_err(|error| format!("failed to prepare dump of `{table}`: {error}"))?;
        let mut rows = rows_statement
            .query([])
            .map_err(|error| format!("failed to read rows from `{table}`: {error}"))?;
        let mut text = String::from("#columns");
        for (name, _) in &columns {
            text.push('\t');
            text.push_str(name);
        }
        text.push('\n');
        while let Some(row) = rows
            .next()
            .map_err(|error| format!("failed to read row from `{table}`: {error}"))?
        {
            for index in 0..columns.len() {
                if index > 0 {
                    text.push('\t');
                }
                let value = row
                    .get_ref(index)
                    .map_err(|error| format!("failed to read `{table}` cell: {error}"))?;
                text.push_str(&render_sql_value(value));
            }
            text.push('\n');
        }
        result.push(TableDump { table, text });
    }
    Ok(ArtifactDump { tables: result })
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn render_sql_value(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => "null:".to_string(),
        ValueRef::Integer(number) => format!("int:{number}"),
        ValueRef::Real(number) => format!("real:{number:?}"),
        ValueRef::Text(bytes) => {
            format!("text:{}", escape_sql_text(&String::from_utf8_lossy(bytes)))
        }
        ValueRef::Blob(bytes) => {
            let mut hex = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                let _ = write!(hex, "{byte:02x}");
            }
            format!("blob:{hex}")
        }
    }
}

fn escape_sql_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn validate_expected_output(
    artifact: &Path,
    inputs: &[StagedInput],
    require_exact_diagnostics: bool,
    level: &str,
) -> Result<(), String> {
    let connection = Connection::open_with_flags(artifact, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("failed to open artifact for expected-fact checks: {error}"))?;
    for input in inputs {
        let diagnostic_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM parse_diagnostics WHERE path = ?1 AND language = ?2",
                rusqlite::params![input.relative_path, input.language],
                |row| row.get(0),
            )
            .map_err(|error| format!("{} diagnostic count query: {error}", input.relative_path))?;
        if require_exact_diagnostics && diagnostic_count != input.expected_parse_diagnostics as i64
        {
            return Err(format!(
                "{} parse diagnostics were {diagnostic_count}; current manifest pins {}",
                input.relative_path, input.expected_parse_diagnostics
            ));
        }
        let expected_facts = input
            .expected_facts
            .iter()
            .filter(|assertion| fact_domain_available_at_level(&assertion.domain, level))
            .cloned()
            .collect::<Vec<_>>();
        crate::coverage::verify_fact_assertions(
            &connection,
            &input.relative_path,
            &input.language,
            &expected_facts,
        )
        .map_err(|error| format!("{} positive fact check: {error}", input.relative_path))?;
        crate::coverage::verify_fact_assertions(
            &connection,
            &input.relative_path,
            &input.language,
            &input.negative_controls,
        )
        .map_err(|error| format!("{} negative control: {error}", input.relative_path))?;
    }
    Ok(())
}

fn fact_domain_available_at_level(domain: &str, level: &str) -> bool {
    level == "full"
        || ![
            "type_argument_usages",
            "source_regions",
            "complexity_metrics",
        ]
        .contains(&domain)
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_batch_output(runtime_failures: &[&str]) -> BatchOutput {
        BatchOutput {
            metrics: SampleMetrics {
                elapsed_ms: 2_000,
                peak_rss_bytes: 1024,
                artifact_bytes: 2048,
            },
            health: empty_health(),
            runtime_failures: runtime_failures
                .iter()
                .map(|failure| (*failure).to_string())
                .collect(),
            operation_states: BTreeMap::new(),
            normalized: normalized_metrics(2_000_000_000, 1, 1, 0, 2048),
        }
    }

    #[test]
    fn sample_spread_rejects_missing_and_excessive_samples() {
        assert!(!sample_spread_is_conclusive(&[2_000, 2_010]));
        assert!(!sample_spread_is_conclusive(&[
            2_000, 2_010, 2_005, 2_500, 2_020
        ]));
    }

    #[test]
    fn semantic_dump_keeps_capability_gaps_and_omits_only_scan_history_fields() {
        let dir = tempdir().expect("create temporary artifact directory");
        let database = dir.path().join("artifact.sqlite");
        let connection = Connection::open(&database).expect("open temporary artifact");
        connection
            .execute_batch(
                "CREATE TABLE artifact_metadata (key TEXT, value TEXT);
                 CREATE TABLE extraction_revisions (revision_id INTEGER);
                 CREATE TABLE revision_file_changes (revision_id INTEGER);
                 CREATE TABLE files (file_id TEXT, path TEXT, indexed_at TEXT, last_revision_id INTEGER);
                 CREATE TABLE language_capability_gaps (language TEXT, gap_key TEXT);
                 INSERT INTO files VALUES ('file-1', 'rust/lib.rs', '2026-01-01', 4);
                 INSERT INTO language_capability_gaps VALUES ('rust', 'http_client.instance_receiver');",
            )
            .expect("create artifact rows");
        drop(connection);

        let dump = dump_comparison_artifact(&database).expect("dump semantic rows");

        assert_eq!(
            dump.table_names().collect::<Vec<_>>(),
            vec!["files", "language_capability_gaps"]
        );
        assert_eq!(
            dump.find("files").expect("files table"),
            "#columns\tfile_id\tpath\ntext:file-1\ttext:rust/lib.rs\n"
        );
        assert!(
            dump.find("language_capability_gaps")
                .expect("capability gap table")
                .contains("http_client.instance_receiver")
        );
    }

    #[test]
    fn references_level_skips_only_full_level_fact_domains() {
        assert!(fact_domain_available_at_level("symbols", "references"));
        assert!(fact_domain_available_at_level(
            "relationships",
            "references"
        ));
        assert!(!fact_domain_available_at_level(
            "type_argument_usages",
            "references"
        ));
        assert!(!fact_domain_available_at_level(
            "source_regions",
            "references"
        ));
        assert!(!fact_domain_available_at_level(
            "complexity_metrics",
            "references"
        ));
        assert!(fact_domain_available_at_level(
            "type_argument_usages",
            "full"
        ));
    }

    #[test]
    fn comparison_plan_requires_explicit_binary_manifest_and_output_paths() {
        let error = plan_from_args(&[]).expect_err("comparison inputs are mandatory");
        assert_eq!(error.message, "missing --baseline <path>");
    }

    #[test]
    fn workload_reports_require_expected_change_and_unchanged_file_counts() {
        let report = |status: &str, changed: u64, unchanged: u64| {
            serde_json::json!({
                "status": status,
                "counts": {
                    "files_changed": changed,
                    "files_unchanged": unchanged,
                },
            })
        };
        let mut health = empty_health();
        health.files_scanned = 2;

        assert!(
            validate_operation(&report("ok", 2, 0), WorkloadKind::FreshScan, 2, &health).is_ok()
        );
        assert!(
            validate_operation(&report("ok", 1, 1), WorkloadKind::FreshScan, 2, &health).is_err()
        );
        assert!(
            validate_operation(
                &report("no_change", 0, 2),
                WorkloadKind::NoChangeScan,
                2,
                &health
            )
            .is_ok()
        );
        assert!(
            validate_operation(
                &report("no_change", 0, 1),
                WorkloadKind::NoChangeScan,
                2,
                &health
            )
            .is_err()
        );
        assert!(
            validate_operation(
                &report("ok", 1, 1),
                WorkloadKind::SingleFileUpdate,
                2,
                &health
            )
            .is_ok()
        );
        assert!(
            validate_operation(
                &report("ok", 1, 0),
                WorkloadKind::SingleFileUpdate,
                2,
                &health
            )
            .is_err()
        );
    }

    #[test]
    fn comparison_summary_policy_records_the_acceptance_thresholds() {
        let policy = comparison_policy();
        assert_eq!(policy.warmup_runs_per_binary, 1);
        assert_eq!(policy.paired_samples_per_workload, 5);
        assert_eq!(policy.minimum_baseline_batch_ms, 2_000);
        assert_eq!(policy.maximum_calibration_operations, 4_096);
        assert_eq!(policy.elapsed_growth_percent, 10);
        assert_eq!(policy.elapsed_growth_ms, 100);
        assert_eq!(policy.peak_rss_growth_percent, 15);
        assert_eq!(policy.peak_rss_growth_bytes, 32 * 1024 * 1024);
        assert_eq!(policy.artifact_growth_percent, 10);
        assert_eq!(policy.artifact_growth_bytes, 1024 * 1024);
        assert_eq!(
            policy.transient_tables,
            vec![
                "artifact_metadata",
                "extraction_revisions",
                "revision_file_changes"
            ]
        );
        assert_eq!(
            policy.excluded_volatile_columns["files"],
            vec!["indexed_at", "last_revision_id"]
        );
        assert!(policy.budget_growth_thresholds_both_required);
        assert!(
            policy
                .timed_interval
                .contains("outside the timed intervals")
        );
        assert!(
            policy
                .workload_preparation
                .contains("page cache is not dropped")
        );
    }

    #[test]
    fn only_measurement_noise_or_budget_failures_receive_one_clean_retry() {
        assert!(should_retry_measurement(&[
            "budget: elapsed_ms grew".to_string()
        ]));
        assert!(should_retry_measurement(&[
            "timing: sample spread is inconclusive".to_string()
        ]));
        assert!(!should_retry_measurement(&[]));
        assert!(!should_retry_measurement(&[
            "correctness: lost rows".to_string()
        ]));
        assert!(!should_retry_measurement(&[
            "budget: elapsed_ms grew".to_string(),
            "correctness: lost rows".to_string(),
        ]));
    }

    #[test]
    fn allowlist_provenance_pins_the_exact_versioned_sidecar_bytes() {
        let dir = tempdir().expect("create temporary allowlist directory");
        let path = dir.path().join("row-allowlist.json");
        let bytes = br#"{"format_version":1,"entries":[]}"#;
        fs::write(&path, bytes).expect("write versioned allowlist");
        let loaded = load_allowlist(dir.path(), Some(Path::new("row-allowlist.json")))
            .expect("load versioned allowlist");
        assert_eq!(loaded.path.as_deref(), Some(path.as_path()));
        assert_eq!(loaded.sha256.as_deref(), Some(sha256(bytes).as_str()));
        assert!(loaded.entries.is_empty());
    }

    #[test]
    fn build_sidecar_is_required_and_its_file_hash_is_pinned_before_measurement() {
        let dir = tempdir().expect("create temporary build directory");
        let binary = dir.path().join("julie-extract");
        fs::write(&binary, b"binary bytes").expect("write binary fixture");
        let binary_sha256 = sha256(b"binary bytes");
        let recipe = vec![
            "cargo".to_string(),
            "build".to_string(),
            "--release".to_string(),
        ];
        let record = BuildMetadata {
            format_version: 1,
            binary_sha256,
            source: BuildSourceIdentity {
                commit: "a".repeat(40),
                tree_hash_algorithm: SOURCE_TREE_HASH_ALGORITHM.to_string(),
                tree_sha256: "b".repeat(64),
                dirty: false,
                dirty_state_sha256: None,
            },
            build: ExtractorBuildMetadata {
                rustc: "rustc 1.99.0".to_string(),
                cargo: "cargo 1.99.0".to_string(),
                host_target: "x86_64-unknown-linux-gnu".to_string(),
                target: "x86_64-unknown-linux-gnu".to_string(),
                profile: "release".to_string(),
                features: vec!["default".to_string()],
                options: BTreeMap::new(),
                recipe_sha256: build_recipe_sha256(&recipe),
                recipe,
                evidence: BuildEvidence {
                    kind: "build-log".to_string(),
                    reference: "run/1".to_string(),
                    sha256: "c".repeat(64),
                },
            },
        };
        let metadata_path = PathBuf::from(format!("{}.build.json", binary.display()));
        let bytes = serde_json::to_vec(&record).expect("serialize build sidecar");
        fs::write(&metadata_path, &bytes).expect("write build sidecar");

        let loaded = load_build_metadata(dir.path(), &binary, None, &sha256(&bytes))
            .expect("load hash-pinned build sidecar");
        assert_eq!(loaded.path, metadata_path);
        assert_eq!(loaded.sha256, sha256(&bytes));

        let error = load_build_metadata(dir.path(), &binary, None, &"d".repeat(64))
            .expect_err("unmatched sidecar bytes fail before any scan");
        assert!(error.message.contains("build metadata SHA-256 mismatch"));
    }

    #[test]
    fn baseline_expectation_receipt_file_hash_is_pinned() {
        let dir = tempdir().expect("create temporary receipt directory");
        let path = dir.path().join("baseline-expectations.json");
        let receipt = BaselineExpectationReceipt {
            format_version: 1,
            baseline_binary_sha256: "a".repeat(64),
            manifest_sha256: "b".repeat(64),
            source: BuildSourceIdentity {
                commit: "c".repeat(40),
                tree_hash_algorithm: SOURCE_TREE_HASH_ALGORITHM.to_string(),
                tree_sha256: "d".repeat(64),
                dirty: false,
                dirty_state_sha256: None,
            },
            snapshots: vec![BaselineSnapshotExpectation {
                language: "rust".to_string(),
                level: "full".to_string(),
                workload: "fresh_scan".to_string(),
                source_state_sha256: "e".repeat(64),
                semantic_dump_sha256: "f".repeat(64),
                diagnostic_counts_by_file: BTreeMap::new(),
                health: empty_health(),
            }],
        };
        let bytes = serde_json::to_vec(&receipt).expect("serialize expectation receipt");
        fs::write(&path, &bytes).expect("write expectation receipt");
        assert!(
            load_baseline_expectations(
                dir.path(),
                Path::new("baseline-expectations.json"),
                &sha256(&bytes)
            )
            .is_ok()
        );
        let error = load_baseline_expectations(
            dir.path(),
            Path::new("baseline-expectations.json"),
            &"0".repeat(64),
        )
        .expect_err("changed expectations fail before any scan");
        assert!(
            error
                .message
                .contains("baseline expectations SHA-256 mismatch")
        );
    }

    #[test]
    fn preflight_and_staged_source_state_hashes_match_for_original_and_update() {
        let dir = tempdir().expect("create temporary source directory");
        let source_path = dir.path().join("source.ts");
        let source_bytes = b"const value = 1;";
        fs::write(&source_path, source_bytes).expect("write source fixture");
        let input = ComparisonInput {
            language: "typescript".to_string(),
            repo_source: source_path.clone(),
            relative_path: "typescript/source.ts".to_string(),
            source_sha256: sha256(source_bytes),
            expected_facts: Vec::new(),
            negative_controls: Vec::new(),
            expected_parse_diagnostics: 0,
        };
        let original_preflight = comparison_input_state_sha256(std::slice::from_ref(&input), None)
            .expect("hash original source state");
        let updated_preflight =
            comparison_input_state_sha256(std::slice::from_ref(&input), Some(0))
                .expect("hash updated source state");
        let stage = StagedCorpus {
            root: dir.path().join("stage"),
            artifact: dir.path().join("artifact.sqlite"),
            validation_artifact: dir.path().join("validation.sqlite"),
            inputs: vec![StagedInput {
                language: input.language,
                relative_path: input.relative_path,
                path: source_path.clone(),
                original_bytes: source_bytes.to_vec(),
                expected_facts: Vec::new(),
                negative_controls: Vec::new(),
                expected_parse_diagnostics: 0,
            }],
            update_index: 0,
        };
        assert_eq!(
            staged_source_state_sha256(&stage).expect("hash staged original"),
            original_preflight
        );
        let mut updated = source_bytes.to_vec();
        updated.push(b'\n');
        fs::write(source_path, updated).expect("write update variant");
        assert_eq!(
            staged_source_state_sha256(&stage).expect("hash staged update"),
            updated_preflight
        );
    }

    #[test]
    fn per_language_runs_require_language_scoped_row_allowances() {
        let mut entry = AllowedRowChange {
            id: "row-1".to_string(),
            ledger_marker: "marker-1".to_string(),
            language: None,
            level: None,
            workload: None,
            table: "symbols".to_string(),
            action: RowChangeAction::Add,
            canonical_row: String::new(),
            row_sha256: String::new(),
            count: 1,
        };
        assert!(validate_language_scope(&[entry.clone()], None).is_ok());
        assert!(validate_language_scope(&[entry.clone()], Some("rust")).is_err());
        entry.language = Some("rust".to_string());
        assert!(validate_language_scope(&[entry], Some("rust")).is_ok());
    }

    #[test]
    fn compare_cli_requires_budget_approval_path_and_hash_together() {
        let error = plan_from_args(&[
            "--budget-approvals".to_string(),
            "approval.json".to_string(),
        ])
        .expect_err("approval content cannot be accepted without a hash pin");
        assert!(error.message.contains("must be supplied together"));
    }

    #[test]
    fn only_noise_and_budget_failures_trigger_one_measurement_repeat() {
        assert!(should_retry_measurement(&[
            "timing: candidate sample spread is inconclusive".to_string()
        ]));
        assert!(should_retry_measurement(&[
            "budget: elapsed_ms grew".to_string()
        ]));
        assert!(!should_retry_measurement(&[
            "budget: elapsed_ms grew".to_string(),
            "correctness: lost a source row".to_string(),
        ]));
    }

    #[test]
    fn earlier_operation_sample_failures_survive_batch_combination() {
        let combined = combine_batch_outputs(vec![
            sample_batch_output(&["updated state differs from fresh output"]),
            sample_batch_output(&["restored state passes fresh comparison"]),
        ])
        .expect("combine all paired samples");
        assert_eq!(
            combined.runtime_failures,
            [
                "restored state passes fresh comparison",
                "updated state differs from fresh output"
            ]
        );
    }
}
