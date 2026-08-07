use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "julie-extract",
    version,
    about = "Create and inspect Julie extraction artifacts"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Scan(ScanArgs),
    Update(UpdateArgs),
    Delete(DeleteArgs),
    Info(InfoArgs),
    Export(ExportArgs),
    Languages(LanguagesArgs),
    Rebind(RebindArgs),
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    /// Source root directory to extract.
    #[arg(long)]
    pub root: PathBuf,
    /// SQLite artifact path to create or refresh.
    #[arg(long)]
    pub db: PathBuf,
    /// Re-extract every file, ignoring stored content hashes.
    #[arg(long)]
    pub force: bool,
    /// Extra gitignore-style ignore file. Its rules take precedence over
    /// .gitignore and .julieignore rules. Repeatable.
    #[arg(long = "ignore-file")]
    pub ignore_files: Vec<PathBuf>,
    /// Fail on an artifact whose schema version does not match this binary. Write
    /// commands refuse an older artifact regardless of this flag; the flag extends
    /// the refusal to read commands.
    #[arg(long)]
    pub strict_schema: bool,
    /// Emit the machine-readable JSON report on stdout.
    #[arg(long)]
    pub json: bool,
    /// Number of parallel extraction workers (0 = auto-detect from available cores).
    #[arg(long, short = 'j', default_value_t = 0)]
    pub jobs: usize,
    /// Directory to hold this scan's extraction spool file, created when missing.
    /// Also enables startup removal of spool files in it that no live scan owns.
    /// Absent = the system temporary directory, with no locking and no removal.
    #[arg(long)]
    pub spool_dir: Option<PathBuf>,
    /// Append live scan progress records to this JSONL file. The name must be
    /// `.progress` or end in `.progress`, ignoring case, because creating it
    /// truncates it. Absent = nothing is written and no progress work runs.
    #[arg(long)]
    pub progress_file: Option<PathBuf>,
    /// Abort the scan when this process stops being the DIRECT parent of this one.
    /// A value that is not already the direct parent aborts immediately. Unix only;
    /// accepted and ignored elsewhere. Absent = no watchdog thread.
    #[arg(long)]
    pub parent_pid: Option<u32>,
    /// Extraction level for a NEW artifact: `symbols` (symbol core only — no
    /// identifiers, literals, type-argument usages, source regions, or
    /// structural facts) or `full` (everything; the default). An existing
    /// artifact always keeps the level it was built with; passing a different
    /// level for it is a usage error — rebuild into a fresh artifact instead.
    #[arg(long, value_enum)]
    pub level: Option<LevelArg>,
}

/// CLI value for `--level`, mapped to `julie_extractors::ExtractionLevel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LevelArg {
    Symbols,
    Full,
}

impl From<LevelArg> for julie_extractors::ExtractionLevel {
    fn from(level: LevelArg) -> Self {
        match level {
            LevelArg::Symbols => julie_extractors::ExtractionLevel::Symbols,
            LevelArg::Full => julie_extractors::ExtractionLevel::Full,
        }
    }
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Source root directory the artifact was created from.
    #[arg(long)]
    pub root: PathBuf,
    /// Existing SQLite artifact path.
    #[arg(long)]
    pub db: PathBuf,
    /// File to re-extract, as a path inside the root.
    #[arg(long)]
    pub file: PathBuf,
    /// Extra gitignore-style ignore file. Its rules take precedence over
    /// .gitignore and .julieignore rules. Repeatable.
    #[arg(long = "ignore-file")]
    pub ignore_files: Vec<PathBuf>,
    /// Fail on an artifact whose schema version does not match this binary. Write
    /// commands refuse an older artifact regardless of this flag; the flag extends
    /// the refusal to read commands.
    #[arg(long)]
    pub strict_schema: bool,
    /// Emit the machine-readable JSON report on stdout.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Source root directory the artifact was created from.
    #[arg(long)]
    pub root: PathBuf,
    /// Existing SQLite artifact path.
    #[arg(long)]
    pub db: PathBuf,
    /// File whose rows should be removed, as a path inside the root.
    #[arg(long)]
    pub file: PathBuf,
    /// Fail on an artifact whose schema version does not match this binary. Write
    /// commands refuse an older artifact regardless of this flag; the flag extends
    /// the refusal to read commands.
    #[arg(long)]
    pub strict_schema: bool,
    /// Emit the machine-readable JSON report on stdout.
    #[arg(long)]
    pub json: bool,
}

/// Retarget an artifact at a new source root.
///
/// Rewrites only the recorded root and identity metadata: nothing is copied and
/// nothing is extracted. Run an ordinary `scan` afterwards to reconcile the new
/// root.
#[derive(Debug, Args)]
pub struct RebindArgs {
    /// Source root directory the artifact should be retargeted at.
    #[arg(long)]
    pub root: PathBuf,
    /// Existing SQLite artifact path.
    #[arg(long)]
    pub db: PathBuf,
    /// Fail on an artifact whose schema version does not match this binary. Write
    /// commands refuse an older artifact regardless of this flag; the flag extends
    /// the refusal to read commands.
    #[arg(long)]
    pub strict_schema: bool,
    /// Emit the machine-readable JSON report on stdout.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct InfoArgs {
    /// Existing SQLite artifact path.
    #[arg(long)]
    pub db: PathBuf,
    /// Fail on an artifact whose schema version does not match this binary. Write
    /// commands refuse an older artifact regardless of this flag; the flag extends
    /// the refusal to read commands.
    #[arg(long)]
    pub strict_schema: bool,
    /// Emit the machine-readable JSON report on stdout.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Existing SQLite artifact path.
    #[arg(long)]
    pub db: PathBuf,
    /// Export format. Only "jsonl" is supported.
    #[arg(long)]
    pub format: String,
    /// Output path for the export.
    #[arg(long)]
    pub out: PathBuf,
    /// Fail on an artifact whose schema version does not match this binary. Write
    /// commands refuse an older artifact regardless of this flag; the flag extends
    /// the refusal to read commands.
    #[arg(long)]
    pub strict_schema: bool,
    /// Emit the machine-readable JSON report on stdout.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct LanguagesArgs {
    /// Emit the machine-readable JSON report on stdout.
    #[arg(long)]
    pub json: bool,
}
