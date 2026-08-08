use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::thread;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use super::{StoreLog, StoreLogEntry, StoreLogError};

const MANIFEST_HASH_DOMAIN: &[u8] = b"julie-store-manifest-v2";

/// Hash algorithm used by the manifest canonical encoding.
pub const MANIFEST_HASH_ALGORITHM: &str = "sha256";

/// Maximum optimistic recomputations after a concurrent manifest publication.
pub const MANIFEST_PUBLISH_MAX_RETRIES: u32 = 6;

/// View-local state attached to one path in an immutable manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestEntryStatus {
    Indexed,
    FailedPreserved,
    Failed,
}

impl ManifestEntryStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Indexed => "indexed",
            Self::FailedPreserved => "failed_preserved",
            Self::Failed => "failed",
        }
    }
}

/// One path in a view manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub path: String,
    pub language: String,
    pub version_id: Option<i64>,
    pub status: ManifestEntryStatus,
    pub observed_content_hash: String,
    pub indexed_at: String,
    pub error_class: Option<String>,
    pub error_json: Option<String>,
}

impl ManifestEntry {
    pub fn indexed(
        path: impl Into<String>,
        language: impl Into<String>,
        version_id: i64,
        observed_content_hash: impl Into<String>,
        indexed_at: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            language: language.into(),
            version_id: Some(version_id),
            status: ManifestEntryStatus::Indexed,
            observed_content_hash: observed_content_hash.into(),
            indexed_at: indexed_at.into(),
            error_class: None,
            error_json: None,
        }
    }

    pub fn failed_preserved(
        path: impl Into<String>,
        language: impl Into<String>,
        version_id: i64,
        observed_content_hash: impl Into<String>,
        indexed_at: impl Into<String>,
        error_class: impl Into<String>,
        error_json: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            language: language.into(),
            version_id: Some(version_id),
            status: ManifestEntryStatus::FailedPreserved,
            observed_content_hash: observed_content_hash.into(),
            indexed_at: indexed_at.into(),
            error_class: Some(error_class.into()),
            error_json: Some(error_json.into()),
        }
    }

    pub fn failed(
        path: impl Into<String>,
        language: impl Into<String>,
        observed_content_hash: impl Into<String>,
        indexed_at: impl Into<String>,
        error_class: impl Into<String>,
        error_json: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            language: language.into(),
            version_id: None,
            status: ManifestEntryStatus::Failed,
            observed_content_hash: observed_content_hash.into(),
            indexed_at: indexed_at.into(),
            error_class: Some(error_class.into()),
            error_json: Some(error_json.into()),
        }
    }
}

/// Canonical manifest entries and their deterministic semantic hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltManifest {
    pub entries: Vec<ManifestEntry>,
    pub manifest_hash: String,
}

/// Canonicalizes manifest entries against completed file-version identities.
pub struct ManifestBuilder {
    entries: Vec<ManifestEntry>,
}

impl ManifestBuilder {
    pub fn from_entries(entries: impl IntoIterator<Item = ManifestEntry>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    pub fn build(self, connection: &Connection) -> Result<BuiltManifest, ManifestStoreError> {
        build_manifest(connection, self.entries)
    }
}

/// Result class for an immutable manifest publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestPublishDisposition {
    Created,
    Reused,
}

/// Durable result of publishing or reusing a manifest generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestPublishResult {
    pub generation: u64,
    pub manifest_hash: String,
    pub disposition: ManifestPublishDisposition,
    pub effect_sequence: Option<i64>,
    pub retries: u32,
}

/// Whether import created a view binding or confirmed the existing binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewEnsureDisposition {
    Created,
    Existing,
}

/// Typed manifest validation, CAS, or SQLite failure.
#[derive(Debug)]
pub enum ManifestStoreError {
    EmptyViewId,
    EmptyRoot,
    EmptyRequestId,
    InvalidPath {
        path: String,
    },
    DuplicatePath {
        path: String,
    },
    InvalidEntry {
        path: String,
    },
    VersionNotFound {
        version_id: i64,
    },
    VersionIncomplete {
        version_id: i64,
    },
    VersionPathMismatch {
        path: String,
        version_path: String,
    },
    VersionLanguageMismatch {
        path: String,
        language: String,
        version_language: String,
    },
    ObservedHashMismatch {
        path: String,
    },
    ViewNotFound {
        view_id: String,
    },
    ViewRootMismatch {
        view_id: String,
        expected: String,
        found: String,
    },
    GenerationOutOfRange {
        generation: u64,
    },
    GenerationMismatch {
        expected: Option<u64>,
        actual: Option<u64>,
    },
    ManifestNotFound {
        view_id: String,
        generation: u64,
    },
    Log(StoreLogError),
    Sqlite(rusqlite::Error),
}

impl ManifestStoreError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptyViewId => "invalid_view_id",
            Self::EmptyRoot => "invalid_view_root",
            Self::EmptyRequestId => "invalid_request_id",
            Self::InvalidPath { .. } => "invalid_manifest_path",
            Self::DuplicatePath { .. } => "duplicate_manifest_path",
            Self::InvalidEntry { .. } => "invalid_manifest_entry",
            Self::VersionNotFound { .. } => "file_version_not_found",
            Self::VersionIncomplete { .. } => "file_version_incomplete",
            Self::VersionPathMismatch { .. } => "file_version_path_mismatch",
            Self::VersionLanguageMismatch { .. } => "file_version_language_mismatch",
            Self::ObservedHashMismatch { .. } => "observed_content_hash_mismatch",
            Self::ViewNotFound { .. } => "view_not_found",
            Self::ViewRootMismatch { .. } => "view_root_mismatch",
            Self::GenerationOutOfRange { .. } => "manifest_generation_out_of_range",
            Self::GenerationMismatch { .. } => "manifest_generation_mismatch",
            Self::ManifestNotFound { .. } => "manifest_not_found",
            Self::Log(error) => error.code(),
            Self::Sqlite(_) => "store_sqlite_error",
        }
    }
}

impl fmt::Display for ManifestStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyViewId => write!(formatter, "view id must be non-empty"),
            Self::EmptyRoot => write!(formatter, "view root must be non-empty"),
            Self::EmptyRequestId => write!(formatter, "request id must be non-empty"),
            Self::InvalidPath { path } => write!(formatter, "invalid manifest path {path:?}"),
            Self::DuplicatePath { path } => write!(formatter, "duplicate manifest path {path:?}"),
            Self::InvalidEntry { path } => write!(formatter, "invalid manifest entry for {path:?}"),
            Self::VersionNotFound { version_id } => {
                write!(formatter, "file version {version_id} was not found")
            }
            Self::VersionIncomplete { version_id } => write!(
                formatter,
                "file version {version_id} has no L1 completion stamp"
            ),
            Self::VersionPathMismatch { path, version_path } => write!(
                formatter,
                "manifest path {path:?} does not match file-version path {version_path:?}"
            ),
            Self::VersionLanguageMismatch {
                path,
                language,
                version_language,
            } => write!(
                formatter,
                "manifest path {path:?} language {language:?} does not match file-version language {version_language:?}"
            ),
            Self::ObservedHashMismatch { path } => write!(
                formatter,
                "indexed manifest path {path:?} does not match its file-version content hash"
            ),
            Self::ViewNotFound { view_id } => write!(formatter, "view {view_id:?} was not found"),
            Self::ViewRootMismatch {
                view_id,
                expected,
                found,
            } => write!(
                formatter,
                "view {view_id:?} root {found:?} does not match {expected:?}"
            ),
            Self::GenerationOutOfRange { generation } => write!(
                formatter,
                "manifest generation {generation} does not fit SQLite"
            ),
            Self::GenerationMismatch { expected, actual } => write!(
                formatter,
                "manifest generation {actual:?} does not match expected {expected:?}"
            ),
            Self::ManifestNotFound {
                view_id,
                generation,
            } => write!(
                formatter,
                "manifest {view_id:?} generation {generation} was not found"
            ),
            Self::Log(error) => error.fmt(formatter),
            Self::Sqlite(error) => error.fmt(formatter),
        }
    }
}

impl Error for ManifestStoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Log(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for ManifestStoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<StoreLogError> for ManifestStoreError {
    fn from(error: StoreLogError) -> Self {
        Self::Log(error)
    }
}

/// Writes and reads immutable view manifest generations.
pub struct ManifestStore<'connection> {
    connection: &'connection mut Connection,
    max_retries: u32,
}

impl<'connection> ManifestStore<'connection> {
    pub fn new(connection: &'connection mut Connection) -> Self {
        Self {
            connection,
            max_retries: MANIFEST_PUBLISH_MAX_RETRIES,
        }
    }

    pub fn with_retry_limit(connection: &'connection mut Connection, max_retries: u32) -> Self {
        Self {
            connection,
            max_retries,
        }
    }

    pub fn ensure_view(
        &mut self,
        view_id: &str,
        root: &str,
    ) -> Result<ViewEnsureDisposition, ManifestStoreError> {
        validate_view_identity(view_id, root)?;
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO views
             (view_id, root, resolution_state, created_at, updated_at)
             VALUES (?1, ?2, 'unbound',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![view_id, root],
        )?;
        let found = self.connection.query_row(
            "SELECT root FROM views WHERE view_id = ?1",
            [view_id],
            |row| row.get::<_, String>(0),
        )?;
        if found == root {
            Ok(if inserted == 1 {
                ViewEnsureDisposition::Created
            } else {
                ViewEnsureDisposition::Existing
            })
        } else {
            Err(ManifestStoreError::ViewRootMismatch {
                view_id: view_id.to_string(),
                expected: root.to_string(),
                found,
            })
        }
    }

    pub fn ensure_view_in_transaction(
        transaction: &Transaction<'_>,
        view_id: &str,
        root: &str,
    ) -> Result<ViewEnsureDisposition, ManifestStoreError> {
        validate_view_identity(view_id, root)?;
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO views
             (view_id, root, resolution_state, created_at, updated_at)
             VALUES (?1, ?2, 'unbound',
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![view_id, root],
        )?;
        let found = transaction.query_row(
            "SELECT root FROM views WHERE view_id = ?1",
            [view_id],
            |row| row.get::<_, String>(0),
        )?;
        if found == root {
            Ok(if inserted == 1 {
                ViewEnsureDisposition::Created
            } else {
                ViewEnsureDisposition::Existing
            })
        } else {
            Err(ManifestStoreError::ViewRootMismatch {
                view_id: view_id.to_string(),
                expected: root.to_string(),
                found,
            })
        }
    }

    pub fn require_view(&self, view_id: &str, root: &str) -> Result<(), ManifestStoreError> {
        validate_view_identity(view_id, root)?;
        let found = self
            .connection
            .query_row(
                "SELECT root FROM views WHERE view_id = ?1",
                [view_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| ManifestStoreError::ViewNotFound {
                view_id: view_id.to_string(),
            })?;
        if found == root {
            Ok(())
        } else {
            Err(ManifestStoreError::ViewRootMismatch {
                view_id: view_id.to_string(),
                expected: root.to_string(),
                found,
            })
        }
    }

    pub fn current_generation(&self, view_id: &str) -> Result<Option<u64>, ManifestStoreError> {
        current_generation(self.connection, view_id)
    }

    pub fn entries(
        &self,
        view_id: &str,
        generation: u64,
    ) -> Result<Vec<ManifestEntry>, ManifestStoreError> {
        load_entries(self.connection, view_id, generation)
    }

    pub fn publish(
        &mut self,
        view_id: &str,
        expected_generation: Option<u64>,
        entries: impl IntoIterator<Item = ManifestEntry>,
        request_id: &str,
    ) -> Result<ManifestPublishResult, ManifestStoreError> {
        if request_id.is_empty() {
            return Err(ManifestStoreError::EmptyRequestId);
        }
        if view_id.is_empty() {
            return Err(ManifestStoreError::EmptyViewId);
        }
        require_view_exists(self.connection, view_id)?;
        let desired = build_manifest(self.connection, entries.into_iter().collect())?;
        let base_entries = expected_generation
            .map(|generation| load_entries(self.connection, view_id, generation))
            .transpose()?
            .unwrap_or_default();
        let delta = ManifestDelta::between(&base_entries, &desired.entries);
        let mut observed_expected = false;

        for attempt in 0..=self.max_retries {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Deferred)?;
            let actual = current_generation(&transaction, view_id)?;
            if actual == expected_generation {
                observed_expected = true;
            } else if !observed_expected {
                return Err(ManifestStoreError::GenerationMismatch {
                    expected: expected_generation,
                    actual,
                });
            }
            let candidate = if actual == expected_generation {
                desired.clone()
            } else {
                let head = actual
                    .map(|generation| load_entries(&transaction, view_id, generation))
                    .transpose()?
                    .unwrap_or_default();
                build_manifest(&transaction, delta.apply(head))?
            };
            let result = publish_transaction(&transaction, view_id, actual, candidate, request_id)
                .and_then(|result| {
                    transaction.commit()?;
                    Ok(result)
                });
            match result {
                Ok(mut result) => {
                    result.retries = attempt;
                    return Ok(result);
                }
                Err(error) if attempt < self.max_retries && is_retryable(&error) => {
                    let delay = 10_u64 << attempt.min(3);
                    thread::sleep(Duration::from_millis(delay));
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("bounded manifest retry loop always returns")
    }

    pub fn publish_in_transaction(
        transaction: &Transaction<'_>,
        view_id: &str,
        expected_generation: Option<u64>,
        entries: impl IntoIterator<Item = ManifestEntry>,
        request_id: &str,
    ) -> Result<ManifestPublishResult, ManifestStoreError> {
        if request_id.is_empty() {
            return Err(ManifestStoreError::EmptyRequestId);
        }
        if view_id.is_empty() {
            return Err(ManifestStoreError::EmptyViewId);
        }
        require_view_exists(transaction, view_id)?;
        let actual = current_generation(transaction, view_id)?;
        if actual != expected_generation {
            return Err(ManifestStoreError::GenerationMismatch {
                expected: expected_generation,
                actual,
            });
        }
        let manifest = build_manifest(transaction, entries.into_iter().collect())?;
        publish_transaction(transaction, view_id, actual, manifest, request_id)
    }

    pub fn invalidate_resolution_binding(
        transaction: &Transaction<'_>,
        view_id: &str,
    ) -> Result<(), ManifestStoreError> {
        let changed = transaction.execute(
            "UPDATE views
             SET resolution_state = 'unbound',
                 resolution_base_id = NULL,
                 resolution_delta_generation = NULL,
                 resolution_exact_at = NULL
             WHERE view_id = ?1",
            [view_id],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(ManifestStoreError::ViewNotFound {
                view_id: view_id.to_string(),
            })
        }
    }
}

struct ManifestDelta {
    changes: BTreeMap<String, Option<ManifestEntry>>,
}

impl ManifestDelta {
    fn between(base: &[ManifestEntry], desired: &[ManifestEntry]) -> Self {
        let base = base
            .iter()
            .map(|entry| (entry.path.as_str(), entry))
            .collect::<BTreeMap<_, _>>();
        let desired = desired
            .iter()
            .map(|entry| (entry.path.as_str(), entry))
            .collect::<BTreeMap<_, _>>();
        let mut changes = BTreeMap::new();
        for (path, base_entry) in &base {
            match desired.get(path) {
                Some(desired_entry) if semantic_entry_eq(base_entry, desired_entry) => {}
                Some(desired_entry) => {
                    changes.insert((*path).to_string(), Some((*desired_entry).clone()));
                }
                None => {
                    changes.insert((*path).to_string(), None);
                }
            }
        }
        for (path, desired_entry) in desired {
            if !base.contains_key(path) {
                changes.insert(path.to_string(), Some(desired_entry.clone()));
            }
        }
        Self { changes }
    }

    fn apply(&self, entries: Vec<ManifestEntry>) -> Vec<ManifestEntry> {
        let mut entries = entries
            .into_iter()
            .map(|entry| (entry.path.clone(), entry))
            .collect::<BTreeMap<_, _>>();
        for (path, entry) in &self.changes {
            match entry {
                Some(entry) => {
                    entries.insert(path.clone(), entry.clone());
                }
                None => {
                    entries.remove(path);
                }
            }
        }
        entries.into_values().collect()
    }
}

fn semantic_entry_eq(left: &ManifestEntry, right: &ManifestEntry) -> bool {
    left.path == right.path
        && left.language == right.language
        && left.version_id == right.version_id
        && left.status == right.status
        && left.observed_content_hash == right.observed_content_hash
        && left.error_class == right.error_class
        && left.error_json == right.error_json
}

fn publish_transaction(
    transaction: &Transaction<'_>,
    view_id: &str,
    actual_generation: Option<u64>,
    manifest: BuiltManifest,
    request_id: &str,
) -> Result<ManifestPublishResult, ManifestStoreError> {
    let existing_generation = transaction
        .query_row(
            "SELECT generation FROM manifests
             WHERE view_id = ?1 AND manifest_hash = ?2",
            params![view_id, manifest.manifest_hash],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .map(|generation| u64::try_from(generation).expect("manifest generations are positive"));
    if let Some(existing_generation) = existing_generation
        && Some(existing_generation) == actual_generation
    {
        return Ok(ManifestPublishResult {
            generation: existing_generation,
            manifest_hash: manifest.manifest_hash,
            disposition: ManifestPublishDisposition::Reused,
            effect_sequence: None,
            retries: 0,
        });
    }

    let (generation, disposition) = match existing_generation {
        Some(generation) => (generation, ManifestPublishDisposition::Reused),
        None => {
            let max_generation = transaction.query_row(
                "SELECT MAX(generation) FROM manifests WHERE view_id = ?1",
                [view_id],
                |row| row.get::<_, Option<i64>>(0),
            )?;
            let generation = match max_generation {
                Some(max_generation) => max_generation.checked_add(1).ok_or_else(|| {
                    ManifestStoreError::GenerationOutOfRange {
                        generation: u64::try_from(max_generation)
                            .expect("manifest generations are positive")
                            + 1,
                    }
                })?,
                None => 1,
            };
            transaction.execute(
                "INSERT INTO manifests
                 (view_id, generation, manifest_hash, request_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                params![view_id, generation, manifest.manifest_hash, request_id],
            )?;
            for entry in &manifest.entries {
                transaction.execute(
                    "INSERT INTO manifest_entries
                     (view_id, generation, path, language, version_id, status,
                      observed_content_hash, indexed_at, error_class, error_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        view_id,
                        generation,
                        entry.path,
                        entry.language,
                        entry.version_id,
                        entry.status.as_str(),
                        entry.observed_content_hash,
                        entry.indexed_at,
                        entry.error_class,
                        entry.error_json,
                    ],
                )?;
            }
            (
                u64::try_from(generation).expect("manifest generations are positive"),
                ManifestPublishDisposition::Created,
            )
        }
    };

    let payload_json = serde_json::to_string(&serde_json::json!({
        "manifest_hash": manifest.manifest_hash,
    }))
    .expect("manifest effect payload is serializable");
    let created_at =
        transaction.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
            row.get::<_, String>(0)
        })?;
    let effect_sequence = StoreLog::append_effect(
        transaction,
        &StoreLogEntry::new(request_id, "manifest_flipped", payload_json, created_at)
            .with_view(view_id)
            .with_generation(generation),
    )?;
    let generation_sql = sqlite_generation(generation)?;
    let actual_generation_sql = actual_generation.map(sqlite_generation).transpose()?;
    ManifestStore::invalidate_resolution_binding(transaction, view_id)?;
    let changed = transaction.execute(
        "UPDATE views
         SET current_generation = ?1,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE view_id = ?2 AND current_generation IS ?3",
        params![generation_sql, view_id, actual_generation_sql],
    )?;
    if changed != 1 {
        return Err(ManifestStoreError::GenerationMismatch {
            expected: actual_generation,
            actual: current_generation(transaction, view_id)?,
        });
    }
    Ok(ManifestPublishResult {
        generation,
        manifest_hash: manifest.manifest_hash,
        disposition,
        effect_sequence: Some(effect_sequence),
        retries: 0,
    })
}

fn is_retryable(error: &ManifestStoreError) -> bool {
    match error {
        ManifestStoreError::GenerationMismatch { .. } => true,
        ManifestStoreError::Sqlite(error) => matches!(
            error.sqlite_error_code(),
            Some(
                rusqlite::ErrorCode::DatabaseBusy
                    | rusqlite::ErrorCode::DatabaseLocked
                    | rusqlite::ErrorCode::ConstraintViolation
            )
        ),
        _ => false,
    }
}

fn load_entries(
    connection: &Connection,
    view_id: &str,
    generation: u64,
) -> Result<Vec<ManifestEntry>, ManifestStoreError> {
    let generation_sql = sqlite_generation(generation)?;
    let exists = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM manifests WHERE view_id = ?1 AND generation = ?2
         )",
        params![view_id, generation_sql],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(ManifestStoreError::ManifestNotFound {
            view_id: view_id.to_string(),
            generation,
        });
    }
    let mut statement = connection.prepare(
        "SELECT path, language, version_id, status, observed_content_hash, indexed_at,
                error_class, error_json
         FROM manifest_entries
         WHERE view_id = ?1 AND generation = ?2
         ORDER BY path COLLATE BINARY",
    )?;
    let rows = statement.query_map(params![view_id, generation_sql], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<i64>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;
    rows.map(|row| {
        let (
            path,
            language,
            version_id,
            status,
            observed_content_hash,
            indexed_at,
            error_class,
            error_json,
        ) = row?;
        let status = parse_status(&status)
            .ok_or_else(|| ManifestStoreError::InvalidEntry { path: path.clone() })?;
        Ok(ManifestEntry {
            path,
            language,
            version_id,
            status,
            observed_content_hash,
            indexed_at,
            error_class,
            error_json,
        })
    })
    .collect()
}

struct VersionIdentity {
    path: String,
    language: String,
    content_hash: String,
    extraction_epoch: u32,
}

fn build_manifest(
    connection: &Connection,
    mut entries: Vec<ManifestEntry>,
) -> Result<BuiltManifest, ManifestStoreError> {
    for entry in &mut entries {
        entry.path = canonical_path(&entry.path)?;
        if let Some(error_json) = entry.error_json.take() {
            entry.error_json = Some(canonical_error_json(&entry.path, &error_json)?);
        }
        validate_entry(entry)?;
    }
    entries.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
    for pair in entries.windows(2) {
        if pair[0].path == pair[1].path {
            return Err(ManifestStoreError::DuplicatePath {
                path: pair[0].path.clone(),
            });
        }
    }

    let mut canonical = Vec::new();
    encode_bytes(&mut canonical, MANIFEST_HASH_DOMAIN);
    canonical.extend_from_slice(&(entries.len() as u64).to_be_bytes());
    for entry in &entries {
        encode_text(&mut canonical, &entry.path);
        encode_text(&mut canonical, &entry.language);
        encode_text(&mut canonical, entry.status.as_str());
        encode_text(&mut canonical, &entry.observed_content_hash);
        match entry.version_id {
            Some(version_id) => {
                canonical.push(1);
                let identity = version_identity(connection, version_id)?;
                let version_path = canonical_path(&identity.path)?;
                if entry.path != version_path {
                    return Err(ManifestStoreError::VersionPathMismatch {
                        path: entry.path.clone(),
                        version_path,
                    });
                }
                if entry.language != identity.language {
                    return Err(ManifestStoreError::VersionLanguageMismatch {
                        path: entry.path.clone(),
                        language: entry.language.clone(),
                        version_language: identity.language,
                    });
                }
                if entry.status == ManifestEntryStatus::Indexed
                    && entry.observed_content_hash != identity.content_hash
                {
                    return Err(ManifestStoreError::ObservedHashMismatch {
                        path: entry.path.clone(),
                    });
                }
                encode_text(&mut canonical, &version_path);
                encode_text(&mut canonical, &identity.content_hash);
                canonical.extend_from_slice(&u64::from(identity.extraction_epoch).to_be_bytes());
            }
            None => canonical.push(0),
        }
        encode_optional_text(&mut canonical, entry.error_class.as_deref());
        encode_optional_text(&mut canonical, entry.error_json.as_deref());
    }
    let manifest_hash = format!("{:x}", Sha256::digest(&canonical));
    Ok(BuiltManifest {
        entries,
        manifest_hash,
    })
}

fn validate_entry(entry: &ManifestEntry) -> Result<(), ManifestStoreError> {
    if entry.language.is_empty()
        || entry.observed_content_hash.is_empty()
        || entry.indexed_at.is_empty()
    {
        return Err(ManifestStoreError::InvalidEntry {
            path: entry.path.clone(),
        });
    }
    let valid = match entry.status {
        ManifestEntryStatus::Indexed => {
            entry.version_id.is_some() && entry.error_class.is_none() && entry.error_json.is_none()
        }
        ManifestEntryStatus::FailedPreserved => {
            entry.version_id.is_some()
                && entry
                    .error_class
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
                && entry
                    .error_json
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
        }
        ManifestEntryStatus::Failed => {
            entry.version_id.is_none()
                && entry
                    .error_class
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
                && entry
                    .error_json
                    .as_ref()
                    .is_some_and(|value| !value.is_empty())
        }
    };
    if !valid
        || entry
            .error_json
            .as_ref()
            .is_some_and(|value| serde_json::from_str::<serde_json::Value>(value).is_err())
    {
        return Err(ManifestStoreError::InvalidEntry {
            path: entry.path.clone(),
        });
    }
    Ok(())
}

fn parse_status(status: &str) -> Option<ManifestEntryStatus> {
    match status {
        "indexed" => Some(ManifestEntryStatus::Indexed),
        "failed_preserved" => Some(ManifestEntryStatus::FailedPreserved),
        "failed" => Some(ManifestEntryStatus::Failed),
        _ => None,
    }
}

fn version_identity(
    connection: &Connection,
    version_id: i64,
) -> Result<VersionIdentity, ManifestStoreError> {
    let row = connection
        .query_row(
            "SELECT path, language, content_hash, extraction_epoch, complete_l1
             FROM file_versions WHERE version_id = ?1",
            [version_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((path, language, content_hash, extraction_epoch, complete_l1)) = row else {
        return Err(ManifestStoreError::VersionNotFound { version_id });
    };
    if complete_l1.is_none() {
        return Err(ManifestStoreError::VersionIncomplete { version_id });
    }
    Ok(VersionIdentity {
        path,
        language,
        content_hash,
        extraction_epoch,
    })
}

fn current_generation(
    connection: &Connection,
    view_id: &str,
) -> Result<Option<u64>, ManifestStoreError> {
    let generation = connection
        .query_row(
            "SELECT current_generation FROM views WHERE view_id = ?1",
            [view_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()?
        .ok_or_else(|| ManifestStoreError::ViewNotFound {
            view_id: view_id.to_string(),
        })?;
    Ok(generation.map(|value| u64::try_from(value).expect("manifest generations are positive")))
}

fn require_view_exists(connection: &Connection, view_id: &str) -> Result<(), ManifestStoreError> {
    let exists = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM views WHERE view_id = ?1)",
        [view_id],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(ManifestStoreError::ViewNotFound {
            view_id: view_id.to_string(),
        })
    }
}

fn validate_view_identity(view_id: &str, root: &str) -> Result<(), ManifestStoreError> {
    if view_id.is_empty() {
        return Err(ManifestStoreError::EmptyViewId);
    }
    if root.is_empty() {
        return Err(ManifestStoreError::EmptyRoot);
    }
    Ok(())
}

fn canonical_path(path: &str) -> Result<String, ManifestStoreError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\0')
        || path.contains(':')
    {
        return Err(ManifestStoreError::InvalidPath {
            path: path.to_string(),
        });
    }
    let slash_path = path.replace('\\', "/");
    if slash_path.starts_with('/') {
        return Err(ManifestStoreError::InvalidPath {
            path: path.to_string(),
        });
    }
    let mut segments = Vec::new();
    for segment in slash_path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                return Err(ManifestStoreError::InvalidPath {
                    path: path.to_string(),
                });
            }
            _ => segments.push(segment),
        }
    }
    if segments.is_empty() {
        return Err(ManifestStoreError::InvalidPath {
            path: path.to_string(),
        });
    }
    Ok(segments.join("/"))
}

fn canonical_error_json(path: &str, error_json: &str) -> Result<String, ManifestStoreError> {
    let value = serde_json::from_str(error_json).map_err(|_| ManifestStoreError::InvalidEntry {
        path: path.to_string(),
    })?;
    serde_json::to_string(&canonical_json_value(value)).map_err(|_| {
        ManifestStoreError::InvalidEntry {
            path: path.to_string(),
        }
    })
}

fn canonical_json_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonical_json_value).collect())
        }
        serde_json::Value::Object(values) => {
            let values = values
                .into_iter()
                .map(|(key, value)| (key, canonical_json_value(value)))
                .collect::<BTreeMap<_, _>>();
            serde_json::Value::Object(values.into_iter().collect())
        }
        other => other,
    }
}

fn sqlite_generation(generation: u64) -> Result<i64, ManifestStoreError> {
    i64::try_from(generation).map_err(|_| ManifestStoreError::GenerationOutOfRange { generation })
}

fn encode_text(target: &mut Vec<u8>, value: &str) {
    encode_bytes(target, value.as_bytes());
}

fn encode_bytes(target: &mut Vec<u8>, value: &[u8]) {
    target.extend_from_slice(&(value.len() as u64).to_be_bytes());
    target.extend_from_slice(value);
}

fn encode_optional_text(target: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            target.push(1);
            encode_text(target, value);
        }
        None => target.push(0),
    }
}
