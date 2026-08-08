use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

use super::{StoreSchemaError, create_coordinator_schema, create_store_schema};

const INITIAL_GENERATION: &str = "gen-001";
const PARTIAL_GENERATION: &str = ".gen-001.partial";

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
    ) -> Result<Self, StoreLayoutError> {
        let root = root.as_ref();
        fs::create_dir_all(root)?;
        let root = root.canonicalize()?;
        reap_scaffolding(&root)?;
        let coordinator_db = root.join("coord.db");
        validate_owned_coordinator_path(&root, &coordinator_db)?;
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
            fs::create_dir(partial_generation.join("bases"))?;
            let partial_store_db = partial_generation.join("store.db");
            initialize_store_database(&partial_store_db, family_id, creator_version)?;
            sync_file(&partial_store_db)?;
            sync_directory(&partial_generation)?;
            fs::rename(&partial_generation, &generation_dir)?;
            sync_directory(&root)?;
        }

        let partial_current = root.join("CURRENT.partial");
        let mut current = File::create(&partial_current)?;
        current.write_all(format!("{INITIAL_GENERATION}\n").as_bytes())?;
        current.sync_all()?;
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
        let generation_dir = root.join(&generation_name).canonicalize()?;
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
            generation_name,
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
            | Self::FamilyMismatch { .. }
            | Self::InvalidGeneration { .. }
            | Self::PathEscapesRoot { .. }
            | Self::UnexpectedPathType { .. }
            | Self::PragmaMismatch { .. }
            | Self::TextPragmaMismatch { .. } => None,
        }
    }
}

fn reap_scaffolding(root: &Path) -> io::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "CURRENT.partial" || partial_generation_scaffold(&name) {
            let file_type = entry.file_type()?;
            if file_type.is_dir() && !file_type.is_symlink() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
    }
    Ok(())
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
    let connection = Connection::open(path)?;
    configure_writer_pragmas(&connection)?;
    create_store_schema(&connection)?;
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

fn valid_generation_name(value: &str) -> bool {
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

fn initialize_store_database(
    path: &Path,
    family_id: &str,
    creator_version: &str,
) -> Result<(), StoreLayoutError> {
    let mut connection = Connection::open(path)?;
    configure_writer_pragmas(&connection)?;
    create_store_schema(&connection)?;
    let transaction = connection.transaction()?;
    for (key, value) in [
        ("family_id", family_id),
        ("extraction_identity_epoch", "1"),
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
    configure_writer_pragmas(&connection)?;
    create_coordinator_schema(&connection)?;
    drop(connection);
    sync_file(path)?;
    Ok(())
}

fn configure_writer_pragmas(connection: &Connection) -> Result<(), StoreLayoutError> {
    connection.execute_batch(
        "PRAGMA page_size = 4096;
         PRAGMA auto_vacuum = INCREMENTAL;",
    )?;
    verify_integer_pragma(connection, "page_size", 4096)?;
    verify_integer_pragma(connection, "auto_vacuum", 2)?;
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA wal_autocheckpoint = 1000;
         PRAGMA synchronous = FULL;
         PRAGMA foreign_keys = ON;
         PRAGMA secure_delete = ON;",
    )?;
    verify_text_pragma(connection, "journal_mode", "wal")?;
    verify_integer_pragma(connection, "wal_autocheckpoint", 1000)?;
    verify_integer_pragma(connection, "synchronous", 2)?;
    verify_integer_pragma(connection, "foreign_keys", 1)?;
    verify_integer_pragma(connection, "secure_delete", 1)?;
    Ok(())
}

fn verify_integer_pragma(
    connection: &Connection,
    pragma: &'static str,
    expected: i64,
) -> Result<(), StoreLayoutError> {
    let found = connection.query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))?;
    if found == expected {
        Ok(())
    } else {
        Err(StoreLayoutError::PragmaMismatch {
            pragma,
            expected,
            found,
        })
    }
}

fn verify_text_pragma(
    connection: &Connection,
    pragma: &'static str,
    expected: &'static str,
) -> Result<(), StoreLayoutError> {
    let found = connection.query_row(&format!("PRAGMA {pragma}"), [], |row| {
        row.get::<_, String>(0)
    })?;
    if found.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(StoreLayoutError::TextPragmaMismatch {
            pragma,
            expected,
            found,
        })
    }
}

fn sync_file(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}
