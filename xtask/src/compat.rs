use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

const COMPAT_BASELINE_EXTRACTION_IDENTITY_EPOCH: u32 = 2;
const CURRENT_EXTRACTION_IDENTITY_EPOCH: u32 = 3;

const DEFAULT_FIXTURE: &str = "fixtures/extraction/resolution_contract";
const LEDGER_PATH: &str = "docs/contracts/extraction-output-changes.md";
const VERSION_MANIFEST: &str = "crates/julie-extract-cli/Cargo.toml";
pub const DEFAULT_MAX_DIFF_ROWS: usize = 20;
const FIELD_SEPARATOR: char = '\t';
const COLUMN_HEADER_PREFIX: &str = "#columns";

/// `artifact_metadata`, `extraction_revisions` and `revision_file_changes` carry per-scan identity
/// and timestamps, so two runs of the SAME binary already differ there.
/// `identifier_resolutions` and `pending_resolutions` were removed in schema v7. The previous
/// release still writes them; excluding them keeps the gate on fact-table identity and records
/// the overlay removal as an intentional break in `docs/contracts/extraction-output-changes.md`.
const EXCLUDED_TABLES: &[&str] = &[
    "artifact_metadata",
    "extraction_revisions",
    "revision_file_changes",
    "identifier_resolutions",
    "pending_resolutions",
    "language_capability_gaps",
];

const VOLATILE_COLUMNS: &[(&str, &[&str])] = &[("files", &["indexed_at", "last_revision_id"])];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatOutcome {
    Pass,
    Notice,
    Fail,
}

impl CompatOutcome {
    pub fn exit_code(self) -> u8 {
        match self {
            CompatOutcome::Pass | CompatOutcome::Notice => 0,
            CompatOutcome::Fail => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDump {
    pub table: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactDump {
    pub tables: Vec<TableDump>,
}

impl ArtifactDump {
    pub fn find(&self, table: &str) -> Option<&str> {
        self.tables
            .iter()
            .find(|dump| dump.table == table)
            .map(|dump| dump.text.as_str())
    }

    pub fn table_names(&self) -> impl Iterator<Item = &str> {
        self.tables.iter().map(|dump| dump.table.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineDifference {
    pub line: usize,
    pub previous: Option<String>,
    pub current: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableDifference {
    OnlyInPrevious {
        table: String,
    },
    OnlyInCurrent {
        table: String,
    },
    RowsDiffer {
        table: String,
        previous_lines: usize,
        current_lines: usize,
        first_differences: Vec<LineDifference>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactDiff {
    pub differences: Vec<TableDifference>,
}

impl ArtifactDiff {
    pub fn is_identical(&self) -> bool {
        self.differences.is_empty()
    }
}

/// The ledger documents exactly these two `classification:` values, and requires the line. A
/// section carrying neither has not classified the change, so it cannot authorize the NOTICE path.
const DECLARED_CLASSIFICATIONS: &[&str] = &["compatible", "incompatible"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub version: String,
    pub classification: Option<String>,
}

impl LedgerEntry {
    pub fn authorizes_notice(&self) -> bool {
        self.classification.as_deref().is_some_and(|value| {
            DECLARED_CLASSIFICATIONS
                .iter()
                .any(|declared| value.eq_ignore_ascii_case(declared))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatPlan {
    pub previous_binary: PathBuf,
    pub current_binary: Option<PathBuf>,
    pub fixture: PathBuf,
    pub out_dir: Option<PathBuf>,
    pub max_diff_rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatReport {
    pub outcome: CompatOutcome,
    pub version: String,
    pub previous_binary_version: String,
    pub previous_extraction_identity_epoch: u32,
    pub current_extraction_identity_epoch: u32,
    pub previous_binary: PathBuf,
    pub current_binary: PathBuf,
    pub fixture: PathBuf,
    pub tables_compared: usize,
    pub diff: ArtifactDiff,
    pub declaration: Option<LedgerEntry>,
}

#[derive(Debug)]
pub enum CompatError {
    Usage(String),
    Io {
        context: String,
        source: std::io::Error,
    },
    Sqlite {
        context: String,
        source: rusqlite::Error,
    },
    CommandFailed {
        command: String,
        code: Option<i32>,
        stderr: String,
    },
    MissingInput(String),
}

impl CompatError {
    /// Exit code 1 is reserved for the gate verdict alone, so every harness or environment
    /// problem reports 2 and can never be mistaken for "the extractor changed its output".
    pub fn exit_code(&self) -> u8 {
        match self {
            CompatError::Usage(_)
            | CompatError::Io { .. }
            | CompatError::Sqlite { .. }
            | CompatError::CommandFailed { .. }
            | CompatError::MissingInput(_) => 2,
        }
    }
}

impl std::fmt::Display for CompatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompatError::Usage(message) => f.write_str(message),
            CompatError::Io { context, source } => write!(f, "{context}: {source}"),
            CompatError::Sqlite { context, source } => write!(f, "{context}: {source}"),
            CompatError::CommandFailed {
                command,
                code,
                stderr,
            } => write!(
                f,
                "`{command}` failed with exit code {code:?}: {}",
                stderr.trim()
            ),
            CompatError::MissingInput(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for CompatError {}

pub fn decide(diff: &ArtifactDiff, declared: bool) -> CompatOutcome {
    if diff.is_identical() {
        CompatOutcome::Pass
    } else if declared {
        CompatOutcome::Notice
    } else {
        CompatOutcome::Fail
    }
}

pub fn verdict(diff: &ArtifactDiff, declaration: Option<&LedgerEntry>) -> CompatOutcome {
    decide(
        diff,
        declaration.is_some_and(LedgerEntry::authorizes_notice),
    )
}

pub fn verdict_for_epochs(
    diff: &ArtifactDiff,
    previous_epoch: u32,
    current_epoch: u32,
    declaration: Option<&LedgerEntry>,
) -> CompatOutcome {
    if diff.is_identical() {
        CompatOutcome::Pass
    } else if current_epoch > previous_epoch
        && declaration.is_some_and(LedgerEntry::authorizes_notice)
    {
        CompatOutcome::Notice
    } else {
        CompatOutcome::Fail
    }
}

pub fn fail_reason(version: &str, declaration: Option<&LedgerEntry>) -> String {
    let documented = DECLARED_CLASSIFICATIONS
        .iter()
        .map(|value| format!("`{value}`"))
        .collect::<Vec<_>>()
        .join(", ");
    match declaration {
        None => format!("version {version} has no `## {version}` entry in {LEDGER_PATH}"),
        Some(LedgerEntry {
            classification: None,
            ..
        }) => format!(
            "the `## {version}` entry in {LEDGER_PATH} has no `classification:` line; the ledger requires one of {documented}"
        ),
        Some(LedgerEntry {
            classification: Some(value),
            ..
        }) => format!(
            "the `## {version}` entry in {LEDGER_PATH} declares `classification: {value}`, which is not one of {documented}"
        ),
    }
}

pub fn diff_dumps(
    previous: &ArtifactDump,
    current: &ArtifactDump,
    max_differences: usize,
) -> ArtifactDiff {
    let tables = previous
        .table_names()
        .chain(current.table_names())
        .collect::<BTreeSet<_>>();

    let mut differences = Vec::new();
    for table in tables {
        match (previous.find(table), current.find(table)) {
            (Some(_), None) => differences.push(TableDifference::OnlyInPrevious {
                table: table.to_string(),
            }),
            (None, Some(_)) => differences.push(TableDifference::OnlyInCurrent {
                table: table.to_string(),
            }),
            (Some(previous_text), Some(current_text)) if previous_text != current_text => {
                differences.push(diff_table(
                    table,
                    previous_text,
                    current_text,
                    max_differences,
                ));
            }
            _ => {}
        }
    }

    ArtifactDiff { differences }
}

fn diff_table(
    table: &str,
    previous_text: &str,
    current_text: &str,
    max_differences: usize,
) -> TableDifference {
    let previous_lines = previous_text.lines().collect::<Vec<_>>();
    let current_lines = current_text.lines().collect::<Vec<_>>();

    let mut first_differences = Vec::new();
    for index in 0..previous_lines.len().max(current_lines.len()) {
        if first_differences.len() >= max_differences {
            break;
        }
        let previous = previous_lines.get(index).copied();
        let current = current_lines.get(index).copied();
        if previous != current {
            first_differences.push(LineDifference {
                line: index + 1,
                previous: previous.map(str::to_string),
                current: current.map(str::to_string),
            });
        }
    }

    TableDifference::RowsDiffer {
        table: table.to_string(),
        previous_lines: previous_lines.len(),
        current_lines: current_lines.len(),
        first_differences,
    }
}

pub fn find_ledger_entry(markdown: &str, version: &str) -> Option<LedgerEntry> {
    let mut lines = markdown.lines();
    let mut fenced = false;

    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let Some(heading) = line.trim().strip_prefix("## ") else {
            continue;
        };
        if !heading_declares(heading, version) {
            continue;
        }

        let mut classification = None;
        for entry_line in lines.by_ref() {
            let entry_line = entry_line.trim();
            if entry_line.starts_with("## ") {
                break;
            }
            if let Some(value) = entry_line.strip_prefix("classification:") {
                classification = Some(value.trim().to_string());
                break;
            }
        }

        return Some(LedgerEntry {
            version: version.to_string(),
            classification,
        });
    }

    None
}

pub fn default_fixture() -> PathBuf {
    repo_root().join(DEFAULT_FIXTURE)
}

pub fn current_build_version() -> Result<String, CompatError> {
    manifest_version(&repo_root().join(VERSION_MANIFEST))
}

pub fn declared_change_for_current_build() -> Result<Option<LedgerEntry>, CompatError> {
    let version = current_build_version()?;
    let ledger = read_ledger(&repo_root().join(LEDGER_PATH))?;
    Ok(find_ledger_entry(&ledger, &version))
}

fn heading_declares(heading: &str, version: &str) -> bool {
    let Some(token) = heading.split_whitespace().next() else {
        return false;
    };
    let token = token.trim_matches('`');
    token == version || token.strip_prefix('v') == Some(version)
}

pub fn plan_from_args(args: &[String]) -> Result<CompatPlan, CompatError> {
    let mut previous_binary = None;
    let mut current_binary = None;
    let mut fixture = None;
    let mut out_dir = None;
    let mut max_diff_rows = DEFAULT_MAX_DIFF_ROWS;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--previous-binary" => {
                index += 1;
                previous_binary = Some(required_path(args, index, "--previous-binary")?);
            }
            "--current-binary" => {
                index += 1;
                current_binary = Some(required_path(args, index, "--current-binary")?);
            }
            "--fixture" => {
                index += 1;
                fixture = Some(required_path(args, index, "--fixture")?);
            }
            "--out-dir" => {
                index += 1;
                out_dir = Some(required_path(args, index, "--out-dir")?);
            }
            "--max-diff-rows" => {
                index += 1;
                max_diff_rows = required_value(args, index, "--max-diff-rows")?
                    .parse()
                    .map_err(|_| {
                        CompatError::Usage("--max-diff-rows expects a number".to_string())
                    })?;
            }
            other => {
                return Err(CompatError::Usage(format!(
                    "unexpected compat-check argument `{other}`"
                )));
            }
        }
        index += 1;
    }

    let previous_binary = previous_binary.ok_or_else(|| {
        CompatError::Usage(
            "usage: cargo xtask compat-check --previous-binary <path> [--current-binary <path>] [--fixture <path>] [--out-dir <path>] [--max-diff-rows <n>]; missing --previous-binary"
                .to_string(),
        )
    })?;

    Ok(CompatPlan {
        previous_binary,
        current_binary,
        fixture: fixture.unwrap_or_else(default_fixture),
        out_dir,
        max_diff_rows,
    })
}

pub fn run_from_args(args: &[String]) -> ExitCode {
    match plan_from_args(args).and_then(run) {
        Ok(report) => {
            print_report(&report);
            ExitCode::from(report.outcome.exit_code())
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(error.exit_code())
        }
    }
}

pub fn run(plan: CompatPlan) -> Result<CompatReport, CompatError> {
    let repo_root = repo_root();
    let version = current_build_version()?;

    let previous_binary = require_existing_binary(&plan.previous_binary)?;
    let current_binary = match &plan.current_binary {
        Some(path) => require_existing_binary(path)?,
        None => build_current_binary(&repo_root)?,
    };

    let fixture = fs::canonicalize(&plan.fixture).map_err(|source| CompatError::Io {
        context: format!("failed to resolve fixture {}", plan.fixture.display()),
        source,
    })?;

    let out_dir = plan
        .out_dir
        .clone()
        .unwrap_or_else(|| repo_root.join("target").join("compat-check"));
    fs::create_dir_all(&out_dir).map_err(|source| CompatError::Io {
        context: format!("failed to create output dir {}", out_dir.display()),
        source,
    })?;

    let previous_db = clean_artifact_path(&out_dir, "previous")?;
    let current_db = clean_artifact_path(&out_dir, "current")?;

    scan_fixture(&previous_binary, &fixture, &previous_db)?;
    scan_fixture(&current_binary, &fixture, &current_db)?;

    let previous_dump = dump_artifact(&previous_db)?;
    let current_dump = dump_artifact(&current_db)?;
    write_dump(&out_dir.join("previous.dump.txt"), &previous_dump)?;
    write_dump(&out_dir.join("current.dump.txt"), &current_dump)?;

    let diff = diff_dumps(&previous_dump, &current_dump, plan.max_diff_rows);
    let declaration = declared_change_for_current_build()?;

    Ok(CompatReport {
        outcome: verdict_for_epochs(
            &diff,
            COMPAT_BASELINE_EXTRACTION_IDENTITY_EPOCH,
            CURRENT_EXTRACTION_IDENTITY_EPOCH,
            declaration.as_ref(),
        ),
        version,
        previous_binary_version: binary_version(&previous_binary)?,
        previous_extraction_identity_epoch: COMPAT_BASELINE_EXTRACTION_IDENTITY_EPOCH,
        current_extraction_identity_epoch: CURRENT_EXTRACTION_IDENTITY_EPOCH,
        previous_binary,
        current_binary,
        fixture,
        tables_compared: previous_dump
            .table_names()
            .chain(current_dump.table_names())
            .collect::<BTreeSet<_>>()
            .len(),
        diff,
        declaration,
    })
}

fn print_report(report: &CompatReport) {
    match report.outcome {
        CompatOutcome::Pass => {
            println!(
                "compat-check ok: {} extraction tables byte-identical between {} ({}) and {} ({})",
                report.tables_compared,
                report.previous_binary_version,
                report.previous_binary.display(),
                report.version,
                report.current_binary.display(),
            );
        }
        CompatOutcome::Notice => {
            println!("{}", render_diff(report));
            let classification = report
                .declaration
                .as_ref()
                .and_then(|entry| entry.classification.clone())
                .unwrap_or_else(|| "unclassified".to_string());
            println!(
                "NOTICE: extraction output differs from {} at extraction identity epoch {} and is declared in {LEDGER_PATH} under `## {}` with epoch {} (classification: {classification})",
                report.previous_binary_version,
                report.previous_extraction_identity_epoch,
                report.version,
                report.current_extraction_identity_epoch,
            );
        }
        CompatOutcome::Fail => {
            eprintln!("{}", render_diff(report));
            eprintln!(
                "compat-check failed: extraction output differs from {} and {}",
                report.previous_binary_version,
                report_fail_reason(report),
            );
        }
    }
}

fn render_diff(report: &CompatReport) -> String {
    let mut rendered = format!(
        "extraction output differs on {}\n  previous: {} ({}) epoch {}\n  current:  {} ({}) epoch {}\n",
        report.fixture.display(),
        report.previous_binary_version,
        report.previous_binary.display(),
        report.previous_extraction_identity_epoch,
        report.version,
        report.current_binary.display(),
        report.current_extraction_identity_epoch,
    );

    for difference in &report.diff.differences {
        match difference {
            TableDifference::OnlyInPrevious { table } => {
                let _ = writeln!(
                    rendered,
                    "\ntable `{table}`: present only in the previous build"
                );
            }
            TableDifference::OnlyInCurrent { table } => {
                let _ = writeln!(
                    rendered,
                    "\ntable `{table}`: present only in the current build"
                );
            }
            TableDifference::RowsDiffer {
                table,
                previous_lines,
                current_lines,
                first_differences,
            } => {
                let _ = writeln!(
                    rendered,
                    "\ntable `{table}`: {previous_lines} dump lines previous, {current_lines} current"
                );
                for difference in first_differences {
                    let _ = writeln!(rendered, "  line {}", difference.line);
                    let _ = writeln!(
                        rendered,
                        "    - {}",
                        difference.previous.as_deref().unwrap_or("<absent>")
                    );
                    let _ = writeln!(
                        rendered,
                        "    + {}",
                        difference.current.as_deref().unwrap_or("<absent>")
                    );
                }
            }
        }
    }

    rendered
}

fn report_fail_reason(report: &CompatReport) -> String {
    if !report.diff.is_identical()
        && report.current_extraction_identity_epoch <= report.previous_extraction_identity_epoch
    {
        format!(
            "the extraction identity epoch remained {}; any extraction difference requires a strictly newer epoch and a classified ledger entry",
            report.current_extraction_identity_epoch
        )
    } else {
        fail_reason(&report.version, report.declaration.as_ref())
    }
}

pub fn dump_artifact(db_path: &Path) -> Result<ArtifactDump, CompatError> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(
        |source| CompatError::Sqlite {
            context: format!("failed to open artifact {}", db_path.display()),
            source,
        },
    )?;
    dump_connection(&conn)
}

pub fn dump_connection(conn: &Connection) -> Result<ArtifactDump, CompatError> {
    let mut tables = Vec::new();
    for table in comparable_tables(conn)? {
        let text = dump_table(conn, &table)?;
        tables.push(TableDump { table, text });
    }
    Ok(ArtifactDump { tables })
}

fn comparable_tables(conn: &Connection) -> Result<Vec<String>, CompatError> {
    let mut statement = conn
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .map_err(|source| CompatError::Sqlite {
            context: "failed to enumerate artifact tables".to_string(),
            source,
        })?;

    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|source| CompatError::Sqlite {
            context: "failed to enumerate artifact tables".to_string(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| CompatError::Sqlite {
            context: "failed to enumerate artifact tables".to_string(),
            source,
        })?;

    Ok(names
        .into_iter()
        .filter(|name| !EXCLUDED_TABLES.contains(&name.as_str()))
        .collect())
}

struct ColumnInfo {
    name: String,
    primary_key_position: i64,
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<ColumnInfo>, CompatError> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({})", quote_identifier(table)))
        .map_err(|source| CompatError::Sqlite {
            context: format!("failed to read columns of `{table}`"),
            source,
        })?;

    statement
        .query_map([], |row| {
            Ok(ColumnInfo {
                name: row.get::<_, String>(1)?,
                primary_key_position: row.get::<_, i64>(5)?,
            })
        })
        .and_then(Iterator::collect)
        .map_err(|source| CompatError::Sqlite {
            context: format!("failed to read columns of `{table}`"),
            source,
        })
}

fn dump_table(conn: &Connection, table: &str) -> Result<String, CompatError> {
    let volatile = VOLATILE_COLUMNS
        .iter()
        .find_map(|(name, columns)| (*name == table).then_some(*columns))
        .unwrap_or(&[]);

    let projected = table_columns(conn, table)?
        .into_iter()
        .filter(|column| !volatile.contains(&column.name.as_str()))
        .collect::<Vec<_>>();
    if projected.is_empty() {
        return Ok(String::new());
    }

    let mut key = projected
        .iter()
        .filter(|column| column.primary_key_position > 0)
        .collect::<Vec<_>>();
    key.sort_by_key(|column| column.primary_key_position);
    let order = if key.is_empty() {
        projected.iter().collect::<Vec<_>>()
    } else {
        key
    };

    let sql = format!(
        "SELECT {} FROM {} ORDER BY {}",
        identifier_list(projected.iter()),
        quote_identifier(table),
        identifier_list(order.into_iter()),
    );

    let mut statement = conn.prepare(&sql).map_err(|source| CompatError::Sqlite {
        context: format!("failed to prepare dump of `{table}`"),
        source,
    })?;

    let mut dumped = String::new();
    dumped.push_str(COLUMN_HEADER_PREFIX);
    for column in &projected {
        dumped.push(FIELD_SEPARATOR);
        dumped.push_str(&column.name);
    }
    dumped.push('\n');

    let mut rows = statement.query([]).map_err(|source| CompatError::Sqlite {
        context: format!("failed to dump `{table}`"),
        source,
    })?;
    while let Some(row) = rows.next().map_err(|source| CompatError::Sqlite {
        context: format!("failed to dump `{table}`"),
        source,
    })? {
        for index in 0..projected.len() {
            if index > 0 {
                dumped.push(FIELD_SEPARATOR);
            }
            let value = row.get_ref(index).map_err(|source| CompatError::Sqlite {
                context: format!("failed to read column {index} of `{table}`"),
                source,
            })?;
            dumped.push_str(&render_value(value));
        }
        dumped.push('\n');
    }

    Ok(dumped)
}

fn render_value(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => "null:".to_string(),
        ValueRef::Integer(number) => format!("int:{number}"),
        ValueRef::Real(number) => format!("real:{number:?}"),
        ValueRef::Text(bytes) => format!("text:{}", escape(&String::from_utf8_lossy(bytes))),
        ValueRef::Blob(bytes) => {
            let mut hex = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                let _ = write!(hex, "{byte:02x}");
            }
            format!("blob:{hex}")
        }
    }
}

fn escape(value: &str) -> String {
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

fn identifier_list<'a>(columns: impl Iterator<Item = &'a ColumnInfo>) -> String {
    columns
        .map(|column| quote_identifier(&column.name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn scan_fixture(binary: &Path, fixture: &Path, db_path: &Path) -> Result<(), CompatError> {
    let output = Command::new(binary)
        .args([
            "scan",
            "--root",
            path_str(fixture)?,
            "--db",
            path_str(db_path)?,
            "--jobs",
            "1",
            "--json",
        ])
        .output()
        .map_err(|source| CompatError::Io {
            context: format!("failed to run {}", binary.display()),
            source,
        })?;

    if output.status.success() {
        return Ok(());
    }
    Err(command_failed(
        &format!("{} scan", binary.display()),
        output,
    ))
}

fn build_current_binary(repo_root: &Path) -> Result<PathBuf, CompatError> {
    let output = Command::new("cargo")
        .args([
            "build",
            "--release",
            "-p",
            "julie-extract-cli",
            "--bin",
            "julie-extract",
        ])
        .current_dir(repo_root)
        .output()
        .map_err(|source| CompatError::Io {
            context: "failed to run cargo build for julie-extract".to_string(),
            source,
        })?;
    if !output.status.success() {
        return Err(command_failed(
            "cargo build --release -p julie-extract-cli --bin julie-extract",
            output,
        ));
    }

    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root.join("target"));
    require_existing_binary(&target_dir.join("release").join(binary_name()))
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "julie-extract.exe"
    } else {
        "julie-extract"
    }
}

fn binary_version(binary: &Path) -> Result<String, CompatError> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|source| CompatError::Io {
            context: format!("failed to run {} --version", binary.display()),
            source,
        })?;
    if !output.status.success() {
        return Err(command_failed(
            &format!("{} --version", binary.display()),
            output,
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn require_existing_binary(path: &Path) -> Result<PathBuf, CompatError> {
    if !path.is_file() {
        return Err(CompatError::MissingInput(format!(
            "julie-extract binary not found at {}",
            path.display()
        )));
    }
    fs::canonicalize(path).map_err(|source| CompatError::Io {
        context: format!("failed to resolve binary {}", path.display()),
        source,
    })
}

fn clean_artifact_path(out_dir: &Path, stem: &str) -> Result<PathBuf, CompatError> {
    let db_path = out_dir.join(format!("{stem}.sqlite"));
    for path in [
        db_path.clone(),
        out_dir.join(format!("{stem}.sqlite-wal")),
        out_dir.join(format!("{stem}.sqlite-shm")),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(CompatError::Io {
                    context: format!("failed to remove {}", path.display()),
                    source,
                });
            }
        }
    }
    Ok(db_path)
}

fn write_dump(path: &Path, dump: &ArtifactDump) -> Result<(), CompatError> {
    let mut rendered = String::new();
    for table in &dump.tables {
        let _ = writeln!(rendered, "#table\t{}", table.table);
        rendered.push_str(&table.text);
    }
    fs::write(path, rendered).map_err(|source| CompatError::Io {
        context: format!("failed to write {}", path.display()),
        source,
    })
}

fn read_ledger(path: &Path) -> Result<String, CompatError> {
    fs::read_to_string(path).map_err(|source| CompatError::Io {
        context: format!("failed to read declared-changes ledger {}", path.display()),
        source,
    })
}

fn manifest_version(path: &Path) -> Result<String, CompatError> {
    let contents = fs::read_to_string(path).map_err(|source| CompatError::Io {
        context: format!("failed to read Cargo manifest {}", path.display()),
        source,
    })?;
    contents
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("version = ")
                .map(|value| value.trim().trim_matches('"').to_string())
        })
        .ok_or_else(|| {
            CompatError::MissingInput(format!("no version in Cargo manifest {}", path.display()))
        })
}

fn command_failed(command: &str, output: Output) -> CompatError {
    CompatError::CommandFailed {
        command: command.to_string(),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

fn path_str(path: &Path) -> Result<&str, CompatError> {
    path.to_str().ok_or_else(|| {
        CompatError::MissingInput(format!("path is not valid UTF-8: {}", path.display()))
    })
}

fn required_value<'a>(
    args: &'a [String],
    index: usize,
    flag: &str,
) -> Result<&'a str, CompatError> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| CompatError::Usage(format!("missing value for {flag}")))
}

fn required_path(args: &[String], index: usize, flag: &str) -> Result<PathBuf, CompatError> {
    required_value(args, index, flag).map(PathBuf::from)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under repo root")
        .to_path_buf()
}

#[cfg(test)]
mod epoch_policy_tests {
    use super::{ArtifactDiff, CompatOutcome, LedgerEntry, TableDifference, verdict_for_epochs};

    fn difference() -> ArtifactDiff {
        ArtifactDiff {
            differences: vec![TableDifference::OnlyInCurrent {
                table: "symbols".to_string(),
            }],
        }
    }

    fn declaration() -> LedgerEntry {
        LedgerEntry {
            version: "2.30.0".to_string(),
            classification: Some("compatible".to_string()),
        }
    }

    #[test]
    fn same_epoch_difference_fails_even_when_classified() {
        assert_eq!(
            verdict_for_epochs(&difference(), 1, 1, Some(&declaration())),
            CompatOutcome::Fail
        );
    }

    #[test]
    fn epoch_bump_requires_a_classified_difference() {
        assert_eq!(
            verdict_for_epochs(&difference(), 1, 2, None),
            CompatOutcome::Fail
        );
        assert_eq!(
            verdict_for_epochs(&difference(), 1, 2, Some(&declaration())),
            CompatOutcome::Notice
        );
    }

    #[test]
    fn byte_identical_output_passes_without_an_epoch_bump() {
        assert_eq!(
            verdict_for_epochs(&ArtifactDiff::default(), 1, 1, None),
            CompatOutcome::Pass
        );
    }
}
