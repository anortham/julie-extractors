use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::coordinator::{PidStatus, process_status};
use super::pragmas::{
    PragmaError, WriterPragmaProfile, configure_writer_pragmas, validate_store_file_pragmas,
};
use super::schema::validate_store_schema_version;
use super::{StoreSchemaError, create_coordinator_schema, create_store_schema};

const INITIAL_GENERATION: &str = "gen-001";
const PARTIAL_GENERATION: &str = ".gen-001.partial";
const PARTIAL_OWNER_FILE: &str = "OWNER.json";

/// Durable ownership of an unpublished generation directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartialGenerationOwner {
    pub run_id: String,
    pub owner_id: String,
    pub owner_pid: u32,
    pub fencing_token: i64,
    pub expires_at: i64,
}

/// Paths belonging to one published store generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLayout {
    root: PathBuf,
    generation_name: String,
    generation_dir: PathBuf,
    store_db: PathBuf,
    coordinator_db: PathBuf,
    spool_dir: PathBuf,
    scratch_dir: PathBuf,
    bases_dir: PathBuf,
}

impl StoreLayout {
    /// Creates the first generation and publishes `CURRENT` after durable initialization.
    pub fn create(
        root: impl AsRef<Path>,
        family_id: &str,
        creator_version: &str,
        extraction_identity_epoch: u32,
    ) -> Result<Self, StoreLayoutError> {
        let root = root.as_ref();
        fs::create_dir_all(root)?;
        let root = root.canonicalize()?;
        let coordinator_db = root.join("coord.db");
        validate_owned_coordinator_path(&root, &coordinator_db)?;
        if !root.join("CURRENT").exists() {
            let generations = named_generations(&root)?;
            if !generations.is_empty() {
                return Err(StoreLayoutError::CurrentRecoveryRequired { generations });
            }
        }
        reap_scaffolding(&root)?;
        if root.join("CURRENT").exists() {
            let layout = Self::open(&root)?;
            validate_existing_generation(layout.store_db(), family_id)?;
            return Ok(layout);
        }
        let spool_dir = root.join("spool");
        let scratch_dir = root.join("scratch");
        fs::create_dir_all(&spool_dir)?;
        fs::create_dir_all(&scratch_dir)?;

        let generation_dir = root.join(INITIAL_GENERATION);
        let existing_store_db = match fs::symlink_metadata(&generation_dir) {
            Ok(_) => {
                let resolved_generation = generation_dir.canonicalize()?;
                ensure_within_root(&root, &resolved_generation)?;
                ensure_path_type(&resolved_generation, PathKind::Directory)?;
                let store_db =
                    canonicalize_within_root(&root, resolved_generation.join("store.db"))?;
                ensure_path_type(&store_db, PathKind::File)?;
                let bases_dir = canonicalize_within_root(&root, resolved_generation.join("bases"))?;
                ensure_path_type(&bases_dir, PathKind::Directory)?;
                validate_existing_generation(&store_db, family_id)?;
                Some(store_db)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };

        initialize_coordinator_database(&coordinator_db)?;

        if existing_store_db.is_none() {
            let partial_generation = root.join(PARTIAL_GENERATION);
            if partial_generation.exists() {
                fs::remove_dir_all(&partial_generation)?;
            }
            fs::create_dir(&partial_generation)?;
            write_partial_generation_owner(
                &partial_generation,
                &PartialGenerationOwner {
                    run_id: "layout-create".to_string(),
                    owner_id: format!("layout-{}", std::process::id()),
                    owner_pid: std::process::id(),
                    fencing_token: 1,
                    expires_at: i64::MAX,
                },
            )?;
            fs::create_dir(partial_generation.join("bases"))?;
            let partial_store_db = partial_generation.join("store.db");
            initialize_store_database(
                &partial_store_db,
                family_id,
                creator_version,
                extraction_identity_epoch,
            )?;
            sync_file(&partial_store_db)?;
            fs::remove_file(partial_generation.join(PARTIAL_OWNER_FILE))?;
            sync_directory(&partial_generation)?;
            fs::rename(&partial_generation, &generation_dir)?;
            sync_directory(&root)?;
        }

        let partial_current = root.join("CURRENT.partial");
        let mut current = File::create(&partial_current)?;
        current.write_all(format!("{INITIAL_GENERATION}\n").as_bytes())?;
        current.sync_all()?;
        drop(current);
        fs::rename(&partial_current, root.join("CURRENT"))?;
        sync_directory(&root)?;

        Self::open(root)
    }

    /// Resolves the generation named by `CURRENT`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, StoreLayoutError> {
        let root = root.as_ref().canonicalize()?;
        let current_path = root.join("CURRENT");
        let resolved_current = match current_path.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(StoreLayoutError::CurrentMissing { path: current_path });
            }
            Err(error) => return Err(error.into()),
        };
        ensure_within_root(&root, &resolved_current)?;
        ensure_path_type(&resolved_current, PathKind::File)?;
        let generation_name = match fs::read_to_string(&resolved_current) {
            Ok(value) => value.trim().to_string(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(StoreLayoutError::CurrentMissing { path: current_path });
            }
            Err(error) => return Err(error.into()),
        };
        if !valid_generation_name(&generation_name) {
            return Err(StoreLayoutError::InvalidGeneration {
                value: generation_name,
            });
        }
        Self::open_named_generation(&root, &generation_name)
    }

    pub(crate) fn open_named_generation(
        root: impl AsRef<Path>,
        generation_name: &str,
    ) -> Result<Self, StoreLayoutError> {
        let root = root.as_ref().canonicalize()?;
        if !valid_generation_name(generation_name) {
            return Err(StoreLayoutError::InvalidGeneration {
                value: generation_name.to_string(),
            });
        }
        let generation_dir = root.join(generation_name).canonicalize()?;
        ensure_within_root(&root, &generation_dir)?;
        ensure_path_type(&generation_dir, PathKind::Directory)?;
        let store_db = canonicalize_within_root(&root, generation_dir.join("store.db"))?;
        let coordinator_db = canonicalize_within_root(&root, root.join("coord.db"))?;
        let spool_dir = canonicalize_within_root(&root, root.join("spool"))?;
        let scratch_dir = canonicalize_within_root(&root, root.join("scratch"))?;
        let bases_dir = canonicalize_within_root(&root, generation_dir.join("bases"))?;
        ensure_path_type(&store_db, PathKind::File)?;
        ensure_path_type(&coordinator_db, PathKind::File)?;
        ensure_path_type(&spool_dir, PathKind::Directory)?;
        ensure_path_type(&scratch_dir, PathKind::Directory)?;
        ensure_path_type(&bases_dir, PathKind::Directory)?;
        Ok(Self {
            root,
            generation_name: generation_name.to_string(),
            generation_dir,
            store_db,
            coordinator_db,
            spool_dir,
            scratch_dir,
            bases_dir,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn generation_name(&self) -> &str {
        &self.generation_name
    }

    pub fn generation_dir(&self) -> &Path {
        &self.generation_dir
    }

    pub fn store_db(&self) -> &Path {
        &self.store_db
    }

    pub fn coordinator_db(&self) -> &Path {
        &self.coordinator_db
    }

    pub fn spool_dir(&self) -> &Path {
        &self.spool_dir
    }

    pub fn scratch_dir(&self) -> &Path {
        &self.scratch_dir
    }

    pub fn bases_dir(&self) -> &Path {
        &self.bases_dir
    }
}

/// Removes leftover resolution base files and both scratch families.
///
/// The `bases/` directory itself stays so [`StoreLayout::open`] can resolve it.
/// Callers must close any handles to those files first.
/// Runs once during the initial writer retirement migration.
pub(crate) fn reap_retired_resolution_files(layout: &StoreLayout) -> Result<(), StoreLayoutError> {
    reap_directory_files(layout.bases_dir())?;
    let entries = match fs::read_dir(layout.scratch_dir()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        if is_retired_resolution_scratch_name(&entry.file_name().to_string_lossy()) {
            remove_path_and_sidecars(&entry.path())?;
        }
    }
    Ok(())
}

fn reap_directory_files(dir: &Path) -> Result<(), StoreLayoutError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

pub(crate) fn is_retired_resolution_scratch_name(name: &str) -> bool {
    let base = name
        .strip_suffix("-wal")
        .or_else(|| name.strip_suffix("-shm"))
        .unwrap_or(name);
    (base.starts_with("resolve-") && base.contains(".db"))
        || (base.starts_with("resolution-") && base.contains(".partial.db"))
}

fn remove_path_and_sidecars(path: &Path) -> Result<(), StoreLayoutError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        fs::remove_file(path)?;
        return Ok(());
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path)?;
        return Ok(());
    }
    fs::remove_file(path)?;
    Ok(())
}

/// Failure to create or resolve a store layout.
#[derive(Debug)]
pub enum StoreLayoutError {
    FamilyMismatch {
        expected: String,
        found: String,
    },
    CurrentMissing {
        path: PathBuf,
    },
    CurrentRecoveryRequired {
        generations: Vec<String>,
    },
    PartialGenerationRecoveryRequired {
        path: PathBuf,
    },
    InvalidGeneration {
        value: String,
    },
    PathEscapesRoot {
        path: PathBuf,
    },
    UnexpectedPathType {
        path: PathBuf,
        expected: &'static str,
    },
    PragmaMismatch {
        pragma: &'static str,
        expected: i64,
        found: i64,
    },
    TextPragmaMismatch {
        pragma: &'static str,
        expected: &'static str,
        found: String,
    },
    Io(io::Error),
    Schema(StoreSchemaError),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for StoreLayoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FamilyMismatch { expected, found } => {
                write!(
                    formatter,
                    "store family {found:?} does not match {expected:?}"
                )
            }
            Self::CurrentMissing { path } => {
                write!(formatter, "store CURRENT is missing at {}", path.display())
            }
            Self::CurrentRecoveryRequired { generations } => write!(
                formatter,
                "store CURRENT is missing beside published generations: {}",
                generations.join(", ")
            ),
            Self::PartialGenerationRecoveryRequired { path } => write!(
                formatter,
                "partial generation requires owned recovery at {}",
                path.display()
            ),
            Self::InvalidGeneration { value } => {
                write!(formatter, "invalid store generation name {value:?}")
            }
            Self::PathEscapesRoot { path } => {
                write!(
                    formatter,
                    "store path escapes the family root: {}",
                    path.display()
                )
            }
            Self::UnexpectedPathType { path, expected } => write!(
                formatter,
                "store path {} is not a {expected}",
                path.display()
            ),
            Self::PragmaMismatch {
                pragma,
                expected,
                found,
            } => write!(
                formatter,
                "SQLite pragma {pragma} is {found}, expected {expected}"
            ),
            Self::TextPragmaMismatch {
                pragma,
                expected,
                found,
            } => write!(
                formatter,
                "SQLite pragma {pragma} is {found:?}, expected {expected:?}"
            ),
            Self::Io(error) => error.fmt(formatter),
            Self::Schema(error) => error.fmt(formatter),
            Self::Sqlite(error) => error.fmt(formatter),
        }
    }
}

impl Error for StoreLayoutError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Schema(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::CurrentMissing { .. }
            | Self::CurrentRecoveryRequired { .. }
            | Self::PartialGenerationRecoveryRequired { .. }
            | Self::FamilyMismatch { .. }
            | Self::InvalidGeneration { .. }
            | Self::PathEscapesRoot { .. }
            | Self::UnexpectedPathType { .. }
            | Self::PragmaMismatch { .. }
            | Self::TextPragmaMismatch { .. } => None,
        }
    }
}

pub(crate) fn named_generations(root: &Path) -> Result<Vec<String>, StoreLayoutError> {
    let mut generations = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if valid_generation_name(name) {
            let generation = entry.path().canonicalize()?;
            ensure_within_root(root, &generation)?;
            ensure_path_type(&generation, PathKind::Directory)?;
            generations.push(name.to_string());
        }
    }
    generations.sort();
    Ok(generations)
}

fn reap_scaffolding(root: &Path) -> Result<(), StoreLayoutError> {
    let intent = maintenance_intent(root)?;
    let mut reaped = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "CURRENT.partial" {
            if intent
                .as_ref()
                .is_some_and(|intent| intent.expires_at > now_ms())
            {
                return Err(StoreLayoutError::PartialGenerationRecoveryRequired {
                    path: entry.path(),
                });
            }
            let file_type = entry.file_type()?;
            reaped.push((entry.path(), file_type.is_dir() && !file_type.is_symlink()));
        } else if partial_generation_scaffold(&name) {
            let file_type = entry.file_type()?;
            if !file_type.is_dir() || file_type.is_symlink() {
                return Err(StoreLayoutError::PartialGenerationRecoveryRequired {
                    path: entry.path(),
                });
            }
            let resolved = entry.path().canonicalize()?;
            ensure_within_root(root, &resolved)?;
            let owner = read_partial_generation_owner(root, &resolved)?;
            let matches_intent = intent.as_ref().is_some_and(|intent| {
                intent.run_id == owner.run_id
                    && intent.owner_id == owner.owner_id
                    && intent.fencing_token == owner.fencing_token
            });
            let intent_allows_reap = match intent.as_ref() {
                None => true,
                Some(intent) => matches_intent && intent.expires_at <= now_ms(),
            };
            if !intent_allows_reap || process_status(owner.owner_pid) != PidStatus::Dead {
                return Err(StoreLayoutError::PartialGenerationRecoveryRequired {
                    path: entry.path(),
                });
            }
            reaped.push((resolved, true));
        }
    }
    for (path, directory) in reaped {
        if directory {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[derive(Debug)]
struct MaintenanceIntentOwner {
    run_id: String,
    owner_id: String,
    fencing_token: i64,
    expires_at: i64,
}

fn maintenance_intent(root: &Path) -> Result<Option<MaintenanceIntentOwner>, StoreLayoutError> {
    let path = root.join("coord.db");
    if !path.exists() {
        return Ok(None);
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    Ok(connection
        .query_row(
            "SELECT run_id, owner_id, fencing_token, expires_at
             FROM maintenance_intent WHERE resource = 'store-maintenance'",
            [],
            |row| {
                Ok(MaintenanceIntentOwner {
                    run_id: row.get(0)?,
                    owner_id: row.get(1)?,
                    fencing_token: row.get(2)?,
                    expires_at: row.get(3)?,
                })
            },
        )
        .optional()?)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}

pub fn write_partial_generation_owner(
    partial_generation: &Path,
    owner: &PartialGenerationOwner,
) -> Result<(), StoreLayoutError> {
    ensure_path_type(partial_generation, PathKind::Directory)?;
    let path = partial_generation.join(PARTIAL_OWNER_FILE);
    let bytes = serde_json::to_vec(owner).map_err(io::Error::other)?;
    let mut file = File::create(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn read_partial_generation_owner(
    root: &Path,
    partial_generation: &Path,
) -> Result<PartialGenerationOwner, StoreLayoutError> {
    let path = partial_generation.join(PARTIAL_OWNER_FILE);
    let metadata = fs::symlink_metadata(&path).map_err(|_| {
        StoreLayoutError::PartialGenerationRecoveryRequired {
            path: partial_generation.to_path_buf(),
        }
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(StoreLayoutError::PartialGenerationRecoveryRequired {
            path: partial_generation.to_path_buf(),
        });
    }
    let path = path.canonicalize()?;
    ensure_within_root(root, &path)?;
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(|_| {
        StoreLayoutError::PartialGenerationRecoveryRequired {
            path: partial_generation.to_path_buf(),
        }
    })
}

fn partial_generation_scaffold(name: &str) -> bool {
    let Some(value) = name.strip_prefix(".gen-") else {
        return false;
    };
    let Some((digits, suffix)) = value.split_once(".partial") else {
        return false;
    };
    digits.len() >= 3
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (suffix.is_empty() || suffix.starts_with('-'))
}

fn validate_existing_generation(path: &Path, expected: &str) -> Result<(), StoreLayoutError> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    validate_store_schema_version(&connection)?;
    validate_store_file_pragmas(&connection)?;
    let found = connection.query_row(
        "SELECT value FROM store_meta WHERE key = 'family_id'",
        [],
        |row| row.get::<_, String>(0),
    )?;
    if found == expected {
        Ok(())
    } else {
        Err(StoreLayoutError::FamilyMismatch {
            expected: expected.to_string(),
            found,
        })
    }
}

pub(crate) fn valid_generation_name(value: &str) -> bool {
    value
        .strip_prefix("gen-")
        .is_some_and(|digits| digits.len() >= 3 && digits.bytes().all(|byte| byte.is_ascii_digit()))
}

fn canonicalize_within_root(root: &Path, path: PathBuf) -> Result<PathBuf, StoreLayoutError> {
    let path = path.canonicalize()?;
    ensure_within_root(root, &path)?;
    Ok(path)
}

fn ensure_within_root(root: &Path, path: &Path) -> Result<(), StoreLayoutError> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(StoreLayoutError::PathEscapesRoot {
            path: path.to_path_buf(),
        })
    }
}

fn validate_owned_coordinator_path(root: &Path, path: &Path) -> Result<(), StoreLayoutError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            match path.canonicalize() {
                Ok(resolved) => ensure_within_root(root, &resolved)?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            Err(StoreLayoutError::UnexpectedPathType {
                path: path.to_path_buf(),
                expected: "owned regular file",
            })
        }
        Ok(_) => ensure_path_type(path, PathKind::File),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[derive(Clone, Copy)]
enum PathKind {
    File,
    Directory,
}

fn ensure_path_type(path: &Path, kind: PathKind) -> Result<(), StoreLayoutError> {
    let metadata = fs::metadata(path)?;
    let matches = match kind {
        PathKind::File => metadata.is_file(),
        PathKind::Directory => metadata.is_dir(),
    };
    if matches {
        Ok(())
    } else {
        Err(StoreLayoutError::UnexpectedPathType {
            path: path.to_path_buf(),
            expected: match kind {
                PathKind::File => "regular file",
                PathKind::Directory => "directory",
            },
        })
    }
}

impl From<io::Error> for StoreLayoutError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<StoreSchemaError> for StoreLayoutError {
    fn from(error: StoreSchemaError) -> Self {
        Self::Schema(error)
    }
}

impl From<rusqlite::Error> for StoreLayoutError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

pub(crate) fn initialize_store_database(
    path: &Path,
    family_id: &str,
    creator_version: &str,
    extraction_identity_epoch: u32,
) -> Result<(), StoreLayoutError> {
    let mut connection = Connection::open(path)?;
    configure_writer_pragmas(&connection, WriterPragmaProfile::Routine)?;
    create_store_schema(&connection)?;
    let transaction = connection.transaction()?;
    let extraction_identity_epoch = extraction_identity_epoch.to_string();
    for (key, value) in [
        ("family_id", family_id),
        (
            "extraction_identity_epoch",
            extraction_identity_epoch.as_str(),
        ),
        ("min_reader_version", creator_version),
        ("min_writer_version", creator_version),
        ("created_by_version", creator_version),
        ("binary_version", creator_version),
    ] {
        transaction.execute(
            "INSERT INTO store_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn initialize_coordinator_database(path: &Path) -> Result<(), StoreLayoutError> {
    let connection = Connection::open(path)?;
    configure_writer_pragmas(&connection, WriterPragmaProfile::Routine)?;
    create_coordinator_schema(&connection)?;
    drop(connection);
    sync_file(path)?;
    Ok(())
}

impl From<PragmaError> for StoreLayoutError {
    fn from(error: PragmaError) -> Self {
        match error {
            PragmaError::Sqlite(error) => Self::Sqlite(error),
            PragmaError::IntegerMismatch {
                pragma,
                expected,
                found,
            } => Self::PragmaMismatch {
                pragma,
                expected,
                found,
            },
            PragmaError::TextMismatch {
                pragma,
                expected,
                found,
            } => Self::TextPragmaMismatch {
                pragma,
                expected,
                found,
            },
        }
    }
}

pub(crate) fn sync_file(path: &Path) -> io::Result<()> {
    OpenOptions::new().write(true).open(path)?.sync_all()
}

#[cfg(unix)]
pub(crate) fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
pub(crate) fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

    struct TempStore {
        path: PathBuf,
    }

    impl TempStore {
        fn new(name: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "julie-store-layout-{name}-{}-{nonce}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempStore {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn reap_retired_resolution_scratch_propagates_read_dir_errors() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempStore::new("scratch-reap-denied");
        let layout = StoreLayout::create(&temp.path, "family-a", "2.30.0", 7).unwrap();
        fs::write(layout.scratch_dir().join("resolve-exact-request.db"), b"x").unwrap();
        let original = fs::metadata(layout.scratch_dir()).unwrap().permissions();
        let mut denied = original.clone();
        denied.set_mode(0o000);
        fs::set_permissions(layout.scratch_dir(), denied).unwrap();
        let error = reap_retired_resolution_files(&layout);
        fs::set_permissions(layout.scratch_dir(), original).unwrap();
        let error = error.expect_err("scratch read_dir errors must propagate");
        match error {
            StoreLayoutError::Io(error) => {
                assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            }
            other => panic!("expected io permission denied, got {other}"),
        }
    }
}
