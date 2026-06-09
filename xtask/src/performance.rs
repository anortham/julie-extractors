use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use julie_extract_artifact::metadata::ArtifactMetadata;
use julie_extract_artifact::model::{
    ArtifactComplexityMetric, ArtifactFile, ArtifactIdentifier, ArtifactLiteral,
    ArtifactParseDiagnostic, ArtifactPendingRelationship, ArtifactRelationship,
    ArtifactSourceRegion, ArtifactStructuralFact, ArtifactSymbol, ArtifactSymbolAnnotation,
    ArtifactTypeArgument, ArtifactTypeArgumentUsage, ArtifactTypeFact, FileStatus, RevisionInput,
    RowCounts, WriteMode, WriteOperation,
};
use julie_extract_artifact::writer::{ArtifactWriteError, ArtifactWriter};
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WriterCurrentSchemaInput {
    pub files: usize,
    pub symbols_per_file: usize,
    pub identifiers_per_file: usize,
    pub source_regions_per_file: usize,
}

impl From<&WriterCurrentSchemaPlan> for WriterCurrentSchemaInput {
    fn from(plan: &WriterCurrentSchemaPlan) -> Self {
        Self {
            files: plan.files,
            symbols_per_file: plan.symbols_per_file,
            identifiers_per_file: plan.identifiers_per_file,
            source_regions_per_file: plan.source_regions_per_file,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WriterCurrentSchemaRowCounts {
    pub files: i64,
    pub symbols: i64,
    pub symbol_annotations: i64,
    pub identifiers: i64,
    pub relationships: i64,
    pub pending_relationships: i64,
    pub type_facts: i64,
    pub type_argument_usages: i64,
    pub type_arguments: i64,
    pub literals: i64,
    pub source_regions: i64,
    pub structural_facts: i64,
    pub complexity_metrics: i64,
    pub parse_diagnostics: i64,
    pub revision_file_changes: i64,
}

impl WriterCurrentSchemaRowCounts {
    fn extraction_rows(&self) -> i64 {
        self.files
            + self.symbols
            + self.symbol_annotations
            + self.identifiers
            + self.relationships
            + self.pending_relationships
            + self.type_facts
            + self.type_argument_usages
            + self.type_arguments
            + self.literals
            + self.source_regions
            + self.structural_facts
            + self.complexity_metrics
            + self.parse_diagnostics
    }
}

impl From<&RowCounts> for WriterCurrentSchemaRowCounts {
    fn from(rows: &RowCounts) -> Self {
        Self {
            files: rows.files,
            symbols: rows.symbols,
            symbol_annotations: rows.symbol_annotations,
            identifiers: rows.identifiers,
            relationships: rows.relationships,
            pending_relationships: rows.pending_relationships,
            type_facts: rows.type_facts,
            type_argument_usages: rows.type_argument_usages,
            type_arguments: rows.type_arguments,
            literals: rows.literals,
            source_regions: rows.source_regions,
            structural_facts: rows.structural_facts,
            complexity_metrics: rows.complexity_metrics,
            parse_diagnostics: rows.parse_diagnostics,
            revision_file_changes: rows.revision_file_changes,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WriterCurrentSchemaSummary {
    pub out_dir: PathBuf,
    pub db_path: PathBuf,
    pub summary_path: PathBuf,
    pub input: WriterCurrentSchemaInput,
    pub rows_written: WriterCurrentSchemaRowCounts,
    pub transactions_committed: usize,
    pub files_changed: usize,
    pub elapsed_write_ms: u128,
    pub rows_per_second: Option<f64>,
    pub sqlite_bytes: u64,
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
    Sqlite {
        context: String,
        source: rusqlite::Error,
    },
    Artifact(ArtifactWriteError),
    Dogfood(dogfood::DogfoodError),
}

impl PerformanceError {
    fn exit_code(&self) -> u8 {
        match self {
            PerformanceError::Usage(_) => 2,
            PerformanceError::Io { .. }
            | PerformanceError::Json { .. }
            | PerformanceError::Sqlite { .. }
            | PerformanceError::Artifact(_)
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
            PerformanceError::Sqlite { context, source } => write!(f, "{context}: {source}"),
            PerformanceError::Artifact(error) => write!(f, "{error}"),
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

impl From<ArtifactWriteError> for PerformanceError {
    fn from(error: ArtifactWriteError) -> Self {
        PerformanceError::Artifact(error)
    }
}

pub fn run_from_args(args: &[String]) -> ExitCode {
    let result = match args.first().map(String::as_str) {
        Some("baseline") => run_baseline_from_args(args).map(|_| ()),
        Some("writer-current-schema") => run_writer_current_schema_from_args(args).map(|_| ()),
        Some(other) => Err(PerformanceError::Usage(format!(
            "unknown performance subcommand `{other}`; expected `baseline` or `writer-current-schema`"
        ))),
        None => Err(PerformanceError::Usage(
            "usage: cargo xtask performance <baseline|writer-current-schema> ...".to_string(),
        )),
    };

    match result {
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

pub fn run_writer_current_schema_from_args(
    args: &[String],
) -> Result<WriterCurrentSchemaSummary, PerformanceError> {
    let plan = plan_writer_current_schema_from_args(args)?;
    run_writer_current_schema(plan)
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
                symbols_per_file = required_positive_usize(&args, index + 1, "--symbols-per-file")?;
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

pub fn run_writer_current_schema(
    plan: WriterCurrentSchemaPlan,
) -> Result<WriterCurrentSchemaSummary, PerformanceError> {
    fs::create_dir_all(&plan.out_dir).map_err(|source| PerformanceError::Io {
        context: format!(
            "failed to create writer current-schema output dir {}",
            plan.out_dir.display()
        ),
        source,
    })?;
    remove_stale_file(&plan.db_path)?;
    remove_stale_file(&plan.summary_path)?;

    let files = writer_current_schema_files(&plan);
    let mut writer = ArtifactWriter::open_path(&plan.db_path, writer_current_schema_metadata())
        .map_err(|source| PerformanceError::Sqlite {
            context: format!(
                "failed to open writer current-schema artifact {}",
                plan.db_path.display()
            ),
            source,
        })?;
    let started = Instant::now();
    let result = writer.write_scan(writer_current_schema_revision(), &files)?;
    let elapsed = started.elapsed();
    drop(writer);

    let sqlite_bytes = fs::metadata(&plan.db_path)
        .map_err(|source| PerformanceError::Io {
            context: format!(
                "failed to inspect writer current-schema artifact {}",
                plan.db_path.display()
            ),
            source,
        })?
        .len();
    let rows_written = WriterCurrentSchemaRowCounts::from(&result.rows_written);
    let elapsed_seconds = elapsed.as_secs_f64();
    let rows_per_second = if elapsed_seconds > 0.0 {
        Some(rows_written.extraction_rows() as f64 / elapsed_seconds)
    } else {
        None
    };

    let summary = WriterCurrentSchemaSummary {
        out_dir: plan.out_dir.clone(),
        db_path: plan.db_path.clone(),
        summary_path: plan.summary_path.clone(),
        input: WriterCurrentSchemaInput::from(&plan),
        rows_written,
        transactions_committed: result.transactions_committed,
        files_changed: result.files_changed,
        elapsed_write_ms: elapsed.as_millis(),
        rows_per_second,
        sqlite_bytes,
    };
    write_json_summary(&plan.summary_path, &summary)?;
    Ok(summary)
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
    write_json_summary(&plan.summary_path, &summary)?;
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

fn remove_stale_file(path: &Path) -> Result<(), PerformanceError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(PerformanceError::Io {
            context: format!("failed to remove stale output {}", path.display()),
            source,
        }),
    }
}

fn writer_current_schema_metadata() -> ArtifactMetadata {
    ArtifactMetadata {
        artifact_id: "artifact-writer-current-schema-performance".to_string(),
        root_path: "/synthetic/writer-current-schema".to_string(),
        binary_version: "julie-extract performance writer-current-schema".to_string(),
        hash_algorithm: "blake3".to_string(),
        parser_inventory_fingerprint: "sha256:synthetic-parser-inventory".to_string(),
        capability_snapshot_fingerprint: "sha256:synthetic-capability-snapshot".to_string(),
        created_at: "2026-06-09T00:00:00Z".to_string(),
        updated_at: "2026-06-09T00:00:00Z".to_string(),
    }
}

fn writer_current_schema_revision() -> RevisionInput {
    RevisionInput {
        operation: WriteOperation::Scan,
        mode: Some(WriteMode::Incremental),
        started_at: "2026-06-09T00:00:00Z".to_string(),
        completed_at: "2026-06-09T00:00:01Z".to_string(),
        binary_version: "julie-extract performance writer-current-schema".to_string(),
        input_root: Some("/synthetic/writer-current-schema".to_string()),
    }
}

fn writer_current_schema_files(plan: &WriterCurrentSchemaPlan) -> Vec<ArtifactFile> {
    (0..plan.files)
        .map(|file_index| writer_current_schema_file(file_index, plan))
        .collect()
}

fn writer_current_schema_file(file_index: usize, plan: &WriterCurrentSchemaPlan) -> ArtifactFile {
    let file_id = format!("file-{file_index}");
    let mut file = ArtifactFile {
        file_id: file_id.clone(),
        path: format!("src/generated/file_{file_index}.rs"),
        language: "rust".to_string(),
        content_hash: format!("blake3:file:{file_index}"),
        content_bytes: 4096,
        line_count: Some(200),
        indexed_at: "2026-06-09T00:00:00Z".to_string(),
        status: FileStatus::Indexed,
        metadata_json: None,
        symbols: (0..plan.symbols_per_file)
            .map(|symbol_index| writer_current_schema_symbol(&file_id, symbol_index))
            .collect(),
        symbol_annotations: Vec::new(),
        identifiers: Vec::new(),
        relationships: Vec::new(),
        pending_relationships: Vec::new(),
        type_facts: Vec::new(),
        type_argument_usages: Vec::new(),
        type_arguments: Vec::new(),
        literals: Vec::new(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        parse_diagnostics: Vec::new(),
    };

    file.symbol_annotations
        .push(writer_current_schema_symbol_annotation(&file_id));
    file.identifiers = (0..plan.identifiers_per_file)
        .map(|identifier_index| {
            writer_current_schema_identifier(&file_id, identifier_index, plan.symbols_per_file)
        })
        .collect();
    file.relationships = (0..plan.symbols_per_file.saturating_sub(1))
        .map(|relationship_index| writer_current_schema_relationship(&file_id, relationship_index))
        .collect();
    file.pending_relationships
        .push(writer_current_schema_pending_relationship(&file_id));
    file.type_facts = (0..plan.symbols_per_file)
        .map(|type_index| writer_current_schema_type_fact(&file_id, type_index))
        .collect();
    file.type_argument_usages = (0..plan.identifiers_per_file)
        .map(|usage_index| writer_current_schema_type_argument_usage(&file_id, usage_index))
        .collect();
    file.type_arguments = (0..plan.identifiers_per_file)
        .map(|argument_index| writer_current_schema_type_argument(&file_id, argument_index))
        .collect();
    file.literals.push(writer_current_schema_literal(&file_id));
    file.source_regions = (0..plan.source_regions_per_file)
        .map(|region_index| writer_current_schema_source_region(&file_id, region_index))
        .collect();
    file.structural_facts
        .push(writer_current_schema_structural_fact(&file_id));
    file.complexity_metrics
        .push(writer_current_schema_file_complexity_metric(&file_id));
    file.complexity_metrics
        .extend((0..plan.symbols_per_file).map(|symbol_index| {
            writer_current_schema_symbol_complexity_metric(&file_id, symbol_index)
        }));
    file.parse_diagnostics
        .push(writer_current_schema_parse_diagnostic(&file_id));
    file
}

fn writer_current_schema_symbol(file_id: &str, symbol_index: usize) -> ArtifactSymbol {
    let start_line = (symbol_index * 4 + 1) as i64;
    let start_byte = (symbol_index * 128) as i64;
    ArtifactSymbol {
        symbol_id: format!("{file_id}-symbol-{symbol_index}"),
        name: format!("symbol_{symbol_index}"),
        kind: "function".to_string(),
        signature: Some(format!("fn symbol_{symbol_index}()")),
        doc_comment: Some(format!("Synthetic symbol {symbol_index}")),
        visibility: Some("public".to_string()),
        parent_symbol_id: None,
        start_line,
        start_column: 0,
        end_line: start_line + 2,
        end_column: 1,
        start_byte,
        end_byte: start_byte + 96,
        body_start_line: Some(start_line + 1),
        body_start_column: Some(0),
        body_end_line: Some(start_line + 2),
        body_end_column: Some(1),
        body_start_byte: Some(start_byte + 16),
        body_end_byte: Some(start_byte + 96),
        body_hash: Some(format!("md5:body:{file_id}:{symbol_index}")),
        semantic_group: Some("function".to_string()),
        confidence: Some(1.0),
        content_type: Some("code".to_string()),
        is_test: false,
        test_container: false,
        test_lifecycle: false,
        metadata_json: None,
    }
}

fn writer_current_schema_symbol_annotation(file_id: &str) -> ArtifactSymbolAnnotation {
    ArtifactSymbolAnnotation {
        annotation_id: format!("{file_id}-annotation-0"),
        symbol_id: format!("{file_id}-symbol-0"),
        annotation: "route".to_string(),
        annotation_key: "route".to_string(),
        raw_text: Some("#[route]".to_string()),
        carrier: Some("attribute".to_string()),
        metadata_json: None,
    }
}

fn writer_current_schema_identifier(
    file_id: &str,
    identifier_index: usize,
    symbols_per_file: usize,
) -> ArtifactIdentifier {
    let containing_symbol_index = identifier_index % symbols_per_file;
    let target_symbol_index = (identifier_index + 1) % symbols_per_file;
    let start_line = (identifier_index + 1) as i64;
    ArtifactIdentifier {
        identifier_id: format!("{file_id}-identifier-{identifier_index}"),
        name: format!("identifier_{identifier_index}"),
        kind: "call".to_string(),
        containing_symbol_id: Some(format!("{file_id}-symbol-{containing_symbol_index}")),
        target_symbol_id: Some(format!("{file_id}-symbol-{target_symbol_index}")),
        start_line,
        start_column: 4,
        end_line: start_line,
        end_column: 20,
        start_byte: (identifier_index * 24) as i64,
        end_byte: (identifier_index * 24 + 16) as i64,
        confidence: 1.0,
        code_context: Some("synthetic call".to_string()),
        metadata_json: None,
    }
}

fn writer_current_schema_relationship(
    file_id: &str,
    relationship_index: usize,
) -> ArtifactRelationship {
    ArtifactRelationship {
        relationship_id: format!("{file_id}-relationship-{relationship_index}"),
        from_symbol_id: format!("{file_id}-symbol-{relationship_index}"),
        to_symbol_id: format!("{file_id}-symbol-{}", relationship_index + 1),
        kind: "calls".to_string(),
        start_line: Some((relationship_index + 1) as i64),
        start_column: Some(4),
        end_line: Some((relationship_index + 1) as i64),
        end_column: Some(20),
        start_byte: Some((relationship_index * 32) as i64),
        end_byte: Some((relationship_index * 32 + 16) as i64),
        confidence: 1.0,
        metadata_json: None,
    }
}

fn writer_current_schema_pending_relationship(file_id: &str) -> ArtifactPendingRelationship {
    ArtifactPendingRelationship {
        pending_relationship_id: format!("{file_id}-pending-0"),
        from_symbol_id: format!("{file_id}-symbol-0"),
        caller_scope_symbol_id: Some(format!("{file_id}-symbol-0")),
        kind: "uses".to_string(),
        target_display_name: "external::Target".to_string(),
        target_terminal_name: "Target".to_string(),
        target_receiver: Some("external".to_string()),
        target_namespace_json: r#"["external"]"#.to_string(),
        target_import_context: Some("use external::Target;".to_string()),
        start_line: 2,
        start_column: Some(4),
        end_line: Some(2),
        end_column: Some(20),
        start_byte: Some(32),
        end_byte: Some(48),
        confidence: 0.9,
        metadata_json: None,
    }
}

fn writer_current_schema_type_fact(file_id: &str, type_index: usize) -> ArtifactTypeFact {
    ArtifactTypeFact {
        type_fact_id: format!("{file_id}-type-fact-{type_index}"),
        symbol_id: format!("{file_id}-symbol-{type_index}"),
        resolved_type: format!("Type{type_index}"),
        generic_params_json: Some(r#"["T"]"#.to_string()),
        constraints_json: None,
        is_inferred: true,
        metadata_json: None,
    }
}

fn writer_current_schema_type_argument_usage(
    file_id: &str,
    usage_index: usize,
) -> ArtifactTypeArgumentUsage {
    ArtifactTypeArgumentUsage {
        usage_id: format!("{file_id}-type-usage-{usage_index}"),
        identifier_id: format!("{file_id}-identifier-{usage_index}"),
        metadata_json: None,
    }
}

fn writer_current_schema_type_argument(
    file_id: &str,
    argument_index: usize,
) -> ArtifactTypeArgument {
    ArtifactTypeArgument {
        type_argument_id: format!("{file_id}-type-argument-{argument_index}"),
        usage_id: format!("{file_id}-type-usage-{argument_index}"),
        parent_type_argument_id: None,
        ordinal: 0,
        type_name: format!("Argument{argument_index}"),
    }
}

fn writer_current_schema_literal(file_id: &str) -> ArtifactLiteral {
    ArtifactLiteral {
        literal_id: format!("{file_id}-literal-0"),
        literal_text: "/api/synthetic".to_string(),
        kind: "url".to_string(),
        carrier: Some("route".to_string()),
        arg_position: 0,
        containing_symbol_id: Some(format!("{file_id}-symbol-0")),
        start_line: 3,
        start_column: 8,
        end_line: 3,
        end_column: 24,
        start_byte: 64,
        end_byte: 80,
        confidence: 1.0,
        metadata_json: None,
    }
}

fn writer_current_schema_source_region(file_id: &str, region_index: usize) -> ArtifactSourceRegion {
    let kind = match region_index % 3 {
        0 => "comment",
        1 => "doc_comment",
        _ => "string_literal",
    };
    let start_line = (region_index + 1) as i64;
    ArtifactSourceRegion {
        source_region_id: format!("{file_id}-source-region-{region_index}"),
        kind: kind.to_string(),
        containing_symbol_id: Some(format!("{file_id}-symbol-0")),
        start_line,
        start_column: 0,
        end_line: start_line,
        end_column: 16,
        start_byte: (region_index * 40) as i64,
        end_byte: (region_index * 40 + 16) as i64,
        metadata_json: Some(r#"{"synthetic":true}"#.to_string()),
    }
}

fn writer_current_schema_structural_fact(file_id: &str) -> ArtifactStructuralFact {
    ArtifactStructuralFact {
        structural_fact_id: format!("{file_id}-structural-fact-0"),
        pattern_id: "rust.unsafe_block.v1".to_string(),
        capture_name: "unsafe_block".to_string(),
        node_kind: "unsafe_block".to_string(),
        containing_symbol_id: Some(format!("{file_id}-symbol-0")),
        start_line: 4,
        start_column: 4,
        end_line: 6,
        end_column: 5,
        start_byte: 96,
        end_byte: 160,
        confidence: 1.0,
        metadata_json: Some(r#"{"pattern_version":1,"query_family":"safety"}"#.to_string()),
    }
}

fn writer_current_schema_file_complexity_metric(file_id: &str) -> ArtifactComplexityMetric {
    ArtifactComplexityMetric {
        complexity_metric_id: format!("{file_id}-complexity-file"),
        scope: "file".to_string(),
        symbol_id: None,
        algorithm_id: "julie-ast-complexity-v1".to_string(),
        covered_lines: 200,
        covered_bytes: 4096,
        decision_count: 4,
        loop_count: 2,
        max_nesting_depth: 3,
        parameter_count: None,
        start_line: 1,
        start_column: 0,
        end_line: 200,
        end_column: 0,
        start_byte: 0,
        end_byte: 4096,
        metadata_json: Some(r#"{"metric_version":1,"synthetic":true}"#.to_string()),
    }
}

fn writer_current_schema_symbol_complexity_metric(
    file_id: &str,
    symbol_index: usize,
) -> ArtifactComplexityMetric {
    let start_line = (symbol_index * 4 + 1) as i64;
    let start_byte = (symbol_index * 128) as i64;
    ArtifactComplexityMetric {
        complexity_metric_id: format!("{file_id}-complexity-symbol-{symbol_index}"),
        scope: "symbol".to_string(),
        symbol_id: Some(format!("{file_id}-symbol-{symbol_index}")),
        algorithm_id: "julie-ast-complexity-v1".to_string(),
        covered_lines: 3,
        covered_bytes: 96,
        decision_count: 1,
        loop_count: 1,
        max_nesting_depth: 2,
        parameter_count: Some(2),
        start_line,
        start_column: 0,
        end_line: start_line + 2,
        end_column: 1,
        start_byte,
        end_byte: start_byte + 96,
        metadata_json: Some(r#"{"metric_version":1,"synthetic":true}"#.to_string()),
    }
}

fn writer_current_schema_parse_diagnostic(file_id: &str) -> ArtifactParseDiagnostic {
    ArtifactParseDiagnostic {
        diagnostic_id: format!("{file_id}-diagnostic-0"),
        kind: "error".to_string(),
        message: Some("synthetic recoverable parser diagnostic".to_string()),
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 1,
        start_byte: 0,
        end_byte: 1,
        metadata_json: None,
    }
}

fn write_json_summary<T: Serialize>(path: &Path, summary: &T) -> Result<(), PerformanceError> {
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
