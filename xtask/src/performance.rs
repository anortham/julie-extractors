use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Serialize;

use crate::dogfood::{self, DogfoodMetrics, DogfoodOutputPaths, DogfoodPlan};

const MIN_BASELINE_RUNS: usize = 3;
const DEFAULT_WRITER_CURRENT_SCHEMA_FILES: usize = 10_000;
const DEFAULT_WRITER_CURRENT_SCHEMA_SYMBOLS_PER_FILE: usize = 8;
const DEFAULT_WRITER_CURRENT_SCHEMA_IDENTIFIERS_PER_FILE: usize = 24;
const DEFAULT_WRITER_CURRENT_SCHEMA_SOURCE_REGIONS_PER_FILE: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselinePlan {
    pub root: PathBuf,
    pub out_dir: PathBuf,
    pub binary: PathBuf,
    pub runs: usize,
    pub summary_path: PathBuf,
}

impl BaselinePlan {
    pub fn run_output_dirs(&self) -> Vec<PathBuf> {
        (1..=self.runs)
            .map(|run_index| self.run_output_dir(run_index))
            .collect()
    }

    fn run_output_dir(&self, run_index: usize) -> PathBuf {
        self.out_dir.join(format!("run-{run_index:03}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterCurrentSchemaPlan {
    pub out_dir: PathBuf,
    pub db_path: PathBuf,
    pub summary_path: PathBuf,
    pub files: usize,
    pub symbols_per_file: usize,
    pub identifiers_per_file: usize,
    pub source_regions_per_file: usize,
}

impl WriterCurrentSchemaPlan {
    fn new(
        out_dir: PathBuf,
        files: usize,
        symbols_per_file: usize,
        identifiers_per_file: usize,
        source_regions_per_file: usize,
    ) -> Self {
        let db_path = out_dir.join("artifact.sqlite");
        let summary_path = out_dir.join("writer-current-schema-summary.json");
        Self {
            out_dir,
            db_path,
            summary_path,
            files,
            symbols_per_file,
            identifiers_per_file,
            source_regions_per_file,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BaselineRun {
    pub run_index: usize,
    pub out_dir: PathBuf,
    pub metrics: DogfoodMetrics,
}

impl BaselineRun {
    pub fn new(run_index: usize, out_dir: PathBuf, metrics: DogfoodMetrics) -> Self {
        Self {
            run_index,
            out_dir,
            metrics,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct MetricSummary {
    pub min: f64,
    pub median: f64,
    pub max: f64,
}

impl MetricSummary {
    pub fn new(min: f64, median: f64, max: f64) -> Self {
        Self { min, median, max }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BaselineAggregates {
    pub scan_duration_ms: MetricSummary,
    pub rescan_duration_ms: MetricSummary,
    pub info_duration_ms: MetricSummary,
    pub export_duration_ms: MetricSummary,
    pub files: MetricSummary,
    pub symbols: MetricSummary,
    pub sqlite_bytes: MetricSummary,
    pub jsonl_bytes: MetricSummary,
    pub jsonl_records: MetricSummary,
    pub rows_per_second: Option<MetricSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct BaselineSummary {
    pub root: PathBuf,
    pub out_dir: PathBuf,
    pub binary: PathBuf,
    pub runs: usize,
    pub samples: Vec<BaselineRun>,
    pub aggregates: BaselineAggregates,
}

#[derive(Debug)]
pub enum PerformanceError {
    Usage(String),
    Io {
        context: String,
        source: std::io::Error,
    },
    Json {
        context: String,
        source: serde_json::Error,
    },
    Dogfood(dogfood::DogfoodError),
}

impl PerformanceError {
    fn exit_code(&self) -> u8 {
        match self {
            PerformanceError::Usage(_) => 2,
            PerformanceError::Io { .. }
            | PerformanceError::Json { .. }
            | PerformanceError::Dogfood(_) => 1,
        }
    }
}

impl std::fmt::Display for PerformanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PerformanceError::Usage(message) => write!(f, "{message}"),
            PerformanceError::Io { context, source } => write!(f, "{context}: {source}"),
            PerformanceError::Json { context, source } => write!(f, "{context}: {source}"),
            PerformanceError::Dogfood(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for PerformanceError {}

impl From<dogfood::DogfoodError> for PerformanceError {
    fn from(error: dogfood::DogfoodError) -> Self {
        PerformanceError::Dogfood(error)
    }
}

pub fn run_from_args(args: &[String]) -> ExitCode {
    match run_baseline_from_args(args) {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(error.exit_code())
        }
    }
}

pub fn run_baseline_from_args(args: &[String]) -> Result<BaselineSummary, PerformanceError> {
    let plan = plan_baseline_from_args(args)?;
    run_baseline(plan)
}

pub fn plan_writer_current_schema_from_args<I, S>(
    args: I,
) -> Result<WriterCurrentSchemaPlan, PerformanceError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some("writer-current-schema") {
        return Err(PerformanceError::Usage(
            "usage: cargo xtask performance writer-current-schema --out-dir <path> [--files <n>] [--symbols-per-file <n>] [--identifiers-per-file <n>] [--source-regions-per-file <n>]; expected `writer-current-schema`".to_string(),
        ));
    }

    let mut out_dir = None;
    let mut files = DEFAULT_WRITER_CURRENT_SCHEMA_FILES;
    let mut symbols_per_file = DEFAULT_WRITER_CURRENT_SCHEMA_SYMBOLS_PER_FILE;
    let mut identifiers_per_file = DEFAULT_WRITER_CURRENT_SCHEMA_IDENTIFIERS_PER_FILE;
    let mut source_regions_per_file = DEFAULT_WRITER_CURRENT_SCHEMA_SOURCE_REGIONS_PER_FILE;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--out-dir" => {
                out_dir = Some(required_value(&args, index + 1, "--out-dir")?);
                index += 2;
            }
            "--files" => {
                files = required_positive_usize(&args, index + 1, "--files")?;
                index += 2;
            }
            "--symbols-per-file" => {
                symbols_per_file =
                    required_positive_usize(&args, index + 1, "--symbols-per-file")?;
                index += 2;
            }
            "--identifiers-per-file" => {
                identifiers_per_file =
                    required_positive_usize(&args, index + 1, "--identifiers-per-file")?;
                index += 2;
            }
            "--source-regions-per-file" => {
                source_regions_per_file =
                    required_positive_usize(&args, index + 1, "--source-regions-per-file")?;
                index += 2;
            }
            other => {
                return Err(PerformanceError::Usage(format!(
                    "unknown performance writer-current-schema argument `{other}`"
                )));
            }
        }
    }

    let out_dir =
        out_dir.ok_or_else(|| PerformanceError::Usage("missing --out-dir".to_string()))?;

    Ok(WriterCurrentSchemaPlan::new(
        out_dir,
        files,
        symbols_per_file,
        identifiers_per_file,
        source_regions_per_file,
    ))
}

pub fn plan_baseline_from_args<I, S>(args: I) -> Result<BaselinePlan, PerformanceError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some("baseline") {
        return Err(PerformanceError::Usage(
            "usage: cargo xtask performance baseline --root <path> --out-dir <path> --binary <path> --runs <n>; expected `baseline`".to_string(),
        ));
    }

    let mut root = None;
    let mut out_dir = None;
    let mut binary = None;
    let mut runs = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--root" => {
                root = Some(required_value(&args, index + 1, "--root")?);
                index += 2;
            }
            "--out-dir" => {
                out_dir = Some(required_value(&args, index + 1, "--out-dir")?);
                index += 2;
            }
            "--binary" => {
                binary = Some(required_value(&args, index + 1, "--binary")?);
                index += 2;
            }
            "--runs" => {
                let value = required_value(&args, index + 1, "--runs")?;
                let parsed = value.to_string_lossy().parse::<usize>().map_err(|_| {
                    PerformanceError::Usage("--runs must be an integer".to_string())
                })?;
                if parsed < MIN_BASELINE_RUNS {
                    return Err(PerformanceError::Usage(format!(
                        "--runs must be at least {MIN_BASELINE_RUNS}"
                    )));
                }
                runs = Some(parsed);
                index += 2;
            }
            other => {
                return Err(PerformanceError::Usage(format!(
                    "unknown performance baseline argument `{other}`"
                )));
            }
        }
    }

    let root = root.ok_or_else(|| PerformanceError::Usage("missing --root".to_string()))?;
    let out_dir =
        out_dir.ok_or_else(|| PerformanceError::Usage("missing --out-dir".to_string()))?;
    let binary = binary.ok_or_else(|| PerformanceError::Usage("missing --binary".to_string()))?;
    let runs = runs.ok_or_else(|| PerformanceError::Usage("missing --runs".to_string()))?;
    let summary_path = out_dir.join("baseline-summary.json");

    Ok(BaselinePlan {
        root,
        out_dir,
        binary,
        runs,
        summary_path,
    })
}

pub fn run_baseline(plan: BaselinePlan) -> Result<BaselineSummary, PerformanceError> {
    fs::create_dir_all(&plan.out_dir).map_err(|source| PerformanceError::Io {
        context: format!(
            "failed to create performance baseline output dir {}",
            plan.out_dir.display()
        ),
        source,
    })?;

    let mut samples = Vec::with_capacity(plan.runs);
    for run_index in 1..=plan.runs {
        let run_out_dir = plan.run_output_dir(run_index);
        let metrics = dogfood::run_repo(DogfoodPlan {
            root: plan.root.clone(),
            out_dir: run_out_dir.clone(),
            binary: plan.binary.clone(),
            build_default_binary: false,
            paths: DogfoodOutputPaths::new(&run_out_dir),
        })?;
        samples.push(BaselineRun::new(run_index, run_out_dir, metrics));
    }

    let summary = summarize_baseline(&plan, samples)?;
    write_summary(&plan.summary_path, &summary)?;
    Ok(summary)
}

pub fn summarize_baseline(
    plan: &BaselinePlan,
    samples: Vec<BaselineRun>,
) -> Result<BaselineSummary, PerformanceError> {
    if samples.len() != plan.runs {
        return Err(PerformanceError::Usage(format!(
            "expected {} baseline runs, got {}",
            plan.runs,
            samples.len()
        )));
    }
    validate_stable_evidence(&samples)?;

    let aggregates = BaselineAggregates {
        scan_duration_ms: metric_summary(
            samples
                .iter()
                .map(|run| run.metrics.scan_duration_ms as f64),
        ),
        rescan_duration_ms: metric_summary(
            samples
                .iter()
                .map(|run| run.metrics.rescan_duration_ms as f64),
        ),
        info_duration_ms: metric_summary(
            samples
                .iter()
                .map(|run| run.metrics.info_duration_ms as f64),
        ),
        export_duration_ms: metric_summary(
            samples
                .iter()
                .map(|run| run.metrics.export_duration_ms as f64),
        ),
        files: metric_summary(samples.iter().map(|run| run.metrics.files as f64)),
        symbols: metric_summary(samples.iter().map(|run| run.metrics.symbols as f64)),
        sqlite_bytes: metric_summary(samples.iter().map(|run| run.metrics.sqlite_bytes as f64)),
        jsonl_bytes: metric_summary(samples.iter().map(|run| run.metrics.jsonl_bytes as f64)),
        jsonl_records: metric_summary(samples.iter().map(|run| run.metrics.jsonl_records as f64)),
        rows_per_second: rows_per_second_summary(&samples),
    };

    Ok(BaselineSummary {
        root: plan.root.clone(),
        out_dir: plan.out_dir.clone(),
        binary: plan.binary.clone(),
        runs: plan.runs,
        samples,
        aggregates,
    })
}

fn validate_stable_evidence(samples: &[BaselineRun]) -> Result<(), PerformanceError> {
    let Some(first) = samples.first() else {
        return Err(PerformanceError::Usage(
            "baseline summary requires at least one run".to_string(),
        ));
    };

    for sample in samples.iter().skip(1) {
        if sample.metrics.sqlite_schema_version != first.metrics.sqlite_schema_version
            || sample.metrics.extract_contract_version != first.metrics.extract_contract_version
            || sample.metrics.jsonl_schema_version != first.metrics.jsonl_schema_version
        {
            return Err(PerformanceError::Usage(format!(
                "schema versions changed between baseline runs; run 1 and run {} are not comparable",
                sample.run_index
            )));
        }
        if sample.metrics.root_path != first.metrics.root_path {
            return Err(PerformanceError::Usage(format!(
                "root path changed between baseline runs; run 1 and run {} are not comparable",
                sample.run_index
            )));
        }
        if sample.metrics.files != first.metrics.files
            || sample.metrics.symbols != first.metrics.symbols
        {
            return Err(PerformanceError::Usage(format!(
                "file or symbol totals changed between baseline runs; run 1 and run {} are not comparable",
                sample.run_index
            )));
        }
        if sample.metrics.row_totals != first.metrics.row_totals {
            return Err(PerformanceError::Usage(format!(
                "row totals changed between baseline runs; run 1 and run {} are not comparable",
                sample.run_index
            )));
        }
        if sample.metrics.jsonl_records != first.metrics.jsonl_records
            || sample.metrics.jsonl_records_by_kind != first.metrics.jsonl_records_by_kind
        {
            return Err(PerformanceError::Usage(format!(
                "jsonl record counts changed between baseline runs; run 1 and run {} are not comparable",
                sample.run_index
            )));
        }
    }

    Ok(())
}

fn rows_per_second_summary(samples: &[BaselineRun]) -> Option<MetricSummary> {
    samples
        .iter()
        .map(|run| run.metrics.rows_per_second)
        .collect::<Option<Vec<_>>>()
        .map(metric_summary)
}

fn metric_summary(values: impl IntoIterator<Item = f64>) -> MetricSummary {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_by(|left, right| left.total_cmp(right));
    let min = values[0];
    let max = values[values.len() - 1];
    let median = if values.len() % 2 == 0 {
        let upper = values.len() / 2;
        (values[upper - 1] + values[upper]) / 2.0
    } else {
        values[values.len() / 2]
    };
    MetricSummary::new(min, median, max)
}

fn required_value(args: &[String], index: usize, flag: &str) -> Result<PathBuf, PerformanceError> {
    args.get(index)
        .map(PathBuf::from)
        .ok_or_else(|| PerformanceError::Usage(format!("missing {flag} value")))
}

fn required_positive_usize(
    args: &[String],
    index: usize,
    flag: &str,
) -> Result<usize, PerformanceError> {
    let value = required_value(args, index, flag)?;
    let parsed = value
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|_| PerformanceError::Usage(format!("{flag} must be an integer")))?;
    if parsed == 0 {
        return Err(PerformanceError::Usage(format!(
            "{flag} must be greater than zero"
        )));
    }
    Ok(parsed)
}

fn write_summary(path: &Path, summary: &BaselineSummary) -> Result<(), PerformanceError> {
    let file = File::create(path).map_err(|source| PerformanceError::Io {
        context: format!(
            "failed to create performance baseline summary {}",
            path.display()
        ),
        source,
    })?;
    serde_json::to_writer_pretty(file, summary).map_err(|source| PerformanceError::Json {
        context: format!(
            "failed to write performance baseline summary {}",
            path.display()
        ),
        source,
    })
}
