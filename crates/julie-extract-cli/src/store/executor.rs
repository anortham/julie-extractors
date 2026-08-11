use std::path::PathBuf;
use std::sync::Arc;

use julie_extract_artifact::model::FileStatus;
use julie_extract_artifact::store::{
    CoordinatorExecutor, CoordinatorRequest, ExecutionContext, ExecutionQuantum, ManifestEntry,
    ManifestEntryStatus, ManifestPublishDisposition, ManifestPublishResult, ManifestStore,
    RequestKind, ResolutionBindingError, ResolutionBindingStore, ResolutionViewBinding,
    StoreFileVersion, StoreLevel, StoreLog, StoreLogEntry, StoreWriteRequest, StoreWriter,
    ViewResolutionState,
};
use julie_extractors::{
    EXTRACTION_IDENTITY_EPOCH, ExtractionLevel, detect_language_from_extension,
};
use rayon::prelude::*;
use rusqlite::{OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::capability_snapshot::artifact_capability_snapshot;
use crate::extraction::{
    extract_artifact_file_from_snapshot_at, read_source_snapshot, select_extraction_pool,
};
use crate::paths::FileTarget;
use crate::progress::{Counter, ScanProgress};
use crate::spool::create_scan_spool;

#[cfg(feature = "test-store-contract")]
macro_rules! store_test_crash {
    ($boundary:literal) => {
        julie_extract_artifact::store::test_hooks::crash_if($boundary)
    };
}

#[cfg(not(feature = "test-store-contract"))]
macro_rules! store_test_crash {
    ($boundary:literal) => {};
}

static IMPORT_SPOOL_IO: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportRequestPayload {
    pub schema_version: u32,
    pub family_id: String,
    pub root: String,
    pub view_id: String,
    pub requested_level: RequestedLevel,
    pub files: Vec<PlannedImportFile>,
    pub controls: ImportScanControls,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateRequestPayload {
    pub schema_version: u32,
    pub family_id: String,
    pub root: String,
    pub view_id: String,
    pub requested_level: RequestedLevel,
    pub file: PlannedImportFile,
    pub controls: ImportScanControls,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeleteRequestPayload {
    pub schema_version: u32,
    pub family_id: String,
    pub root: String,
    pub view_id: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FromArtifactRequestPayload {
    pub schema_version: u32,
    pub family_id: String,
    pub root: String,
    pub view_id: String,
    pub source: ArtifactSourceIdentity,
    pub files: Vec<PlannedArtifactFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ArtifactSourceIdentity {
    pub path: String,
    pub artifact_id: String,
    pub file_bytes: u64,
    pub file_sha256: String,
    pub extraction_epoch: u32,
    pub resolver_output_epoch: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlannedArtifactFile {
    pub file_id: String,
    pub path: String,
    pub language: String,
    pub content_hash: String,
    pub content_bytes: u64,
    pub indexed_at: String,
    pub status: String,
}

impl UpdateRequestPayload {
    fn execution_payload(&self) -> ImportRequestPayload {
        ImportRequestPayload {
            schema_version: self.schema_version,
            family_id: self.family_id.clone(),
            root: self.root.clone(),
            view_id: self.view_id.clone(),
            requested_level: self.requested_level,
            files: vec![self.file.clone()],
            controls: self.controls.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RequestedLevel {
    L1,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlannedImportFile {
    pub root_relative_path: String,
    pub content_hash: String,
    pub content_bytes: u64,
}

impl PlannedImportFile {
    fn target(&self, root: &std::path::Path) -> FileTarget {
        FileTarget {
            absolute_path: root.join(&self.root_relative_path),
            root_relative_path: self.root_relative_path.clone(),
        }
    }

    fn language(&self) -> String {
        std::path::Path::new(&self.root_relative_path)
            .extension()
            .and_then(|value| value.to_str())
            .and_then(detect_language_from_extension)
            .unwrap_or("unknown")
            .to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportScanControls {
    pub jobs: usize,
    #[serde(default)]
    pub ignore_files: Vec<String>,
    pub spool_dir: Option<String>,
    pub progress_file: Option<String>,
    #[serde(default = "default_l1_chunk_versions")]
    pub l1_chunk_versions: usize,
    #[serde(default = "default_deep_chunk_versions")]
    pub deep_chunk_versions: usize,
}

impl ImportScanControls {
    pub(crate) fn matches_runtime_controls(&self, other: &Self) -> bool {
        self.jobs == other.jobs
            && self.ignore_files == other.ignore_files
            && self.spool_dir == other.spool_dir
            && self.progress_file == other.progress_file
    }
}

impl Default for ImportScanControls {
    fn default() -> Self {
        Self {
            jobs: 0,
            ignore_files: Vec::new(),
            spool_dir: None,
            progress_file: None,
            l1_chunk_versions: default_l1_chunk_versions(),
            deep_chunk_versions: default_deep_chunk_versions(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FailureFact {
    path: String,
    language: String,
    version_id: Option<i64>,
    content_hash: String,
    indexed_at: String,
    error_json: String,
}

impl FailureFact {
    fn from_entry(entry: &ManifestEntry) -> Self {
        Self {
            path: entry.path.clone(),
            language: entry.language.clone(),
            version_id: entry.version_id,
            content_hash: entry.observed_content_hash.clone(),
            indexed_at: entry.indexed_at.clone(),
            error_json: entry.error_json.clone().unwrap_or_else(|| "{}".to_string()),
        }
    }

    fn into_entry(self) -> ManifestEntry {
        match self.version_id {
            Some(version_id) => ManifestEntry::failed_preserved(
                self.path,
                self.language,
                version_id,
                self.content_hash,
                self.indexed_at,
                "extract",
                self.error_json,
            ),
            None => ManifestEntry::failed(
                self.path,
                self.language,
                self.content_hash,
                self.indexed_at,
                "extract",
                self.error_json,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ImportChunk {
    level: StoreLevel,
    start: usize,
    end: usize,
}

struct DurableRequestState {
    failures: std::collections::BTreeMap<String, ManifestEntry>,
    manifest_generation: Option<u64>,
    manifest_hash: Option<String>,
    manifest_disposition: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilePlanOperation {
    Import,
    Update,
}

impl FilePlanOperation {
    fn event(self, suffix: &str) -> String {
        let operation = match self {
            Self::Import => "import",
            Self::Update => "update",
        };
        format!("store_{operation}_{suffix}")
    }
}

pub(crate) struct StoreRequestExecutor {
    store_db: PathBuf,
    family_id: String,
    watchdog: Option<crate::watchdog::ParentWatchdog>,
    progress: std::collections::BTreeMap<(String, String), Arc<ScanProgress>>,
}

/// A durable request may describe a million-file repository without permitting
/// an unbounded coordinator row or deserialization allocation.
pub(crate) const IMPORT_PAYLOAD_MAX_BYTES: usize = 64 * 1024 * 1024;
/// The plan cap is deliberately above the largest supported repository class.
pub(crate) const IMPORT_PLAN_MAX_FILES: usize = 1_000_000;
const IMPORT_JOBS_MAX: usize = 1024;
const IMPORT_IGNORE_FILES_MAX: usize = 1024;

impl StoreRequestExecutor {
    pub(crate) fn new(
        store_db: PathBuf,
        family_id: String,
        watchdog: Option<crate::watchdog::ParentWatchdog>,
    ) -> Self {
        Self {
            store_db,
            family_id,
            watchdog,
            progress: std::collections::BTreeMap::new(),
        }
    }

    pub(crate) fn validate_payload_json(
        &self,
        payload_json: &str,
    ) -> Result<ImportRequestPayload, String> {
        validate_payload_bounds(payload_json.len(), 0)?;
        let payload: ImportRequestPayload = serde_json::from_str(payload_json)
            .map_err(|_| "invalid_import_request_payload:invalid_json".to_string())?;
        if payload.schema_version != 1 {
            return Err("invalid_import_request_payload:unsupported_schema".to_string());
        }
        if payload.family_id != self.family_id {
            return Err("invalid_import_request_payload:family_mismatch".to_string());
        }
        if payload.view_id.is_empty()
            || payload.view_id.len() > super::args::MAX_STORE_IDENTIFIER_BYTES
            || payload.view_id.as_bytes().contains(&0)
        {
            return Err("invalid_import_request_payload:invalid_view".to_string());
        }
        if payload.root.is_empty()
            || payload.root.len() > super::args::MAX_STORE_PATH_BYTES
            || payload.root.as_bytes().contains(&0)
        {
            return Err("invalid_import_request_payload:invalid_root".to_string());
        }
        validate_payload_bounds(payload_json.len(), payload.files.len())?;
        if payload.controls.jobs > IMPORT_JOBS_MAX
            || payload.controls.l1_chunk_versions == 0
            || payload.controls.l1_chunk_versions > MAX_CHUNK_VERSIONS
            || payload.controls.deep_chunk_versions == 0
            || payload.controls.deep_chunk_versions > MAX_CHUNK_VERSIONS
            || payload.controls.ignore_files.len() > IMPORT_IGNORE_FILES_MAX
        {
            return Err("invalid_import_request_payload:controls_out_of_range".to_string());
        }
        for path in [
            payload.controls.spool_dir.as_deref(),
            payload.controls.progress_file.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if path.is_empty()
                || path.len() > super::args::MAX_STORE_PATH_BYTES
                || path.as_bytes().contains(&0)
                || !std::path::Path::new(path).is_absolute()
            {
                return Err("invalid_import_request_payload:invalid_control_path".to_string());
            }
        }
        for path in &payload.controls.ignore_files {
            if path.is_empty()
                || path.len() > super::args::MAX_STORE_PATH_BYTES
                || path.as_bytes().contains(&0)
                || !std::path::Path::new(path).is_absolute()
            {
                return Err("invalid_import_request_payload:invalid_control_path".to_string());
            }
        }
        let root = std::path::Path::new(&payload.root);
        let mut previous: Option<&str> = None;
        for file in &payload.files {
            let path = file.root_relative_path.as_str();
            if path.is_empty()
                || path.len() > super::args::MAX_STORE_PATH_BYTES
                || path.starts_with('/')
                || path.contains(['\\', ':', '\0'])
                || path
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
                || !root.join(path).starts_with(root)
            {
                return Err("invalid_import_request_payload:invalid_file_path".to_string());
            }
            if previous.is_some_and(|previous| previous >= path) {
                return Err("invalid_import_request_payload:plan_not_strictly_sorted".to_string());
            }
            let hash = file.content_hash.as_bytes();
            if hash.len() != 71
                || !file.content_hash.starts_with("blake3:")
                || !hash[7..]
                    .iter()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            {
                return Err("invalid_import_request_payload:invalid_content_hash".to_string());
            }
            previous = Some(path);
        }
        if !root.is_absolute()
            || (root.exists()
                && root
                    .canonicalize()
                    .ok()
                    .as_deref()
                    .and_then(std::path::Path::to_str)
                    != Some(payload.root.as_str()))
        {
            return Err("invalid_import_request_payload:root_not_canonical".to_string());
        }
        Ok(payload)
    }

    pub(crate) fn validate_from_artifact_payload_json(
        &self,
        payload_json: &str,
    ) -> Result<FromArtifactRequestPayload, String> {
        validate_payload_bounds(payload_json.len(), 0)?;
        let payload: FromArtifactRequestPayload = serde_json::from_str(payload_json)
            .map_err(|_| "invalid_from_artifact_request_payload:invalid_json".to_string())?;
        if payload.schema_version != 1 {
            return Err("invalid_from_artifact_request_payload:unsupported_schema".to_string());
        }
        if payload.family_id != self.family_id {
            return Err("invalid_from_artifact_request_payload:family_mismatch".to_string());
        }
        validate_payload_bounds(payload_json.len(), payload.files.len())?;
        let root = std::path::Path::new(&payload.root);
        let source_path = std::path::Path::new(&payload.source.path);
        if !root.is_absolute()
            || payload.view_id.is_empty()
            || payload.view_id.len() > super::args::MAX_STORE_IDENTIFIER_BYTES
            || !source_path.is_absolute()
            || payload.source.artifact_id.is_empty()
            || payload.source.file_sha256.len() != 64
            || !payload
                .source
                .file_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || payload.source.file_bytes == 0
            || payload.source.extraction_epoch != EXTRACTION_IDENTITY_EPOCH
            || payload.source.resolver_output_epoch != crate::resolution::RESOLUTION_VERSION
            || (root.exists() && root.canonicalize().ok().as_deref() != Some(root))
        {
            return Err("invalid_from_artifact_request_payload:identity".to_string());
        }
        let mut previous = None::<&str>;
        for file in &payload.files {
            if file.file_id.is_empty()
                || file.language.is_empty()
                || !valid_root_relative_path(root, &file.path)
                || !valid_blake3_hash(&file.content_hash)
                || !matches!(
                    file.status.as_str(),
                    "indexed" | "failed_preserved" | "unsupported"
                )
                || previous.is_some_and(|previous| previous >= file.path.as_str())
            {
                return Err("invalid_from_artifact_request_payload:file_plan".to_string());
            }
            previous = Some(&file.path);
        }
        Ok(payload)
    }

    pub(crate) fn validate_update_payload_json(
        &self,
        payload_json: &str,
    ) -> Result<UpdateRequestPayload, String> {
        validate_payload_bounds(payload_json.len(), 1)?;
        let payload: UpdateRequestPayload = serde_json::from_str(payload_json)
            .map_err(|_| "invalid_update_request_payload:invalid_json".to_string())?;
        let execution_payload = payload.execution_payload();
        let import_json = serde_json::to_string(&execution_payload)
            .map_err(|_| "invalid_update_request_payload:invalid_json".to_string())?;
        self.validate_payload_json(&import_json).map_err(|error| {
            error.replacen(
                "invalid_import_request_payload",
                "invalid_update_request_payload",
                1,
            )
        })?;
        Ok(payload)
    }

    pub(crate) fn validate_delete_payload_json(
        &self,
        payload_json: &str,
    ) -> Result<DeleteRequestPayload, String> {
        validate_payload_bounds(payload_json.len(), 0)?;
        let payload: DeleteRequestPayload = serde_json::from_str(payload_json)
            .map_err(|_| "invalid_delete_request_payload:invalid_json".to_string())?;
        if payload.schema_version != 1 {
            return Err("invalid_delete_request_payload:unsupported_schema".to_string());
        }
        if payload.family_id != self.family_id {
            return Err("invalid_delete_request_payload:family_mismatch".to_string());
        }
        if payload.view_id.is_empty()
            || payload.view_id.len() > super::args::MAX_STORE_IDENTIFIER_BYTES
            || payload.view_id.as_bytes().contains(&0)
        {
            return Err("invalid_delete_request_payload:invalid_view".to_string());
        }
        if payload.root.is_empty()
            || payload.root.len() > super::args::MAX_STORE_PATH_BYTES
            || payload.root.as_bytes().contains(&0)
        {
            return Err("invalid_delete_request_payload:invalid_root".to_string());
        }
        validate_payload_bounds(payload_json.len(), payload.files.len())?;
        let root = std::path::Path::new(&payload.root);
        let mut previous: Option<&str> = None;
        for path in &payload.files {
            if !valid_root_relative_path(root, path) {
                return Err("invalid_delete_request_payload:invalid_file_path".to_string());
            }
            if previous.is_some_and(|previous| previous >= path.as_str()) {
                return Err("invalid_delete_request_payload:plan_not_strictly_sorted".to_string());
            }
            previous = Some(path);
        }
        if payload.files.is_empty() {
            return Err("invalid_delete_request_payload:empty_plan".to_string());
        }
        if !root.is_absolute()
            || (root.exists()
                && root
                    .canonicalize()
                    .ok()
                    .as_deref()
                    .and_then(std::path::Path::to_str)
                    != Some(payload.root.as_str()))
        {
            return Err("invalid_delete_request_payload:root_not_canonical".to_string());
        }
        Ok(payload)
    }

    fn validated_payload(
        &self,
        request: &CoordinatorRequest,
    ) -> Result<ImportRequestPayload, String> {
        self.validate_payload_json(&request.payload_json)
    }

    fn progress_for(
        &mut self,
        transaction: &Transaction<'_>,
        request: &CoordinatorRequest,
        payload: &ImportRequestPayload,
        failed_files: usize,
        operation: FilePlanOperation,
    ) -> Result<Option<Arc<ScanProgress>>, String> {
        let Some(progress_file) = payload.controls.progress_file.as_deref() else {
            return Ok(None);
        };
        let key = (request.request_id.clone(), progress_file.to_string());
        if let Some(progress) = self.progress.get(&key) {
            return Ok(Some(Arc::clone(progress)));
        }
        let progress = Arc::new(
            ScanProgress::create_for_artifact(std::path::Path::new(progress_file), &self.store_db)
                .map_err(|error| format!("{error:?}"))?,
        );
        progress.enter_phase(match operation {
            FilePlanOperation::Import => "store_import",
            FilePlanOperation::Update => "store_update",
        });
        let completed_extractions: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM store_log
                 WHERE request_id = ?1 AND event_kind = 'version_level_completed'
                   AND level IN (1, 2)",
                [&request.request_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let completed_extractions = u64::try_from(completed_extractions)
            .map_err(|_| "invalid_progress_count".to_string())?
            .saturating_add(u64::try_from(failed_files).unwrap_or(u64::MAX));
        if completed_extractions > 0 {
            progress.advance(Counter::Extracted, completed_extractions);
            progress.advance(Counter::Spooled, completed_extractions);
        }
        self.progress.insert(key, Arc::clone(&progress));
        Ok(Some(progress))
    }

    fn extract(
        root: &std::path::Path,
        planned: &PlannedImportFile,
        spool_dir: Option<&std::path::Path>,
        progress: Option<&ScanProgress>,
        level: ExtractionLevel,
        indexed_at: &str,
    ) -> Result<StoreFileVersion, String> {
        validate_target_within_root(root, &planned.root_relative_path)?;
        let target = planned.target(root);
        let snapshot = read_source_snapshot(&target).map_err(|error| error.message.clone())?;
        if snapshot.content_hash != planned.content_hash {
            return Err(if level == ExtractionLevel::Full {
                "changed_between_waves".to_string()
            } else {
                "changed_during_l1_wave".to_string()
            });
        }
        let extension = target
            .absolute_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let language = detect_language_from_extension(extension)
            .unwrap_or("unknown")
            .to_string();
        if let Some(progress) = progress {
            progress.advance(Counter::Extracted, 1);
        }
        let artifact = extract_artifact_file_from_snapshot_at(
            root,
            &target,
            language,
            indexed_at.to_string(),
            snapshot,
            level,
        )
        .map_err(|error| error.message)?;
        let _spool_guard = IMPORT_SPOOL_IO
            .lock()
            .map_err(|_| "import_spool_lock_poisoned".to_string())?;
        let mut spool = create_scan_spool(spool_dir).map_err(|error| error.to_string())?;
        spool
            .file_spool_mut()
            .push(&artifact)
            .map_err(|error| error.to_string())?;
        if let Some(progress) = progress {
            progress.advance(Counter::Spooled, 1);
        }
        spool
            .file_spool_mut()
            .finish()
            .map_err(|error| error.to_string())?;
        let mut reader = spool
            .file_spool_mut()
            .reader()
            .map_err(|error| error.to_string())?;
        let header = reader
            .next_header()
            .ok_or_else(|| "empty_extraction_spool".to_string())?
            .map_err(|error| error.to_string())?;
        let artifact = reader
            .read_file(header)
            .map_err(|error| error.to_string())?;
        StoreFileVersion::try_from_artifact_file(EXTRACTION_IDENTITY_EPOCH, &artifact)
            .map_err(|error| error.to_string())
    }

    fn validate_full(
        transaction: &Transaction<'_>,
        planned: &PlannedImportFile,
        full: &StoreFileVersion,
    ) -> Result<StoreFileVersion, String> {
        let stored = StoreWriter::lookup_version_in_transaction(
            transaction,
            &planned.root_relative_path,
            &planned.content_hash,
            EXTRACTION_IDENTITY_EPOCH,
            StoreLevel::L1,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "l1_version_missing_before_deepening".to_string())?;
        if !StoreWriter::l1_projection_matches_in_transaction(transaction, &stored, full)
            .map_err(|error| error.to_string())?
        {
            return Err("l1_projection_mismatch".to_string());
        }
        Ok(full.clone())
    }

    fn manifest_entries(
        transaction: &Transaction<'_>,
        files: &[PlannedImportFile],
        failures: &std::collections::BTreeMap<String, ManifestEntry>,
        indexed_at: &str,
    ) -> Result<Vec<ManifestEntry>, String> {
        files
            .iter()
            .map(|file| {
                if let Some(failure) = failures.get(&file.root_relative_path) {
                    return Ok(failure.clone());
                }
                let version = StoreWriter::lookup_version_in_transaction(
                    transaction,
                    &file.root_relative_path,
                    &file.content_hash,
                    EXTRACTION_IDENTITY_EPOCH,
                    StoreLevel::L1,
                )
                .map_err(|error| error.to_string())?
                .ok_or_else(|| {
                    format!("l1_version_missing_at_publish:{}", file.root_relative_path)
                })?;
                Ok(ManifestEntry::indexed(
                    &file.root_relative_path,
                    file.language(),
                    version.version_id,
                    &file.content_hash,
                    indexed_at,
                ))
            })
            .collect()
    }

    fn result(
        transaction: &Transaction<'_>,
        payload: ImportRequestPayload,
        generation: u64,
        hash: String,
        full: bool,
        manifest_disposition: &str,
        operation: FilePlanOperation,
    ) -> Result<ExecutionQuantum, String> {
        let counts = terminal_row_counts(
            transaction,
            &payload.view_id,
            i64::try_from(generation).map_err(|_| "invalid_manifest_generation")?,
        )?;
        Ok(ExecutionQuantum::Complete {
            event_kind: operation.event("completed"),
            result_json: serde_json::json!({
                "family_id": payload.family_id,
                "l1": true,
                "l2": full,
                "l3": full,
                "manifest_generation": generation,
                "manifest_hash": hash,
                "manifest_disposition": manifest_disposition,
                "row_counts": {
                    "file_versions": counts.0,
                    "l1": counts.1,
                    "l2": counts.2,
                    "l3": counts.3,
                },
                "root": payload.root,
                "view_id": payload.view_id,
            })
            .to_string(),
        })
    }

    fn require_existing_view(
        transaction: &Transaction<'_>,
        view_id: &str,
        root: &str,
    ) -> Result<(), String> {
        let stored = transaction
            .query_row(
                "SELECT root FROM views WHERE view_id = ?1",
                [view_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "view_not_found".to_string())?;
        if stored != root {
            return Err("view_root_mismatch".to_string());
        }
        Ok(())
    }

    fn current_manifest(
        transaction: &Transaction<'_>,
        view_id: &str,
    ) -> Result<(Option<u64>, Vec<ManifestEntry>), String> {
        let generation = transaction
            .query_row(
                "SELECT current_generation FROM views WHERE view_id = ?1",
                [view_id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(|error| error.to_string())?
            .map(|value| u64::try_from(value).map_err(|_| "invalid_manifest_generation"))
            .transpose()?;
        let Some(generation) = generation else {
            return Ok((None, Vec::new()));
        };
        let mut statement = transaction
            .prepare(
                "SELECT path, language, version_id, status, observed_content_hash, indexed_at,
                        error_class, error_json
                 FROM manifest_entries
                 WHERE view_id = ?1 AND generation = ?2 ORDER BY path",
            )
            .map_err(|error| error.to_string())?;
        let generation_sql =
            i64::try_from(generation).map_err(|_| "invalid_manifest_generation")?;
        let entries = statement
            .query_map(rusqlite::params![view_id, generation_sql], |row| {
                let status = match row.get::<_, String>(3)?.as_str() {
                    "indexed" => ManifestEntryStatus::Indexed,
                    "failed_preserved" => ManifestEntryStatus::FailedPreserved,
                    "failed" => ManifestEntryStatus::Failed,
                    value => {
                        return Err(rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            format!("invalid manifest status {value}").into(),
                        ));
                    }
                };
                Ok(ManifestEntry {
                    path: row.get(0)?,
                    language: row.get(1)?,
                    version_id: row.get(2)?,
                    status,
                    observed_content_hash: row.get(4)?,
                    indexed_at: row.get(5)?,
                    error_class: row.get(6)?,
                    error_json: row.get(7)?,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        Ok((Some(generation), entries))
    }

    fn publish_file_plan(
        transaction: &Transaction<'_>,
        payload: &ImportRequestPayload,
        failures: &std::collections::BTreeMap<String, ManifestEntry>,
        indexed_at: &str,
        request_id: &str,
        operation: FilePlanOperation,
    ) -> Result<ManifestPublishResult, String> {
        let replacements =
            Self::manifest_entries(transaction, &payload.files, failures, indexed_at)?;
        let (expected, current) = Self::current_manifest(transaction, &payload.view_id)?;
        let entries = match operation {
            FilePlanOperation::Import => replacements,
            FilePlanOperation::Update => {
                let mut entries = current
                    .into_iter()
                    .map(|entry| (entry.path.clone(), entry))
                    .collect::<std::collections::BTreeMap<_, _>>();
                for replacement in replacements {
                    entries.insert(replacement.path.clone(), replacement);
                }
                entries.into_values().collect()
            }
        };
        store_test_crash!("manifest_before_publish");
        let published = ManifestStore::publish_in_transaction(
            transaction,
            &payload.view_id,
            expected,
            entries,
            request_id,
        )
        .map_err(|error| format!("store_import_publish_manifest:{error}"))?;
        store_test_crash!("manifest_after_publish_before_commit");
        Ok(published)
    }

    fn execute_delete(
        &self,
        transaction: &Transaction<'_>,
        request: &CoordinatorRequest,
        context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        if context.next_chunk_index != 0 {
            return Err("chunk_index_out_of_range".to_string());
        }
        let payload = self.validate_delete_payload_json(&request.payload_json)?;
        Self::require_existing_view(transaction, &payload.view_id, &payload.root)?;
        let (expected, entries) = Self::current_manifest(transaction, &payload.view_id)?;
        let deleted = payload
            .files
            .iter()
            .collect::<std::collections::BTreeSet<_>>();
        let entries = entries
            .into_iter()
            .filter(|entry| !deleted.contains(&entry.path))
            .collect::<Vec<_>>();
        store_test_crash!("manifest_before_publish");
        let published = ManifestStore::publish_in_transaction(
            transaction,
            &payload.view_id,
            expected,
            entries,
            &request.request_id,
        )
        .map_err(|error| error.to_string())?;
        store_test_crash!("manifest_after_publish_before_commit");
        let counts = terminal_row_counts(
            transaction,
            &payload.view_id,
            i64::try_from(published.generation).map_err(|_| "invalid_manifest_generation")?,
        )?;
        Ok(ExecutionQuantum::Complete {
            event_kind: "store_delete_completed".to_string(),
            result_json: serde_json::json!({
                "family_id": payload.family_id,
                "l1": true,
                "l2": false,
                "l3": false,
                "manifest_generation": published.generation,
                "manifest_hash": published.manifest_hash,
                "manifest_disposition": manifest_disposition(published.disposition),
                "row_counts": {
                    "file_versions": counts.0,
                    "l1": counts.1,
                    "l2": counts.2,
                    "l3": counts.3,
                },
                "root": payload.root,
                "view_id": payload.view_id,
            })
            .to_string(),
        })
    }

    fn execute_from_artifact(
        &self,
        transaction: &Transaction<'_>,
        request: &CoordinatorRequest,
        context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        let payload = self.validate_from_artifact_payload_json(&request.payload_json)?;
        super::from_artifact::verify_source_identity(&payload)?;
        ManifestStore::ensure_view_in_transaction(transaction, &payload.view_id, &payload.root)
            .map_err(|error| error.to_string())?;
        let chunk_ranges = chunk_ranges(
            &payload
                .files
                .iter()
                .map(|file| estimate_projected_wal_bytes(file.content_bytes))
                .collect::<Vec<_>>(),
            DEFAULT_L1_CHUNK_VERSIONS,
        );
        let chunk_index = usize::try_from(context.next_chunk_index)
            .map_err(|_| "chunk_index_out_of_range".to_string())?;
        let indexed_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| error.to_string())?;
        if let Some((start, end)) = chunk_ranges.get(chunk_index).copied() {
            let write_request = StoreWriteRequest::bulk(&request.request_id, &indexed_at);
            for planned in &payload.files[start..end] {
                if planned.status == "unsupported" {
                    continue;
                }
                let mut artifact =
                    super::from_artifact::load_artifact_file(&payload.source, planned)?;
                artifact.status = FileStatus::Indexed;
                let version = StoreFileVersion::try_from_artifact_file(
                    payload.source.extraction_epoch,
                    &artifact,
                )
                .map_err(|error| error.to_string())?;
                for level in [StoreLevel::L1, StoreLevel::L2, StoreLevel::L3] {
                    StoreWriter::write_level_in_transaction(
                        transaction,
                        &write_request,
                        (level == StoreLevel::L1).then_some(&artifact_capability_snapshot()),
                        &version,
                        level,
                    )
                    .map_err(|error| error.to_string())?;
                }
            }
            return Ok(ExecutionQuantum::Progress {
                event_kind: "store_from_artifact_versions_written".to_string(),
                payload_json: serde_json::json!({
                    "end": end,
                    "source_artifact_id": payload.source.artifact_id,
                    "start": start,
                })
                .to_string(),
                level: Some(StoreLevel::L3),
            });
        }
        if chunk_index == chunk_ranges.len() {
            let entries = payload
                .files
                .iter()
                .map(|planned| match planned.status.as_str() {
                    "indexed" | "failed_preserved" => {
                        let version = StoreWriter::lookup_version_in_transaction(
                            transaction,
                            &planned.path,
                            &planned.content_hash,
                            payload.source.extraction_epoch,
                            StoreLevel::L3,
                        )
                        .map_err(|error| error.to_string())?
                        .ok_or_else(|| format!("from_artifact_version_missing:{}", planned.path))?;
                        if planned.status == "indexed" {
                            Ok(ManifestEntry::indexed(
                                &planned.path,
                                &planned.language,
                                version.version_id,
                                &planned.content_hash,
                                &planned.indexed_at,
                            ))
                        } else {
                            Ok(ManifestEntry::failed_preserved(
                                &planned.path,
                                &planned.language,
                                version.version_id,
                                &planned.content_hash,
                                &planned.indexed_at,
                                "source_failed_preserved",
                                "{\"source_status\":\"failed_preserved\"}",
                            ))
                        }
                    }
                    "unsupported" => Ok(ManifestEntry::failed(
                        &planned.path,
                        &planned.language,
                        &planned.content_hash,
                        &planned.indexed_at,
                        "unsupported",
                        "{\"source_status\":\"unsupported\"}",
                    )),
                    _ => Err(format!(
                        "invalid_from_artifact_file_status:{}",
                        planned.path
                    )),
                })
                .collect::<Result<Vec<_>, String>>()?;
            let expected = transaction
                .query_row(
                    "SELECT current_generation FROM views WHERE view_id=?1",
                    [&payload.view_id],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .map_err(|error| error.to_string())?
                .map(|generation| {
                    u64::try_from(generation).map_err(|_| "invalid_manifest_generation".to_string())
                })
                .transpose()?;
            store_test_crash!("from_artifact_manifest_before_publish");
            let published = ManifestStore::publish_in_transaction(
                transaction,
                &payload.view_id,
                expected,
                entries,
                &request.request_id,
            )
            .map_err(|error| error.to_string())?;
            store_test_crash!("from_artifact_manifest_after_publish_before_commit");
            return Ok(ExecutionQuantum::Progress {
                event_kind: "store_from_artifact_manifest_published".to_string(),
                payload_json: serde_json::json!({
                    "generation": published.generation,
                    "manifest_disposition": manifest_disposition(published.disposition),
                    "manifest_hash": published.manifest_hash,
                })
                .to_string(),
                level: Some(StoreLevel::L1),
            });
        }
        if chunk_index != chunk_ranges.len().saturating_add(1) {
            return Err("chunk_index_out_of_range".to_string());
        }
        let (generation, manifest_hash) = transaction
            .query_row(
                "SELECT view.current_generation,manifest.manifest_hash
                 FROM views AS view JOIN manifests AS manifest
                   ON manifest.view_id=view.view_id
                  AND manifest.generation=view.current_generation
                 WHERE view.view_id=?1",
                [&payload.view_id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(|error| error.to_string())?;
        let base = super::from_artifact::materialize_resolution_base(
            transaction,
            &self.store_db,
            &payload,
            generation,
            &manifest_hash,
            &request.request_id,
            &indexed_at,
        )?;
        // Same-quantum constraint: if later steps fail, roll back the catalog and
        // remove any final base file this call published so the FS does not outlive
        // the uncommitted ready/building rows.
        let published_cleanup = if base.published_new_file {
            self.store_db
                .parent()
                .map(|generation_dir| generation_dir.join(format!("bases/{}.db", base.base_id)))
        } else {
            None
        };
        let cleanup_published = |keep: bool| {
            if keep {
                return;
            }
            if let Some(path) = published_cleanup.as_ref() {
                let _ = super::from_artifact::remove_base_file_set_for_cleanup(path);
            }
        };
        let complete = (|| {
            store_test_crash!("from_artifact_base_before_catalog");
            let identifier_count = i64::try_from(base.identity.counts.identifiers)
                .map_err(|_| "resolution_identifier_count_out_of_range".to_string())?;
            let pending_count = i64::try_from(base.identity.counts.pending)
                .map_err(|_| "resolution_pending_count_out_of_range".to_string())?;
            let registered: (String, i64, String, i64, i64) = transaction
                .query_row(
                    "SELECT manifest_hash,resolver_output_epoch,file_sha256,
                        identifier_count,pending_count
                 FROM resolution_bases WHERE base_id=?1 AND state='ready'",
                    [&base.base_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .map_err(|error| error.to_string())?;
            if registered
                != (
                    manifest_hash.clone(),
                    payload.source.resolver_output_epoch,
                    base.identity.file_sha256.clone(),
                    identifier_count,
                    pending_count,
                )
            {
                return Err("resolution_base_catalog_identity_mismatch".to_string());
            }
            debug_assert!(base.already_ready || registered.2 == base.identity.file_sha256);
            let delta_generation: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(delta_generation),0) FROM resolution_deltas WHERE view_id=?1",
                [&payload.view_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| error.to_string())?
            .checked_add(1)
            .ok_or_else(|| "resolution_delta_generation_out_of_range".to_string())?;
            transaction
                .execute(
                    "INSERT INTO resolution_deltas
                 (view_id,delta_generation,base_id,manifest_generation,manifest_hash,
                  resolver_output_epoch,identifier_replacements,pending_replacements,
                  pending_tombstones,exact_gap_rows,exact_gap_files,exact_gap_json,
                  request_id,created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,0,0,0,0,0,
                         '{\"files\":[],\"rows\":[]}',?7,?8)",
                    rusqlite::params![
                        payload.view_id,
                        delta_generation,
                        base.base_id,
                        generation,
                        manifest_hash,
                        payload.source.resolver_output_epoch,
                        request.request_id,
                        indexed_at,
                    ],
                )
                .map_err(|error| error.to_string())?;
            store_test_crash!("from_artifact_exact_before_cas");
            ResolutionBindingStore::publish_exact_binding_in_transaction(
                transaction,
                &ResolutionViewBinding {
                    view_id: payload.view_id.clone(),
                    manifest_generation: generation,
                    manifest_hash: manifest_hash.clone(),
                    base_id: base.base_id.clone(),
                    delta_generation,
                    state: ViewResolutionState::Exact,
                    exact_at: Some(generation),
                },
                payload.source.resolver_output_epoch,
                &indexed_at,
            )
            .map_err(|error| match error {
                ResolutionBindingError::CasLost { .. } => "resolution_binding_cas_lost".to_string(),
                error => error.to_string(),
            })?;
            store_test_crash!("from_artifact_exact_after_cas_before_commit");
            StoreLog::append_effect(
                transaction,
                &StoreLogEntry::new(
                    &request.request_id,
                    "resolution_bound",
                    serde_json::json!({
                        "base_id": base.base_id,
                        "delta_generation": delta_generation,
                        "manifest_generation": generation,
                        "state": "exact",
                    })
                    .to_string(),
                    &indexed_at,
                )
                .with_view(&payload.view_id)
                .with_generation(
                    u64::try_from(generation)
                        .map_err(|_| "invalid_manifest_generation".to_string())?,
                ),
            )
            .map_err(|error| error.to_string())?;
            let counts = terminal_row_counts(transaction, &payload.view_id, generation)?;
            let state = load_durable_request_state(transaction, &request.request_id)?;
            Ok(ExecutionQuantum::Complete {
                event_kind: "store_from_artifact_completed".to_string(),
                result_json: serde_json::json!({
                    "family_id": payload.family_id,
                    "l1": true,
                    "l2": true,
                    "l3": true,
                    "manifest_generation": generation,
                    "manifest_hash": manifest_hash,
                    "manifest_disposition": state.manifest_disposition,
                    "row_counts": {
                        "file_versions": counts.0,
                        "l1": counts.1,
                        "l2": counts.2,
                        "l3": counts.3,
                    },
                    "root": payload.root,
                    "view_id": payload.view_id,
                })
                .to_string(),
            })
        })();
        match complete {
            Ok(quantum) => {
                cleanup_published(true);
                Ok(quantum)
            }
            Err(error) => {
                cleanup_published(false);
                Err(error)
            }
        }
    }
}

fn terminal_row_counts(
    transaction: &Transaction<'_>,
    view_id: &str,
    generation: i64,
) -> Result<(u64, u64, u64, u64), String> {
    let counts: (i64, i64, i64, i64) = transaction
        .query_row(
            "WITH request_versions AS (
               SELECT DISTINCT version_id FROM manifest_entries
               WHERE view_id = ?1 AND generation = ?2 AND version_id IS NOT NULL
             )
             SELECT
               (SELECT COUNT(*) FROM request_versions),
               (SELECT COUNT(*) FROM symbols WHERE version_id IN request_versions)
                 + (SELECT COUNT(*) FROM symbol_annotations WHERE version_id IN request_versions)
                 + (SELECT COUNT(*) FROM reference_sites WHERE level = 1 AND version_id IN request_versions)
                 + (SELECT COUNT(*) FROM relationships WHERE version_id IN request_versions)
                 + (SELECT COUNT(*) FROM pending_relationships WHERE version_id IN request_versions)
                 + (SELECT COUNT(*) FROM type_facts WHERE version_id IN request_versions)
                 + (SELECT COUNT(*) FROM complexity_metrics WHERE version_id IN request_versions)
                 + (SELECT COUNT(*) FROM parse_diagnostics WHERE version_id IN request_versions),
               (SELECT COUNT(*) FROM identifiers WHERE version_id IN request_versions)
                 + (SELECT COUNT(*) FROM reference_sites WHERE level = 2 AND version_id IN request_versions),
               (SELECT COUNT(*) FROM type_arguments WHERE version_id IN request_versions)
                 + (SELECT COUNT(*) FROM type_argument_usages WHERE version_id IN request_versions)
                 + (SELECT COUNT(*) FROM literals WHERE version_id IN request_versions)
                 + (SELECT COUNT(*) FROM source_regions WHERE version_id IN request_versions)
                 + (SELECT COUNT(*) FROM structural_facts WHERE version_id IN request_versions)",
            rusqlite::params![view_id, generation],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|error| error.to_string())?;
    Ok((
        u64::try_from(counts.0).map_err(|_| "invalid_row_count")?,
        u64::try_from(counts.1).map_err(|_| "invalid_row_count")?,
        u64::try_from(counts.2).map_err(|_| "invalid_row_count")?,
        u64::try_from(counts.3).map_err(|_| "invalid_row_count")?,
    ))
}

fn validate_payload_bounds(serialized_bytes: usize, files: usize) -> Result<(), String> {
    if serialized_bytes > IMPORT_PAYLOAD_MAX_BYTES {
        return Err("invalid_import_request_payload:payload_too_large".to_string());
    }
    if files > IMPORT_PLAN_MAX_FILES {
        return Err("invalid_import_request_payload:too_many_files".to_string());
    }
    Ok(())
}

fn valid_root_relative_path(root: &std::path::Path, path: &str) -> bool {
    !path.is_empty()
        && path.len() <= super::args::MAX_STORE_PATH_BYTES
        && !path.starts_with('/')
        && !path.contains(['\\', ':', '\0'])
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        && root.join(path).starts_with(root)
}

fn valid_blake3_hash(hash: &str) -> bool {
    hash.strip_prefix("blake3:").is_some_and(|value| {
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

pub(crate) fn validate_target_within_root(
    root: &std::path::Path,
    root_relative_path: &str,
) -> Result<(), String> {
    let target = root.join(root_relative_path);
    if let Ok(canonical) = target.canonicalize()
        && !canonical.starts_with(root)
    {
        return Err("invalid_file_path:outside_root".to_string());
    }
    Ok(())
}

fn load_durable_request_state(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<DurableRequestState, String> {
    let mut statement = transaction
        .prepare(
            "SELECT payload_json FROM store_log
             WHERE request_id = ?1 AND terminal = 0 ORDER BY sequence",
        )
        .map_err(|error| error.to_string())?;
    let payloads = statement
        .query_map([request_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    let mut failures = std::collections::BTreeMap::new();
    let mut manifest_generation = None;
    let mut manifest_hash = None;
    let mut manifest_disposition = "not_published";
    for payload_json in payloads {
        let value: serde_json::Value =
            serde_json::from_str(&payload_json.map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        if let Some(facts) = value.get("failures") {
            for fact in serde_json::from_value::<Vec<FailureFact>>(facts.clone())
                .map_err(|error| error.to_string())?
            {
                failures.insert(fact.path.clone(), fact.into_entry());
            }
        }
        if let Some(generation) = value.get("generation").and_then(serde_json::Value::as_u64) {
            manifest_generation = Some(generation);
        }
        if let Some(hash) = value
            .get("manifest_hash")
            .and_then(serde_json::Value::as_str)
        {
            manifest_hash = Some(hash.to_string());
        }
        manifest_disposition = match value
            .get("manifest_disposition")
            .and_then(|value| value.as_str())
        {
            Some("created") => "created",
            Some("reused") => "reused",
            _ => manifest_disposition,
        };
    }
    Ok(DurableRequestState {
        failures,
        manifest_generation,
        manifest_hash,
        manifest_disposition,
    })
}

fn failure_facts(failures: &std::collections::BTreeMap<String, ManifestEntry>) -> Vec<FailureFact> {
    failures.values().map(FailureFact::from_entry).collect()
}

const DEFAULT_L1_CHUNK_VERSIONS: usize = 100;
const DEFAULT_DEEP_CHUNK_VERSIONS: usize = 8;
const MAX_CHUNK_VERSIONS: usize = 1_000_000;
const WAL_BUDGET_BYTES: u64 = 128 * 1024 * 1024;

fn default_l1_chunk_versions() -> usize {
    DEFAULT_L1_CHUNK_VERSIONS
}

fn default_deep_chunk_versions() -> usize {
    DEFAULT_DEEP_CHUNK_VERSIONS
}

pub(crate) fn frozen_chunk_versions_from_environment() -> Result<(usize, usize), String> {
    let Some(value) = std::env::var_os("MILLER_STORE_CHUNK_VERSIONS") else {
        return Ok((DEFAULT_L1_CHUNK_VERSIONS, DEFAULT_DEEP_CHUNK_VERSIONS));
    };
    let value = value
        .into_string()
        .map_err(|_| "invalid_store_chunk_versions:non_utf8".to_string())?;
    let configured = value
        .parse::<usize>()
        .map_err(|_| "invalid_store_chunk_versions:expected_non_negative_integer".to_string())?;
    if configured > MAX_CHUNK_VERSIONS {
        return Err("invalid_store_chunk_versions:out_of_range".to_string());
    }
    let limit = configured.max(1);
    Ok((limit, limit))
}

pub(crate) fn estimate_projected_wal_bytes(source_bytes: u64) -> u64 {
    source_bytes.saturating_mul(16).saturating_add(64 * 1024)
}

fn map_with_jobs<T, R, F>(items: &[T], jobs: usize, map: F) -> Result<Vec<R>, String>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Send + Sync,
{
    let pool = select_extraction_pool(jobs, |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .stack_size(16 * 1024 * 1024)
            .build()
    })
    .map_err(|error| format!("extraction_pool_unavailable: {error}"))?;
    Ok(pool.install(|| items.par_iter().map(map).collect()))
}

fn build_chunks(
    files: &[PlannedImportFile],
    level: StoreLevel,
    version_limit: usize,
) -> Vec<ImportChunk> {
    chunk_ranges(
        &files
            .iter()
            .map(|file| estimate_projected_wal_bytes(file.content_bytes))
            .collect::<Vec<_>>(),
        version_limit,
    )
    .into_iter()
    .map(|(start, end)| ImportChunk { level, start, end })
    .collect()
}

fn chunk_ranges(sizes: &[u64], version_limit: usize) -> Vec<(usize, usize)> {
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < sizes.len() {
        let mut end = start;
        let mut estimated_wal = 0u64;
        while end < sizes.len() && end - start < version_limit {
            let next = sizes[end].max(1);
            if end > start && estimated_wal.saturating_add(next) > WAL_BUDGET_BYTES {
                break;
            }
            estimated_wal = estimated_wal.saturating_add(next);
            end += 1;
        }
        chunks.push((start, end));
        start = end;
    }
    chunks
}

impl CoordinatorExecutor for StoreRequestExecutor {
    fn execute_quantum(
        &mut self,
        transaction: &Transaction<'_>,
        request: &CoordinatorRequest,
        context: ExecutionContext,
    ) -> Result<ExecutionQuantum, String> {
        let (operation, payload) = match request.kind {
            RequestKind::Import => (FilePlanOperation::Import, self.validated_payload(request)?),
            RequestKind::Update => (
                FilePlanOperation::Update,
                self.validate_update_payload_json(&request.payload_json)?
                    .execution_payload(),
            ),
            RequestKind::Delete => return self.execute_delete(transaction, request, context),
            RequestKind::FromArtifact => {
                return self.execute_from_artifact(transaction, request, context);
            }
            RequestKind::Resolve | RequestKind::Export => {
                return Err(format!(
                    "unsupported_request_kind:{}",
                    request.kind.as_str()
                ));
            }
        };
        if self
            .watchdog
            .as_ref()
            .is_some_and(crate::watchdog::ParentWatchdog::parent_exited)
        {
            return Err("parent_process_exited".to_string());
        }
        let root = PathBuf::from(&payload.root);
        let spool_dir = payload.controls.spool_dir.as_deref().map(PathBuf::from);
        let requested_full = payload.requested_level == RequestedLevel::Full;
        let l1_chunks = build_chunks(
            &payload.files,
            StoreLevel::L1,
            payload.controls.l1_chunk_versions,
        );
        let l1_chunk_count = l1_chunks.len();
        let mut chunks = l1_chunks;
        if requested_full {
            chunks.extend(build_chunks(
                &payload.files,
                StoreLevel::L3,
                payload.controls.deep_chunk_versions,
            ));
        }
        let DurableRequestState {
            mut failures,
            manifest_generation,
            manifest_hash,
            manifest_disposition: mut persisted_manifest_disposition,
        } = load_durable_request_state(transaction, &request.request_id)
            .map_err(|error| format!("store_import_load_request_state:{error}"))?;
        let progress =
            self.progress_for(transaction, request, &payload, failures.len(), operation)?;
        match operation {
            FilePlanOperation::Import => {
                ManifestStore::ensure_view_in_transaction(
                    transaction,
                    &payload.view_id,
                    &payload.root,
                )
                .map_err(|error| format!("store_import_ensure_view:{error}"))?;
            }
            FilePlanOperation::Update => {
                Self::require_existing_view(transaction, &payload.view_id, &payload.root)?;
            }
        }
        let indexed_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| error.to_string())?;
        let chunk_index = usize::try_from(context.next_chunk_index)
            .map_err(|_| "chunk_index_out_of_range".to_string())?;
        if payload.files.is_empty() {
            let published = Self::publish_file_plan(
                transaction,
                &payload,
                &failures,
                &indexed_at,
                &request.request_id,
                operation,
            )?;
            return Self::result(
                transaction,
                payload,
                published.generation,
                published.manifest_hash,
                requested_full,
                manifest_disposition(published.disposition),
                operation,
            );
        }
        let chunk = chunks
            .get(chunk_index)
            .copied()
            .ok_or_else(|| "chunk_index_out_of_range".to_string())?;
        if requested_full && chunk_index == l1_chunk_count {
            wait_for_full_resume_test_hook()?;
        }
        let mut work = Vec::new();
        for discovered in &payload.files[chunk.start..chunk.end] {
            if chunk.level != StoreLevel::L1
                && failures.contains_key(&discovered.root_relative_path)
            {
                continue;
            }
            let complete = StoreWriter::lookup_version_in_transaction(
                transaction,
                &discovered.root_relative_path,
                &discovered.content_hash,
                EXTRACTION_IDENTITY_EPOCH,
                chunk.level,
            )
            .map_err(|error| format!("store_import_lookup_version:{error}"))?;
            if complete.is_some() {
                continue;
            }
            work.push(discovered.clone());
        }
        let extraction_level = if chunk.level == StoreLevel::L1 {
            ExtractionLevel::Symbols
        } else {
            ExtractionLevel::Full
        };
        let extracted = map_with_jobs(&work, payload.controls.jobs, |discovered| {
            Self::extract(
                &root,
                discovered,
                spool_dir.as_deref(),
                progress.as_deref(),
                extraction_level,
                &indexed_at,
            )
        })
        .map_err(|error| format!("store_import_extract:{error}"))?;
        for (discovered, extracted) in work.into_iter().zip(extracted) {
            let write_request = StoreWriteRequest::bulk(&request.request_id, &indexed_at);
            match chunk.level {
                StoreLevel::L1 => {
                    let version = match extracted {
                        Ok(version) => version,
                        Err(message) => {
                            if message == "invalid_file_path:outside_root" {
                                return Err(message);
                            }
                            let prior_version = transaction
                                .query_row(
                                    "SELECT me.version_id
                                     FROM views v JOIN manifest_entries me
                                       ON me.view_id = v.view_id
                                      AND me.generation = v.current_generation
                                     WHERE v.view_id = ?1 AND me.path = ?2",
                                    rusqlite::params![
                                        payload.view_id,
                                        discovered.root_relative_path
                                    ],
                                    |row| row.get::<_, Option<i64>>(0),
                                )
                                .optional()
                                .map_err(|error| error.to_string())?
                                .flatten();
                            let error_json = serde_json::json!({ "message": message }).to_string();
                            let failure = match prior_version {
                                Some(version_id) => ManifestEntry::failed_preserved(
                                    &discovered.root_relative_path,
                                    discovered.language(),
                                    version_id,
                                    &discovered.content_hash,
                                    &indexed_at,
                                    "extract",
                                    error_json,
                                ),
                                None => ManifestEntry::failed(
                                    &discovered.root_relative_path,
                                    discovered.language(),
                                    &discovered.content_hash,
                                    &indexed_at,
                                    "extract",
                                    error_json,
                                ),
                            };
                            failures.insert(discovered.root_relative_path.clone(), failure);
                            continue;
                        }
                    };
                    let snapshot = artifact_capability_snapshot();
                    StoreWriter::write_level_in_transaction(
                        transaction,
                        &write_request,
                        Some(&snapshot),
                        &version,
                        StoreLevel::L1,
                    )
                    .map_err(|error| format!("store_import_write_l1:{error}"))?;
                }
                StoreLevel::L2 | StoreLevel::L3 => {
                    let full = Self::validate_full(transaction, &discovered, &extracted?)?;
                    StoreWriter::write_level_in_transaction(
                        transaction,
                        &write_request,
                        None,
                        &full,
                        StoreLevel::L2,
                    )
                    .map_err(|error| error.to_string())?;
                    store_test_crash!("deep_after_l2_before_l3");
                    StoreWriter::write_level_in_transaction(
                        transaction,
                        &write_request,
                        None,
                        &full,
                        StoreLevel::L3,
                    )
                    .map_err(|error| error.to_string())?;
                    store_test_crash!("deep_after_l3_before_commit");
                }
            }
        }
        if chunk.level == StoreLevel::L1 && chunk_index + 1 == l1_chunk_count {
            let published = Self::publish_file_plan(
                transaction,
                &payload,
                &failures,
                &indexed_at,
                &request.request_id,
                operation,
            )?;
            persisted_manifest_disposition = manifest_disposition(published.disposition);
            wait_for_l1_test_hook()?;
            if !requested_full {
                store_test_crash!("l1_only_final_before_terminal");
                if let Some(progress) = progress.as_deref() {
                    progress.enter_phase("complete");
                }
                return Self::result(
                    transaction,
                    payload,
                    published.generation,
                    published.manifest_hash,
                    false,
                    persisted_manifest_disposition,
                    operation,
                );
            }
            return Ok(ExecutionQuantum::Progress {
                event_kind: operation.event("l1_published"),
                payload_json: serde_json::json!({
                    "completed_files": chunk.end,
                    "failures": failure_facts(&failures),
                    "generation": published.generation,
                    "manifest_hash": published.manifest_hash,
                    "manifest_disposition": persisted_manifest_disposition,
                })
                .to_string(),
                level: Some(StoreLevel::L1),
            });
        }
        if chunk_index + 1 < chunks.len() {
            return Ok(ExecutionQuantum::Progress {
                event_kind: operation.event(&format!("l{}_chunk", chunk.level.as_i64())),
                payload_json: serde_json::json!({
                    "completed_files": chunk.end,
                    "failures": failure_facts(&failures),
                    "manifest_disposition": persisted_manifest_disposition,
                })
                .to_string(),
                level: Some(chunk.level),
            });
        }
        let generation = manifest_generation.ok_or("missing_l1_manifest_generation")?;
        let hash = manifest_hash.ok_or("missing_l1_manifest_hash")?;
        if let Some(progress) = progress.as_deref() {
            progress.enter_phase("complete");
        }
        Self::result(
            transaction,
            payload,
            generation,
            hash,
            true,
            persisted_manifest_disposition,
            operation,
        )
    }
}

fn manifest_disposition(disposition: ManifestPublishDisposition) -> &'static str {
    match disposition {
        ManifestPublishDisposition::Created => "created",
        ManifestPublishDisposition::Reused => "reused",
    }
}

#[cfg(feature = "test-store-contract")]
fn wait_for_test_hook(
    ready_variable: &str,
    resume_variable: &str,
    missing_resume_error: &str,
    timeout_error: &str,
) -> Result<(), String> {
    let Ok(ready) = std::env::var(ready_variable) else {
        return Ok(());
    };
    let resume = std::env::var(resume_variable).map_err(|_| missing_resume_error.to_string())?;
    std::fs::write(ready, b"ready").map_err(|error| error.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !std::path::Path::new(&resume).exists() {
        if std::time::Instant::now() >= deadline {
            return Err(timeout_error.to_string());
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    Ok(())
}

#[cfg(feature = "test-store-contract")]
fn wait_for_l1_test_hook() -> Result<(), String> {
    wait_for_test_hook(
        "JULIE_EXTRACT_STORE_TEST_L1_READY_FILE",
        "JULIE_EXTRACT_STORE_TEST_L1_RESUME_FILE",
        "missing_l1_test_resume_file",
        "l1_test_hook_timeout",
    )
}

#[cfg(not(feature = "test-store-contract"))]
fn wait_for_l1_test_hook() -> Result<(), String> {
    Ok(())
}

#[cfg(feature = "test-store-contract")]
fn wait_for_full_resume_test_hook() -> Result<(), String> {
    wait_for_test_hook(
        "JULIE_EXTRACT_STORE_TEST_FULL_RESUME_READY_FILE",
        "JULIE_EXTRACT_STORE_TEST_FULL_RESUME_FILE",
        "missing_full_resume_test_file",
        "full_resume_test_hook_timeout",
    )
}

#[cfg(not(feature = "test-store-contract"))]
fn wait_for_full_resume_test_hook() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        IMPORT_PAYLOAD_MAX_BYTES, IMPORT_PLAN_MAX_FILES, MAX_CHUNK_VERSIONS, StoreRequestExecutor,
        WAL_BUDGET_BYTES, chunk_ranges, estimate_projected_wal_bytes, map_with_jobs,
        validate_payload_bounds,
    };

    #[test]
    fn wal_budget_splits_before_the_next_version_would_exceed_128_mib() {
        let sizes = [70 * 1024 * 1024, 70 * 1024 * 1024, 1];
        assert_eq!(chunk_ranges(&sizes, 100), [(0, 1), (1, 3)]);
        for (start, end) in chunk_ranges(&sizes, 100) {
            assert!(sizes[start..end].iter().sum::<u64>() <= WAL_BUDGET_BYTES);
        }
    }

    #[test]
    fn jobs_greater_than_one_runs_import_extraction_concurrently() {
        let in_flight = std::sync::atomic::AtomicUsize::new(0);
        let maximum = std::sync::atomic::AtomicUsize::new(0);
        let values = (0..8).collect::<Vec<_>>();
        let output = map_with_jobs(&values, 4, |_| {
            let current = in_flight.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            maximum.fetch_max(current, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(20));
            in_flight.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            1
        })
        .unwrap();
        assert_eq!(output.len(), 8);
        assert!(maximum.load(std::sync::atomic::Ordering::SeqCst) >= 2);
    }

    #[test]
    fn wal_estimate_accounts_for_projected_row_amplification() {
        let source_bytes = 8 * 1024 * 1024;
        assert!(estimate_projected_wal_bytes(source_bytes) > source_bytes);
        assert!(estimate_projected_wal_bytes(source_bytes) >= WAL_BUDGET_BYTES);
    }

    #[test]
    fn chunk_limits_keep_the_l1_and_deep_defaults_independent() {
        let sizes = [1; 101];
        assert_eq!(chunk_ranges(&sizes, 100), [(0, 100), (100, 101)]);
        assert_eq!(chunk_ranges(&sizes[..17], 8), [(0, 8), (8, 16), (16, 17)]);
    }

    #[test]
    fn durable_chunk_limits_must_be_positive_and_bounded() {
        let executor = StoreRequestExecutor::new(
            std::path::PathBuf::from("/trusted/store.db"),
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11".to_string(),
            None,
        );
        for (field, value) in [("l1_chunk_versions", 0), ("deep_chunk_versions", 0)] {
            let payload = serde_json::json!({
                "schema_version": 1,
                "family_id": "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
                "root": "/trusted/root",
                "view_id": "view-main",
                "requested_level": "l1",
                "files": [],
                "controls": {
                    "jobs": 1,
                    "l1_chunk_versions": if field == "l1_chunk_versions" { value } else { 100 },
                    "deep_chunk_versions": if field == "deep_chunk_versions" { value } else { 8 },
                },
            });
            assert_eq!(
                executor
                    .validate_payload_json(&payload.to_string())
                    .unwrap_err(),
                "invalid_import_request_payload:controls_out_of_range"
            );
        }
        let payload = serde_json::json!({
            "schema_version": 1,
            "family_id": "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "root": "/trusted/root",
            "view_id": "view-main",
            "requested_level": "l1",
            "files": [],
            "controls": {
                "jobs": 1,
                "l1_chunk_versions": MAX_CHUNK_VERSIONS + 1,
                "deep_chunk_versions": 8,
            },
        });
        assert_eq!(
            executor
                .validate_payload_json(&payload.to_string())
                .unwrap_err(),
            "invalid_import_request_payload:controls_out_of_range"
        );
    }

    #[test]
    fn durable_plan_bounds_cover_large_repositories_and_reject_the_next_value() {
        assert_eq!(IMPORT_PAYLOAD_MAX_BYTES, 64 * 1024 * 1024);
        assert_eq!(IMPORT_PLAN_MAX_FILES, 1_000_000);
        assert!(validate_payload_bounds(IMPORT_PAYLOAD_MAX_BYTES, IMPORT_PLAN_MAX_FILES).is_ok());
        assert_eq!(
            validate_payload_bounds(IMPORT_PAYLOAD_MAX_BYTES + 1, 0).unwrap_err(),
            "invalid_import_request_payload:payload_too_large"
        );
        assert_eq!(
            validate_payload_bounds(0, IMPORT_PLAN_MAX_FILES + 1).unwrap_err(),
            "invalid_import_request_payload:too_many_files"
        );
    }

    #[test]
    fn durable_plan_rejects_a_caller_supplied_wal_estimate() {
        let executor = StoreRequestExecutor::new(
            std::path::PathBuf::from("/trusted/store.db"),
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11".to_string(),
            None,
        );
        let payload = serde_json::json!({
            "schema_version": 1,
            "family_id": "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "root": "/trusted/root",
            "view_id": "view-main",
            "requested_level": "l1",
            "files": [{
                "root_relative_path": "lib.rs",
                "content_hash": format!("blake3:{}", "0".repeat(64)),
                "content_bytes": 1,
                "projected_wal_bytes": 1,
            }],
            "controls": { "jobs": 1 },
        });
        assert_eq!(
            executor
                .validate_payload_json(&payload.to_string())
                .unwrap_err(),
            "invalid_import_request_payload:invalid_json"
        );
    }
}
