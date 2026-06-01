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
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    #[arg(long)]
    pub root: PathBuf,
    #[arg(long)]
    pub db: PathBuf,
    #[arg(long)]
    pub force: bool,
    #[arg(long = "ignore-file")]
    pub ignore_files: Vec<PathBuf>,
    #[arg(long)]
    pub strict_schema: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    #[arg(long)]
    pub root: PathBuf,
    #[arg(long)]
    pub db: PathBuf,
    #[arg(long)]
    pub file: PathBuf,
    #[arg(long = "ignore-file")]
    pub ignore_files: Vec<PathBuf>,
    #[arg(long)]
    pub strict_schema: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct DeleteArgs {
    #[arg(long)]
    pub root: PathBuf,
    #[arg(long)]
    pub db: PathBuf,
    #[arg(long)]
    pub file: PathBuf,
    #[arg(long)]
    pub strict_schema: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct InfoArgs {
    #[arg(long)]
    pub db: PathBuf,
    #[arg(long)]
    pub strict_schema: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    #[arg(long)]
    pub db: PathBuf,
    #[arg(long)]
    pub format: String,
    #[arg(long)]
    pub out: PathBuf,
    #[arg(long)]
    pub strict_schema: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct LanguagesArgs {
    #[arg(long)]
    pub json: bool,
}
