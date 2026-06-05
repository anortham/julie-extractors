use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleasePackageKind {
    Binary,
    Checksum,
    Doc,
    ReleaseNote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleasePackageItem {
    pub kind: ReleasePackageKind,
    pub path_template: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagePackagePlan {
    pub version: String,
    pub target: String,
    pub out_dir: PathBuf,
    pub binary: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagePackageResult {
    pub staged_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePreflightPlan {
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePreflightResult {
    pub checked_targets: Vec<String>,
    pub checked_inputs: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum ReleasePackageError {
    Usage(String),
    Io {
        context: String,
        source: std::io::Error,
    },
    MissingInput {
        path: PathBuf,
    },
    OutputDirectoryNotEmpty {
        path: PathBuf,
    },
    ForbiddenPackagePath {
        path_template: String,
        rendered_path: PathBuf,
    },
    VersionMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
}

impl ReleasePackageError {
    fn exit_code(&self) -> u8 {
        match self {
            ReleasePackageError::Usage(_) => 2,
            ReleasePackageError::Io { .. }
            | ReleasePackageError::MissingInput { .. }
            | ReleasePackageError::OutputDirectoryNotEmpty { .. }
            | ReleasePackageError::ForbiddenPackagePath { .. }
            | ReleasePackageError::VersionMismatch { .. } => 1,
        }
    }
}

impl std::fmt::Display for ReleasePackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReleasePackageError::Usage(message) => f.write_str(message),
            ReleasePackageError::Io { context, source } => write!(f, "{context}: {source}"),
            ReleasePackageError::MissingInput { path } => {
                write!(f, "missing release input {}", path.display())
            }
            ReleasePackageError::OutputDirectoryNotEmpty { path } => {
                write!(f, "output directory must be empty: {}", path.display())
            }
            ReleasePackageError::ForbiddenPackagePath {
                path_template,
                rendered_path,
            } => write!(
                f,
                "forbidden package path `{}` rendered as {}",
                path_template,
                rendered_path.display()
            ),
            ReleasePackageError::VersionMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "{} version is {actual}, expected {expected}",
                path.display()
            ),
        }
    }
}

const RELEASE_TARGETS: &[(&str, &str)] = &[
    ("x86_64-unknown-linux-gnu", ""),
    ("aarch64-apple-darwin", ""),
    ("x86_64-apple-darwin", ""),
    ("x86_64-pc-windows-msvc", ".exe"),
];

const RELEASE_CRATE_MANIFESTS: &[&str] = &[
    "crates/julie-extract-artifact/Cargo.toml",
    "crates/julie-extract-cli/Cargo.toml",
    "crates/julie-extractors/Cargo.toml",
];

impl std::error::Error for ReleasePackageError {}

pub fn release_package_items() -> Vec<ReleasePackageItem> {
    vec![
        ReleasePackageItem {
            kind: ReleasePackageKind::Binary,
            path_template: "dist/{target}/julie-extract{exe_suffix}",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Checksum,
            path_template: "dist/{target}/julie-extract{exe_suffix}.sha256",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Doc,
            path_template: "docs/contracts/cli.md",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Doc,
            path_template: "docs/contracts/sqlite-schema-v1.md",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Doc,
            path_template: "docs/contracts/sqlite-schema-v2.md",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Doc,
            path_template: "docs/contracts/jsonl-v1.md",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Doc,
            path_template: "docs/contracts/jsonl-v2.md",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Doc,
            path_template: "docs/contracts/reports.md",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Doc,
            path_template: "docs/contracts/extracted-data-v1.md",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Doc,
            path_template: "docs/contracts/extracted-data-v2.md",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Doc,
            path_template: "docs/architecture/product-boundary.md",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Doc,
            path_template: "docs/architecture/schema-principles.md",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Doc,
            path_template: "docs/testing-strategy.md",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::Doc,
            path_template: "docs/release.md",
        },
        ReleasePackageItem {
            kind: ReleasePackageKind::ReleaseNote,
            path_template: "docs/release-notes/v{version}.md",
        },
    ]
}

pub fn render_release_package_list() -> String {
    let mut output = String::new();
    for item in release_package_items() {
        output.push_str(match item.kind {
            ReleasePackageKind::Binary => "binary",
            ReleasePackageKind::Checksum => "checksum",
            ReleasePackageKind::Doc => "doc",
            ReleasePackageKind::ReleaseNote => "release_note",
        });
        output.push('\t');
        output.push_str(item.path_template);
        output.push('\n');
    }
    output
}

pub fn plan_package_from_args<I, S>(args: I) -> Result<StagePackagePlan, ReleasePackageError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();

    if args.first().map(String::as_str) != Some("package") {
        return Err(ReleasePackageError::Usage(
            "usage: cargo xtask release package --version <version> --target <target> --out-dir <path> [--binary <path>]; expected `package`".to_string(),
        ));
    }

    let mut version = None;
    let mut target = None;
    let mut out_dir = None;
    let mut binary = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--version" => {
                index += 1;
                version = Some(required_string(&args, index, "--version")?);
            }
            "--target" => {
                index += 1;
                target = Some(required_string(&args, index, "--target")?);
            }
            "--out-dir" => {
                index += 1;
                out_dir = Some(required_path(&args, index, "--out-dir")?);
            }
            "--binary" => {
                index += 1;
                binary = Some(required_path(&args, index, "--binary")?);
            }
            other => {
                return Err(ReleasePackageError::Usage(format!(
                    "unexpected release package argument `{other}`"
                )));
            }
        }
        index += 1;
    }

    let version =
        version.ok_or_else(|| ReleasePackageError::Usage("missing --version".to_string()))?;
    let target =
        target.ok_or_else(|| ReleasePackageError::Usage("missing --target".to_string()))?;
    let out_dir =
        out_dir.ok_or_else(|| ReleasePackageError::Usage("missing --out-dir".to_string()))?;
    let binary = match binary {
        Some(path) => path,
        None => default_release_binary_path(&target)?,
    };

    Ok(StagePackagePlan {
        version,
        target,
        out_dir,
        binary,
    })
}

pub fn plan_preflight_from_args<I, S>(args: I) -> Result<ReleasePreflightPlan, ReleasePackageError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect::<Vec<_>>();

    if args.first().map(String::as_str) != Some("preflight") {
        return Err(ReleasePackageError::Usage(
            "usage: cargo xtask release preflight --version <version>; expected `preflight`"
                .to_string(),
        ));
    }

    let mut version = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--version" => {
                index += 1;
                version = Some(required_string(&args, index, "--version")?);
            }
            other => {
                return Err(ReleasePackageError::Usage(format!(
                    "unexpected release preflight argument `{other}`"
                )));
            }
        }
        index += 1;
    }

    let version =
        version.ok_or_else(|| ReleasePackageError::Usage("missing --version".to_string()))?;
    Ok(ReleasePreflightPlan { version })
}

pub fn run_preflight_from_args(args: &[String]) -> ExitCode {
    match plan_preflight_from_args(args)
        .and_then(|plan| preflight_release_from_root(&repo_root(), plan))
    {
        Ok(result) => {
            println!(
                "release preflight ok: {} targets, {} inputs",
                result.checked_targets.len(),
                result.checked_inputs.len()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(error.exit_code())
        }
    }
}

pub fn run_package_from_args(args: &[String]) -> ExitCode {
    match plan_package_from_args(args).and_then(|plan| stage_package_from_root(&repo_root(), plan))
    {
        Ok(result) => {
            for path in result.staged_files {
                println!("{}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(error.exit_code())
        }
    }
}

pub fn preflight_release_from_root(
    repo_root: &Path,
    plan: ReleasePreflightPlan,
) -> Result<ReleasePreflightResult, ReleasePackageError> {
    validate_release_version(&plan.version)?;
    let mut checked_inputs = Vec::new();

    for manifest in RELEASE_CRATE_MANIFESTS {
        let path = repo_root.join(manifest);
        let actual = cargo_manifest_version(&path)?;
        if actual != plan.version {
            return Err(ReleasePackageError::VersionMismatch {
                path,
                expected: plan.version,
                actual,
            });
        }
        checked_inputs.push(PathBuf::from(manifest));
    }

    for (target, exe_suffix) in RELEASE_TARGETS {
        let items =
            validate_package_manifest(&release_package_items(), &plan.version, target, exe_suffix)?;
        for item in items {
            if matches!(
                item.kind,
                ReleasePackageKind::Doc | ReleasePackageKind::ReleaseNote
            ) {
                let source = repo_root.join(&item.relative_path);
                ensure_file_exists(&source)?;
                checked_inputs.push(item.relative_path);
            }
        }
    }
    checked_inputs.sort();
    checked_inputs.dedup();

    Ok(ReleasePreflightResult {
        checked_targets: RELEASE_TARGETS
            .iter()
            .map(|(target, _)| (*target).to_string())
            .collect(),
        checked_inputs,
    })
}

pub fn stage_package_from_root(
    repo_root: &Path,
    plan: StagePackagePlan,
) -> Result<StagePackageResult, ReleasePackageError> {
    let exe_suffix = exe_suffix_for_target(&plan.target)?;
    let items = validate_package_manifest(
        &release_package_items(),
        &plan.version,
        &plan.target,
        exe_suffix,
    )?;
    ensure_empty_output_dir(&plan.out_dir)?;

    let binary_relative = render_template(
        "dist/{target}/julie-extract{exe_suffix}",
        &plan.version,
        &plan.target,
        exe_suffix,
    );
    let binary_destination = plan.out_dir.join(&binary_relative);
    let mut staged_files = Vec::new();

    for item in items {
        match item.kind {
            ReleasePackageKind::Binary => {
                ensure_file_exists(&plan.binary)?;
                copy_file(&plan.binary, &binary_destination)?;
                staged_files.push(item.relative_path);
            }
            ReleasePackageKind::Checksum => {
                let checksum = sha256_file_hex(&binary_destination)?;
                let checksum_line =
                    format!("{checksum}  {}\n", path_to_posix_string(&binary_relative));
                write_file(
                    &plan.out_dir.join(&item.relative_path),
                    checksum_line.as_bytes(),
                )?;
                staged_files.push(item.relative_path);
            }
            ReleasePackageKind::Doc | ReleasePackageKind::ReleaseNote => {
                let source = repo_root.join(&item.relative_path);
                ensure_file_exists(&source)?;
                copy_file(&source, &plan.out_dir.join(&item.relative_path))?;
                staged_files.push(item.relative_path);
            }
        }
    }

    staged_files.sort();
    Ok(StagePackageResult { staged_files })
}

pub fn validate_package_manifest(
    items: &[ReleasePackageItem],
    version: &str,
    target: &str,
    exe_suffix: &str,
) -> Result<Vec<RenderedPackageItem>, ReleasePackageError> {
    let mut rendered = Vec::new();
    for item in items {
        let relative = render_template(item.path_template, version, target, exe_suffix);
        if !is_safe_package_path(&relative) {
            return Err(ReleasePackageError::ForbiddenPackagePath {
                path_template: item.path_template.to_string(),
                rendered_path: relative,
            });
        }
        rendered.push(RenderedPackageItem {
            kind: item.kind,
            relative_path: relative,
        });
    }
    Ok(rendered)
}

pub fn collect_staged_files(out_dir: &Path) -> Result<Vec<PathBuf>, ReleasePackageError> {
    let mut files = Vec::new();
    collect_staged_files_inner(out_dir, out_dir, &mut files)?;
    files.sort();
    Ok(files)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPackageItem {
    pub kind: ReleasePackageKind,
    pub relative_path: PathBuf,
}

fn required_string(
    args: &[String],
    index: usize,
    flag: &str,
) -> Result<String, ReleasePackageError> {
    args.get(index)
        .cloned()
        .ok_or_else(|| ReleasePackageError::Usage(format!("missing value for {flag}")))
}

fn required_path(
    args: &[String],
    index: usize,
    flag: &str,
) -> Result<PathBuf, ReleasePackageError> {
    required_string(args, index, flag).map(PathBuf::from)
}

fn default_release_binary_path(target: &str) -> Result<PathBuf, ReleasePackageError> {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"));
    Ok(target_dir
        .join(target)
        .join("release")
        .join(format!("julie-extract{}", exe_suffix_for_target(target)?)))
}

fn validate_release_version(version: &str) -> Result<(), ReleasePackageError> {
    if version.is_empty()
        || !version
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '+'))
    {
        return Err(ReleasePackageError::Usage(format!(
            "invalid release version `{version}`"
        )));
    }
    Ok(())
}

fn cargo_manifest_version(path: &Path) -> Result<String, ReleasePackageError> {
    let contents = fs::read_to_string(path).map_err(|source| ReleasePackageError::Io {
        context: format!("failed to read Cargo manifest {}", path.display()),
        source,
    })?;
    contents
        .lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("version = ")
                .map(|value| value.trim().trim_matches('"').to_string())
        })
        .ok_or_else(|| ReleasePackageError::MissingInput {
            path: path.to_path_buf(),
        })
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under repo root")
        .to_path_buf()
}

fn exe_suffix_for_target(target: &str) -> Result<&'static str, ReleasePackageError> {
    RELEASE_TARGETS
        .iter()
        .find_map(|(candidate, suffix)| (*candidate == target).then_some(*suffix))
        .ok_or_else(|| {
            let supported = RELEASE_TARGETS
                .iter()
                .map(|(candidate, _)| *candidate)
                .collect::<Vec<_>>()
                .join(", ");
            ReleasePackageError::Usage(format!(
                "unsupported --target `{target}`; expected one of: {supported}"
            ))
        })
}

fn render_template(template: &str, version: &str, target: &str, exe_suffix: &str) -> PathBuf {
    PathBuf::from(
        template
            .replace("{version}", version)
            .replace("{target}", target)
            .replace("{exe_suffix}", exe_suffix),
    )
}

fn is_safe_package_path(path: &Path) -> bool {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return false;
    }
    if path
        .as_os_str()
        .to_string_lossy()
        .chars()
        .any(|ch| ch == '\\')
    {
        return false;
    }
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
}

fn ensure_empty_output_dir(path: &Path) -> Result<(), ReleasePackageError> {
    if !path.exists() {
        fs::create_dir_all(path).map_err(|source| ReleasePackageError::Io {
            context: format!("failed to create output directory {}", path.display()),
            source,
        })?;
        return Ok(());
    }
    let mut entries = fs::read_dir(path).map_err(|source| ReleasePackageError::Io {
        context: format!("failed to read output directory {}", path.display()),
        source,
    })?;
    if entries
        .next()
        .transpose()
        .map_err(|source| ReleasePackageError::Io {
            context: format!("failed to read output directory {}", path.display()),
            source,
        })?
        .is_some()
    {
        return Err(ReleasePackageError::OutputDirectoryNotEmpty {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn ensure_file_exists(path: &Path) -> Result<(), ReleasePackageError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(ReleasePackageError::MissingInput {
            path: path.to_path_buf(),
        })
    }
}

fn copy_file(source: &Path, destination: &Path) -> Result<(), ReleasePackageError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|source| ReleasePackageError::Io {
            context: format!("failed to create package directory {}", parent.display()),
            source,
        })?;
    }
    fs::copy(source, destination).map_err(|error| ReleasePackageError::Io {
        context: format!(
            "failed to copy {} to {}",
            source.display(),
            destination.display()
        ),
        source: error,
    })?;
    Ok(())
}

fn write_file(destination: &Path, bytes: &[u8]) -> Result<(), ReleasePackageError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|source| ReleasePackageError::Io {
            context: format!("failed to create package directory {}", parent.display()),
            source,
        })?;
    }
    fs::write(destination, bytes).map_err(|source| ReleasePackageError::Io {
        context: format!("failed to write {}", destination.display()),
        source,
    })
}

fn sha256_file_hex(path: &Path) -> Result<String, ReleasePackageError> {
    let bytes = fs::read(path).map_err(|source| ReleasePackageError::Io {
        context: format!("failed to read {}", path.display()),
        source,
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn path_to_posix_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn collect_staged_files_inner(
    root: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), ReleasePackageError> {
    if !current.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current).map_err(|source| ReleasePackageError::Io {
        context: format!("failed to read {}", current.display()),
        source,
    })? {
        let entry = entry.map_err(|source| ReleasePackageError::Io {
            context: format!("failed to read {}", current.display()),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_staged_files_inner(root, &path, files)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("walked path should live under root")
                .to_path_buf();
            files.push(relative);
        }
    }
    Ok(())
}
