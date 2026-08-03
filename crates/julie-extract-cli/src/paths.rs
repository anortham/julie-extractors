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

/// Resolve `--spool-dir`. The directory is created when missing because callers
/// hand out a per-workspace scratch path that does not exist on a fresh checkout.
pub fn canonicalize_spool_dir(path: &Path) -> Result<PathBuf, PathPolicyError> {
    if !path.exists()
        && let Err(error) = std::fs::create_dir_all(path)
    {
        return Err(invalid_path(
            path,
            format!("spool directory could not be created: {error}"),
        ));
    }
    let canonical = path.canonicalize().map_err(|error| {
        invalid_path(
            path,
            format!("spool directory could not be canonicalized: {error}"),
        )
    })?;
    if !canonical.is_dir() {
        return Err(invalid_path(path, "spool directory is not a directory"));
    }
    Ok(canonical)
}

/// The suffix `--progress-file` names must carry. Creating the progress file
/// truncates it, so the flag would otherwise destroy whatever the caller's
/// templating happened to point it at — a source file, a lockfile, an export.
/// Requiring a name nothing else uses makes that class impossible instead of
/// guarding one instance of it at a time. [`is_progress_file_name`] owns which
/// spellings of it are accepted.
pub const PROGRESS_FILE_EXTENSION: &str = "progress";

/// Resolve `--progress-file`, resolved exactly as [`canonicalize_db_path`]
/// resolves the artifact so the two can be compared for collisions.
///
/// The final component is resolved too when it already exists. Canonicalizing
/// only the parent would leave `--db <symlink>` resolved to the link's target
/// while `--progress-file <same symlink>` kept the link's own path, so
/// [`reject_progress_file_collision`] would compare two spellings of one file,
/// pass, and truncate the artifact the link points at.
///
/// A path whose final component IS a symbolic link is then refused outright.
/// The name rule can only be applied to what the link resolves to, so the link
/// itself is always a second name for a file the caller did not spell out, and
/// creating the progress file writes straight through it.
pub fn canonicalize_progress_file(path: &Path) -> Result<PathBuf, PathPolicyError> {
    let resolved = resolve_progress_file(path)?;
    if !is_progress_file_name(&resolved) {
        return Err(invalid_path(
            path,
            format!(
                "progress file must be named `.{PROGRESS_FILE_EXTENSION}` or end in \
                 `.{PROGRESS_FILE_EXTENSION}`, ignoring case"
            ),
        ));
    }
    if is_symbolic_link(path) {
        return Err(invalid_path(
            path,
            "progress file must not be a symbolic link",
        ));
    }
    Ok(resolved)
}

fn is_symbolic_link(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
}

/// Whether a resolved path carries an accepted `--progress-file` name: the bare
/// dotfile `.progress`, or any name whose extension is `progress`.
///
/// The bare dotfile is accepted because Rust reads a leading-dot-only name as a
/// stem with no extension, so `.progress` — the most obvious spelling of a
/// hidden progress file — would otherwise be refused with a message saying it
/// needs the extension it plainly has.
///
/// Case is ignored deliberately. The rule's whole job is that the suffix belongs
/// to nothing else, which a case variant does not weaken; and on a
/// case-insensitive volume `scan.PROGRESS` and `scan.progress` ARE one file, so
/// accepting one spelling while refusing the other would make the same argv work
/// on one machine and fail on the next.
fn is_progress_file_name(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    name.eq_ignore_ascii_case(&format!(".{PROGRESS_FILE_EXTENSION}"))
        || path
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case(PROGRESS_FILE_EXTENSION))
}

fn resolve_progress_file(path: &Path) -> Result<PathBuf, PathPolicyError> {
    if path.exists() {
        return path.canonicalize().map_err(|error| {
            invalid_path(
                path,
                format!("progress file could not be canonicalized: {error}"),
            )
        });
    }

    let file_name = path
        .file_name()
        .ok_or_else(|| invalid_path(path, "progress file path must include a file name"))?;
    let parent = non_empty_parent(path).unwrap_or_else(|| Path::new("."));
    let parent = parent.canonicalize().map_err(|error| {
        invalid_path(
            path,
            format!("progress file parent could not be canonicalized: {error}"),
        )
    })?;
    Ok(parent.join(file_name))
}

/// The artifact database's write-ahead-log and shared-memory sidecars. A
/// progress file that IS one of them wrecks the artifact just as thoroughly as
/// one that is the artifact.
const ARTIFACT_SIDECAR_SUFFIXES: [&str; 2] = ["-wal", "-shm"];

/// Refuse a `--progress-file` that is the artifact database or one of its
/// sidecars. The progress file is opened with `File::create`, which truncates,
/// before the artifact is opened at all — a templating bug pointing both flags
/// at one file would otherwise destroy the artifact before the scan had even
/// validated that it could run.
///
/// The comparison is file IDENTITY, not path spelling. Two earlier rounds of
/// this guard compared paths — first literally, then after resolving symlinks —
/// and each left the same class open, because one file can always be reached
/// through a name the other spelling does not equal: `ln artifact.sqlite
/// scan.progress` gives one inode two names that both satisfy the name rule,
/// and a case-insensitive volume answers to `INDEX.PROGRESS` and
/// `index.progress` alike. [`is_one_file`] is what closes that class; see it for
/// what the answer costs off Unix.
///
/// The sidecar arms are reachable for the same reason: `artifact.sqlite-wal`
/// cannot be named on the command line (the name rule refuses it), but a hard
/// link named `scan.progress` pointing at it can.
pub fn reject_progress_file_collision(
    progress_path: &Path,
    db_path: &Path,
) -> Result<(), PathPolicyError> {
    if is_one_file(progress_path, db_path) {
        return Err(invalid_path(
            progress_path,
            "progress file must not be the artifact database",
        ));
    }
    for suffix in ARTIFACT_SIDECAR_SUFFIXES {
        if is_one_file(progress_path, &artifact_sidecar(db_path, suffix)) {
            return Err(invalid_path(
                progress_path,
                format!("progress file must not be the artifact database's `{suffix}` sidecar"),
            ));
        }
    }
    Ok(())
}

fn artifact_sidecar(db_path: &Path, suffix: &str) -> PathBuf {
    let mut sidecar = db_path.as_os_str().to_os_string();
    sidecar.push(suffix);
    PathBuf::from(sidecar)
}

/// Whether two paths name one file.
///
/// The answer is file identity on every supported platform, and it is exact
/// whenever both paths exist: device plus inode on Unix, volume serial plus file
/// index on Windows. That is the only thing that sees through a hard link, and
/// through a case-variant spelling on a case-insensitive volume. Paths that do
/// not both exist cannot be compared that way and answer `false`, which is why
/// the caller runs this again once the progress file has been created.
///
/// The Windows half comes from `same-file` rather than from `std`, because
/// `volume_serial_number` and `file_index` on `std::os::windows::fs::MetadataExt`
/// are behind the unstable `windows_by_handle` feature and the workspace sets
/// `unsafe_code = "forbid"`, which cannot be relaxed per crate — so the Win32
/// call behind them is unreachable from here directly. The crate is already in
/// the lock graph (`ignore` → `walkdir` → `same-file`), so this costs no new
/// build unit and no new supply-chain surface. It replaced a case-insensitive
/// path comparison that could not see an NTFS hard link, and therefore let one
/// truncate a multi-gigabyte artifact.
///
/// Identity is also the conservative answer when it cannot be established: an
/// unopenable path answers `false` and the guard permits the argv, which is the
/// same outcome as before and is bounded by the fact that a path this process
/// cannot open for reading is one it cannot truncate either.
fn is_one_file(left: &Path, right: &Path) -> bool {
    left == right || same_file::is_same_file(left, right).unwrap_or(false)
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

pub fn invalid_path(path: impl AsRef<Path>, message: impl Into<String>) -> PathPolicyError {
    PathPolicyError::InvalidPath {
        path: path.as_ref().display().to_string(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn accepted(name: &str) -> bool {
        let temp = TempDir::new().unwrap();
        canonicalize_progress_file(&temp.path().join(name)).is_ok()
    }

    #[test]
    fn a_progress_file_named_only_progress_is_accepted() {
        assert!(accepted(".progress"));
    }

    #[test]
    fn a_progress_file_name_is_matched_without_case() {
        for name in [
            "scan.PROGRESS",
            "scan.Progress",
            ".PROGRESS",
            "SCAN.progress",
        ] {
            assert!(accepted(name), "{name} must be accepted");
        }
    }

    #[test]
    fn a_name_that_is_not_a_progress_file_is_refused() {
        for name in [
            "lib.rs",
            "scan.progresss",
            "progress",
            "scan.progress.bak",
            "artifact.sqlite-wal",
        ] {
            assert!(!accepted(name), "{name} must be refused");
        }
    }

    #[test]
    fn the_refusal_message_states_both_accepted_spellings() {
        let temp = TempDir::new().unwrap();
        let Err(PathPolicyError::InvalidPath { message, .. }) =
            canonicalize_progress_file(&temp.path().join("lib.rs"))
        else {
            panic!("a non-progress name must be refused as an invalid path");
        };
        assert!(message.contains("`.progress`"), "{message}");
        assert!(message.contains("ignoring case"), "{message}");
    }

    #[test]
    fn the_artifact_is_refused_and_a_sidecar_name_never_gets_past_the_name_rule() {
        let db = Path::new("/ws/index.progress");

        assert!(reject_progress_file_collision(db, db).is_err());
        assert!(reject_progress_file_collision(Path::new("/ws/scan.progress"), db).is_ok());
        assert!(
            !accepted("index.progress-wal") && !accepted("index.progress-shm"),
            "a sidecar cannot be named on the command line; only a link can reach one"
        );
    }

    #[test]
    fn two_distinct_files_are_not_a_collision() {
        let temp = TempDir::new().unwrap();
        let db = temp.path().join("artifact.sqlite");
        let progress = temp.path().join("scan.progress");
        std::fs::write(&db, b"artifact").unwrap();
        std::fs::write(&progress, b"").unwrap();

        assert!(reject_progress_file_collision(&progress, &db).is_ok());
    }

    #[test]
    fn a_progress_file_hard_linked_to_the_artifact_is_refused() {
        let temp = TempDir::new().unwrap();
        let db = temp.path().join("artifact.sqlite");
        std::fs::write(&db, b"artifact").unwrap();
        let progress = temp.path().join("scan.progress");
        std::fs::hard_link(&db, &progress).unwrap();

        assert!(matches!(
            reject_progress_file_collision(&progress, &db),
            Err(PathPolicyError::InvalidPath { .. })
        ));
    }

    #[test]
    fn a_progress_file_hard_linked_to_an_artifact_sidecar_is_refused() {
        for suffix in ["-wal", "-shm"] {
            let temp = TempDir::new().unwrap();
            let db = temp.path().join("artifact.sqlite");
            std::fs::write(&db, b"artifact").unwrap();
            let sidecar = temp.path().join(format!("artifact.sqlite{suffix}"));
            std::fs::write(&sidecar, b"sidecar").unwrap();
            let progress = temp.path().join("scan.progress");
            std::fs::hard_link(&sidecar, &progress).unwrap();

            let Err(PathPolicyError::InvalidPath { message, .. }) =
                reject_progress_file_collision(&progress, &db)
            else {
                panic!("a hard link to the {suffix} sidecar must be refused");
            };
            assert!(message.contains(suffix), "{message}");
        }
    }

    #[test]
    fn a_case_variant_of_the_artifact_is_refused_where_the_filesystem_ignores_case() {
        let temp = TempDir::new().unwrap();
        if !ignores_case(temp.path()) {
            return;
        }
        let db = temp.path().join("index.progress");
        std::fs::write(&db, b"artifact").unwrap();

        assert!(reject_progress_file_collision(&temp.path().join("INDEX.PROGRESS"), &db).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_progress_path_is_refused() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("real.progress");
        std::fs::write(&target, b"").unwrap();
        let link = temp.path().join("link.progress");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let Err(PathPolicyError::InvalidPath { message, .. }) = canonicalize_progress_file(&link)
        else {
            panic!("a symlinked progress path must be refused");
        };
        assert!(message.contains("symbolic link"), "{message}");
    }

    fn ignores_case(dir: &Path) -> bool {
        let probe = dir.join("case-probe");
        std::fs::write(&probe, b"").unwrap();
        let ignored = dir.join("CASE-PROBE").exists();
        std::fs::remove_file(&probe).unwrap();
        ignored
    }
}
