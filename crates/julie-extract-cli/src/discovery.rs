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
            Some(language) => FileSelection::Supported {
                language: language.to_string(),
            },
            None => FileSelection::Unsupported {
                reason: UnsupportedReason::UnsupportedExtension,
            },
        }
    }

    pub fn discover(&self) -> DiscoverySummary {
        let mut summary = DiscoverySummary {
            supported_files: Vec::new(),
            unsupported_files: 0,
        };
        self.discover_dir(&self.root, &mut summary);
        summary
            .supported_files
            .sort_by(|left, right| left.root_relative_path.cmp(&right.root_relative_path));
        summary
    }

    fn discover_dir(&self, dir: &Path, summary: &mut DiscoverySummary) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(relative) = crate::paths::root_relative_unix(&self.root, &path) else {
                continue;
            };
            if path.is_dir() {
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
            if !path.is_file() {
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
    let mut files = vec![root.join(".gitignore"), root.join(".julieignore")];
    collect_nested_gitignore(root, 8, &mut files);
    files
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
    "target",
    "dist",
    "build",
    ".cache",
];

const HARD_EXCLUDE_SUFFIXES: &[&str] = &[".min.js", ".bundle.js"];

const HARD_EXCLUDE_PATTERNS: &[&str] = &[
    ".git/",
    ".hg/",
    ".svn/",
    ".julie/",
    ".memories/",
    "node_modules/",
    "target/",
    "dist/",
    "build/",
    ".cache/",
    "*.min.js",
    "*.bundle.js",
];
