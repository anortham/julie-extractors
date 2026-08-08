use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

pub const MAX_STORE_PATH_BYTES: usize = 4096;
pub const MAX_STORE_IDENTIFIER_BYTES: usize = 128;
pub const MAX_REQUEST_TIMEOUT_SECONDS: u64 = 86_400;
pub const DEFAULT_REQUEST_TIMEOUT_SECONDS: u64 = 30;

#[derive(Debug, Parser)]
#[command(
    name = "julie-extract",
    about = "Internal versioned-store contract parser"
)]
pub struct StoreCli {
    #[command(subcommand)]
    pub command: StoreRootCommand,
}

#[derive(Debug, Subcommand)]
pub enum StoreRootCommand {
    Store(StoreArgs),
}

#[derive(Debug, Args)]
pub struct StoreArgs {
    #[command(subcommand)]
    pub command: StoreCommand,
}

#[derive(Debug, Subcommand)]
pub enum StoreCommand {
    Import(StoreImportArgs),
    Update(StoreUpdateArgs),
    Delete(StoreDeleteArgs),
    Resolve(StoreResolveArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StoreLevelArg {
    #[value(name = "l1")]
    L1,
    #[value(name = "full")]
    Full,
}

#[derive(Debug, Args)]
pub struct StoreImportArgs {
    /// Family-store directory. It is created when absent.
    #[arg(long, value_parser = parse_store_path)]
    pub store: PathBuf,
    /// UUID minted by the family owner.
    #[arg(long, value_parser = parse_family_id)]
    pub family: String,
    /// Source root whose files populate the view.
    #[arg(long, value_parser = parse_store_path)]
    pub root: PathBuf,
    /// Stable view identifier within the family store.
    #[arg(long, value_parser = parse_store_identifier)]
    pub view: String,
    /// Extraction depth requested for this import.
    #[arg(long, value_enum, default_value_t = StoreLevelArg::Full)]
    pub level: StoreLevelArg,
    #[command(flatten)]
    pub scan: StoreScanControls,
    #[command(flatten)]
    pub request: StoreRequestControls,
    /// Emit the machine-readable store report.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct StoreUpdateArgs {
    /// Existing family-store directory.
    #[arg(long, value_parser = parse_store_path)]
    pub store: PathBuf,
    /// Expected family UUID. Defaults to the existing store family.
    #[arg(long, value_parser = parse_family_id)]
    pub family: Option<String>,
    /// Source root already bound to the view.
    #[arg(long, value_parser = parse_store_path)]
    pub root: PathBuf,
    /// Existing stable view identifier.
    #[arg(long, value_parser = parse_store_identifier)]
    pub view: String,
    /// One root-relative source file to update.
    #[arg(long = "file", value_parser = parse_store_path)]
    pub file: PathBuf,
    /// Extraction depth requested for this update.
    #[arg(long, value_enum, default_value_t = StoreLevelArg::Full)]
    pub level: StoreLevelArg,
    #[command(flatten)]
    pub scan: StoreScanControls,
    #[command(flatten)]
    pub request: StoreRequestControls,
    /// Emit the machine-readable store report.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct StoreDeleteArgs {
    /// Existing family-store directory.
    #[arg(long, value_parser = parse_store_path)]
    pub store: PathBuf,
    /// Expected family UUID. Defaults to the existing store family.
    #[arg(long, value_parser = parse_family_id)]
    pub family: Option<String>,
    /// Source root already bound to the view.
    #[arg(long, value_parser = parse_store_path)]
    pub root: PathBuf,
    /// Existing stable view identifier.
    #[arg(long, value_parser = parse_store_identifier)]
    pub view: String,
    /// Root-relative path to remove. Repeatable.
    #[arg(long = "file", required = true, value_parser = parse_store_path)]
    pub files: Vec<PathBuf>,
    #[command(flatten)]
    pub request: StoreRequestControls,
    /// Emit the machine-readable store report.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct StoreResolveArgs {
    /// Existing family-store directory.
    #[arg(long, value_parser = parse_store_path)]
    pub store: PathBuf,
    /// Expected family UUID. Defaults to the existing store family.
    #[arg(long, value_parser = parse_family_id)]
    pub family: Option<String>,
    /// Existing stable view identifier.
    #[arg(long, value_parser = parse_store_identifier)]
    pub view: String,
    #[command(flatten)]
    pub request: StoreRequestControls,
    /// Emit the machine-readable store report.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args, Default)]
pub struct StoreScanControls {
    /// Extra gitignore-style ignore file. Repeatable.
    #[arg(long = "ignore-file", value_parser = parse_store_path)]
    pub ignore_files: Vec<PathBuf>,
    /// Number of parallel extraction workers (0 = auto-detect).
    #[arg(long, short = 'j', default_value_t = 0)]
    pub jobs: usize,
    /// Directory to hold this request's extraction spool.
    #[arg(long, value_parser = parse_store_path)]
    pub spool_dir: Option<PathBuf>,
    /// Append request progress records to this file.
    #[arg(long, value_parser = parse_store_path)]
    pub progress_file: Option<PathBuf>,
    /// Abort extraction when this process stops being the direct parent.
    #[arg(long)]
    pub parent_pid: Option<u32>,
}

#[derive(Debug, Args)]
pub struct StoreRequestControls {
    /// Caller-supplied request identity. Missing values are minted by the executor.
    #[arg(long, value_parser = parse_store_identifier)]
    pub request_id: Option<String>,
    /// Idempotency key used to observe a prior request on retry.
    #[arg(long, value_parser = parse_store_identifier)]
    pub idempotency_key: Option<String>,
    /// Maximum time to wait for request acknowledgment.
    #[arg(
        long = "request-timeout-seconds",
        default_value_t = DEFAULT_REQUEST_TIMEOUT_SECONDS,
        value_parser = parse_request_timeout
    )]
    pub request_timeout_seconds: u64,
}

impl Default for StoreRequestControls {
    fn default() -> Self {
        Self {
            request_id: None,
            idempotency_key: None,
            request_timeout_seconds: DEFAULT_REQUEST_TIMEOUT_SECONDS,
        }
    }
}

fn parse_store_identifier(value: &str) -> Result<String, String> {
    if value.is_empty() {
        return Err("value must not be empty".to_string());
    }
    if value.len() > MAX_STORE_IDENTIFIER_BYTES {
        return Err(format!(
            "value must be at most {MAX_STORE_IDENTIFIER_BYTES} UTF-8 bytes"
        ));
    }
    if value.as_bytes().contains(&0) {
        return Err("value must not contain NUL".to_string());
    }
    Ok(value.to_string())
}

fn parse_family_id(value: &str) -> Result<String, String> {
    let value = parse_store_identifier(value)?;
    let bytes = value.as_bytes();
    let shape = [8, 4, 4, 4, 12];
    if bytes.len() != 36
        || value
            .chars()
            .any(|character| character.is_ascii_uppercase())
        || !shape.iter().enumerate().all(|(index, expected)| {
            let start = shape[..index].iter().sum::<usize>() + index;
            bytes[start..start + *expected]
                .iter()
                .all(u8::is_ascii_hexdigit)
        })
        || ![8usize, 13, 18, 23]
            .iter()
            .all(|&index| bytes.get(index) == Some(&b'-'))
    {
        return Err("family must be a canonical UUID".to_string());
    }
    Ok(value)
}

fn parse_store_path(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() {
        return Err("path must not be empty".to_string());
    }
    if value.len() > MAX_STORE_PATH_BYTES {
        return Err(format!(
            "path must be at most {MAX_STORE_PATH_BYTES} UTF-8 bytes"
        ));
    }
    if value.as_bytes().contains(&0) {
        return Err("path must not contain NUL".to_string());
    }
    Ok(PathBuf::from(value))
}

fn parse_request_timeout(value: &str) -> Result<u64, String> {
    let timeout = value
        .parse::<u64>()
        .map_err(|_| "request timeout must be an integer number of seconds".to_string())?;
    if timeout == 0 || timeout > MAX_REQUEST_TIMEOUT_SECONDS {
        return Err(format!(
            "request timeout must be between 1 and {MAX_REQUEST_TIMEOUT_SECONDS} seconds"
        ));
    }
    Ok(timeout)
}

#[cfg(test)]
mod tests {
    use super::{parse_family_id, parse_request_timeout, parse_store_identifier, parse_store_path};

    #[test]
    fn family_id_accepts_uuid_shape() {
        assert!(parse_family_id("9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11").is_ok());
        assert!(parse_family_id("9F8C2C9A-3B92-4F38-9B0D-0E2B8C7A4D11").is_err());
        assert!(parse_family_id("family-1").is_err());
    }

    #[test]
    fn parser_bounds_are_explicit() {
        assert!(parse_store_identifier(&"x".repeat(129)).is_err());
        assert!(parse_store_path(&format!("/{}", "x".repeat(4096))).is_err());
        assert!(parse_request_timeout("0").is_err());
        assert!(parse_request_timeout("86401").is_err());
    }
}
