use std::fs;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use julie_extractors::detect_language_from_extension;

use crate::paths::{FileTarget, PathPolicyError, canonicalize_ignore_file};

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
}

#[derive(Debug, Clone)]
pub struct DiscoveryPolicy {
    root: PathBuf,
    db_path: PathBuf,
    matcher: Gitignore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySummary {
    pub supported_files: Vec<FileTarget>,
    pub unsupported_files: usize,
    pub errors: Vec<DiscoveryError>,
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
        let matcher = build_ignore_matcher(root, ignore_files)?;
        Ok(Self {
            root: root.to_path_buf(),
            db_path: db_path.to_path_buf(),
            matcher,
        })
    }

    pub fn select_file(&self, target: &FileTarget) -> FileSelection {
        if is_hard_excluded(
            &target.absolute_path,
            &target.root_relative_path,
            &self.db_path,
        ) {
            return FileSelection::Unsupported {
                reason: UnsupportedReason::HardExcluded,
            };
        }
        if self
            .matcher
            .matched_path_or_any_parents(&target.root_relative_path, false)
            .is_ignore()
        {
            return FileSelection::Unsupported {
                reason: UnsupportedReason::Ignored,
            };
        }
        match language_for_path(&target.absolute_path) {
            Some(language) => {
                if is_oversized_source_file(&target.absolute_path) {
                    return FileSelection::Unsupported {
                        reason: UnsupportedReason::HardExcluded,
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

    pub fn discover(&self) -> DiscoverySummary {
        let mut summary = DiscoverySummary {
            supported_files: Vec::new(),
            unsupported_files: 0,
            errors: Vec::new(),
        };
        self.discover_dir(&self.root, &mut summary);
        summary
            .supported_files
            .sort_by(|left, right| left.root_relative_path.cmp(&right.root_relative_path));
        summary
    }

    fn discover_dir(&self, dir: &Path, summary: &mut DiscoverySummary) {
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
                if is_hard_excluded(&path, &relative, &self.db_path)
                    || self
                        .matcher
                        .matched_path_or_any_parents(&relative, true)
                        .is_ignore()
                {
                    continue;
                }
                self.discover_dir(&path, summary);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let target = FileTarget {
                absolute_path: path,
                root_relative_path: relative,
            };
            match self.select_file(&target) {
                FileSelection::Supported { .. } => summary.supported_files.push(target),
                FileSelection::Unsupported { .. } => summary.unsupported_files += 1,
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

fn build_ignore_matcher(
    root: &Path,
    ignore_files: &[PathBuf],
) -> Result<Gitignore, PathPolicyError> {
    let mut builder = GitignoreBuilder::new(root);

    for path in root_ignore_files(root) {
        if path.is_file() {
            builder.add(&path);
        }
    }

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

    for pattern in HARD_EXCLUDE_PATTERNS {
        builder
            .add_line(None, pattern)
            .map_err(|error| PathPolicyError::InvalidPath {
                path: root.display().to_string(),
                message: format!("invalid built-in ignore pattern {pattern}: {error}"),
            })?;
    }

    builder
        .build()
        .map_err(|error| PathPolicyError::InvalidPath {
            path: root.display().to_string(),
            message: format!("ignore matcher could not be built: {error}"),
        })
}

fn root_ignore_files(root: &Path) -> Vec<PathBuf> {
    let mut files = ancestor_gitignore_files(root);
    files.push(root.join(".gitignore"));
    files.push(root.join(".julieignore"));
    collect_nested_gitignore(root, 8, &mut files);
    files
}

fn ancestor_gitignore_files(root: &Path) -> Vec<PathBuf> {
    let Some(git_root) = find_git_root(root) else {
        return Vec::new();
    };
    if git_root == root {
        return Vec::new();
    }

    let mut files = Vec::new();
    let mut current = root;
    while let Some(parent) = current.parent() {
        if !parent.starts_with(&git_root) {
            break;
        }
        let candidate = parent.join(".gitignore");
        if candidate.is_file() {
            files.push(candidate);
        }
        if parent == git_root {
            break;
        }
        current = parent;
    }
    files.reverse();
    files
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

fn collect_nested_gitignore(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if name.starts_with('.') || HARD_EXCLUDE_DIRS.contains(&name) {
                continue;
            }
            collect_nested_gitignore(&path, depth - 1, files);
        } else if path.file_name().and_then(|name| name.to_str()) == Some(".gitignore") {
            files.push(path);
        }
    }
}

fn language_for_path(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?;
    detect_language_from_extension(extension)
}

fn is_hard_excluded(path: &Path, relative_path: &str, db_path: &Path) -> bool {
    path == db_path
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

const MAX_SOURCE_FILE_BYTES: usize = 1024 * 1024;

fn is_oversized_source_file(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.len() > MAX_SOURCE_FILE_BYTES as u64)
        .unwrap_or(false)
}

const HARD_EXCLUDE_PATTERNS: &[&str] = &[
    ".git/",
    ".hg/",
    ".svn/",
    ".julie/",
    ".memories/",
    "node_modules/",
    "vendor/",
    "target/",
    "dist/",
    "build/",
    ".cache/",
    "*.min.js",
    "*.bundle.js",
    "*.generated.js",
    "*.generated.jsx",
    "*.generated.ts",
    "*.generated.tsx",
    "*.generated.d.ts",
];

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
    fn oversized_javascript_is_hard_excluded() {
        let fixture = DiscoveryFixture::new();
        let oversized = fixture.write(
            "assets/viewer-runtime.js",
            &"x".repeat(MAX_SOURCE_FILE_BYTES + 1),
        );
        let selection = fixture.policy().select_file(&oversized);

        assert_eq!(
            selection,
            FileSelection::Unsupported {
                reason: UnsupportedReason::HardExcluded
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
