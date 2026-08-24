use std::fs;
use std::path::{Path, PathBuf};

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use julie_extractors::detect_language_for_source;

use crate::limits::{MAX_SOURCE_FILE_BYTES, slow_file_skip_message};
use crate::paths::{FileTarget, PathPolicyError, canonicalize_ignore_file};
use crate::progress::{Counter, ScanProgress};
use crate::spool::is_spool_artifact_name;

/// Directory entries between progress ticks. The walk is a single serial pass
/// over every entry in the tree, so consulting the clock on each one would put
/// a timestamp read on the hottest loop of the discovery phase.
const DISCOVERY_PROGRESS_TICK: u64 = 256;

struct DiscoveryWalk<'a> {
    progress: Option<&'a ScanProgress>,
    entries_seen: u64,
}

impl DiscoveryWalk<'_> {
    fn tick(&mut self) {
        self.entries_seen += 1;
        if self.entries_seen.is_multiple_of(DISCOVERY_PROGRESS_TICK)
            && let Some(progress) = self.progress
        {
            progress.advance(Counter::Discovered, DISCOVERY_PROGRESS_TICK);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSelection {
    Supported { language: String },
    Unsupported { reason: UnsupportedReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedReason {
    Ignored,
    HardExcluded,
    UnsupportedExtension,
    /// Otherwise-supported file exceeded [`MAX_SOURCE_FILE_BYTES`] and was
    /// skipped. Distinct from [`Self::HardExcluded`] so callers can surface a
    /// typed `slow_file_skipped` warning instead of counting the file as a
    /// silent unsupported.
    Oversized,
}

/// Paths a scan writes to while it runs, which must therefore stay out of its
/// own discovery walk. Both are `None` unless the matching flag was passed.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryExclusions {
    /// A `--progress-file` written inside the root would otherwise be discovered
    /// and extracted while it is being appended to, exactly as the artifact would
    /// be. Excluded on the same terms.
    pub progress_path: Option<PathBuf>,
    /// A `--spool-dir` inside the root holds `.jsonl` spools, and `jsonl` is a
    /// supported extension — so a spool a concurrent scan still owns would be
    /// walked and extracted as if it were source.
    pub spool_dir: Option<PathBuf>,
}

impl DiscoveryExclusions {
    fn excludes(&self, path: &Path) -> bool {
        self.progress_path.as_deref() == Some(path)
            || self.spool_dir.as_deref() == Some(path)
            || self.holds_spool_artifact(path)
    }

    /// Belt and braces for a `--spool-dir` that is the scan root itself, where
    /// the directory-level skip never gets a chance to fire.
    fn holds_spool_artifact(&self, path: &Path) -> bool {
        let Some(spool_dir) = self.spool_dir.as_deref() else {
            return false;
        };
        path.parent() == Some(spool_dir)
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(is_spool_artifact_name)
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveryPolicy {
    root: PathBuf,
    db_path: PathBuf,
    exclusions: DiscoveryExclusions,
    /// Rules from `--ignore-file`, anchored at the scan root. The caller
    /// layer is consulted first and is decisive, so explicit invocation
    /// rules always win over in-tree ignore files.
    caller_matcher: Gitignore,
    /// One matcher per directory that owns ignore files: ancestor
    /// directories up to the git root, the scan root, and nested
    /// directories, each anchored at its own directory. Ordered shallowest
    /// first; consulted in reverse so the deepest matching scope decides.
    scopes: Vec<(PathBuf, Gitignore)>,
    /// Non-fatal problems loading in-tree ignore files, surfaced through
    /// `DiscoverySummary::errors`.
    warnings: Vec<DiscoveryError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySummary {
    pub supported_files: Vec<FileTarget>,
    pub unsupported_files: usize,
    pub errors: Vec<DiscoveryError>,
    /// Otherwise-supported files skipped for exceeding [`MAX_SOURCE_FILE_BYTES`],
    /// surfaced by callers as `slow_file_skipped` warnings rather than folded
    /// silently into `unsupported_files`.
    pub slow_file_skips: Vec<DiscoveryError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryError {
    pub path: String,
    pub root_relative_path: String,
    pub message: String,
}

impl DiscoveryPolicy {
    pub fn build(
        root: &Path,
        db_path: &Path,
        ignore_files: &[PathBuf],
    ) -> Result<Self, PathPolicyError> {
        Self::build_excluding(root, db_path, DiscoveryExclusions::default(), ignore_files)
    }

    pub fn build_excluding(
        root: &Path,
        db_path: &Path,
        exclusions: DiscoveryExclusions,
        ignore_files: &[PathBuf],
    ) -> Result<Self, PathPolicyError> {
        let caller_matcher = build_caller_matcher(root, ignore_files)?;
        let mut scopes = Vec::new();
        let mut warnings = Vec::new();
        for dir in ancestor_ignore_scopes(root) {
            let files = [dir.join(".gitignore")];
            add_dir_scope(&dir, &files, root, &mut scopes, &mut warnings);
        }
        let root_files = [root.join(".gitignore"), root.join(".julieignore")];
        add_dir_scope(root, &root_files, root, &mut scopes, &mut warnings);
        collect_nested_scopes(root, root, &caller_matcher, &mut scopes, &mut warnings);
        Ok(Self {
            root: root.to_path_buf(),
            db_path: db_path.to_path_buf(),
            exclusions,
            caller_matcher,
            scopes,
            warnings,
        })
    }

    /// Gitignore semantics: path prefixes are decided top-down, so a file
    /// cannot be re-included when a parent directory is excluded. Each
    /// prefix takes the caller `--ignore-file` decision first, then the
    /// deepest in-tree scope that matches.
    fn is_ignored(&self, root_relative_path: &str, is_dir: bool) -> bool {
        let components: Vec<&str> = root_relative_path.split('/').collect();
        let mut prefix = String::with_capacity(root_relative_path.len());
        for (index, component) in components.iter().enumerate() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            let prefix_is_dir = index + 1 < components.len() || is_dir;
            if decide_ignored(
                &self.caller_matcher,
                &self.scopes,
                &self.root,
                &prefix,
                prefix_is_dir,
            ) == Some(true)
            {
                return true;
            }
        }
        false
    }

    pub fn select_file(&self, target: &FileTarget) -> FileSelection {
        if is_hard_excluded(
            &target.absolute_path,
            &target.root_relative_path,
            &self.db_path,
            &self.exclusions,
        ) {
            return FileSelection::Unsupported {
                reason: UnsupportedReason::HardExcluded,
            };
        }
        if self.is_ignored(&target.root_relative_path, false) {
            return FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored,
            };
        }
        match language_for_path(&target.absolute_path) {
            Some(language) => {
                if is_oversized_source_file(&target.absolute_path) {
                    return FileSelection::Unsupported {
                        reason: UnsupportedReason::Oversized,
                    };
                }
                FileSelection::Supported {
                    language: language.to_string(),
                }
            }
            None => FileSelection::Unsupported {
                reason: UnsupportedReason::UnsupportedExtension,
            },
        }
    }

    #[cfg(test)]
    pub fn discover(&self) -> DiscoverySummary {
        self.discover_with_progress(None)
    }

    pub fn discover_with_progress(&self, progress: Option<&ScanProgress>) -> DiscoverySummary {
        let mut summary = DiscoverySummary {
            supported_files: Vec::new(),
            unsupported_files: 0,
            errors: self.warnings.clone(),
            slow_file_skips: Vec::new(),
        };
        let mut walk = DiscoveryWalk {
            progress,
            entries_seen: 0,
        };
        self.discover_dir(&self.root, &mut summary, &mut walk);
        summary
            .supported_files
            .sort_by(|left, right| left.root_relative_path.cmp(&right.root_relative_path));
        if let Some(progress) = progress {
            progress.advance(
                Counter::Discovered,
                walk.entries_seen % DISCOVERY_PROGRESS_TICK,
            );
            progress.advance(Counter::Supported, summary.supported_files.len() as u64);
        }
        summary
    }

    fn discover_dir(&self, dir: &Path, summary: &mut DiscoverySummary, walk: &mut DiscoveryWalk) {
        let entries =
            match fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(error) => {
                    summary.errors.push(self.discovery_error(
                        dir,
                        format!("source directory could not be read: {error}"),
                    ));
                    return;
                }
            };

        for entry in entries {
            walk.tick();
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    summary.errors.push(self.discovery_error(
                        dir,
                        format!("source directory entry could not be read: {error}"),
                    ));
                    continue;
                }
            };
            let path = entry.path();
            let relative = match crate::paths::root_relative_unix(&self.root, &path) {
                Ok(relative) => relative,
                Err(error) => {
                    summary.errors.push(
                        self.discovery_error(&path, format!("source path was invalid: {error:?}")),
                    );
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    summary.errors.push(self.discovery_error(
                        &path,
                        format!("source file type could not be read: {error}"),
                    ));
                    continue;
                }
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if is_hard_excluded(&path, &relative, &self.db_path, &self.exclusions)
                    || self.is_ignored(&relative, true)
                {
                    continue;
                }
                self.discover_dir(&path, summary, walk);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            // Skipped rather than counted as unsupported: a scan's own progress
            // file and spool must not move `files_scanned`, or it reports
            // different counts purely because it was asked to report progress or
            // to place its spool somewhere the caller can supervise.
            if self.exclusions.excludes(&path) {
                continue;
            }
            let target = FileTarget {
                absolute_path: path,
                root_relative_path: relative,
            };
            match self.select_file(&target) {
                FileSelection::Supported { .. } => summary.supported_files.push(target),
                FileSelection::Unsupported { reason } => {
                    summary.unsupported_files += 1;
                    if reason == UnsupportedReason::Oversized {
                        summary.slow_file_skips.push(DiscoveryError {
                            path: target.absolute_path.display().to_string(),
                            root_relative_path: target.root_relative_path.clone(),
                            message: slow_file_skip_message(),
                        });
                    }
                }
            }
        }
    }

    fn discovery_error(&self, path: &Path, message: String) -> DiscoveryError {
        let root_relative_path = if path == self.root {
            ".".to_string()
        } else {
            crate::paths::root_relative_unix(&self.root, path).unwrap_or_else(|_| {
                path.strip_prefix(&self.root)
                    .ok()
                    .and_then(|relative| relative.to_str())
                    .unwrap_or("")
                    .replace(std::path::MAIN_SEPARATOR, "/")
            })
        };
        DiscoveryError {
            path: path.display().to_string(),
            root_relative_path,
            message,
        }
    }
}

pub fn canonicalize_ignore_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, PathPolicyError> {
    paths
        .iter()
        .map(|path| canonicalize_ignore_file(path))
        .collect()
}

/// Build the matcher for caller-supplied `--ignore-file` rules. Caller input
/// is operator configuration, so unreadable files and invalid patterns are
/// hard errors, unlike in-tree ignore files which only warn.
fn build_caller_matcher(
    root: &Path,
    ignore_files: &[PathBuf],
) -> Result<Gitignore, PathPolicyError> {
    if ignore_files.is_empty() {
        return Ok(Gitignore::empty());
    }
    let mut builder = GitignoreBuilder::new(root);
    for ignore_file in ignore_files {
        let contents =
            fs::read_to_string(ignore_file).map_err(|error| PathPolicyError::InvalidPath {
                path: ignore_file.display().to_string(),
                message: format!("ignore file could not be read: {error}"),
            })?;
        for (line_number, line) in contents.lines().enumerate() {
            builder
                .add_line(None, line)
                .map_err(|error| PathPolicyError::InvalidPath {
                    path: ignore_file.display().to_string(),
                    message: format!(
                        "invalid ignore pattern on line {}: {error}",
                        line_number + 1
                    ),
                })?;
        }
    }
    builder
        .build()
        .map_err(|error| PathPolicyError::InvalidPath {
            path: root.display().to_string(),
            message: format!("ignore matcher could not be built: {error}"),
        })
}

/// One layered ignore decision for a single root-relative path, with no
/// parent-directory checks: the caller `--ignore-file` matcher is decisive
/// first, then in-tree scopes deepest-first. `None` means no rule matched.
fn decide_ignored(
    caller_matcher: &Gitignore,
    scopes: &[(PathBuf, Gitignore)],
    root: &Path,
    relative: &str,
    is_dir: bool,
) -> Option<bool> {
    match caller_matcher.matched(relative, is_dir) {
        Match::Ignore(_) => return Some(true),
        Match::Whitelist(_) => return Some(false),
        Match::None => {}
    }
    let absolute = root.join(relative);
    for (dir, matcher) in scopes.iter().rev() {
        let Ok(scope_relative) = absolute.strip_prefix(dir) else {
            continue;
        };
        if scope_relative.as_os_str().is_empty() {
            continue;
        }
        match matcher.matched(scope_relative, is_dir) {
            Match::Ignore(_) => return Some(true),
            Match::Whitelist(_) => return Some(false),
            Match::None => {}
        }
    }
    None
}

/// Add one in-tree ignore scope anchored at `dir`. Files are added in the
/// given order so later files win on conflicts (`.julieignore` is always
/// passed after `.gitignore`). In-tree ignore files must not break the scan:
/// load failures become warnings and the readable rules still apply.
fn add_dir_scope(
    dir: &Path,
    files: &[PathBuf],
    root: &Path,
    scopes: &mut Vec<(PathBuf, Gitignore)>,
    warnings: &mut Vec<DiscoveryError>,
) {
    let existing: Vec<&PathBuf> = files.iter().filter(|file| file.is_file()).collect();
    if existing.is_empty() {
        return;
    }
    let mut builder = GitignoreBuilder::new(dir);
    for file in existing {
        if let Some(error) = builder.add(file) {
            warnings.push(ignore_file_warning(root, file, &error));
        }
    }
    match builder.build() {
        Ok(matcher) => scopes.push((dir.to_path_buf(), matcher)),
        Err(error) => warnings.push(ignore_file_warning(root, dir, &error)),
    }
}

/// Walk the tree top-down collecting per-directory ignore scopes. Pruning
/// mirrors discovery: symlinks and hard-excluded directories are skipped, and
/// ignored directories are not descended into, because git never reads ignore
/// files inside excluded directories.
fn collect_nested_scopes(
    root: &Path,
    dir: &Path,
    caller_matcher: &Gitignore,
    scopes: &mut Vec<(PathBuf, Gitignore)>,
    warnings: &mut Vec<DiscoveryError>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() || !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if HARD_EXCLUDE_DIRS.contains(&name) {
            continue;
        }
        let Ok(relative) = crate::paths::root_relative_unix(root, &path) else {
            continue;
        };
        if decide_ignored(caller_matcher, scopes, root, &relative, true) == Some(true) {
            continue;
        }
        let files = [path.join(".gitignore"), path.join(".julieignore")];
        add_dir_scope(&path, &files, root, scopes, warnings);
        collect_nested_scopes(root, &path, caller_matcher, scopes, warnings);
    }
}

fn ignore_file_warning(root: &Path, path: &Path, error: &ignore::Error) -> DiscoveryError {
    let root_relative_path = crate::paths::root_relative_unix(root, path).unwrap_or_default();
    DiscoveryError {
        path: path.display().to_string(),
        root_relative_path,
        message: format!("ignore file could not be loaded: {error}"),
    }
}

/// Ancestor directories between the git root and the scan root that own a
/// `.gitignore`, shallowest first, each becoming a scope anchored at its own
/// directory so anchored patterns keep git semantics.
fn ancestor_ignore_scopes(root: &Path) -> Vec<PathBuf> {
    let Some(git_root) = find_git_root(root) else {
        return Vec::new();
    };
    if git_root == root {
        return Vec::new();
    }

    let mut dirs = Vec::new();
    let mut current = root;
    while let Some(parent) = current.parent() {
        if !parent.starts_with(&git_root) {
            break;
        }
        if parent.join(".gitignore").is_file() {
            dirs.push(parent.to_path_buf());
        }
        if parent == git_root {
            break;
        }
        current = parent;
    }
    dirs.reverse();
    dirs
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        let git_metadata = current.join(".git");
        if git_metadata.is_dir() || git_metadata.is_file() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn language_for_path(path: &Path) -> Option<&'static str> {
    let file_path = path.to_str()?;
    detect_language_for_source(file_path, "")
}

fn is_hard_excluded(
    path: &Path,
    relative_path: &str,
    db_path: &Path,
    exclusions: &DiscoveryExclusions,
) -> bool {
    path == db_path
        || exclusions.excludes(path)
        || relative_path
            .split('/')
            .any(|component| HARD_EXCLUDE_DIRS.contains(&component))
        || HARD_EXCLUDE_SUFFIXES
            .iter()
            .any(|suffix| relative_path.ends_with(suffix))
}

const HARD_EXCLUDE_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".julie",
    ".memories",
    "node_modules",
    "vendor",
    "target",
    "dist",
    "build",
    ".cache",
    // .NET build artifacts. `bin` is deliberately absent: many ecosystems keep real tracked
    // source under bin/, and a hard exclude cannot be whitelisted back the way ignore files can.
    "obj",
    "TestResults",
];

const HARD_EXCLUDE_SUFFIXES: &[&str] = &[
    ".min.js",
    ".bundle.js",
    ".generated.js",
    ".generated.jsx",
    ".generated.ts",
    ".generated.tsx",
    ".generated.d.ts",
];

fn is_oversized_source_file(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.len() > MAX_SOURCE_FILE_BYTES as u64)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn generated_typescript_is_hard_excluded() {
        let fixture = DiscoveryFixture::new();
        let generated = fixture.write(
            "src/config/schema.generated.ts",
            "export const schema = {};\n",
        );
        let selection = fixture.policy().select_file(&generated);

        assert_eq!(
            selection,
            FileSelection::Unsupported {
                reason: UnsupportedReason::HardExcluded
            }
        );
    }

    #[test]
    fn oversized_javascript_is_skipped_as_slow_file() {
        let fixture = DiscoveryFixture::new();
        let oversized = fixture.write(
            "assets/viewer-runtime.js",
            &"x".repeat(MAX_SOURCE_FILE_BYTES + 1),
        );
        let selection = fixture.policy().select_file(&oversized);

        assert_eq!(
            selection,
            FileSelection::Unsupported {
                reason: UnsupportedReason::Oversized
            }
        );
    }

    #[test]
    fn discover_records_slow_file_skip_for_oversized_source_file() {
        let fixture = DiscoveryFixture::new();
        let oversized = fixture.write("src/huge.rs", &"x".repeat(MAX_SOURCE_FILE_BYTES + 1));
        let summary = fixture.policy().discover();

        assert!(
            summary
                .slow_file_skips
                .iter()
                .any(|skip| skip.root_relative_path == oversized.root_relative_path),
            "expected a slow_file_skips entry for {}, got: {:?}",
            oversized.root_relative_path,
            summary.slow_file_skips
        );
        assert_eq!(summary.unsupported_files, 1);
    }

    #[test]
    fn discover_records_slow_file_skip_for_oversized_qmltypes_file() {
        let fixture = DiscoveryFixture::new();
        let oversized = fixture.write(
            "src/QtQuick.qmltypes",
            &"x".repeat(MAX_SOURCE_FILE_BYTES + 1),
        );
        let selection = fixture.policy().select_file(&oversized);
        let summary = fixture.policy().discover();

        assert_eq!(
            selection,
            FileSelection::Unsupported {
                reason: UnsupportedReason::Oversized
            }
        );
        assert!(
            summary
                .slow_file_skips
                .iter()
                .any(|skip| skip.root_relative_path == oversized.root_relative_path),
            "expected a slow_file_skips entry for {}, got: {:?}",
            oversized.root_relative_path,
            summary.slow_file_skips
        );
        assert!(
            summary
                .supported_files
                .iter()
                .all(|file| file.root_relative_path != oversized.root_relative_path)
        );
    }

    #[test]
    fn discover_selects_qmltypes_file_at_the_source_limit() {
        let fixture = DiscoveryFixture::new();
        let at_limit = fixture.write("src/QtQuick.qmltypes", &"x".repeat(MAX_SOURCE_FILE_BYTES));
        let selection = fixture.policy().select_file(&at_limit);
        let summary = fixture.policy().discover();

        assert_eq!(
            selection,
            FileSelection::Supported {
                language: "qml".to_string()
            }
        );
        assert!(
            summary
                .supported_files
                .iter()
                .any(|file| file.root_relative_path == at_limit.root_relative_path)
        );
        assert!(
            summary
                .slow_file_skips
                .iter()
                .all(|skip| skip.root_relative_path != at_limit.root_relative_path)
        );
    }

    #[test]
    fn discover_selects_exact_qmldir_basename_but_rejects_other_extensionless_files() {
        let fixture = DiscoveryFixture::new();
        let qmldir = fixture.write("src/qmldir", "module Example.Module\n");
        let readme = fixture.write("src/README", "not source\n");
        let policy = fixture.policy();

        assert_eq!(
            policy.select_file(&qmldir),
            FileSelection::Supported {
                language: "qmldir".to_string()
            }
        );
        assert_eq!(
            policy.select_file(&readme),
            FileSelection::Unsupported {
                reason: UnsupportedReason::UnsupportedExtension
            }
        );
    }

    #[test]
    fn vendor_directory_is_hard_excluded() {
        let fixture = DiscoveryFixture::new();
        let vendored = fixture.write("vendor/pkg/index.rs", "pub fn vendored() {}\n");
        let selection = fixture.policy().select_file(&vendored);

        assert_eq!(
            selection,
            FileSelection::Unsupported {
                reason: UnsupportedReason::HardExcluded
            }
        );
    }

    #[test]
    fn dotnet_test_results_directory_is_hard_excluded() {
        let fixture = DiscoveryFixture::new();
        let artifact = fixture.write("tests/TestResults/CoverageHelper.cs", "public class C {}\n");
        let selection = fixture.policy().select_file(&artifact);

        assert_eq!(
            selection,
            FileSelection::Unsupported {
                reason: UnsupportedReason::HardExcluded
            }
        );
    }

    #[test]
    fn dotnet_obj_directory_is_hard_excluded() {
        let fixture = DiscoveryFixture::new();
        let generated = fixture.write(
            "src/App/obj/Debug/net10.0/App.AssemblyInfo.cs",
            "[assembly: System.Reflection.AssemblyTitleAttribute(\"App\")]\n",
        );
        let selection = fixture.policy().select_file(&generated);

        assert_eq!(
            selection,
            FileSelection::Unsupported {
                reason: UnsupportedReason::HardExcluded
            }
        );
    }

    #[test]
    fn nested_workspace_inherits_git_root_gitignore() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let workspace = repo.join("packages").join("app");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(workspace.join("private_data")).unwrap();
        fs::write(repo.join(".gitignore"), "private_data/\n").unwrap();
        let ignored_path = workspace.join("private_data").join("secret.rs");
        fs::write(&ignored_path, "pub fn secret() {}\n").unwrap();

        let policy =
            DiscoveryPolicy::build(&workspace, &workspace.join("artifact.sqlite"), &[]).unwrap();
        let selection = policy.select_file(&FileTarget {
            absolute_path: ignored_path,
            root_relative_path: "private_data/secret.rs".to_string(),
        });

        assert_eq!(
            selection,
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
    }

    #[test]
    fn nested_workspace_inherits_git_root_gitignore_when_git_metadata_is_file() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let workspace = repo.join("packages").join("app");
        fs::create_dir_all(repo.join(".git-dir")).unwrap();
        fs::create_dir_all(workspace.join("private_data")).unwrap();
        fs::write(repo.join(".git"), "gitdir: .git-dir\n").unwrap();
        fs::write(repo.join(".gitignore"), "private_data/\n").unwrap();
        let ignored_path = workspace.join("private_data").join("secret.rs");
        fs::write(&ignored_path, "pub fn secret() {}\n").unwrap();

        let policy =
            DiscoveryPolicy::build(&workspace, &workspace.join("artifact.sqlite"), &[]).unwrap();
        let selection = policy.select_file(&FileTarget {
            absolute_path: ignored_path,
            root_relative_path: "private_data/secret.rs".to_string(),
        });

        assert_eq!(
            selection,
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
    }

    #[test]
    fn root_julieignore_is_honored() {
        let fixture = DiscoveryFixture::new();
        fixture.write(".julieignore", "*.tm.jsonl\n");
        let target = fixture.write("i18n/de.tm.jsonl", "{\"key\": \"value\"}\n");
        let selection = fixture.policy().select_file(&target);

        assert_eq!(
            selection,
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
    }

    #[test]
    fn nested_julieignore_is_honored() {
        let fixture = DiscoveryFixture::new();
        fixture.write("ui/.julieignore", "*.tm.jsonl\n");
        let target = fixture.write("ui/i18n/de.tm.jsonl", "{\"key\": \"value\"}\n");
        let selection = fixture.policy().select_file(&target);

        assert_eq!(
            selection,
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
    }

    #[test]
    fn nested_gitignore_patterns_are_relative_to_their_directory() {
        let fixture = DiscoveryFixture::new();
        fixture.write("ui/.gitignore", "i18n/\n");
        let inside = fixture.write("ui/i18n/de.tm.jsonl", "{\"key\": \"value\"}\n");
        let outside = fixture.write("docs/i18n/guide.md", "# Guide\n");

        let policy = fixture.policy();
        assert_eq!(
            policy.select_file(&inside),
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
        assert!(matches!(
            policy.select_file(&outside),
            FileSelection::Supported { .. }
        ));
    }

    #[test]
    fn nested_julieignore_patterns_are_relative_to_their_directory() {
        let fixture = DiscoveryFixture::new();
        fixture.write("ui/.julieignore", "i18n/\n");
        let inside = fixture.write("ui/i18n/de.tm.jsonl", "{\"key\": \"value\"}\n");
        let outside = fixture.write("docs/i18n/guide.md", "# Guide\n");

        let policy = fixture.policy();
        assert_eq!(
            policy.select_file(&inside),
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
        assert!(matches!(
            policy.select_file(&outside),
            FileSelection::Supported { .. }
        ));
    }

    #[test]
    fn julieignore_overrides_gitignore_in_same_directory() {
        let fixture = DiscoveryFixture::new();
        fixture.write("ui/.gitignore", "*.tm.jsonl\n");
        fixture.write("ui/.julieignore", "!de.tm.jsonl\n");
        let target = fixture.write("ui/de.tm.jsonl", "{\"key\": \"value\"}\n");

        assert!(matches!(
            fixture.policy().select_file(&target),
            FileSelection::Supported { .. }
        ));
    }

    #[test]
    fn nested_whitelist_overrides_root_ignore() {
        let fixture = DiscoveryFixture::new();
        fixture.write(".julieignore", "*.tm.jsonl\n");
        fixture.write("ui/.julieignore", "!de.tm.jsonl\n");
        let kept = fixture.write("ui/de.tm.jsonl", "{\"key\": \"value\"}\n");
        let dropped = fixture.write("docs/fr.tm.jsonl", "{\"key\": \"value\"}\n");

        let policy = fixture.policy();
        assert!(matches!(
            policy.select_file(&kept),
            FileSelection::Supported { .. }
        ));
        assert_eq!(
            policy.select_file(&dropped),
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
    }

    #[test]
    fn whitelist_cannot_reinclude_under_ignored_directory() {
        let fixture = DiscoveryFixture::new();
        fixture.write(".gitignore", "ui/\n");
        fixture.write("ui/.julieignore", "!keep.ts\n");
        let target = fixture.write("ui/keep.ts", "export const keep = 1;\n");

        assert_eq!(
            fixture.policy().select_file(&target),
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
    }

    #[test]
    fn nested_directory_whitelist_does_not_override_file_rule() {
        let fixture = DiscoveryFixture::new();
        fixture.write(".julieignore", "*.gen.ts\n");
        fixture.write("ui/.gitignore", "!keep/\n");
        let target = fixture.write("ui/keep/app.gen.ts", "export const app = 1;\n");

        assert_eq!(
            fixture.policy().select_file(&target),
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
    }

    #[test]
    fn ignore_file_excludes_win_over_nested_whitelist() {
        let fixture = DiscoveryFixture::new();
        fixture.write("ui/.julieignore", "!de.tm.jsonl\n");
        let target = fixture.write("ui/de.tm.jsonl", "{\"key\": \"value\"}\n");

        let policy = fixture.policy_with_ignore_lines("*.tm.jsonl\n");
        assert_eq!(
            policy.select_file(&target),
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
    }

    #[test]
    fn ignore_file_whitelist_wins_over_in_tree_ignore() {
        let fixture = DiscoveryFixture::new();
        fixture.write("ui/.gitignore", "*.gen.ts\n");
        let target = fixture.write("ui/special.gen.ts", "export const special = 1;\n");

        let policy = fixture.policy_with_ignore_lines("!special.gen.ts\n");
        assert!(matches!(
            policy.select_file(&target),
            FileSelection::Supported { .. }
        ));
    }

    #[test]
    fn ignore_file_whitelist_wins_over_root_ignore() {
        let fixture = DiscoveryFixture::new();
        fixture.write(".gitignore", "secrets/\n");
        let target = fixture.write("secrets/key.ts", "export const key = 1;\n");

        let policy = fixture.policy_with_ignore_lines("!secrets/\n");
        assert!(matches!(
            policy.select_file(&target),
            FileSelection::Supported { .. }
        ));
    }

    #[test]
    fn ancestor_gitignore_anchored_patterns_apply_relative_to_ancestor() {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let workspace = repo.join("sub");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(workspace.join("docs")).unwrap();
        fs::create_dir_all(workspace.join("cache")).unwrap();
        fs::write(repo.join(".gitignore"), "/docs\nsub/cache/\n").unwrap();
        let docs_path = workspace.join("docs").join("page.rs");
        let cache_path = workspace.join("cache").join("entry.rs");
        fs::write(&docs_path, "pub fn page() {}\n").unwrap();
        fs::write(&cache_path, "pub fn entry() {}\n").unwrap();

        let policy =
            DiscoveryPolicy::build(&workspace, &workspace.join("artifact.sqlite"), &[]).unwrap();
        assert!(matches!(
            policy.select_file(&FileTarget {
                absolute_path: docs_path,
                root_relative_path: "docs/page.rs".to_string(),
            }),
            FileSelection::Supported { .. }
        ));
        assert_eq!(
            policy.select_file(&FileTarget {
                absolute_path: cache_path,
                root_relative_path: "cache/entry.rs".to_string(),
            }),
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
    }

    #[test]
    fn ignore_files_in_dot_directories_are_honored() {
        let fixture = DiscoveryFixture::new();
        fixture.write(".devcontainer/.gitignore", "cache/\n");
        let target = fixture.write(".devcontainer/cache/tool.ts", "export const tool = 1;\n");

        assert_eq!(
            fixture.policy().select_file(&target),
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
    }

    #[test]
    fn deeply_nested_ignore_files_are_honored() {
        let fixture = DiscoveryFixture::new();
        let dir = "a/b/c/d/e/f/g/h/i";
        fixture.write(&format!("{dir}/.gitignore"), "*.gen.rs\n");
        let target = fixture.write(&format!("{dir}/skip.gen.rs"), "pub fn skip() {}\n");

        assert_eq!(
            fixture.policy().select_file(&target),
            FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_nested_ignore_file_is_reported() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = DiscoveryFixture::new();
        let ignore_file = fixture.write("ui/.gitignore", "*.gen.ts\n");
        fs::set_permissions(
            &ignore_file.absolute_path,
            fs::Permissions::from_mode(0o000),
        )
        .unwrap();

        let summary = fixture.policy().discover();
        fs::set_permissions(
            &ignore_file.absolute_path,
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(
            summary
                .errors
                .iter()
                .any(|error| error.message.contains("ignore file")),
            "expected an ignore-file warning in discovery errors, got: {:?}",
            summary.errors
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directories_are_not_traversed_for_ignore_files() {
        let fixture = DiscoveryFixture::new();
        fixture.write("real/code.rs", "pub fn code() {}\n");
        std::os::unix::fs::symlink(fixture.root(), fixture.root().join("loop")).unwrap();

        let summary = fixture.policy().discover();
        assert_eq!(summary.supported_files.len(), 1);
    }

    #[test]
    fn a_progress_file_inside_the_root_is_hard_excluded() {
        let fixture = DiscoveryFixture::new();
        fixture.write("src/code.rs", "pub fn code() {}\n");
        let progress = fixture.write("scan.jsonl", "{\"phase\":\"discovery\"}\n");

        let with_exclusion = fixture
            .policy_excluding(DiscoveryExclusions {
                progress_path: Some(progress.absolute_path.clone()),
                ..DiscoveryExclusions::default()
            })
            .discover();
        assert_eq!(with_exclusion.supported_files.len(), 1);
        assert_eq!(
            with_exclusion.unsupported_files, 0,
            "the progress file must not be counted at all"
        );

        let without_exclusion = fixture.policy().discover();
        assert_eq!(
            without_exclusion.supported_files.len(),
            2,
            "the exclusion, not the extension, is what keeps the progress file out"
        );
    }

    #[test]
    fn selecting_the_progress_file_directly_reports_it_as_hard_excluded() {
        let fixture = DiscoveryFixture::new();
        let progress = fixture.write("scan.jsonl", "{\"phase\":\"discovery\"}\n");

        let selection = fixture
            .policy_excluding(DiscoveryExclusions {
                progress_path: Some(progress.absolute_path.clone()),
                ..DiscoveryExclusions::default()
            })
            .select_file(&progress);

        assert_eq!(
            selection,
            FileSelection::Unsupported {
                reason: UnsupportedReason::HardExcluded
            }
        );
    }

    #[test]
    fn a_spool_directory_inside_the_root_is_skipped_whole() {
        let fixture = DiscoveryFixture::new();
        fixture.write("src/code.rs", "pub fn code() {}\n");
        let spool = fixture.write(
            "spools/julie-extract-scan-owned-spool-7-9.jsonl",
            "{\"path\":\"src/code.rs\"}\n",
        );
        fixture.write("spools/julie-extract-scan-owned-spool-7-9.jsonl.lock", "");

        let excluded = fixture
            .policy_excluding(DiscoveryExclusions {
                spool_dir: Some(fixture.root().join("spools")),
                ..DiscoveryExclusions::default()
            })
            .discover();
        assert_eq!(excluded.supported_files.len(), 1);
        assert_eq!(
            excluded.unsupported_files, 0,
            "a skipped spool directory must not be counted at all"
        );

        let without_exclusion = fixture.policy().discover();
        assert_eq!(
            without_exclusion.supported_files.len(),
            2,
            "jsonl is a supported extension, so only the exclusion keeps a spool out"
        );
        assert_eq!(
            fixture
                .policy_excluding(DiscoveryExclusions {
                    spool_dir: Some(fixture.root().join("spools")),
                    ..DiscoveryExclusions::default()
                })
                .select_file(&spool),
            FileSelection::Unsupported {
                reason: UnsupportedReason::HardExcluded
            }
        );
    }

    #[test]
    fn a_spool_directory_that_is_the_root_still_keeps_spool_files_out() {
        let fixture = DiscoveryFixture::new();
        fixture.write("code.rs", "pub fn code() {}\n");
        fixture.write("julie-extract-scan-owned-spool-7-9.jsonl", "{}\n");
        fixture.write("julie-extract-scan-owned-spool-7-9.jsonl.lock", "");
        fixture.write("julie-extract-scan-spool-7-9.jsonl", "{}\n");

        let summary = fixture
            .policy_excluding(DiscoveryExclusions {
                spool_dir: Some(fixture.root().to_path_buf()),
                ..DiscoveryExclusions::default()
            })
            .discover();

        assert_eq!(summary.supported_files.len(), 1);
        assert_eq!(summary.unsupported_files, 0);
    }

    #[test]
    fn a_spool_shaped_file_outside_the_spool_directory_is_still_scanned() {
        let fixture = DiscoveryFixture::new();
        fixture.write("src/julie-extract-scan-owned-spool-7-9.jsonl", "{}\n");

        let summary = fixture
            .policy_excluding(DiscoveryExclusions {
                spool_dir: Some(fixture.root().join("spools")),
                ..DiscoveryExclusions::default()
            })
            .discover();

        assert_eq!(
            summary.supported_files.len(),
            1,
            "the name shape only suppresses files the scan itself could have written"
        );
    }

    struct DiscoveryFixture {
        temp: TempDir,
    }

    impl DiscoveryFixture {
        fn new() -> Self {
            Self {
                temp: TempDir::new().unwrap(),
            }
        }

        fn root(&self) -> &Path {
            self.temp.path()
        }

        fn policy(&self) -> DiscoveryPolicy {
            DiscoveryPolicy::build(self.root(), &self.root().join("artifact.sqlite"), &[]).unwrap()
        }

        fn policy_excluding(&self, exclusions: DiscoveryExclusions) -> DiscoveryPolicy {
            DiscoveryPolicy::build_excluding(
                self.root(),
                &self.root().join("artifact.sqlite"),
                exclusions,
                &[],
            )
            .unwrap()
        }

        fn policy_with_ignore_lines(&self, lines: &str) -> DiscoveryPolicy {
            let ignore_path = self.root().join("caller.ignore");
            fs::write(&ignore_path, lines).unwrap();
            DiscoveryPolicy::build(
                self.root(),
                &self.root().join("artifact.sqlite"),
                &[ignore_path],
            )
            .unwrap()
        }

        fn write(&self, path: &str, contents: &str) -> FileTarget {
            let absolute_path = self.root().join(path);
            if let Some(parent) = absolute_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&absolute_path, contents).unwrap();
            FileTarget {
                absolute_path,
                root_relative_path: path.to_string(),
            }
        }
    }
}
