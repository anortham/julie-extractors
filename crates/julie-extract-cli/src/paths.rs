use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTarget {
    pub absolute_path: PathBuf,
    pub root_relative_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathPolicyError {
    InvalidPath {
        path: String,
        message: String,
    },
    FileOutsideRoot {
        path: String,
        root_path: String,
    },
    FileNotFound {
        path: String,
        root_relative_path: Option<String>,
    },
}

pub fn canonicalize_root(path: &Path) -> Result<PathBuf, PathPolicyError> {
    let canonical = path.canonicalize().map_err(|error| {
        invalid_path(
            path,
            format!("source root could not be canonicalized: {error}"),
        )
    })?;
    if !canonical.is_dir() {
        return Err(invalid_path(path, "source root is not a directory"));
    }
    Ok(canonical)
}

pub fn canonicalize_db_path(path: &Path) -> Result<PathBuf, PathPolicyError> {
    if path.exists() {
        return path.canonicalize().map_err(|error| {
            invalid_path(
                path,
                format!("SQLite artifact could not be canonicalized: {error}"),
            )
        });
    }

    let file_name = path
        .file_name()
        .ok_or_else(|| invalid_path(path, "SQLite artifact path must include a file name"))?;
    let parent = non_empty_parent(path).unwrap_or_else(|| Path::new("."));
    let parent = parent.canonicalize().map_err(|error| {
        invalid_path(
            path,
            format!("SQLite artifact parent could not be canonicalized: {error}"),
        )
    })?;
    Ok(parent.join(file_name))
}

pub fn canonicalize_ignore_file(path: &Path) -> Result<PathBuf, PathPolicyError> {
    let canonical = path.canonicalize().map_err(|error| {
        invalid_path(
            path,
            format!("ignore file could not be canonicalized: {error}"),
        )
    })?;
    if !canonical.is_file() {
        return Err(invalid_path(path, "ignore file is not a regular file"));
    }
    Ok(canonical)
}

pub fn canonicalize_update_file(root: &Path, file: &Path) -> Result<FileTarget, PathPolicyError> {
    let lexical = lexical_normalize(&candidate_path(root, file));
    if lexical.exists() {
        let canonical = lexical.canonicalize().map_err(|error| {
            invalid_path(
                file,
                format!("update target could not be canonicalized: {error}"),
            )
        })?;
        if !canonical.starts_with(root) {
            return Err(PathPolicyError::FileOutsideRoot {
                path: canonical.display().to_string(),
                root_path: root.display().to_string(),
            });
        }
        if !canonical.is_file() {
            return Err(PathPolicyError::FileNotFound {
                path: canonical.display().to_string(),
                root_relative_path: root_relative_unix(root, &canonical).ok(),
            });
        }
        return file_target(root, canonical);
    }

    if !lexical.starts_with(root) {
        return Err(PathPolicyError::FileOutsideRoot {
            path: lexical.display().to_string(),
            root_path: root.display().to_string(),
        });
    }
    Err(PathPolicyError::FileNotFound {
        path: lexical.display().to_string(),
        root_relative_path: root_relative_unix(root, &lexical).ok(),
    })
}

pub fn normalize_delete_file(root: &Path, file: &Path) -> Result<FileTarget, PathPolicyError> {
    let lexical = lexical_normalize(&candidate_path(root, file));
    let accepted = if lexical.exists() {
        lexical.canonicalize().map_err(|error| {
            invalid_path(
                file,
                format!("delete target could not be canonicalized: {error}"),
            )
        })?
    } else {
        lexical
    };
    file_target(root, accepted)
}

pub fn root_relative_unix(root: &Path, absolute_path: &Path) -> Result<String, PathPolicyError> {
    let relative =
        absolute_path
            .strip_prefix(root)
            .map_err(|_| PathPolicyError::FileOutsideRoot {
                path: absolute_path.display().to_string(),
                root_path: root.display().to_string(),
            })?;

    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(os_str_to_utf8(part, absolute_path)?),
            Component::CurDir => {}
            _ => {
                return Err(invalid_path(
                    absolute_path,
                    "accepted file path must normalize to a relative file path",
                ));
            }
        }
    }

    if parts.is_empty() {
        return Err(invalid_path(absolute_path, "file path cannot be the root"));
    }
    Ok(parts.join("/"))
}

fn file_target(root: &Path, absolute_path: PathBuf) -> Result<FileTarget, PathPolicyError> {
    let root_relative_path = root_relative_unix(root, &absolute_path)?;
    Ok(FileTarget {
        absolute_path,
        root_relative_path,
    })
}

fn candidate_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn non_empty_parent(path: &Path) -> Option<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn os_str_to_utf8<'a>(part: &'a OsStr, path: &Path) -> Result<&'a str, PathPolicyError> {
    part.to_str().ok_or_else(|| {
        invalid_path(
            path,
            "stored root-relative paths must be valid UTF-8 strings",
        )
    })
}

fn invalid_path(path: impl AsRef<Path>, message: impl Into<String>) -> PathPolicyError {
    PathPolicyError::InvalidPath {
        path: path.as_ref().display().to_string(),
        message: message.into(),
    }
}
