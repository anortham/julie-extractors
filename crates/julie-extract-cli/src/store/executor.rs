use std::path::PathBuf;
use std::sync::Arc;

use julie_extract_artifact::store::{
    CoordinatorExecutor, CoordinatorRequest, ExecutionContext, ExecutionQuantum, ManifestEntry,
    ManifestPublishDisposition, ManifestStore, RequestKind, StoreFileVersion, StoreLevel,
    StoreWriteRequest, StoreWriter,
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

static IMPORT_SPOOL_IO: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ImportRequestPayload {
    pub schema_version: u32,
    pub family_id: String,
    pub root: String,
    pub view_id: String,
    pub requested_level: RequestedLevel,
    pub files: Vec<PlannedImportFile>,
    pub controls: ImportScanControls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RequestedLevel {
    L1,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlannedImportFile {
    pub root_relative_path: String,
    pub content_hash: String,
    pub content_bytes: u64,
    #[serde(default)]
    pub projected_wal_bytes: u64,
}

impl PlannedImportFile {
    fn target(&self, root: &std::path::Path) -> FileTarget {
        FileTarget {
            absolute_path: root.join(&self.root_relative_path),
            root_relative_path: self.root_relative_path.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ImportScanControls {
    pub jobs: usize,
    #[serde(default)]
    pub store_db: String,
    pub spool_dir: Option<String>,
    pub progress_file: Option<String>,
    pub parent_pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FailureFact {
    path: String,
    version_id: Option<i64>,
    content_hash: String,
    indexed_at: String,
    error_json: String,
}

impl FailureFact {
    fn from_entry(entry: &ManifestEntry) -> Self {
        Self {
            path: entry.path.clone(),
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
                version_id,
                self.content_hash,
                self.indexed_at,
                "extract",
                self.error_json,
            ),
            None => ManifestEntry::failed(
                self.path,
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

pub(crate) struct StoreRequestExecutor;

impl StoreRequestExecutor {
    pub(crate) fn new() -> Self {
        Self
    }

    fn progress_for(
        transaction: &Transaction<'_>,
        request: &CoordinatorRequest,
        payload: &ImportRequestPayload,
        failed_files: usize,
    ) -> Result<Option<Arc<ScanProgress>>, String> {
        let Some(progress_file) = payload.controls.progress_file.as_deref() else {
            return Ok(None);
        };
        let progress = Arc::new(
            ScanProgress::create_for_artifact(
                std::path::Path::new(progress_file),
                std::path::Path::new(&payload.controls.store_db),
            )
            .map_err(|error| format!("{error:?}"))?,
        );
        progress.enter_phase("store_import");
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
                    version.version_id,
                    &file.content_hash,
                    indexed_at,
                ))
            })
            .collect()
    }

    fn result(
        payload: ImportRequestPayload,
        generation: u64,
        hash: String,
        full: bool,
        manifest_disposition: &str,
    ) -> ExecutionQuantum {
        ExecutionQuantum::Complete {
            event_kind: "store_import_completed".to_string(),
            result_json: serde_json::json!({
                "family_id": payload.family_id,
                "l1": true,
                "l2": full,
                "l3": full,
                "manifest_generation": generation,
                "manifest_hash": hash,
                "manifest_disposition": manifest_disposition,
                "root": payload.root,
                "view_id": payload.view_id,
            })
            .to_string(),
        }
    }
}

fn load_durable_request_state(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<
    (
        std::collections::BTreeMap<String, ManifestEntry>,
        &'static str,
    ),
    String,
> {
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
    let mut disposition = "not_published";
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
        disposition = match value
            .get("manifest_disposition")
            .and_then(|value| value.as_str())
        {
            Some("created") => "created",
            Some("reused") => "reused",
            _ => disposition,
        };
    }
    Ok((failures, disposition))
}

fn failure_facts(failures: &std::collections::BTreeMap<String, ManifestEntry>) -> Vec<FailureFact> {
    failures.values().map(FailureFact::from_entry).collect()
}

const DEFAULT_CHUNK_VERSIONS: usize = 100;
const WAL_BUDGET_BYTES: u64 = 128 * 1024 * 1024;

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

fn build_chunks(files: &[PlannedImportFile], level: StoreLevel) -> Vec<ImportChunk> {
    let configured = std::env::var("MILLER_STORE_CHUNK_VERSIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_CHUNK_VERSIONS);
    let version_limit = if configured == 0 { 1 } else { configured };
    chunk_ranges(
        &files
            .iter()
            .map(|file| {
                file.projected_wal_bytes
                    .max(estimate_projected_wal_bytes(file.content_bytes))
            })
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
        if request.kind != RequestKind::Import {
            return Err("unsupported_store_request_kind".to_string());
        }
        let payload: ImportRequestPayload =
            serde_json::from_str(&request.payload_json).map_err(|_| "invalid_import_request")?;
        if payload.schema_version != 1 {
            return Err("unsupported_import_request_schema".to_string());
        }
        if payload.controls.parent_pid.is_some_and(|pid| {
            matches!(
                crate::watchdog::process_status(pid),
                julie_extract_artifact::store::PidStatus::Dead
            )
        }) {
            return Err("parent_process_exited".to_string());
        }
        let root = PathBuf::from(&payload.root);
        let spool_dir = payload.controls.spool_dir.as_deref().map(PathBuf::from);
        let requested_full = payload.requested_level == RequestedLevel::Full;
        let l1_chunks = build_chunks(&payload.files, StoreLevel::L1);
        let l1_chunk_count = l1_chunks.len();
        let mut chunks = l1_chunks;
        if requested_full {
            chunks.extend(build_chunks(&payload.files, StoreLevel::L3));
        }
        let (mut failures, mut persisted_manifest_disposition) =
            load_durable_request_state(transaction, &request.request_id)?;
        let progress = Self::progress_for(transaction, request, &payload, failures.len())?;
        ManifestStore::ensure_view_in_transaction(transaction, &payload.view_id, &payload.root)
            .map_err(|error| error.to_string())?;
        let indexed_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| error.to_string())?;
        let chunk_index = usize::try_from(context.next_chunk_index)
            .map_err(|_| "chunk_index_out_of_range".to_string())?;
        if payload.files.is_empty() {
            let expected = transaction
                .query_row(
                    "SELECT current_generation FROM views WHERE view_id = ?1",
                    [&payload.view_id],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .map_err(|error| error.to_string())?
                .map(|generation| {
                    u64::try_from(generation).map_err(|_| "invalid_manifest_generation")
                })
                .transpose()?;
            let entries =
                Self::manifest_entries(transaction, &payload.files, &failures, &indexed_at)?;
            let published = ManifestStore::publish_in_transaction(
                transaction,
                &payload.view_id,
                expected,
                entries,
                &request.request_id,
            )
            .map_err(|error| error.to_string())?;
            return Ok(Self::result(
                payload,
                published.generation,
                published.manifest_hash,
                requested_full,
                manifest_disposition(published.disposition),
            ));
        }
        let chunk = chunks
            .get(chunk_index)
            .copied()
            .ok_or_else(|| "chunk_index_out_of_range".to_string())?;
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
            .map_err(|error| error.to_string())?;
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
        })?;
        for (discovered, extracted) in work.into_iter().zip(extracted) {
            let write_request = StoreWriteRequest::bulk(&request.request_id, &indexed_at);
            match chunk.level {
                StoreLevel::L1 => {
                    let version = match extracted {
                        Ok(version) => version,
                        Err(message) => {
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
                                    version_id,
                                    &discovered.content_hash,
                                    &indexed_at,
                                    "extract",
                                    error_json,
                                ),
                                None => ManifestEntry::failed(
                                    &discovered.root_relative_path,
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
                    .map_err(|error| error.to_string())?;
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
                    StoreWriter::write_level_in_transaction(
                        transaction,
                        &write_request,
                        None,
                        &full,
                        StoreLevel::L3,
                    )
                    .map_err(|error| error.to_string())?;
                }
            }
        }
        if chunk.level == StoreLevel::L1 && chunk_index + 1 == l1_chunk_count {
            let expected = transaction
                .query_row(
                    "SELECT current_generation FROM views WHERE view_id = ?1",
                    [&payload.view_id],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .map_err(|error| error.to_string())?
                .map(|generation| {
                    u64::try_from(generation).map_err(|_| "invalid_manifest_generation")
                })
                .transpose()?;
            let entries =
                Self::manifest_entries(transaction, &payload.files, &failures, &indexed_at)?;
            let published = ManifestStore::publish_in_transaction(
                transaction,
                &payload.view_id,
                expected,
                entries,
                &request.request_id,
            )
            .map_err(|error| error.to_string())?;
            persisted_manifest_disposition = manifest_disposition(published.disposition);
            wait_for_l1_test_hook()?;
            if !requested_full {
                if let Some(progress) = progress.as_deref() {
                    progress.enter_phase("complete");
                }
                return Ok(Self::result(
                    payload,
                    published.generation,
                    published.manifest_hash,
                    false,
                    persisted_manifest_disposition,
                ));
            }
            return Ok(ExecutionQuantum::Progress {
                event_kind: "store_import_l1_published".to_string(),
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
                event_kind: format!("store_import_l{}_chunk", chunk.level.as_i64()),
                payload_json: serde_json::json!({
                    "completed_files": chunk.end,
                    "failures": failure_facts(&failures),
                    "manifest_disposition": persisted_manifest_disposition,
                })
                .to_string(),
                level: Some(chunk.level),
            });
        }
        let generation = transaction
            .query_row(
                "SELECT current_generation FROM views WHERE view_id = ?1",
                [&payload.view_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| error.to_string())?;
        let hash = transaction
            .query_row(
                "SELECT manifest_hash FROM manifests WHERE view_id = ?1 AND generation = ?2",
                rusqlite::params![payload.view_id, generation],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| error.to_string())?;
        if let Some(progress) = progress.as_deref() {
            progress.enter_phase("complete");
        }
        Ok(Self::result(
            payload,
            u64::try_from(generation).map_err(|_| "invalid_manifest_generation")?,
            hash,
            true,
            persisted_manifest_disposition,
        ))
    }
}

fn manifest_disposition(disposition: ManifestPublishDisposition) -> &'static str {
    match disposition {
        ManifestPublishDisposition::Created => "created",
        ManifestPublishDisposition::Reused => "reused",
    }
}

#[cfg(debug_assertions)]
fn wait_for_l1_test_hook() -> Result<(), String> {
    let Ok(ready) = std::env::var("JULIE_EXTRACT_STORE_TEST_L1_READY_FILE") else {
        return Ok(());
    };
    let resume = std::env::var("JULIE_EXTRACT_STORE_TEST_L1_RESUME_FILE")
        .map_err(|_| "missing_l1_test_resume_file".to_string())?;
    std::fs::write(ready, b"ready").map_err(|error| error.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !std::path::Path::new(&resume).exists() {
        if std::time::Instant::now() >= deadline {
            return Err("l1_test_hook_timeout".to_string());
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    Ok(())
}

#[cfg(not(debug_assertions))]
fn wait_for_l1_test_hook() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{WAL_BUDGET_BYTES, chunk_ranges, estimate_projected_wal_bytes, map_with_jobs};

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
}
