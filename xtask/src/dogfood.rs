use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output};
use std::time::{Duration, Instant};

use julie_extract_artifact::jsonl::JSONL_RECORD_KINDS;
use julie_extract_artifact::schema::SQLITE_SCHEMA_VERSION;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

const REPORT_SCHEMA_VERSION: i64 = 3;
const EXTRACT_CONTRACT_VERSION: i64 = 4;
const JSONL_SCHEMA_VERSION: i64 = 5;
const REQUIRED_METADATA_KEYS: &[&str] = &[
    "artifact_id",
    "root_path",
    "schema_version",
    "extract_contract_version",
    "sqlite_schema_version",
    "binary_version",
    "hash_algorithm",
    "parser_inventory_fingerprint",
    "capability_snapshot_fingerprint",
    "created_at",
    "updated_at",
];
const ROW_DOMAIN_TABLES: &[&str] = &[
    "artifact_metadata",
    "parser_inventory",
    "language_capabilities",
    "language_capability_fixtures",
    "language_capability_gaps",
    "extraction_revisions",
    "revision_file_changes",
    "files",
    "symbols",
    "symbol_annotations",
    "identifiers",
    "relationships",
    "pending_relationships",
    "type_facts",
    "type_argument_usages",
    "type_arguments",
    "literals",
    "source_regions",
    "complexity_metrics",
    "structural_facts",
    "parse_diagnostics",
];
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DogfoodPlan {
    pub root: PathBuf,
    pub out_dir: PathBuf,
    pub binary: PathBuf,
    pub build_default_binary: bool,
    pub paths: DogfoodOutputPaths,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DogfoodOutputPaths {
    pub db_path: PathBuf,
    pub jsonl_path: PathBuf,
    pub scan_report_path: PathBuf,
    pub rescan_report_path: PathBuf,
    pub info_report_path: PathBuf,
    pub export_report_path: PathBuf,
    pub metrics_path: PathBuf,
}

impl DogfoodOutputPaths {
    pub fn new(out_dir: &Path) -> Self {
        Self {
            db_path: out_dir.join("artifact.sqlite"),
            jsonl_path: out_dir.join("artifact.jsonl"),
            scan_report_path: out_dir.join("scan-report.json"),
            rescan_report_path: out_dir.join("rescan-report.json"),
            info_report_path: out_dir.join("info-report.json"),
            export_report_path: out_dir.join("export-report.json"),
            metrics_path: out_dir.join("metrics.json"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommandDurations {
    pub scan: Duration,
    pub rescan: Duration,
    pub info: Duration,
    pub export: Duration,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DogfoodMetrics {
    pub sqlite_schema_version: i64,
    pub extract_contract_version: i64,
    pub jsonl_schema_version: i64,
    pub root_path: String,
    pub files: i64,
    pub symbols: i64,
    pub row_totals: BTreeMap<String, i64>,
    pub jsonl_records_by_kind: BTreeMap<String, usize>,
    pub jsonl_records: usize,
    pub sqlite_bytes: u64,
    pub jsonl_bytes: u64,
    pub scan_duration_ms: u128,
    pub rescan_duration_ms: u128,
    pub rescan_files_unchanged: i64,
    pub rescan_files_changed: i64,
    pub rescan_files_deleted: i64,
    pub rescan_files_failed: i64,
    pub info_duration_ms: u128,
    pub export_duration_ms: u128,
    pub rows_per_second: Option<f64>,
}

#[derive(Debug)]
pub enum DogfoodError {
    Usage(String),
    Io {
        context: String,
        source: std::io::Error,
    },
    Sqlite {
        context: String,
        source: rusqlite::Error,
    },
    Json {
        context: String,
        source: serde_json::Error,
    },
    CommandFailed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },
    InvalidEvidence(String),
}

impl DogfoodError {
    fn exit_code(&self) -> u8 {
        match self {
            DogfoodError::Usage(_) => 2,
            DogfoodError::Io { .. }
            | DogfoodError::Sqlite { .. }
            | DogfoodError::Json { .. }
            | DogfoodError::CommandFailed { .. }
            | DogfoodError::InvalidEvidence(_) => 1,
        }
    }
}

impl std::fmt::Display for DogfoodError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DogfoodError::Usage(message) => f.write_str(message),
            DogfoodError::Io { context, source } => write!(f, "{context}: {source}"),
            DogfoodError::Sqlite { context, source } => write!(f, "{context}: {source}"),
            DogfoodError::Json { context, source } => write!(f, "{context}: {source}"),
            DogfoodError::CommandFailed {
                command,
                code,
                stderr,
            } => write!(
                f,
                "`{command}` failed with exit code {:?}: {}",
                code,
                stderr.trim()
            ),
            DogfoodError::InvalidEvidence(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for DogfoodError {}

pub fn plan_repo_from_args<I, S>(args: I) -> Result<DogfoodPlan, DogfoodError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();

    if args.first().map(String::as_str) != Some("repo") {
        return Err(DogfoodError::Usage(
            "usage: cargo xtask dogfood repo --root <path> --out-dir <path> [--binary <path>]; expected `repo`".to_string(),
        ));
    }

    let mut root = None;
    let mut out_dir = None;
    let mut binary = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--root" => {
                index += 1;
                root = Some(required_value(&args, index, "--root")?);
            }
            "--out-dir" => {
                index += 1;
                out_dir = Some(required_value(&args, index, "--out-dir")?);
            }
            "--binary" => {
                index += 1;
                binary = Some(required_value(&args, index, "--binary")?);
            }
            other => {
                return Err(DogfoodError::Usage(format!(
                    "unexpected dogfood argument `{other}`"
                )));
            }
        }
        index += 1;
    }

    let root = root.ok_or_else(|| DogfoodError::Usage("missing --root".to_string()))?;
    let out_dir = out_dir.ok_or_else(|| DogfoodError::Usage("missing --out-dir".to_string()))?;
    let (binary, build_default_binary) = match binary {
        Some(binary) => (binary, false),
        None => (default_binary_path(), true),
    };
    let paths = DogfoodOutputPaths::new(&out_dir);

    Ok(DogfoodPlan {
        root,
        out_dir,
        binary,
        build_default_binary,
        paths,
    })
}

pub fn run_from_args(args: &[String]) -> ExitCode {
    match run_repo_from_args(args) {
        Ok(metrics) => {
            match serde_json::to_string_pretty(&metrics) {
                Ok(json) => println!("{json}"),
                Err(error) => {
                    eprintln!("failed to render dogfood metrics: {error}");
                    return ExitCode::from(1);
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(error.exit_code())
        }
    }
}

pub fn run_repo_from_args(args: &[String]) -> Result<DogfoodMetrics, DogfoodError> {
    let plan = plan_repo_from_args(args)?;
    run_repo(plan)
}

pub fn run_repo(plan: DogfoodPlan) -> Result<DogfoodMetrics, DogfoodError> {
    fs::create_dir_all(&plan.out_dir).map_err(|source| DogfoodError::Io {
        context: format!(
            "failed to create dogfood output dir {}",
            plan.out_dir.display()
        ),
        source,
    })?;
    clear_outputs(&plan.paths)?;

    let root = fs::canonicalize(&plan.root).map_err(|source| DogfoodError::Io {
        context: format!(
            "failed to canonicalize dogfood root {}",
            plan.root.display()
        ),
        source,
    })?;

    if plan.build_default_binary {
        let output = Command::new("cargo")
            .args(["build", "-p", "julie-extract-cli", "--bin", "julie-extract"])
            .current_dir(repo_root())
            .output()
            .map_err(|source| DogfoodError::Io {
                context: "failed to run cargo build for julie-extract".to_string(),
                source,
            })?;
        if !output.status.success() {
            return Err(command_failed(
                "cargo build -p julie-extract-cli --bin julie-extract",
                output,
            ));
        }
    }

    let (scan_duration, scan_output) = run_julie_extract(
        &plan.binary,
        [
            "scan",
            "--root",
            path_str(&root)?,
            "--db",
            path_str(&plan.paths.db_path)?,
            "--json",
        ],
    )?;
    write_command_stdout(&plan.paths.scan_report_path, &scan_output)?;
    ensure_success("julie-extract scan", scan_output)?;

    let (rescan_duration, rescan_output) = run_julie_extract(
        &plan.binary,
        [
            "scan",
            "--root",
            path_str(&root)?,
            "--db",
            path_str(&plan.paths.db_path)?,
            "--json",
        ],
    )?;
    write_command_stdout(&plan.paths.rescan_report_path, &rescan_output)?;
    ensure_success("julie-extract rescan", rescan_output)?;

    let (info_duration, info_output) = run_julie_extract(
        &plan.binary,
        ["info", "--db", path_str(&plan.paths.db_path)?, "--json"],
    )?;
    write_command_stdout(&plan.paths.info_report_path, &info_output)?;
    ensure_success("julie-extract info", info_output)?;

    let (export_duration, export_output) = run_julie_extract(
        &plan.binary,
        [
            "export",
            "--db",
            path_str(&plan.paths.db_path)?,
            "--format",
            "jsonl",
            "--out",
            path_str(&plan.paths.jsonl_path)?,
            "--json",
        ],
    )?;
    write_command_stdout(&plan.paths.export_report_path, &export_output)?;
    ensure_success("julie-extract export", export_output)?;

    let metrics = validate_outputs(
        &plan.paths,
        &root,
        CommandDurations {
            scan: scan_duration,
            rescan: rescan_duration,
            info: info_duration,
            export: export_duration,
        },
    )?;
    write_metrics(&plan.paths.metrics_path, &metrics)?;
    Ok(metrics)
}

pub fn validate_outputs(
    paths: &DogfoodOutputPaths,
    expected_root: &Path,
    durations: CommandDurations,
) -> Result<DogfoodMetrics, DogfoodError> {
    validate_report(&paths.scan_report_path, "scan", "incremental")?;
    let rescan_counts = validate_rescan_report(&paths.rescan_report_path)?;
    validate_report(&paths.info_report_path, "info", "read_only")?;
    validate_report(&paths.export_report_path, "export", "jsonl")?;

    let conn = Connection::open_with_flags(&paths.db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|source| DogfoodError::Sqlite {
            context: format!(
                "failed to open dogfood artifact {}",
                paths.db_path.display()
            ),
            source,
        })?;
    let metadata = read_metadata(&conn)?;
    for key in REQUIRED_METADATA_KEYS {
        let value = required_metadata(&metadata, key)?;
        if value.is_empty() {
            return Err(DogfoodError::InvalidEvidence(format!(
                "artifact metadata `{key}` is empty"
            )));
        }
    }
    let root_path = required_metadata(&metadata, "root_path")?;
    let expected_root = expected_root.display().to_string();
    if root_path != expected_root {
        return Err(DogfoodError::InvalidEvidence(format!(
            "artifact root path `{root_path}` did not match expected `{expected_root}`"
        )));
    }

    let sqlite_schema_version = metadata_i64(&metadata, "sqlite_schema_version")?;
    let extract_contract_version = metadata_i64(&metadata, "extract_contract_version")?;
    let schema_version = metadata_i64(&metadata, "schema_version")?;
    if schema_version != SQLITE_SCHEMA_VERSION || sqlite_schema_version != SQLITE_SCHEMA_VERSION {
        return Err(DogfoodError::InvalidEvidence(format!(
            "artifact schema version was schema={schema_version}, sqlite={sqlite_schema_version}; expected {SQLITE_SCHEMA_VERSION}"
        )));
    }
    if extract_contract_version != EXTRACT_CONTRACT_VERSION {
        return Err(DogfoodError::InvalidEvidence(format!(
            "artifact extract contract version was {extract_contract_version}; expected {EXTRACT_CONTRACT_VERSION}"
        )));
    }
    let hash_algorithm = required_metadata(&metadata, "hash_algorithm")?;
    if hash_algorithm != "blake3" {
        return Err(DogfoodError::InvalidEvidence(format!(
            "artifact hash algorithm was `{hash_algorithm}`; expected `blake3`"
        )));
    }

    let row_totals = row_totals(&conn)?;
    let files = table_count(&conn, "files")?;
    if files == 0 {
        return Err(DogfoodError::InvalidEvidence(
            "artifact contains zero files".to_string(),
        ));
    }
    let symbols = table_count(&conn, "symbols")?;
    if symbols == 0 {
        return Err(DogfoodError::InvalidEvidence(
            "artifact contains zero symbols".to_string(),
        ));
    }

    let jsonl_records_by_kind = validate_jsonl(&paths.jsonl_path)?;
    let jsonl_records = jsonl_records_by_kind.values().sum();
    let sqlite_bytes = file_len(&paths.db_path)?;
    let jsonl_bytes = file_len(&paths.jsonl_path)?;
    let rows_per_second = rows_per_second(files + symbols, durations.scan);

    Ok(DogfoodMetrics {
        sqlite_schema_version,
        extract_contract_version,
        jsonl_schema_version: JSONL_SCHEMA_VERSION,
        root_path,
        files,
        symbols,
        row_totals,
        jsonl_records_by_kind,
        jsonl_records,
        sqlite_bytes,
        jsonl_bytes,
        scan_duration_ms: durations.scan.as_millis(),
        rescan_duration_ms: durations.rescan.as_millis(),
        rescan_files_unchanged: rescan_counts.files_unchanged,
        rescan_files_changed: rescan_counts.files_changed,
        rescan_files_deleted: rescan_counts.files_deleted,
        rescan_files_failed: rescan_counts.files_failed,
        info_duration_ms: durations.info.as_millis(),
        export_duration_ms: durations.export.as_millis(),
        rows_per_second,
    })
}

fn required_value(args: &[String], index: usize, flag: &str) -> Result<PathBuf, DogfoodError> {
    args.get(index)
        .map(PathBuf::from)
        .ok_or_else(|| DogfoodError::Usage(format!("missing value for {flag}")))
}

fn default_binary_path() -> PathBuf {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"));
    target_dir.join("debug").join(debug_binary_name())
}

fn debug_binary_name() -> &'static str {
    if cfg!(windows) {
        "julie-extract.exe"
    } else {
        "julie-extract"
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under repo root")
        .to_path_buf()
}

fn clear_outputs(paths: &DogfoodOutputPaths) -> Result<(), DogfoodError> {
    for path in [
        &paths.db_path,
        &paths.db_path.with_extension("sqlite-wal"),
        &paths.db_path.with_extension("sqlite-shm"),
        &paths.jsonl_path,
        &paths.scan_report_path,
        &paths.rescan_report_path,
        &paths.info_report_path,
        &paths.export_report_path,
        &paths.metrics_path,
    ] {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(DogfoodError::Io {
                    context: format!("failed to remove stale dogfood output {}", path.display()),
                    source,
                });
            }
        }
    }
    Ok(())
}

fn run_julie_extract<'a, I>(binary: &Path, args: I) -> Result<(Duration, Output), DogfoodError>
where
    I: IntoIterator<Item = &'a str>,
{
    let args = args.into_iter().collect::<Vec<_>>();
    let started = Instant::now();
    let output = Command::new(binary)
        .args(&args)
        .output()
        .map_err(|source| DogfoodError::Io {
            context: format!("failed to run {} {}", binary.display(), args.join(" ")),
            source,
        })?;
    Ok((started.elapsed(), output))
}

fn write_command_stdout(path: &Path, output: &Output) -> Result<(), DogfoodError> {
    fs::write(path, &output.stdout).map_err(|source| DogfoodError::Io {
        context: format!("failed to write {}", path.display()),
        source,
    })
}

fn ensure_success(command: &str, output: Output) -> Result<(), DogfoodError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(DogfoodError::CommandFailed {
            command: command.to_string(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn command_failed(command: &str, output: Output) -> DogfoodError {
    DogfoodError::CommandFailed {
        command: command.to_string(),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn validate_report(
    path: &Path,
    expected_operation: &str,
    expected_mode: &str,
) -> Result<(), DogfoodError> {
    let value = read_json(path)?;
    let report_schema_version = value
        .get("report_schema_version")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            DogfoodError::InvalidEvidence(format!(
                "{} report is missing integer report_schema_version",
                expected_operation
            ))
        })?;
    if report_schema_version != REPORT_SCHEMA_VERSION {
        return Err(DogfoodError::InvalidEvidence(format!(
            "{} report schema version was {report_schema_version}; expected {REPORT_SCHEMA_VERSION}",
            expected_operation
        )));
    }
    let status = value.get("status").and_then(Value::as_str).ok_or_else(|| {
        DogfoodError::InvalidEvidence(format!(
            "{} report is missing string status",
            expected_operation
        ))
    })?;
    if status != "ok" {
        return Err(DogfoodError::InvalidEvidence(format!(
            "{} report status was `{status}`",
            expected_operation
        )));
    }

    let operation = value
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            DogfoodError::InvalidEvidence(format!(
                "{} report is missing string operation",
                expected_operation
            ))
        })?;
    if operation != expected_operation {
        return Err(DogfoodError::InvalidEvidence(format!(
            "{expected_operation} report operation was `{operation}`"
        )));
    }
    let mode = value.get("mode").and_then(Value::as_str).ok_or_else(|| {
        DogfoodError::InvalidEvidence(format!(
            "{} report is missing string mode",
            expected_operation
        ))
    })?;
    if mode != expected_mode {
        return Err(DogfoodError::InvalidEvidence(format!(
            "{expected_operation} report mode was `{mode}`; expected `{expected_mode}`"
        )));
    }
    let errors = value
        .get("errors")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            DogfoodError::InvalidEvidence(format!(
                "{} report is missing errors array",
                expected_operation
            ))
        })?;
    if !errors.is_empty() {
        return Err(DogfoodError::InvalidEvidence(format!(
            "{} report had nonempty errors array",
            expected_operation
        )));
    }

    let totals_files = value
        .pointer("/counts/totals/files")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let totals_symbols = value
        .pointer("/counts/totals/symbols")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if matches!(expected_operation, "info" | "export") && (totals_files == 0 || totals_symbols == 0)
    {
        return Err(DogfoodError::InvalidEvidence(format!(
            "{expected_operation} report totals must include files and symbols"
        )));
    }
    if expected_operation == "scan" {
        let files_scanned = value
            .pointer("/counts/files_scanned")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let rows_files = value
            .pointer("/counts/rows_written/files")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let rows_symbols = value
            .pointer("/counts/rows_written/symbols")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if files_scanned == 0 || rows_files == 0 || rows_symbols == 0 {
            return Err(DogfoodError::InvalidEvidence(
                "scan report must include scanned files and written file/symbol rows".to_string(),
            ));
        }
    }
    if expected_operation == "export" {
        let jsonl_schema_version = value
            .pointer("/artifact/jsonl_schema_version")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                DogfoodError::InvalidEvidence(
                    "export report is missing artifact.jsonl_schema_version".to_string(),
                )
            })?;
        if jsonl_schema_version != JSONL_SCHEMA_VERSION {
            return Err(DogfoodError::InvalidEvidence(format!(
                "export report JSONL schema version was {jsonl_schema_version}; expected {JSONL_SCHEMA_VERSION}"
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RescanCounts {
    files_unchanged: i64,
    files_changed: i64,
    files_deleted: i64,
    files_failed: i64,
}

fn validate_rescan_report(path: &Path) -> Result<RescanCounts, DogfoodError> {
    let value = read_json(path)?;
    let report_schema_version = value
        .get("report_schema_version")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            DogfoodError::InvalidEvidence(
                "rescan report is missing integer report_schema_version".to_string(),
            )
        })?;
    if report_schema_version != REPORT_SCHEMA_VERSION {
        return Err(DogfoodError::InvalidEvidence(format!(
            "rescan report schema version was {report_schema_version}; expected {REPORT_SCHEMA_VERSION}"
        )));
    }
    let status = value.get("status").and_then(Value::as_str).ok_or_else(|| {
        DogfoodError::InvalidEvidence("rescan report is missing string status".to_string())
    })?;
    if status != "no_change" {
        return Err(DogfoodError::InvalidEvidence(format!(
            "rescan report status was `{status}`; expected `no_change`"
        )));
    }
    let operation = value
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            DogfoodError::InvalidEvidence("rescan report is missing string operation".to_string())
        })?;
    if operation != "scan" {
        return Err(DogfoodError::InvalidEvidence(format!(
            "rescan report operation was `{operation}`; expected `scan`"
        )));
    }
    let mode = value.get("mode").and_then(Value::as_str).ok_or_else(|| {
        DogfoodError::InvalidEvidence("rescan report is missing string mode".to_string())
    })?;
    if mode != "incremental" {
        return Err(DogfoodError::InvalidEvidence(format!(
            "rescan report mode was `{mode}`; expected `incremental`"
        )));
    }
    let errors = value
        .get("errors")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            DogfoodError::InvalidEvidence("rescan report is missing errors array".to_string())
        })?;
    if !errors.is_empty() {
        return Err(DogfoodError::InvalidEvidence(
            "rescan report had nonempty errors array".to_string(),
        ));
    }
    match value.pointer("/revision/created_revision_id") {
        Some(Value::Null) => {}
        Some(_) => {
            return Err(DogfoodError::InvalidEvidence(
                "rescan report must not create a revision".to_string(),
            ));
        }
        None => {
            return Err(DogfoodError::InvalidEvidence(
                "rescan report is missing revision.created_revision_id".to_string(),
            ));
        }
    }
    validate_zero_rows_written(&value)?;

    let counts = RescanCounts {
        files_unchanged: report_count(&value, "/counts/files_unchanged", "files_unchanged")?,
        files_changed: report_count(&value, "/counts/files_changed", "files_changed")?,
        files_deleted: report_count(&value, "/counts/files_deleted", "files_deleted")?,
        files_failed: report_count(&value, "/counts/files_failed", "files_failed")?,
    };
    if counts.files_unchanged <= 0
        || counts.files_changed != 0
        || counts.files_deleted != 0
        || counts.files_failed != 0
    {
        return Err(DogfoodError::InvalidEvidence(
            "rescan report must include unchanged files and zero changed/deleted/failed files"
                .to_string(),
        ));
    }

    Ok(counts)
}

fn validate_zero_rows_written(value: &Value) -> Result<(), DogfoodError> {
    let rows_written = value
        .pointer("/counts/rows_written")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            DogfoodError::InvalidEvidence(
                "rescan report is missing counts.rows_written object".to_string(),
            )
        })?;
    if rows_written.is_empty() {
        return Err(DogfoodError::InvalidEvidence(
            "rescan report is missing row counts".to_string(),
        ));
    }
    for (table, count) in rows_written {
        let count = count.as_i64().ok_or_else(|| {
            DogfoodError::InvalidEvidence(format!(
                "rescan report row count `{table}` is not an integer"
            ))
        })?;
        if count != 0 {
            return Err(DogfoodError::InvalidEvidence(
                "rescan report must write zero rows".to_string(),
            ));
        }
    }
    Ok(())
}

fn report_count(
    value: &Value,
    pointer: &'static str,
    label: &'static str,
) -> Result<i64, DogfoodError> {
    value
        .pointer(pointer)
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            DogfoodError::InvalidEvidence(format!("rescan report is missing integer {label}"))
        })
}

fn read_json(path: &Path) -> Result<Value, DogfoodError> {
    let bytes = fs::read(path).map_err(|source| DogfoodError::Io {
        context: format!("failed to read {}", path.display()),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| DogfoodError::Json {
        context: format!("failed to parse JSON {}", path.display()),
        source,
    })
}

fn read_metadata(conn: &Connection) -> Result<BTreeMap<String, String>, DogfoodError> {
    let mut stmt = conn
        .prepare("SELECT key, value FROM artifact_metadata ORDER BY key")
        .map_err(|source| DogfoodError::Sqlite {
            context: "failed to prepare artifact metadata query".to_string(),
            source,
        })?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|source| DogfoodError::Sqlite {
            context: "failed to query artifact metadata".to_string(),
            source,
        })?;

    let mut metadata = BTreeMap::new();
    for row in rows {
        let (key, value) = row.map_err(|source| DogfoodError::Sqlite {
            context: "failed to read artifact metadata row".to_string(),
            source,
        })?;
        metadata.insert(key, value);
    }
    Ok(metadata)
}

fn required_metadata(
    metadata: &BTreeMap<String, String>,
    key: &'static str,
) -> Result<String, DogfoodError> {
    metadata
        .get(key)
        .cloned()
        .ok_or_else(|| DogfoodError::InvalidEvidence(format!("artifact metadata missing `{key}`")))
}

fn metadata_i64(
    metadata: &BTreeMap<String, String>,
    key: &'static str,
) -> Result<i64, DogfoodError> {
    let value = required_metadata(metadata, key)?;
    value.parse::<i64>().map_err(|_| {
        DogfoodError::InvalidEvidence(format!(
            "artifact metadata `{key}` value `{value}` is not an integer"
        ))
    })
}

fn table_count(conn: &Connection, table: &'static str) -> Result<i64, DogfoodError> {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get::<_, i64>(0)
    })
    .optional()
    .map_err(|source| DogfoodError::Sqlite {
        context: format!("failed to count table {table}"),
        source,
    })?
    .ok_or_else(|| DogfoodError::InvalidEvidence(format!("table {table} did not return a count")))
}

fn row_totals(conn: &Connection) -> Result<BTreeMap<String, i64>, DogfoodError> {
    let mut totals = BTreeMap::new();
    for table in ROW_DOMAIN_TABLES {
        totals.insert((*table).to_string(), table_count(conn, table)?);
    }
    Ok(totals)
}

fn validate_jsonl(path: &Path) -> Result<BTreeMap<String, usize>, DogfoodError> {
    let file = File::open(path).map_err(|source| DogfoodError::Io {
        context: format!("failed to read {}", path.display()),
        source,
    })?;
    let mut records_by_kind = BTreeMap::<String, usize>::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|source| DogfoodError::Io {
            context: format!(
                "failed to read JSONL record {} in {}",
                index + 1,
                path.display()
            ),
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str::<Value>(&line).map_err(|source| DogfoodError::Json {
            context: format!(
                "failed to parse JSONL record {} in {}",
                index + 1,
                path.display()
            ),
            source,
        })?;
        let schema = value
            .get("jsonl_schema_version")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                DogfoodError::InvalidEvidence(format!(
                    "JSONL record {} is missing integer jsonl_schema_version",
                    index + 1
                ))
            })?;
        if schema != JSONL_SCHEMA_VERSION {
            return Err(DogfoodError::InvalidEvidence(format!(
                "JSONL record {} schema version was {schema}; expected {JSONL_SCHEMA_VERSION}",
                index + 1
            )));
        }
        let extract_contract_version = value
            .get("extract_contract_version")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                DogfoodError::InvalidEvidence(format!(
                    "JSONL record {} is missing integer extract_contract_version",
                    index + 1
                ))
            })?;
        if extract_contract_version != EXTRACT_CONTRACT_VERSION {
            return Err(DogfoodError::InvalidEvidence(format!(
                "JSONL record {} extract contract version was {extract_contract_version}; expected {EXTRACT_CONTRACT_VERSION}",
                index + 1
            )));
        }
        let op = value.get("op").and_then(Value::as_str).ok_or_else(|| {
            DogfoodError::InvalidEvidence(format!(
                "JSONL record {} is missing string op",
                index + 1
            ))
        })?;
        if op != "snapshot" {
            return Err(DogfoodError::InvalidEvidence(format!(
                "JSONL record {} op was `{op}`; expected `snapshot`",
                index + 1
            )));
        }
        let kind = value.get("kind").and_then(Value::as_str).ok_or_else(|| {
            DogfoodError::InvalidEvidence(format!(
                "JSONL record {} is missing string kind",
                index + 1
            ))
        })?;
        if !JSONL_RECORD_KINDS.contains(&kind) {
            return Err(DogfoodError::InvalidEvidence(format!(
                "JSONL record {} has unsupported kind `{kind}`",
                index + 1
            )));
        }
        for field in ["artifact_id", "record_id"] {
            if value.get(field).and_then(Value::as_str).is_none() {
                return Err(DogfoodError::InvalidEvidence(format!(
                    "JSONL record {} is missing string {field}",
                    index + 1
                )));
            }
        }
        if !value.get("record").is_some_and(Value::is_object) {
            return Err(DogfoodError::InvalidEvidence(format!(
                "JSONL record {} is missing object record",
                index + 1
            )));
        }
        *records_by_kind.entry(kind.to_string()).or_insert(0) += 1;
    }
    if records_by_kind.values().sum::<usize>() == 0 {
        return Err(DogfoodError::InvalidEvidence(
            "JSONL export contains zero records".to_string(),
        ));
    }
    for required_kind in ["artifact", "file", "symbol"] {
        if !records_by_kind.contains_key(required_kind) {
            return Err(DogfoodError::InvalidEvidence(format!(
                "JSONL export contains zero `{required_kind}` records"
            )));
        }
    }
    Ok(records_by_kind)
}

fn file_len(path: &Path) -> Result<u64, DogfoodError> {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|source| DogfoodError::Io {
            context: format!("failed to stat {}", path.display()),
            source,
        })
}

fn rows_per_second(rows: i64, duration: Duration) -> Option<f64> {
    let seconds = duration.as_secs_f64();
    if rows <= 0 || seconds == 0.0 {
        None
    } else {
        Some(rows as f64 / seconds)
    }
}

fn write_metrics(path: &Path, metrics: &DogfoodMetrics) -> Result<(), DogfoodError> {
    let json = serde_json::to_vec_pretty(metrics).map_err(|source| DogfoodError::Json {
        context: "failed to serialize dogfood metrics".to_string(),
        source,
    })?;
    fs::write(path, json).map_err(|source| DogfoodError::Io {
        context: format!("failed to write {}", path.display()),
        source,
    })
}

fn path_str(path: &Path) -> Result<&str, DogfoodError> {
    path.to_str().ok_or_else(|| {
        DogfoodError::InvalidEvidence(format!("path is not valid UTF-8: {}", path.display()))
    })
}
