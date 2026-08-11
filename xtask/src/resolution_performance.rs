use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use serde::{Deserialize, Serialize};

pub const MIN_RUNS: usize = 3;
pub const MIN_IDENTIFIER_ROWS_PER_SECOND: f64 = 50_000.0;
pub const MAX_DIFF_OVERHEAD_RATIO: f64 = 0.50;
pub const MAX_TIME_TO_EXACT_MS: u64 = 30_000;
pub const REFUTED_BIND_CONTROL_MS: u64 = 24_390;
pub const REBASE_GAP_BYTES: u64 = 64 * 1024 * 1024;
pub const FIXED_PAIRS: [&str; 2] = ["miller-unchanged", "miller-mutated"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionPerformancePlan {
    pub runs: usize,
    pub out_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolutionPerformanceSample {
    pub pair: String,
    pub run: usize,
    pub resolution_compute_ms: u64,
    pub store_fresh_ms: u64,
    pub diff_ms: u64,
    pub delta_write_ms: u64,
    pub publish_ms: u64,
    pub time_to_exact_ms: u64,
    pub integrity_ms: u64,
    pub identifier_rows: u64,
    pub pending_rows: u64,
    pub peak_rss_bytes: u64,
    pub base_bytes: u64,
    pub delta_bytes: u64,
    pub semantic_differences: u64,
    pub applied_differences: u64,
    pub foreground_bind_ms: u64,
    pub foreground_identifier_work: Option<u64>,
    pub background_pipeline_ms: u64,
    pub resolution_mode: String,
    pub fallback_reason: Option<String>,
    pub canonical_semantic_digest: String,
    pub row_level_differences: u64,
    pub fixture_snapshot_digest: String,
    pub exact_gap_rows: u64,
    pub exact_gap_files: u64,
    pub cumulative_gap_bytes_before: u64,
    pub cumulative_gap_bytes_after: u64,
    pub cumulative_delta_rows_before: u64,
    pub cumulative_delta_rows_after: u64,
    pub rebased: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ResolutionPerformanceVerdict {
    #[serde(flatten)]
    pub sample: ResolutionPerformanceSample,
    pub identifier_rows_per_second: f64,
    pub diff_overhead_ratio: f64,
    pub g1_passed: bool,
    pub g2_passed: bool,
    pub identifier_throughput_status: String,
    pub diff_overhead_status: String,
    pub g3c_passed: Option<bool>,
    pub g4_passed: Option<bool>,
    pub g5_passed: bool,
    pub g6_passed: bool,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ResolutionPerformanceSummary {
    pub runs: usize,
    pub pairs: Vec<String>,
    pub samples: Vec<ResolutionPerformanceVerdict>,
    pub passed: bool,
}

#[derive(Debug)]
pub enum ResolutionPerformanceError {
    Usage(String),
    Io {
        context: String,
        source: std::io::Error,
    },
    Json {
        context: String,
        source: serde_json::Error,
    },
    HarnessFailed(i32),
    Evidence(String),
}

impl std::fmt::Display for ResolutionPerformanceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(message) | Self::Evidence(message) => formatter.write_str(message),
            Self::Io { context, source } => write!(formatter, "{context}: {source}"),
            Self::Json { context, source } => write!(formatter, "{context}: {source}"),
            Self::HarnessFailed(code) => {
                write!(
                    formatter,
                    "store resolution performance harness exited {code}"
                )
            }
        }
    }
}

impl std::error::Error for ResolutionPerformanceError {}

impl ResolutionPerformanceError {
    fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Io { .. } | Self::Json { .. } | Self::HarnessFailed(_) | Self::Evidence(_) => 1,
        }
    }
}

pub fn run_from_args(args: &[String]) -> ExitCode {
    match plan_from_args(args) {
        Ok(plan) => match run(plan) {
            Ok(summary) if summary.passed => ExitCode::SUCCESS,
            Ok(_) => ExitCode::from(1),
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(error.exit_code())
            }
        },
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(error.exit_code())
        }
    }
}

pub fn run(
    mut plan: ResolutionPerformancePlan,
) -> Result<ResolutionPerformanceSummary, ResolutionPerformanceError> {
    if plan.out_dir.is_relative() {
        plan.out_dir = std::env::current_dir()
            .map_err(|source| ResolutionPerformanceError::Io {
                context: "resolve current directory".to_string(),
                source,
            })?
            .join(plan.out_dir);
    }
    fs::create_dir_all(&plan.out_dir).map_err(|source| ResolutionPerformanceError::Io {
        context: format!("create {}", plan.out_dir.display()),
        source,
    })?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args(harness_command_args())
        .env("JULIE_STORE_RESOLUTION_PERF_RUNS", plan.runs.to_string())
        .env("JULIE_STORE_RESOLUTION_PERF_OUT_DIR", &plan.out_dir)
        .status()
        .map_err(|source| ResolutionPerformanceError::Io {
            context: "launch store resolution performance harness".to_string(),
            source,
        })?;
    if !status.success() {
        return Err(ResolutionPerformanceError::HarnessFailed(
            status.code().unwrap_or(1),
        ));
    }

    let mut samples = Vec::with_capacity(plan.runs * FIXED_PAIRS.len());
    for run in 1..=plan.runs {
        for pair in FIXED_PAIRS {
            let path = plan
                .out_dir
                .join(format!("run-{run:03}"))
                .join(format!("{pair}.json"));
            let bytes = fs::read(&path).map_err(|source| ResolutionPerformanceError::Io {
                context: format!("read {}", path.display()),
                source,
            })?;
            samples.push(serde_json::from_slice(&bytes).map_err(|source| {
                ResolutionPerformanceError::Json {
                    context: format!("decode {}", path.display()),
                    source,
                }
            })?);
        }
    }
    let summary = evaluate_samples(plan.runs, samples)?;
    let summary_path = plan.out_dir.join("store-resolution-summary.json");
    let file =
        fs::File::create(&summary_path).map_err(|source| ResolutionPerformanceError::Io {
            context: format!("create {}", summary_path.display()),
            source,
        })?;
    serde_json::to_writer_pretty(file, &summary).map_err(|source| {
        ResolutionPerformanceError::Json {
            context: format!("write {}", summary_path.display()),
            source,
        }
    })?;
    println!("{}", summary_path.display());
    Ok(summary)
}

pub fn harness_command_args() -> &'static [&'static str] {
    &[
        "test",
        "--release",
        "-p",
        "julie-extract-cli",
        "--features",
        "test-store-resolution-contract",
        "--test",
        "store_resolution_performance",
        "store_resolution_performance_gate",
        "--",
        "--exact",
        "--nocapture",
        "--test-threads=1",
    ]
}

pub fn plan_from_args<I, S>(
    args: I,
) -> Result<ResolutionPerformancePlan, ResolutionPerformanceError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some("store-resolution") {
        return Err(usage());
    }

    let mut runs = None;
    let mut out_dir = PathBuf::from("target/performance/store-resolution");
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--runs" => {
                index += 1;
                let value = args.get(index).ok_or_else(usage)?;
                runs = Some(value.parse::<usize>().map_err(|_| {
                    ResolutionPerformanceError::Usage("--runs must be an integer".to_string())
                })?);
            }
            "--out-dir" => {
                index += 1;
                out_dir = PathBuf::from(args.get(index).ok_or_else(usage)?);
            }
            other => {
                return Err(ResolutionPerformanceError::Usage(format!(
                    "unknown performance store-resolution argument `{other}`"
                )));
            }
        }
        index += 1;
    }

    let runs = runs.ok_or_else(usage)?;
    if runs < MIN_RUNS {
        return Err(ResolutionPerformanceError::Usage(format!(
            "--runs must be at least {MIN_RUNS}"
        )));
    }
    Ok(ResolutionPerformancePlan { runs, out_dir })
}

pub fn evaluate_samples(
    runs: usize,
    mut samples: Vec<ResolutionPerformanceSample>,
) -> Result<ResolutionPerformanceSummary, ResolutionPerformanceError> {
    if runs < MIN_RUNS {
        return Err(ResolutionPerformanceError::Evidence(format!(
            "at least {MIN_RUNS} runs are required"
        )));
    }
    samples.sort_by(|left, right| (left.run, &left.pair).cmp(&(right.run, &right.pair)));

    let expected = (1..=runs)
        .flat_map(|run| FIXED_PAIRS.map(move |pair| (run, pair.to_string())))
        .collect::<BTreeSet<_>>();
    let found = samples
        .iter()
        .map(|sample| (sample.run, sample.pair.clone()))
        .collect::<BTreeSet<_>>();
    if found != expected || samples.len() != expected.len() {
        return Err(ResolutionPerformanceError::Evidence(
            "evidence must contain every fixed pair in every run exactly once".to_string(),
        ));
    }
    if samples
        .iter()
        .any(|sample| sample.identifier_rows == 0 || sample.pending_rows == 0)
    {
        return Err(ResolutionPerformanceError::Evidence(
            "every sample must exercise both resolution tables".to_string(),
        ));
    }

    let mut verdicts = samples.into_iter().map(evaluate_sample).collect::<Vec<_>>();
    for run in 1..=runs {
        let full = verdicts
            .iter()
            .position(|sample| sample.sample.run == run && sample.sample.pair == FIXED_PAIRS[0])
            .expect("complete fixed-pair evidence");
        let scoped = verdicts
            .iter()
            .position(|sample| sample.sample.run == run && sample.sample.pair == FIXED_PAIRS[1])
            .expect("complete fixed-pair evidence");
        let faster =
            verdicts[scoped].sample.time_to_exact_ms < verdicts[full].sample.time_to_exact_ms;
        let equivalent = !verdicts[scoped].sample.canonical_semantic_digest.is_empty()
            && verdicts[scoped].sample.canonical_semantic_digest
                == verdicts[full].sample.canonical_semantic_digest
            && !verdicts[scoped].sample.fixture_snapshot_digest.is_empty()
            && verdicts[scoped].sample.fixture_snapshot_digest
                == verdicts[full].sample.fixture_snapshot_digest;
        verdicts[full].g2_passed = equivalent;
        verdicts[full].passed &= equivalent;
        verdicts[scoped].g2_passed = equivalent;
        verdicts[scoped].passed &= equivalent;
        verdicts[full].g6_passed = faster;
        verdicts[full].passed &= faster;
        verdicts[scoped].g6_passed = faster;
        verdicts[scoped].passed &= faster;
    }
    let passed = verdicts.iter().all(|sample| sample.passed);
    Ok(ResolutionPerformanceSummary {
        runs,
        pairs: FIXED_PAIRS.iter().map(|pair| (*pair).to_string()).collect(),
        samples: verdicts,
        passed,
    })
}

fn evaluate_sample(sample: ResolutionPerformanceSample) -> ResolutionPerformanceVerdict {
    let identifier_rows_per_second = if sample.resolution_compute_ms == 0 {
        f64::INFINITY
    } else {
        sample.identifier_rows as f64 * 1_000.0 / sample.resolution_compute_ms as f64
    };
    let diff_overhead_ratio = if sample.resolution_compute_ms == 0 {
        f64::INFINITY
    } else {
        (sample.diff_ms + sample.delta_write_ms) as f64 / sample.resolution_compute_ms as f64
    };
    let g1_passed = sample.semantic_differences == 0
        && sample.applied_differences == 0
        && sample.row_level_differences == 0;
    let g2_passed = true;
    let forced_full_control = sample.pair == FIXED_PAIRS[0];
    let identifier_throughput_status = "report_only".to_string();
    let diff_overhead_status = "report_only".to_string();
    let g3c_passed =
        (!forced_full_control).then_some(sample.time_to_exact_ms <= MAX_TIME_TO_EXACT_MS);
    let g4_passed = (!forced_full_control).then_some(
        sample.rebased
            && sample.cumulative_gap_bytes_before > REBASE_GAP_BYTES
            && sample.cumulative_gap_bytes_after <= REBASE_GAP_BYTES
            && sample.cumulative_gap_bytes_after < sample.cumulative_gap_bytes_before
            && sample.delta_bytes == sample.cumulative_gap_bytes_after
            && sample.exact_gap_rows == 0
            && sample.exact_gap_files == 0
            && sample.cumulative_delta_rows_after == 0
            && sample.base_bytes > 0,
    );
    let g5_passed = if forced_full_control {
        sample.resolution_mode == "full"
            && sample.fallback_reason.as_deref() == Some("incremental_resolution_disabled")
    } else {
        sample.resolution_mode == "scoped" && sample.fallback_reason.is_none()
    };
    let g6_passed = true;
    let passed = g1_passed
        && g2_passed
        && g3c_passed.unwrap_or(true)
        && g4_passed.unwrap_or(true)
        && g5_passed;
    ResolutionPerformanceVerdict {
        sample,
        identifier_rows_per_second,
        diff_overhead_ratio,
        g1_passed,
        g2_passed,
        identifier_throughput_status,
        diff_overhead_status,
        g3c_passed,
        g4_passed,
        g5_passed,
        g6_passed,
        passed,
    }
}

fn usage() -> ResolutionPerformanceError {
    ResolutionPerformanceError::Usage(
        "usage: cargo xtask performance store-resolution --runs <n> [--out-dir <path>]".to_string(),
    )
}
