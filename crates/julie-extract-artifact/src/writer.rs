use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::metadata::{ArtifactMetadata, initialize_metadata};
use crate::model::{
    ArtifactCapabilitySnapshot, ArtifactFile, FileStatus, RevisionChangeKind, RevisionInput,
    RowCounts, WriteMode, WriteOperation, WriteResult,
};
use crate::reports::RowDomainCounts;
use crate::schema::{EXTRACT_CONTRACT_VERSION, SQLITE_SCHEMA_VERSION, create_schema};

mod capabilities;
mod rows;

use rows::{
    ChildRowInserters, FileRowInserters, collect_file_symbol_ids, collect_requested_symbol_ids,
    is_preserved_failure, is_preserved_failure_update, load_symbol_lookup,
    replace_parse_diagnostics, update_failed_preserved_file,
};

pub type ArtifactWriteResult<T> = Result<T, ArtifactWriteError>;

const SQLITE_BULK_CACHE_SIZE_KIB: i64 = -131_072;
const SQLITE_STATEMENT_CACHE_CAPACITY: usize = 64;
#[derive(Debug)]
pub enum ArtifactWriteError {
    Sqlite(rusqlite::Error),
    Spool(ArtifactSpoolError),
    DataLossGuard {
        path: String,
        existing_symbols: i64,
        reason: String,
    },
    SnapshotMissingSpooledPath {
        path: String,
    },
}

impl std::fmt::Display for ArtifactWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactWriteError::Sqlite(error) => write!(f, "{error}"),
            ArtifactWriteError::Spool(error) => write!(f, "{error}"),
            ArtifactWriteError::DataLossGuard {
                path,
                existing_symbols,
                reason,
            } => write!(
                f,
                "refusing to replace {path}: {reason}; existing symbol rows: {existing_symbols}"
            ),
            ArtifactWriteError::SnapshotMissingSpooledPath { path } => write!(
                f,
                "spooled scan file {path} was not present in the current snapshot path set"
            ),
        }
    }
}

impl std::error::Error for ArtifactWriteError {}

impl From<rusqlite::Error> for ArtifactWriteError {
    fn from(value: rusqlite::Error) -> Self {
        ArtifactWriteError::Sqlite(value)
    }
}

impl From<ArtifactSpoolError> for ArtifactWriteError {
    fn from(value: ArtifactSpoolError) -> Self {
        ArtifactWriteError::Spool(value)
    }
}

pub type ArtifactSpoolResult<T> = Result<T, ArtifactSpoolError>;

#[derive(Debug)]
pub enum ArtifactSpoolError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Json {
        path: PathBuf,
        line: Option<usize>,
        source: serde_json::Error,
    },
    Unfinished {
        path: PathBuf,
    },
}

impl std::fmt::Display for ArtifactSpoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactSpoolError::Io { path, source } => {
                write!(
                    f,
                    "artifact file spool I/O failed at {}: {source}",
                    path.display()
                )
            }
            ArtifactSpoolError::Json {
                path,
                line: Some(line),
                source,
            } => write!(
                f,
                "artifact file spool JSON decode failed at {}:{line}: {source}",
                path.display()
            ),
            ArtifactSpoolError::Json {
                path,
                line: None,
                source,
            } => write!(
                f,
                "artifact file spool JSON encode failed at {}: {source}",
                path.display()
            ),
            ArtifactSpoolError::Unfinished { path } => write!(
                f,
                "artifact file spool must be finished before reading: {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ArtifactSpoolError {}

pub struct ArtifactFileSpool {
    path: PathBuf,
    writer: Option<BufWriter<File>>,
    len: usize,
}

impl ArtifactFileSpool {
    pub fn create(path: impl AsRef<Path>) -> ArtifactSpoolResult<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::create(&path).map_err(|source| ArtifactSpoolError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(Self {
            path,
            writer: Some(BufWriter::new(file)),
            len: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn push(&mut self, file: &ArtifactFile) -> ArtifactSpoolResult<()> {
        let Some(writer) = self.writer.as_mut() else {
            return Err(ArtifactSpoolError::Unfinished {
                path: self.path.clone(),
            });
        };
        serde_json::to_writer(&mut *writer, file).map_err(|source| ArtifactSpoolError::Json {
            path: self.path.clone(),
            line: None,
            source,
        })?;
        writer
            .write_all(b"\n")
            .map_err(|source| ArtifactSpoolError::Io {
                path: self.path.clone(),
                source,
            })?;
        self.len += 1;
        Ok(())
    }

    pub fn finish(&mut self) -> ArtifactSpoolResult<()> {
        let Some(mut writer) = self.writer.take() else {
            return Ok(());
        };
        writer.flush().map_err(|source| ArtifactSpoolError::Io {
            path: self.path.clone(),
            source,
        })
    }

    pub fn iter(&self) -> ArtifactSpoolResult<ArtifactFileSpoolIter> {
        if self.writer.is_some() {
            return Err(ArtifactSpoolError::Unfinished {
                path: self.path.clone(),
            });
        }
        let file = File::open(&self.path).map_err(|source| ArtifactSpoolError::Io {
            path: self.path.clone(),
            source,
        })?;
        Ok(ArtifactFileSpoolIter {
            path: self.path.clone(),
            lines: BufReader::new(file).lines(),
            line_number: 0,
        })
    }
}

pub struct ArtifactFileSpoolIter {
    path: PathBuf,
    lines: io::Lines<BufReader<File>>,
    line_number: usize,
}

impl Iterator for ArtifactFileSpoolIter {
    type Item = ArtifactSpoolResult<ArtifactFile>;

    fn next(&mut self) -> Option<Self::Item> {
        let line = match self.lines.next()? {
            Ok(line) => line,
            Err(source) => {
                return Some(Err(ArtifactSpoolError::Io {
                    path: self.path.clone(),
                    source,
                }));
            }
        };
        self.line_number += 1;
        Some(
            serde_json::from_str(&line).map_err(|source| ArtifactSpoolError::Json {
                path: self.path.clone(),
                line: Some(self.line_number),
                source,
            }),
        )
    }
}

pub struct ArtifactWriter {
    connection: Connection,
    metadata: ArtifactMetadata,
    staged_capability_snapshot: Option<ArtifactCapabilitySnapshot>,
    last_capability_rows_written: RowDomainCounts,
}

impl ArtifactWriter {
    pub fn open_in_memory(metadata: ArtifactMetadata) -> rusqlite::Result<Self> {
        let connection = Connection::open_in_memory()?;
        connection.set_prepared_statement_cache_capacity(SQLITE_STATEMENT_CACHE_CAPACITY);
        create_schema(&connection)?;
        initialize_metadata(&connection, &metadata)?;
        Ok(Self {
            connection,
            metadata,
            staged_capability_snapshot: None,
            last_capability_rows_written: RowDomainCounts::default(),
        })
    }

    pub fn open_path(path: impl AsRef<Path>, metadata: ArtifactMetadata) -> rusqlite::Result<Self> {
        let path = path.as_ref();
        let existed = path.exists();
        let connection = Connection::open(path)?;
        connection.set_prepared_statement_cache_capacity(SQLITE_STATEMENT_CACHE_CAPACITY);
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "temp_store", "MEMORY")?;
        connection.pragma_update(None, "cache_size", SQLITE_BULK_CACHE_SIZE_KIB)?;
        create_schema(&connection)?;
        if !existed || metadata_row_count(&connection)? == 0 {
            initialize_metadata(&connection, &metadata)?;
        }
        Ok(Self {
            connection,
            metadata,
            staged_capability_snapshot: None,
            last_capability_rows_written: RowDomainCounts::default(),
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn into_connection(self) -> Connection {
        self.connection
    }

    pub fn stage_capability_snapshot(&mut self, snapshot: ArtifactCapabilitySnapshot) {
        self.staged_capability_snapshot = Some(snapshot);
        self.last_capability_rows_written = RowDomainCounts::default();
    }

    pub fn last_capability_rows_written(&self) -> RowDomainCounts {
        self.last_capability_rows_written.clone()
    }

    pub fn sync_capability_snapshot(
        &mut self,
        snapshot: &ArtifactCapabilitySnapshot,
    ) -> ArtifactWriteResult<RowDomainCounts> {
        let tx = self.connection.transaction()?;
        let counts = capabilities::sync_capability_snapshot_in_tx(&tx, snapshot)?;
        tx.commit()?;
        self.last_capability_rows_written = counts.clone();
        Ok(counts)
    }

    fn staged_capability_snapshot(&mut self) -> Option<ArtifactCapabilitySnapshot> {
        self.last_capability_rows_written = RowDomainCounts::default();
        self.staged_capability_snapshot.take()
    }

    pub fn write_scan(
        &mut self,
        revision: RevisionInput,
        files: &[ArtifactFile],
    ) -> ArtifactWriteResult<WriteResult> {
        debug_assert_eq!(revision.operation, WriteOperation::Scan);
        self.write_scan_snapshot(revision, files)
    }

    pub fn write_scan_spooled(
        &mut self,
        revision: RevisionInput,
        snapshot_paths: &[String],
        spool: &mut ArtifactFileSpool,
    ) -> ArtifactWriteResult<WriteResult> {
        debug_assert_eq!(revision.operation, WriteOperation::Scan);
        spool.finish()?;
        self.write_scan_spooled_snapshot(revision, snapshot_paths, &[], spool)
    }

    pub fn write_scan_spooled_preserving_missing_paths(
        &mut self,
        revision: RevisionInput,
        snapshot_paths: &[String],
        preserved_missing_paths: &[String],
        spool: &mut ArtifactFileSpool,
    ) -> ArtifactWriteResult<WriteResult> {
        debug_assert_eq!(revision.operation, WriteOperation::Scan);
        spool.finish()?;
        self.write_scan_spooled_snapshot(revision, snapshot_paths, preserved_missing_paths, spool)
    }

    pub fn write_update(
        &mut self,
        revision: RevisionInput,
        file: &ArtifactFile,
    ) -> ArtifactWriteResult<WriteResult> {
        debug_assert_eq!(revision.operation, WriteOperation::Update);
        self.write_files(revision, std::slice::from_ref(file))
    }

    pub fn delete_file(
        &mut self,
        revision: RevisionInput,
        path: &str,
    ) -> ArtifactWriteResult<WriteResult> {
        debug_assert_eq!(revision.operation, WriteOperation::Delete);
        self.remove_file_rows(revision, path, RevisionChangeKind::Deleted)
    }

    pub fn remove_unsupported_file(
        &mut self,
        revision: RevisionInput,
        path: &str,
    ) -> ArtifactWriteResult<WriteResult> {
        debug_assert_eq!(revision.operation, WriteOperation::Update);
        self.remove_file_rows(revision, path, RevisionChangeKind::Unsupported)
    }

    fn remove_file_rows(
        &mut self,
        revision: RevisionInput,
        path: &str,
        change_kind: RevisionChangeKind,
    ) -> ArtifactWriteResult<WriteResult> {
        let capability_snapshot = self.staged_capability_snapshot();
        let tx = self.connection.transaction()?;
        let existing = load_existing_file(&tx, path)?;
        let Some(existing) = existing else {
            tx.commit()?;
            self.last_capability_rows_written = RowDomainCounts::default();
            return Ok(WriteResult {
                transactions_committed: 1,
                ..WriteResult::default()
            });
        };
        let capability_rows_written = capabilities::sync_optional_capability_snapshot_in_tx(
            &tx,
            capability_snapshot.as_ref(),
        )?;

        let parent_revision_id = current_revision_id(&tx)?;
        let revision_id = insert_revision(&tx, parent_revision_id, &revision)?;
        write_metadata(&tx, &self.metadata)?;
        delete_file_rows(&tx, &existing.file_id, path)?;

        let row_counts = RowCounts {
            revision_file_changes: insert_revision_file_change(
                &tx,
                revision_id,
                &existing.file_id,
                path,
                change_kind,
            )?,
            ..RowCounts::default()
        };
        let revision_counts =
            revision_counts_with_capabilities(&row_counts, &capability_rows_written);
        update_revision_counts(&tx, revision_id, &revision_counts)?;
        tx.commit()?;
        self.last_capability_rows_written = capability_rows_written;

        Ok(WriteResult {
            revision_id: Some(revision_id),
            rows_written: row_counts,
            files_changed: 1,
            files_deleted: 1,
            files_skipped: 0,
            transactions_committed: 1,
        })
    }

    fn write_files(
        &mut self,
        revision: RevisionInput,
        files: &[ArtifactFile],
    ) -> ArtifactWriteResult<WriteResult> {
        let capability_snapshot = self.staged_capability_snapshot();
        let tx = self.connection.transaction()?;
        let capability_rows_written = capabilities::sync_optional_capability_snapshot_in_tx(
            &tx,
            capability_snapshot.as_ref(),
        )?;
        let mut planned = Vec::new();
        let mut files_skipped = 0;
        let skip_unchanged_content = revision.mode != Some(WriteMode::Force);

        for file in files {
            let existing = load_existing_file(&tx, &file.path)?;
            if skip_unchanged_content
                && existing
                    .as_ref()
                    .is_some_and(|row| row.content_hash == file.content_hash)
            {
                files_skipped += 1;
                continue;
            }

            ensure_data_loss_guard(&tx, file)?;
            let change_kind = match file.status {
                FileStatus::Unsupported => RevisionChangeKind::Unsupported,
                FileStatus::Indexed | FileStatus::FailedPreserved => {
                    if existing.is_some() {
                        RevisionChangeKind::Updated
                    } else {
                        RevisionChangeKind::Inserted
                    }
                }
            };
            planned.push((file, existing, change_kind));
        }

        if planned.is_empty() && !capability_rows_written.has_rows() {
            tx.commit()?;
            self.last_capability_rows_written = capability_rows_written;
            return Ok(WriteResult {
                files_skipped,
                transactions_committed: 1,
                ..WriteResult::default()
            });
        }

        let parent_revision_id = current_revision_id(&tx)?;
        let revision_id = insert_revision(&tx, parent_revision_id, &revision)?;
        write_metadata(&tx, &self.metadata)?;
        let mut row_counts = RowCounts::default();

        for (file, existing, _) in &planned {
            if let Some(existing) = existing {
                delete_file_rows(&tx, &existing.file_id, &file.path)?;
            }
        }

        let symbol_lookup = {
            let mut file_row_inserters = FileRowInserters::prepare(&tx)?;
            for (file, _, change_kind) in &planned {
                file_row_inserters.insert_file(revision_id, file)?;
                row_counts.files += 1;
                row_counts.revision_file_changes += file_row_inserters
                    .insert_revision_file_change(
                        revision_id,
                        &file.file_id,
                        &file.path,
                        *change_kind,
                    )?;
            }

            for (file, _, _) in &planned {
                row_counts.symbols += file_row_inserters.insert_symbols(file, None)?;
            }
            let symbol_lookup = load_symbol_lookup(&tx, planned.iter().map(|(file, _, _)| *file))?;
            file_row_inserters
                .update_symbol_parents(planned.iter().map(|(file, _, _)| *file), &symbol_lookup)?;
            symbol_lookup
        };

        {
            let mut child_row_inserters = ChildRowInserters::prepare(&tx)?;
            for (file, _, _) in &planned {
                child_row_inserters.insert_child_rows(file, &symbol_lookup, &mut row_counts)?;
            }
        }

        let revision_counts =
            revision_counts_with_capabilities(&row_counts, &capability_rows_written);
        update_revision_counts(&tx, revision_id, &revision_counts)?;
        tx.commit()?;
        self.last_capability_rows_written = capability_rows_written;

        Ok(WriteResult {
            revision_id: Some(revision_id),
            files_changed: files.len() - files_skipped,
            files_deleted: 0,
            files_skipped,
            rows_written: row_counts,
            transactions_committed: 1,
        })
    }

    fn write_scan_snapshot(
        &mut self,
        revision: RevisionInput,
        files: &[ArtifactFile],
    ) -> ArtifactWriteResult<WriteResult> {
        let capability_snapshot = self.staged_capability_snapshot();
        let tx = self.connection.transaction()?;
        let capability_rows_written = capabilities::sync_optional_capability_snapshot_in_tx(
            &tx,
            capability_snapshot.as_ref(),
        )?;
        let mut planned = Vec::new();
        let mut files_skipped = 0;
        let skip_unchanged_content = revision.mode != Some(WriteMode::Force);
        let snapshot_paths = files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<HashSet<_>>();
        let deleted = load_existing_files(&tx)?
            .into_iter()
            .filter(|existing| !snapshot_paths.contains(existing.path.as_str()))
            .collect::<Vec<_>>();

        for file in files {
            let existing = load_existing_file(&tx, &file.path)?;
            if skip_unchanged_content
                && existing
                    .as_ref()
                    .is_some_and(|row| row.content_hash == file.content_hash)
            {
                files_skipped += 1;
                continue;
            }

            if file.status != FileStatus::FailedPreserved {
                ensure_data_loss_guard(&tx, file)?;
            }
            let change_kind = match file.status {
                FileStatus::Unsupported => RevisionChangeKind::Unsupported,
                FileStatus::Indexed | FileStatus::FailedPreserved => {
                    if existing.is_some() {
                        RevisionChangeKind::Updated
                    } else {
                        RevisionChangeKind::Inserted
                    }
                }
            };
            planned.push((file, existing, change_kind));
        }

        if planned.is_empty() && deleted.is_empty() && !capability_rows_written.has_rows() {
            tx.commit()?;
            self.last_capability_rows_written = capability_rows_written;
            return Ok(WriteResult {
                files_skipped,
                transactions_committed: 1,
                ..WriteResult::default()
            });
        }

        let parent_revision_id = current_revision_id(&tx)?;
        let revision_id = insert_revision(&tx, parent_revision_id, &revision)?;
        write_metadata(&tx, &self.metadata)?;
        let mut row_counts = RowCounts::default();

        for (file, existing, _) in &planned {
            if is_preserved_failure_update(file, existing.as_ref()) {
                continue;
            }
            if let Some(existing) = existing {
                delete_file_rows(&tx, &existing.file_id, &file.path)?;
            }
        }

        let rewritten_files = planned
            .iter()
            .filter(|(file, existing, _)| !is_preserved_failure_update(file, existing.as_ref()))
            .map(|(file, _, _)| *file)
            .collect::<Vec<_>>();

        let symbol_lookup = {
            let mut file_row_inserters = FileRowInserters::prepare(&tx)?;
            for existing in &deleted {
                delete_file_rows(&tx, &existing.file_id, &existing.path)?;
                row_counts.revision_file_changes += file_row_inserters
                    .insert_revision_file_change(
                        revision_id,
                        &existing.file_id,
                        &existing.path,
                        RevisionChangeKind::Deleted,
                    )?;
            }

            for (file, existing, change_kind) in &planned {
                if let Some(existing) = existing.as_ref().filter(|_| is_preserved_failure(file)) {
                    update_failed_preserved_file(&tx, revision_id, file, &existing.file_id)?;
                    row_counts.files += 1;
                    row_counts.parse_diagnostics += replace_parse_diagnostics(&tx, file)?;
                } else {
                    file_row_inserters.insert_file(revision_id, file)?;
                    row_counts.files += 1;
                }
                row_counts.revision_file_changes += file_row_inserters
                    .insert_revision_file_change(
                        revision_id,
                        &file.file_id,
                        &file.path,
                        *change_kind,
                    )?;
            }

            for file in &rewritten_files {
                row_counts.symbols += file_row_inserters.insert_symbols(file, None)?;
            }
            let symbol_lookup = load_symbol_lookup(&tx, rewritten_files.iter().copied())?;
            file_row_inserters
                .update_symbol_parents(rewritten_files.iter().copied(), &symbol_lookup)?;
            symbol_lookup
        };

        {
            let mut child_row_inserters = ChildRowInserters::prepare(&tx)?;
            for file in &rewritten_files {
                child_row_inserters.insert_child_rows(file, &symbol_lookup, &mut row_counts)?;
            }
        }

        let revision_counts =
            revision_counts_with_capabilities(&row_counts, &capability_rows_written);
        update_revision_counts(&tx, revision_id, &revision_counts)?;
        tx.commit()?;
        self.last_capability_rows_written = capability_rows_written;

        Ok(WriteResult {
            revision_id: Some(revision_id),
            files_changed: planned.len() + deleted.len(),
            files_deleted: deleted.len(),
            files_skipped,
            rows_written: row_counts,
            transactions_committed: 1,
        })
    }

    fn write_scan_spooled_snapshot(
        &mut self,
        revision: RevisionInput,
        snapshot_paths: &[String],
        preserved_missing_paths: &[String],
        spool: &ArtifactFileSpool,
    ) -> ArtifactWriteResult<WriteResult> {
        let capability_snapshot = self.staged_capability_snapshot();
        let tx = self.connection.unchecked_transaction()?;
        // Symbol parent FKs can point to symbols inserted later in the same spooled transaction.
        // Defer validation until commit while keeping connection-level foreign_keys ON so
        // ON DELETE CASCADE/SET NULL actions still run during rewrites and snapshot deletes.
        tx.pragma_update(None, "defer_foreign_keys", "ON")?;
        let capability_rows_written = capabilities::sync_optional_capability_snapshot_in_tx(
            &tx,
            capability_snapshot.as_ref(),
        )?;
        let snapshot_paths = snapshot_paths
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let skip_unchanged_content = revision.mode != Some(WriteMode::Force);
        let mut planned_files: HashMap<String, Option<ExistingFile>> = HashMap::new();
        let mut files_skipped = 0;
        let mut rewritten_file_ids = HashSet::new();
        // Symbol-id sets for the cross-file symbol_lookup, accumulated in this planning pass (which
        // already deserializes every file) so the lookup can be built before symbols are inserted —
        // letting the insert resolve parent_symbol_id inline instead of in a second UPDATE pass.
        let mut requested_symbol_ids = HashSet::new();
        let mut local_symbol_ids = HashSet::new();

        for file in spool.iter()? {
            let file = file?;
            if !snapshot_paths.contains(file.path.as_str()) {
                return Err(ArtifactWriteError::SnapshotMissingSpooledPath {
                    path: file.path.clone(),
                });
            }
            let existing = load_existing_file(&tx, &file.path)?;
            if skip_unchanged_content
                && existing
                    .as_ref()
                    .is_some_and(|row| row.content_hash == file.content_hash)
            {
                files_skipped += 1;
                continue;
            }

            if file.status != FileStatus::FailedPreserved {
                ensure_data_loss_guard(&tx, &file)?;
            }
            // Files that will be rewritten (everything except preserved-failure updates) contribute
            // their symbols to the lookup. This mirrors pass B's else-branch selection exactly.
            if !is_preserved_failure_update(&file, existing.as_ref()) {
                rewritten_file_ids.insert(file.file_id.clone());
                collect_requested_symbol_ids(&file, &mut requested_symbol_ids);
                collect_file_symbol_ids(&file, &mut local_symbol_ids);
            }
            // Carry the existing-file lookup forward; pass B reuses it instead of re-SELECTing.
            // Nothing mutates a planned path's `files` row between here and its insert in pass B,
            // so the value stays valid (per-file deletes happen immediately before each re-insert).
            planned_files.insert(file.path, existing);
        }

        let deleted = load_existing_files(&tx)?
            .into_iter()
            .filter(|existing| {
                !snapshot_paths.contains(existing.path.as_str())
                    && !path_is_preserved_missing(&existing.path, preserved_missing_paths)
            })
            .collect::<Vec<_>>();

        if planned_files.is_empty() && deleted.is_empty() && !capability_rows_written.has_rows() {
            tx.commit()?;
            self.last_capability_rows_written = capability_rows_written;
            return Ok(WriteResult {
                files_skipped,
                transactions_committed: 1,
                ..WriteResult::default()
            });
        }

        let parent_revision_id = current_revision_id(&tx)?;
        let revision_id = insert_revision(&tx, parent_revision_id, &revision)?;
        write_metadata(&tx, &self.metadata)?;
        let mut row_counts = RowCounts::default();
        // Built before the insert pass (the ids were gathered during planning) so symbols can be
        // written with their parent resolved in one statement.
        let symbol_lookup = rows::load_symbol_lookup_for_requested_ids(
            &tx,
            &requested_symbol_ids,
            &local_symbol_ids,
        )?;

        {
            let mut file_row_inserters = FileRowInserters::prepare(&tx)?;
            for file in spool.iter()? {
                let file = file?;
                let Some(existing) = planned_files.get(file.path.as_str()) else {
                    continue;
                };
                let existing = existing.as_ref();

                let change_kind = file_change_kind(&file, existing);
                if is_preserved_failure_update(&file, existing) {
                    if let Some(existing) = existing {
                        update_failed_preserved_file(&tx, revision_id, &file, &existing.file_id)?;
                        row_counts.files += 1;
                        row_counts.parse_diagnostics += replace_parse_diagnostics(&tx, &file)?;
                    }
                } else {
                    if let Some(existing) = existing {
                        delete_file_rows(&tx, &existing.file_id, &file.path)?;
                    }
                    file_row_inserters.insert_file(revision_id, &file)?;
                    row_counts.files += 1;
                    row_counts.symbols +=
                        file_row_inserters.insert_symbols(&file, Some(&symbol_lookup))?;
                }
                row_counts.revision_file_changes += file_row_inserters
                    .insert_revision_file_change(
                        revision_id,
                        &file.file_id,
                        &file.path,
                        change_kind,
                    )?;
            }

            for existing in &deleted {
                delete_file_rows(&tx, &existing.file_id, &existing.path)?;
                row_counts.revision_file_changes += file_row_inserters
                    .insert_revision_file_change(
                        revision_id,
                        &existing.file_id,
                        &existing.path,
                        RevisionChangeKind::Deleted,
                    )?;
            }
        }

        {
            let mut child_row_inserters = ChildRowInserters::prepare(&tx)?;
            for file in spool.iter()? {
                let file = file?;
                if !rewritten_file_ids.contains(&file.file_id) {
                    continue;
                }
                child_row_inserters.insert_child_rows(&file, &symbol_lookup, &mut row_counts)?;
            }
        }

        let revision_counts =
            revision_counts_with_capabilities(&row_counts, &capability_rows_written);
        update_revision_counts(&tx, revision_id, &revision_counts)?;
        tx.commit()?;
        self.last_capability_rows_written = capability_rows_written;

        Ok(WriteResult {
            revision_id: Some(revision_id),
            files_changed: planned_files.len() + deleted.len(),
            files_deleted: deleted.len(),
            files_skipped,
            rows_written: row_counts,
            transactions_committed: 1,
        })
    }
}

struct ExistingFile {
    file_id: String,
    path: String,
    content_hash: String,
}

fn metadata_row_count(connection: &Connection) -> rusqlite::Result<i64> {
    connection.query_row("SELECT COUNT(*) FROM artifact_metadata", [], |row| {
        row.get(0)
    })
}

fn write_metadata(tx: &Transaction<'_>, metadata: &ArtifactMetadata) -> rusqlite::Result<()> {
    let mut statement = tx.prepare(
        "INSERT INTO artifact_metadata (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )?;

    for (key, value) in metadata.rows() {
        statement.execute(params![key, value])?;
    }

    Ok(())
}

fn load_existing_file(tx: &Transaction<'_>, path: &str) -> rusqlite::Result<Option<ExistingFile>> {
    let mut stmt = tx.prepare_cached("SELECT file_id, content_hash FROM files WHERE path = ?1")?;
    stmt.query_row([path], |row| {
        Ok(ExistingFile {
            file_id: row.get(0)?,
            path: path.to_string(),
            content_hash: row.get(1)?,
        })
    })
    .optional()
}

fn load_existing_files(tx: &Transaction<'_>) -> rusqlite::Result<Vec<ExistingFile>> {
    let mut statement = tx.prepare_cached("SELECT file_id, path, content_hash FROM files")?;
    statement
        .query_map([], |row| {
            Ok(ExistingFile {
                file_id: row.get(0)?,
                path: row.get(1)?,
                content_hash: row.get(2)?,
            })
        })?
        .collect()
}

fn path_is_preserved_missing(path: &str, preserved_missing_paths: &[String]) -> bool {
    preserved_missing_paths.iter().any(|prefix| {
        prefix == "."
            || prefix.is_empty()
            || path == prefix
            || path
                .strip_prefix(prefix)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

fn current_revision_id(tx: &Transaction<'_>) -> rusqlite::Result<Option<i64>> {
    tx.query_row(
        "SELECT MAX(revision_id) FROM extraction_revisions",
        [],
        |row| row.get(0),
    )
}

fn insert_revision(
    tx: &Transaction<'_>,
    parent_revision_id: Option<i64>,
    revision: &RevisionInput,
) -> rusqlite::Result<i64> {
    tx.execute(
        "INSERT INTO extraction_revisions
         (parent_revision_id, operation, mode, started_at, completed_at, binary_version,
          extract_contract_version, sqlite_schema_version, input_root, counts_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '{}')",
        params![
            parent_revision_id,
            revision.operation.as_str(),
            revision.mode.map(|mode| mode.as_str()),
            revision.started_at,
            revision.completed_at,
            revision.binary_version,
            EXTRACT_CONTRACT_VERSION,
            SQLITE_SCHEMA_VERSION,
            revision.input_root,
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

fn update_revision_counts(
    tx: &Transaction<'_>,
    revision_id: i64,
    row_counts: &RowDomainCounts,
) -> rusqlite::Result<()> {
    let counts_json = serde_json::to_string(row_counts)
        .expect("RowDomainCounts serialization should be infallible");
    tx.execute(
        "UPDATE extraction_revisions SET counts_json = ?1 WHERE revision_id = ?2",
        params![counts_json, revision_id],
    )?;
    Ok(())
}

fn revision_counts_with_capabilities(
    row_counts: &RowCounts,
    capability_rows_written: &RowDomainCounts,
) -> RowDomainCounts {
    let mut counts = RowDomainCounts::from(row_counts);
    counts.add_counts(capability_rows_written);
    counts
}

fn ensure_data_loss_guard(tx: &Transaction<'_>, file: &ArtifactFile) -> ArtifactWriteResult<()> {
    let existing_symbols: i64 = tx.query_row(
        "SELECT COUNT(*) FROM symbols WHERE path = ?1",
        [file.path.as_str()],
        |row| row.get(0),
    )?;
    if existing_symbols == 0 {
        return Ok(());
    }

    let reason = match file.status {
        FileStatus::FailedPreserved => Some("parser/read failure evidence"),
        FileStatus::Indexed | FileStatus::Unsupported => None,
    };

    if let Some(reason) = reason {
        return Err(ArtifactWriteError::DataLossGuard {
            path: file.path.clone(),
            existing_symbols,
            reason: reason.to_string(),
        });
    }

    Ok(())
}

fn delete_file_rows(tx: &Transaction<'_>, file_id: &str, path: &str) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM type_arguments
         WHERE usage_id IN (
           SELECT usage_id FROM type_argument_usages WHERE file_id = ?1
         )",
        [file_id],
    )?;
    tx.execute(
        "DELETE FROM type_argument_usages WHERE file_id = ?1",
        [file_id],
    )?;
    tx.execute("DELETE FROM literals WHERE file_id = ?1", [file_id])?;
    tx.execute("DELETE FROM source_regions WHERE file_id = ?1", [file_id])?;
    tx.execute("DELETE FROM structural_facts WHERE file_id = ?1", [file_id])?;
    tx.execute(
        "DELETE FROM complexity_metrics WHERE file_id = ?1",
        [file_id],
    )?;
    tx.execute(
        "DELETE FROM pending_relationships WHERE file_id = ?1",
        [file_id],
    )?;
    tx.execute("DELETE FROM relationships WHERE file_id = ?1", [file_id])?;
    tx.execute("DELETE FROM identifiers WHERE file_id = ?1", [file_id])?;
    tx.execute(
        "DELETE FROM type_facts
         WHERE symbol_id IN (SELECT symbol_id FROM symbols WHERE file_id = ?1)",
        [file_id],
    )?;
    tx.execute(
        "DELETE FROM symbol_annotations
         WHERE symbol_id IN (SELECT symbol_id FROM symbols WHERE file_id = ?1)",
        [file_id],
    )?;
    tx.execute(
        "DELETE FROM parse_diagnostics WHERE file_id = ?1",
        [file_id],
    )?;
    tx.execute("DELETE FROM symbols WHERE file_id = ?1", [file_id])?;
    tx.execute(
        "DELETE FROM files WHERE file_id = ?1 OR path = ?2",
        params![file_id, path],
    )?;
    Ok(())
}

fn insert_revision_file_change(
    tx: &Transaction<'_>,
    revision_id: i64,
    file_id: &str,
    path: &str,
    change_kind: RevisionChangeKind,
) -> rusqlite::Result<i64> {
    let mut stmt = tx.prepare_cached(
        "INSERT INTO revision_file_changes (revision_id, file_id, path, change_kind)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    rows::insert_revision_file_change_row(&mut stmt, revision_id, file_id, path, change_kind)
}

fn file_change_kind(file: &ArtifactFile, existing: Option<&ExistingFile>) -> RevisionChangeKind {
    match file.status {
        FileStatus::Unsupported => RevisionChangeKind::Unsupported,
        FileStatus::Indexed | FileStatus::FailedPreserved => {
            if existing.is_some() {
                RevisionChangeKind::Updated
            } else {
                RevisionChangeKind::Inserted
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::model::{
        ArtifactIdentifier, ArtifactPendingRelationship, ArtifactSymbol, ArtifactTypeFact,
    };

    #[test]
    fn scan_prepares_batch_statement_sets_once_per_transaction() {
        rows::writer_prepare_metrics::reset();
        let mut writer = ArtifactWriter::open_in_memory(test_metadata()).unwrap();
        let files = (0..5).map(test_file_with_child_rows).collect::<Vec<_>>();

        let result = writer.write_scan(test_revision(), &files).unwrap();

        assert_eq!(result.transactions_committed, 1);
        assert_eq!(result.rows_written.files, 5);
        assert_eq!(result.rows_written.symbols, 15);
        assert_eq!(result.rows_written.identifiers, 10);
        assert_eq!(result.rows_written.pending_relationships, 5);
        assert_eq!(result.rows_written.type_facts, 5);
        assert_eq!(
            rows::writer_prepare_metrics::file_row_inserter_prepares(),
            1
        );
        assert_eq!(
            rows::writer_prepare_metrics::child_row_inserter_prepares(),
            1
        );
    }

    #[test]
    fn symbol_lookup_uses_current_batch_symbols_without_sqlite_query() {
        let mut connection = Connection::open_in_memory().unwrap();
        create_schema(&connection).unwrap();
        let tx = connection.transaction().unwrap();
        let requested = (0..36_241)
            .map(|index| format!("symbol-{index}"))
            .collect::<HashSet<_>>();

        let lookup =
            rows::load_symbol_lookup_for_requested_ids(&tx, &requested, &requested).unwrap();

        assert_eq!(lookup.ids.len(), requested.len());
    }

    #[test]
    fn symbol_lookup_handles_large_unresolved_requests() {
        let mut connection = Connection::open_in_memory().unwrap();
        create_schema(&connection).unwrap();
        let tx = connection.transaction().unwrap();
        let requested = (0..36_241)
            .map(|index| format!("symbol-{index}"))
            .collect::<HashSet<_>>();

        let lookup =
            rows::load_symbol_lookup_for_requested_ids(&tx, &requested, &HashSet::new()).unwrap();

        assert!(lookup.ids.is_empty());
    }

    fn test_metadata() -> ArtifactMetadata {
        ArtifactMetadata {
            artifact_id: "artifact-writer-test".to_string(),
            root_path: "/repo".to_string(),
            binary_version: "julie-extract 0.1.0".to_string(),
            hash_algorithm: "blake3".to_string(),
            parser_inventory_fingerprint: "sha256:parser".to_string(),
            capability_snapshot_fingerprint: "sha256:cap".to_string(),
            created_at: "2026-05-31T19:20:00Z".to_string(),
            updated_at: "2026-05-31T19:20:00Z".to_string(),
        }
    }

    fn test_revision() -> RevisionInput {
        RevisionInput {
            operation: WriteOperation::Scan,
            mode: Some(WriteMode::Incremental),
            started_at: "2026-05-31T19:20:00Z".to_string(),
            completed_at: "2026-05-31T19:20:01Z".to_string(),
            binary_version: "julie-extract 0.1.0".to_string(),
            input_root: Some("/repo".to_string()),
        }
    }

    fn test_file_with_child_rows(index: usize) -> ArtifactFile {
        let mut file = test_file_with_symbols(index, 3);
        file.identifiers = (0..2)
            .map(|identifier_index| ArtifactIdentifier {
                identifier_id: format!("file-{index}-identifier-{identifier_index}"),
                name: format!("identifier_{index}_{identifier_index}"),
                containing_symbol_id: Some(format!("file-{index}-symbol-0")),
                target_symbol_id: Some(format!("file-{index}-symbol-1")),
                start_line: (identifier_index + 1) as i64,
                end_line: (identifier_index + 1) as i64,
                start_byte: (identifier_index * 8) as i64,
                end_byte: (identifier_index * 8 + 4) as i64,
                ..ArtifactIdentifier::default()
            })
            .collect();
        file.pending_relationships = vec![ArtifactPendingRelationship {
            pending_relationship_id: format!("file-{index}-pending"),
            from_symbol_id: format!("file-{index}-symbol-0"),
            caller_scope_symbol_id: Some(format!("file-{index}-symbol-0")),
            target_display_name: "externalTarget".to_string(),
            target_terminal_name: "externalTarget".to_string(),
            start_line: 1,
            ..ArtifactPendingRelationship::default()
        }];
        file.type_facts = vec![ArtifactTypeFact {
            type_fact_id: format!("file-{index}-type"),
            symbol_id: format!("file-{index}-symbol-0"),
            resolved_type: "Type".to_string(),
            generic_params_json: None,
            constraints_json: None,
            is_inferred: true,
            metadata_json: None,
        }];
        file
    }

    fn test_file_with_symbols(index: usize, symbol_count: usize) -> ArtifactFile {
        ArtifactFile {
            file_id: format!("file-{index}"),
            path: format!("src/file_{index}.rs"),
            language: "rust".to_string(),
            content_hash: format!("hash-{index}"),
            content_bytes: 64,
            line_count: Some(6),
            indexed_at: "2026-05-31T19:20:00Z".to_string(),
            status: FileStatus::Indexed,
            metadata_json: None,
            symbols: (0..symbol_count)
                .map(|symbol_index| ArtifactSymbol {
                    symbol_id: format!("file-{index}-symbol-{symbol_index}"),
                    name: format!("symbol_{index}_{symbol_index}"),
                    kind: "function".to_string(),
                    start_line: (symbol_index + 1) as i64,
                    end_line: (symbol_index + 1) as i64,
                    start_byte: (symbol_index * 8) as i64,
                    end_byte: (symbol_index * 8 + 4) as i64,
                    ..ArtifactSymbol::default()
                })
                .collect(),
            symbol_annotations: Vec::new(),
            identifiers: Vec::new(),
            relationships: Vec::new(),
            pending_relationships: Vec::new(),
            type_facts: Vec::new(),
            type_argument_usages: Vec::new(),
            type_arguments: Vec::new(),
            literals: Vec::new(),
            source_regions: Vec::new(),
            structural_facts: Vec::new(),
            complexity_metrics: Vec::new(),
            parse_diagnostics: Vec::new(),
        }
    }
}
