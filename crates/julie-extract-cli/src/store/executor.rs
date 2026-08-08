use std::collections::BTreeMap;
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
use rusqlite::{OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::capability_snapshot::artifact_capability_snapshot;
use crate::extraction::{extract_artifact_file_from_snapshot_at, read_source_snapshot};
use crate::paths::FileTarget;
use crate::progress::{Counter, ScanProgress};
use crate::spool::create_scan_spool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ImportRequestPayload {
    pub family_id: String,
    pub root: String,
    pub view_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct DiscoveredImportFile {
    pub target: FileTarget,
    pub content_hash: String,
    pub content_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
struct ImportChunk {
    level: StoreLevel,
    start: usize,
    end: usize,
}

pub(crate) struct StoreRequestExecutor {
    root: PathBuf,
    files: Vec<DiscoveredImportFile>,
    spool_dir: Option<PathBuf>,
    requested_full: bool,
    progress: Option<Arc<ScanProgress>>,
    chunks: Vec<ImportChunk>,
    l1_chunk_count: usize,
    manifest_disposition: &'static str,
    watchdog: Option<crate::watchdog::ParentWatchdog>,
    full: BTreeMap<String, StoreFileVersion>,
    failures: BTreeMap<String, ManifestEntry>,
}

impl StoreRequestExecutor {
    pub(crate) fn new(
        root: PathBuf,
        files: Vec<DiscoveredImportFile>,
        spool_dir: Option<PathBuf>,
        requested_full: bool,
        progress: Option<Arc<ScanProgress>>,
        watchdog: Option<crate::watchdog::ParentWatchdog>,
    ) -> Self {
        let l1_chunks = build_chunks(&files, StoreLevel::L1);
        let l1_chunk_count = l1_chunks.len();
        let mut chunks = l1_chunks;
        if requested_full {
            chunks.extend(build_chunks(&files, StoreLevel::L2));
            chunks.extend(build_chunks(&files, StoreLevel::L3));
        }
        Self {
            root,
            files,
            spool_dir,
            requested_full,
            progress,
            chunks,
            l1_chunk_count,
            manifest_disposition: "not_published",
            watchdog,
            full: BTreeMap::new(),
            failures: BTreeMap::new(),
        }
    }

    pub(crate) fn progress(&self) -> Option<&ScanProgress> {
        self.progress.as_deref()
    }

    fn extract(
        &self,
        discovered: &DiscoveredImportFile,
        level: ExtractionLevel,
        indexed_at: &str,
    ) -> Result<StoreFileVersion, String> {
        let snapshot =
            read_source_snapshot(&discovered.target).map_err(|error| error.message.clone())?;
        if snapshot.content_hash != discovered.content_hash {
            return Err(if level == ExtractionLevel::Full {
                "changed_between_waves".to_string()
            } else {
                "changed_during_l1_wave".to_string()
            });
        }
        let extension = discovered
            .target
            .absolute_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let language = detect_language_from_extension(extension)
            .unwrap_or("unknown")
            .to_string();
        if let Some(progress) = self.progress.as_deref() {
            progress.advance(Counter::Extracted, 1);
        }
        let artifact = extract_artifact_file_from_snapshot_at(
            &self.root,
            &discovered.target,
            language,
            indexed_at.to_string(),
            snapshot,
            level,
        )
        .map_err(|error| error.message)?;
        let mut spool =
            create_scan_spool(self.spool_dir.as_deref()).map_err(|error| error.to_string())?;
        spool
            .file_spool_mut()
            .push(&artifact)
            .map_err(|error| error.to_string())?;
        if let Some(progress) = self.progress.as_deref() {
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

    fn validated_full(
        &mut self,
        transaction: &Transaction<'_>,
        discovered: &DiscoveredImportFile,
        indexed_at: &str,
    ) -> Result<StoreFileVersion, String> {
        let full = self.extract(discovered, ExtractionLevel::Full, indexed_at)?;
        let stored = StoreWriter::lookup_version_in_transaction(
            transaction,
            &discovered.target.root_relative_path,
            &discovered.content_hash,
            EXTRACTION_IDENTITY_EPOCH,
            StoreLevel::L1,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "l1_version_missing_before_deepening".to_string())?;
        if !StoreWriter::l1_projection_matches_in_transaction(transaction, &stored, &full)
            .map_err(|error| error.to_string())?
        {
            return Err("l1_projection_mismatch".to_string());
        }
        Ok(full)
    }

    fn manifest_entries(
        &self,
        transaction: &Transaction<'_>,
        indexed_at: &str,
    ) -> Result<Vec<ManifestEntry>, String> {
        self.files
            .iter()
            .map(|file| {
                if let Some(failure) = self.failures.get(&file.target.root_relative_path) {
                    return Ok(failure.clone());
                }
                let version = StoreWriter::lookup_version_in_transaction(
                    transaction,
                    &file.target.root_relative_path,
                    &file.content_hash,
                    EXTRACTION_IDENTITY_EPOCH,
                    StoreLevel::L1,
                )
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "l1_version_missing_at_publish".to_string())?;
                Ok(ManifestEntry::indexed(
                    &file.target.root_relative_path,
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

const DEFAULT_CHUNK_VERSIONS: usize = 100;
const WAL_BUDGET_BYTES: u64 = 128 * 1024 * 1024;

fn build_chunks(files: &[DiscoveredImportFile], level: StoreLevel) -> Vec<ImportChunk> {
    let configured = std::env::var("MILLER_STORE_CHUNK_VERSIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_CHUNK_VERSIONS);
    let version_limit = if configured == 0 { 1 } else { configured };
    chunk_ranges(
        &files
            .iter()
            .map(|file| file.content_bytes)
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
        if self
            .watchdog
            .as_ref()
            .is_some_and(crate::watchdog::ParentWatchdog::parent_exited)
        {
            return Err("parent_process_exited".to_string());
        }
        let payload: ImportRequestPayload =
            serde_json::from_str(&request.payload_json).map_err(|_| "invalid_import_request")?;
        ManifestStore::ensure_view_in_transaction(transaction, &payload.view_id, &payload.root)
            .map_err(|error| error.to_string())?;
        let indexed_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| error.to_string())?;
        let chunk_index = usize::try_from(context.next_chunk_index)
            .map_err(|_| "chunk_index_out_of_range".to_string())?;
        if self.files.is_empty() {
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
            let entries = self.manifest_entries(transaction, &indexed_at)?;
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
                false,
                manifest_disposition(published.disposition),
            ));
        }
        let chunk = self
            .chunks
            .get(chunk_index)
            .copied()
            .ok_or_else(|| "chunk_index_out_of_range".to_string())?;
        for index in chunk.start..chunk.end {
            let discovered = self.files[index].clone();
            if chunk.level != StoreLevel::L1
                && self
                    .failures
                    .contains_key(&discovered.target.root_relative_path)
            {
                continue;
            }
            let complete = StoreWriter::lookup_version_in_transaction(
                transaction,
                &discovered.target.root_relative_path,
                &discovered.content_hash,
                EXTRACTION_IDENTITY_EPOCH,
                chunk.level,
            )
            .map_err(|error| error.to_string())?;
            if complete.is_some() {
                continue;
            }
            let write_request = StoreWriteRequest::bulk(&request.request_id, &indexed_at);
            match chunk.level {
                StoreLevel::L1 => {
                    let version =
                        match self.extract(&discovered, ExtractionLevel::Symbols, &indexed_at) {
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
                                            discovered.target.root_relative_path
                                        ],
                                        |row| row.get::<_, Option<i64>>(0),
                                    )
                                    .optional()
                                    .map_err(|error| error.to_string())?
                                    .flatten();
                                let error_json =
                                    serde_json::json!({ "message": message }).to_string();
                                let failure = match prior_version {
                                    Some(version_id) => ManifestEntry::failed_preserved(
                                        &discovered.target.root_relative_path,
                                        version_id,
                                        &discovered.content_hash,
                                        &indexed_at,
                                        "extract",
                                        error_json,
                                    ),
                                    None => ManifestEntry::failed(
                                        &discovered.target.root_relative_path,
                                        &discovered.content_hash,
                                        &indexed_at,
                                        "extract",
                                        error_json,
                                    ),
                                };
                                self.failures
                                    .insert(discovered.target.root_relative_path.clone(), failure);
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
                StoreLevel::L2 => {
                    let full = self.validated_full(transaction, &discovered, &indexed_at)?;
                    StoreWriter::write_level_in_transaction(
                        transaction,
                        &write_request,
                        None,
                        &full,
                        StoreLevel::L2,
                    )
                    .map_err(|error| error.to_string())?;
                    self.full
                        .insert(discovered.target.root_relative_path.clone(), full);
                }
                StoreLevel::L3 => {
                    let full = match self.full.remove(&discovered.target.root_relative_path) {
                        Some(full) => full,
                        None => self.validated_full(transaction, &discovered, &indexed_at)?,
                    };
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
        if chunk.level == StoreLevel::L1 && chunk_index + 1 == self.l1_chunk_count {
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
            let entries = self.manifest_entries(transaction, &indexed_at)?;
            let published = ManifestStore::publish_in_transaction(
                transaction,
                &payload.view_id,
                expected,
                entries,
                &request.request_id,
            )
            .map_err(|error| error.to_string())?;
            self.manifest_disposition = manifest_disposition(published.disposition);
            wait_for_l1_test_hook()?;
            if !self.requested_full {
                return Ok(Self::result(
                    payload,
                    published.generation,
                    published.manifest_hash,
                    false,
                    self.manifest_disposition,
                ));
            }
            return Ok(ExecutionQuantum::Progress {
                event_kind: "store_import_l1_published".to_string(),
                payload_json: serde_json::json!({
                    "completed_files": chunk.end,
                    "generation": published.generation,
                    "manifest_hash": published.manifest_hash,
                })
                .to_string(),
                level: Some(StoreLevel::L1),
            });
        }
        if chunk_index + 1 < self.chunks.len() {
            return Ok(ExecutionQuantum::Progress {
                event_kind: format!("store_import_l{}_chunk", chunk.level.as_i64()),
                payload_json: serde_json::json!({ "completed_files": chunk.end }).to_string(),
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
        Ok(Self::result(
            payload,
            u64::try_from(generation).map_err(|_| "invalid_manifest_generation")?,
            hash,
            true,
            self.manifest_disposition,
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
    use super::{WAL_BUDGET_BYTES, chunk_ranges};

    #[test]
    fn wal_budget_splits_before_the_next_version_would_exceed_128_mib() {
        let sizes = [70 * 1024 * 1024, 70 * 1024 * 1024, 1];
        assert_eq!(chunk_ranges(&sizes, 100), [(0, 1), (1, 3)]);
        for (start, end) in chunk_ranges(&sizes, 100) {
            assert!(sizes[start..end].iter().sum::<u64>() <= WAL_BUDGET_BYTES);
        }
    }
}
