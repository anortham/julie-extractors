use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rusqlite::{CachedStatement, Connection, OptionalExtension, Transaction, params};

use crate::metadata::{ArtifactMetadata, initialize_metadata};
use crate::model::{
    ArtifactCapabilitySnapshot, ArtifactFile, ArtifactLanguageCapabilityFixtureRow,
    ArtifactLanguageCapabilityGapRow, ArtifactLanguageCapabilityRow, ArtifactParserInventoryRow,
    ArtifactTypeArgument, FileStatus, RevisionChangeKind, RevisionInput, RowCounts, WriteMode,
    WriteOperation, WriteResult,
};
use crate::reports::RowDomainCounts;
use crate::schema::{EXTRACT_CONTRACT_VERSION, SQLITE_SCHEMA_VERSION, create_schema};

pub type ArtifactWriteResult<T> = Result<T, ArtifactWriteError>;

const SQLITE_BULK_CACHE_SIZE_KIB: i64 = -131_072;
const SQLITE_STATEMENT_CACHE_CAPACITY: usize = 64;
const DROP_SYMBOL_LOOKUP_TEMP_TABLE_SQL: &str =
    "DROP TABLE IF EXISTS temp.julie_symbol_lookup_requested";
const CREATE_SYMBOL_LOOKUP_TEMP_TABLE_SQL: &str = "
CREATE TEMP TABLE julie_symbol_lookup_requested (
    symbol_id TEXT PRIMARY KEY
) WITHOUT ROWID
";

#[cfg(test)]
mod writer_prepare_metrics {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static FILE_ROW_INSERTER_PREPARES: AtomicUsize = AtomicUsize::new(0);
    static CHILD_ROW_INSERTER_PREPARES: AtomicUsize = AtomicUsize::new(0);

    pub(super) fn reset() {
        FILE_ROW_INSERTER_PREPARES.store(0, Ordering::SeqCst);
        CHILD_ROW_INSERTER_PREPARES.store(0, Ordering::SeqCst);
    }

    pub(super) fn record_file_row_inserter_prepare() {
        FILE_ROW_INSERTER_PREPARES.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn record_child_row_inserter_prepare() {
        CHILD_ROW_INSERTER_PREPARES.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn file_row_inserter_prepares() -> usize {
        FILE_ROW_INSERTER_PREPARES.load(Ordering::SeqCst)
    }

    pub(super) fn child_row_inserter_prepares() -> usize {
        CHILD_ROW_INSERTER_PREPARES.load(Ordering::SeqCst)
    }
}

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
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn into_connection(self) -> Connection {
        self.connection
    }

    pub fn sync_capability_snapshot(
        &mut self,
        snapshot: &ArtifactCapabilitySnapshot,
    ) -> ArtifactWriteResult<RowDomainCounts> {
        let tx = self.connection.transaction()?;
        let mut counts = RowDomainCounts::default();

        let parser_keys = snapshot
            .parser_inventory
            .iter()
            .map(|row| (row.language.clone(), row.parser_package.clone()))
            .collect::<HashSet<_>>();
        for (language, parser_package) in load_parser_inventory_keys(&tx)? {
            if !parser_keys.contains(&(language.clone(), parser_package.clone())) {
                counts.parser_inventory += tx.execute(
                    "DELETE FROM parser_inventory WHERE language = ?1 AND parser_package = ?2",
                    params![language, parser_package],
                )? as i64;
            }
        }

        let language_keys = snapshot
            .languages
            .iter()
            .map(|row| row.language.clone())
            .collect::<HashSet<_>>();
        let fixture_keys = snapshot
            .languages
            .iter()
            .flat_map(|row| {
                row.fixtures
                    .iter()
                    .map(|fixture| (row.language.clone(), fixture.fixture_name.clone()))
            })
            .collect::<HashSet<_>>();
        let gap_keys = snapshot
            .languages
            .iter()
            .flat_map(|row| row.gaps.iter().map(|gap| gap.gap_id.clone()))
            .collect::<HashSet<_>>();

        for (language, fixture_name) in load_language_capability_fixture_keys(&tx)? {
            if !fixture_keys.contains(&(language.clone(), fixture_name.clone())) {
                counts.language_capability_fixtures += tx.execute(
                    "DELETE FROM language_capability_fixtures
                     WHERE language = ?1 AND fixture_name = ?2",
                    params![language, fixture_name],
                )? as i64;
            }
        }
        for gap_id in load_language_capability_gap_keys(&tx)? {
            if !gap_keys.contains(&gap_id) {
                counts.language_capability_gaps += tx.execute(
                    "DELETE FROM language_capability_gaps WHERE gap_id = ?1",
                    [gap_id],
                )? as i64;
            }
        }
        for language in load_language_capability_keys(&tx)? {
            if !language_keys.contains(&language) {
                counts.language_capabilities += tx.execute(
                    "DELETE FROM language_capabilities WHERE language = ?1",
                    [language],
                )? as i64;
            }
        }

        for row in &snapshot.parser_inventory {
            counts.parser_inventory += upsert_parser_inventory(&tx, row)? as i64;
        }
        for row in &snapshot.languages {
            counts.language_capabilities += upsert_language_capability(&tx, row)? as i64;
            for fixture in &row.fixtures {
                counts.language_capability_fixtures +=
                    upsert_language_capability_fixture(&tx, &row.language, fixture)? as i64;
            }
            for gap in &row.gaps {
                counts.language_capability_gaps +=
                    upsert_language_capability_gap(&tx, &row.language, gap)? as i64;
            }
        }

        tx.commit()?;
        Ok(counts)
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
        self.write_scan_spooled_snapshot(revision, snapshot_paths, spool)
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
        let tx = self.connection.transaction()?;
        let existing = load_existing_file(&tx, path)?;
        let Some(existing) = existing else {
            tx.commit()?;
            return Ok(WriteResult {
                transactions_committed: 1,
                ..WriteResult::default()
            });
        };

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
                RevisionChangeKind::Deleted,
            )?,
            ..RowCounts::default()
        };
        update_revision_counts(&tx, revision_id, &row_counts)?;
        tx.commit()?;

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
        let tx = self.connection.transaction()?;
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

        if planned.is_empty() {
            tx.commit()?;
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
                row_counts.symbols += file_row_inserters.insert_symbols(file)?;
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

        update_revision_counts(&tx, revision_id, &row_counts)?;
        tx.commit()?;

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
        let tx = self.connection.transaction()?;
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

        if planned.is_empty() && deleted.is_empty() {
            tx.commit()?;
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
                row_counts.symbols += file_row_inserters.insert_symbols(file)?;
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

        update_revision_counts(&tx, revision_id, &row_counts)?;
        tx.commit()?;

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
        spool: &ArtifactFileSpool,
    ) -> ArtifactWriteResult<WriteResult> {
        let tx = self.connection.transaction()?;
        let snapshot_paths = snapshot_paths
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let skip_unchanged_content = revision.mode != Some(WriteMode::Force);
        let mut planned_files: HashMap<String, Option<ExistingFile>> = HashMap::new();
        let mut files_skipped = 0;

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
            // Carry the existing-file lookup forward; pass B reuses it instead of re-SELECTing.
            // Nothing mutates a planned path's `files` row between here and its insert in pass B,
            // so the value stays valid (per-file deletes happen immediately before each re-insert).
            planned_files.insert(file.path, existing);
        }

        let deleted = load_existing_files(&tx)?
            .into_iter()
            .filter(|existing| !snapshot_paths.contains(existing.path.as_str()))
            .collect::<Vec<_>>();

        if planned_files.is_empty() && deleted.is_empty() {
            tx.commit()?;
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
        let mut rewritten_file_ids = HashSet::new();

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
                    row_counts.symbols += file_row_inserters.insert_symbols(&file)?;
                    rewritten_file_ids.insert(file.file_id.clone());
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

        let mut requested_symbol_ids = HashSet::new();
        let mut local_symbol_ids = HashSet::new();
        for file in spool.iter()? {
            let file = file?;
            if rewritten_file_ids.contains(&file.file_id) {
                collect_requested_symbol_ids(&file, &mut requested_symbol_ids);
                collect_file_symbol_ids(&file, &mut local_symbol_ids);
            }
        }
        let symbol_lookup =
            load_symbol_lookup_for_requested_ids(&tx, &requested_symbol_ids, &local_symbol_ids)?;

        {
            let mut child_row_inserters = ChildRowInserters::prepare(&tx)?;
            let mut parent_update =
                tx.prepare_cached("UPDATE symbols SET parent_symbol_id = ?1 WHERE symbol_id = ?2")?;
            for file in spool.iter()? {
                let file = file?;
                if !rewritten_file_ids.contains(&file.file_id) {
                    continue;
                }
                update_symbol_parent_rows(
                    &mut parent_update,
                    std::iter::once(&file),
                    &symbol_lookup,
                )?;
                child_row_inserters.insert_child_rows(&file, &symbol_lookup, &mut row_counts)?;
            }
        }

        update_revision_counts(&tx, revision_id, &row_counts)?;
        tx.commit()?;

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

fn load_parser_inventory_keys(tx: &Transaction<'_>) -> rusqlite::Result<HashSet<(String, String)>> {
    let mut statement = tx.prepare("SELECT language, parser_package FROM parser_inventory")?;
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect()
}

fn load_language_capability_keys(tx: &Transaction<'_>) -> rusqlite::Result<HashSet<String>> {
    let mut statement = tx.prepare("SELECT language FROM language_capabilities")?;
    statement.query_map([], |row| row.get(0))?.collect()
}

fn load_language_capability_fixture_keys(
    tx: &Transaction<'_>,
) -> rusqlite::Result<HashSet<(String, String)>> {
    let mut statement =
        tx.prepare("SELECT language, fixture_name FROM language_capability_fixtures")?;
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect()
}

fn load_language_capability_gap_keys(tx: &Transaction<'_>) -> rusqlite::Result<HashSet<String>> {
    let mut statement = tx.prepare("SELECT gap_id FROM language_capability_gaps")?;
    statement.query_map([], |row| row.get(0))?.collect()
}

fn upsert_parser_inventory(
    tx: &Transaction<'_>,
    row: &ArtifactParserInventoryRow,
) -> rusqlite::Result<usize> {
    let metadata_json = row.metadata.as_ref().map(json_string);
    tx.execute(
        "INSERT INTO parser_inventory
         (language, parser_package, parser_version, grammar_version, source, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(language, parser_package) DO UPDATE SET
           parser_version = excluded.parser_version,
           grammar_version = excluded.grammar_version,
           source = excluded.source,
           metadata_json = excluded.metadata_json
         WHERE parser_inventory.parser_version IS NOT excluded.parser_version
            OR parser_inventory.grammar_version IS NOT excluded.grammar_version
            OR parser_inventory.source IS NOT excluded.source
            OR parser_inventory.metadata_json IS NOT excluded.metadata_json",
        params![
            row.language,
            row.parser_package,
            row.parser_version,
            row.grammar_version,
            row.source,
            metadata_json,
        ],
    )
}

fn upsert_language_capability(
    tx: &Transaction<'_>,
    row: &ArtifactLanguageCapabilityRow,
) -> rusqlite::Result<usize> {
    let extensions_json = json_string(&row.extensions);
    let kind_coverage_json = json_string(&row.kind_coverage);
    tx.execute(
        "INSERT INTO language_capabilities
         (language, parser_package, extensions_json, dependency_status,
          target_symbols, target_relationships, target_pending_relationships,
          target_identifiers, target_types, actual_symbols, actual_relationships,
          actual_pending_relationships, actual_identifiers, actual_types,
          kind_coverage_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(language) DO UPDATE SET
           parser_package = excluded.parser_package,
           extensions_json = excluded.extensions_json,
           dependency_status = excluded.dependency_status,
           target_symbols = excluded.target_symbols,
           target_relationships = excluded.target_relationships,
           target_pending_relationships = excluded.target_pending_relationships,
           target_identifiers = excluded.target_identifiers,
           target_types = excluded.target_types,
           actual_symbols = excluded.actual_symbols,
           actual_relationships = excluded.actual_relationships,
           actual_pending_relationships = excluded.actual_pending_relationships,
           actual_identifiers = excluded.actual_identifiers,
           actual_types = excluded.actual_types,
           kind_coverage_json = excluded.kind_coverage_json
         WHERE language_capabilities.parser_package IS NOT excluded.parser_package
            OR language_capabilities.extensions_json IS NOT excluded.extensions_json
            OR language_capabilities.dependency_status IS NOT excluded.dependency_status
            OR language_capabilities.target_symbols IS NOT excluded.target_symbols
            OR language_capabilities.target_relationships IS NOT excluded.target_relationships
            OR language_capabilities.target_pending_relationships IS NOT excluded.target_pending_relationships
            OR language_capabilities.target_identifiers IS NOT excluded.target_identifiers
            OR language_capabilities.target_types IS NOT excluded.target_types
            OR language_capabilities.actual_symbols IS NOT excluded.actual_symbols
            OR language_capabilities.actual_relationships IS NOT excluded.actual_relationships
            OR language_capabilities.actual_pending_relationships IS NOT excluded.actual_pending_relationships
            OR language_capabilities.actual_identifiers IS NOT excluded.actual_identifiers
            OR language_capabilities.actual_types IS NOT excluded.actual_types
            OR language_capabilities.kind_coverage_json IS NOT excluded.kind_coverage_json",
        params![
            row.language,
            row.parser_package,
            extensions_json,
            row.dependency_status,
            bool_int(row.target_capabilities.symbols),
            bool_int(row.target_capabilities.relationships),
            bool_int(row.target_capabilities.pending_relationships),
            bool_int(row.target_capabilities.identifiers),
            bool_int(row.target_capabilities.types),
            bool_int(row.actual_capabilities.symbols),
            bool_int(row.actual_capabilities.relationships),
            bool_int(row.actual_capabilities.pending_relationships),
            bool_int(row.actual_capabilities.identifiers),
            bool_int(row.actual_capabilities.types),
            kind_coverage_json,
        ],
    )
}

fn upsert_language_capability_fixture(
    tx: &Transaction<'_>,
    language: &str,
    fixture: &ArtifactLanguageCapabilityFixtureRow,
) -> rusqlite::Result<usize> {
    tx.execute(
        "INSERT INTO language_capability_fixtures
         (language, fixture_name, source_path, expected_path)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(language, fixture_name) DO UPDATE SET
           source_path = excluded.source_path,
           expected_path = excluded.expected_path
         WHERE language_capability_fixtures.source_path IS NOT excluded.source_path
            OR language_capability_fixtures.expected_path IS NOT excluded.expected_path",
        params![
            language,
            fixture.fixture_name,
            fixture.source_path,
            fixture.expected_path,
        ],
    )
}

fn upsert_language_capability_gap(
    tx: &Transaction<'_>,
    language: &str,
    gap: &ArtifactLanguageCapabilityGapRow,
) -> rusqlite::Result<usize> {
    let evidence_json = json_string(&gap.evidence);
    tx.execute(
        "INSERT INTO language_capability_gaps
         (gap_id, language, capability, status, reason, required_closure, evidence_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(gap_id) DO UPDATE SET
           language = excluded.language,
           capability = excluded.capability,
           status = excluded.status,
           reason = excluded.reason,
           required_closure = excluded.required_closure,
           evidence_json = excluded.evidence_json
         WHERE language_capability_gaps.language IS NOT excluded.language
            OR language_capability_gaps.capability IS NOT excluded.capability
            OR language_capability_gaps.status IS NOT excluded.status
            OR language_capability_gaps.reason IS NOT excluded.reason
            OR language_capability_gaps.required_closure IS NOT excluded.required_closure
            OR language_capability_gaps.evidence_json IS NOT excluded.evidence_json",
        params![
            gap.gap_id,
            language,
            gap.capability,
            gap.status,
            gap.reason,
            gap.required_closure,
            evidence_json,
        ],
    )
}

fn bool_int(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn json_string<T: serde::Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).expect("artifact capability values must serialize")
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
    row_counts: &RowCounts,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE extraction_revisions SET counts_json = ?1 WHERE revision_id = ?2",
        params![row_counts.counts_json(), revision_id],
    )?;
    Ok(())
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
    insert_revision_file_change_row(&mut stmt, revision_id, file_id, path, change_kind)
}

fn insert_revision_file_change_row(
    stmt: &mut CachedStatement<'_>,
    revision_id: i64,
    file_id: &str,
    path: &str,
    change_kind: RevisionChangeKind,
) -> rusqlite::Result<i64> {
    stmt.execute(params![revision_id, file_id, path, change_kind.as_str()])?;
    Ok(1)
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

struct FileRowInserters<'tx> {
    files: CachedStatement<'tx>,
    revision_file_changes: CachedStatement<'tx>,
    symbols: CachedStatement<'tx>,
    symbol_parent_update: CachedStatement<'tx>,
}

impl<'tx> FileRowInserters<'tx> {
    fn prepare(tx: &'tx Transaction<'_>) -> rusqlite::Result<Self> {
        let inserters = Self {
            files: tx.prepare_cached(
                "INSERT INTO files
                 (file_id, path, language, content_hash, content_bytes, line_count, indexed_at,
                  last_revision_id, status, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?,
            revision_file_changes: tx.prepare_cached(
                "INSERT INTO revision_file_changes (revision_id, file_id, path, change_kind)
                 VALUES (?1, ?2, ?3, ?4)",
            )?,
            symbols: tx.prepare_cached(
                "INSERT INTO symbols
                 (symbol_id, file_id, path, language, name, kind, signature, doc_comment,
                  visibility, parent_symbol_id, start_line, start_column, end_line, end_column,
                  start_byte, end_byte, body_start_line, body_start_column, body_end_line,
                  body_end_column, body_start_byte, body_end_byte, body_hash, semantic_group,
                  confidence, content_type, is_test, test_container, test_lifecycle, metadata_json)
                 VALUES
                 (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                  ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29)",
            )?,
            symbol_parent_update: tx
                .prepare_cached("UPDATE symbols SET parent_symbol_id = ?1 WHERE symbol_id = ?2")?,
        };
        #[cfg(test)]
        writer_prepare_metrics::record_file_row_inserter_prepare();
        Ok(inserters)
    }

    fn insert_file(&mut self, revision_id: i64, file: &ArtifactFile) -> rusqlite::Result<()> {
        insert_file_row(&mut self.files, revision_id, file)
    }

    fn insert_revision_file_change(
        &mut self,
        revision_id: i64,
        file_id: &str,
        path: &str,
        change_kind: RevisionChangeKind,
    ) -> rusqlite::Result<i64> {
        insert_revision_file_change_row(
            &mut self.revision_file_changes,
            revision_id,
            file_id,
            path,
            change_kind,
        )
    }

    fn insert_symbols(&mut self, file: &ArtifactFile) -> rusqlite::Result<i64> {
        insert_symbol_rows(&mut self.symbols, file)
    }

    fn update_symbol_parents<'a>(
        &mut self,
        files: impl IntoIterator<Item = &'a ArtifactFile>,
        symbol_lookup: &SymbolLookup,
    ) -> rusqlite::Result<()> {
        update_symbol_parent_rows(&mut self.symbol_parent_update, files, symbol_lookup)
    }
}

struct ChildRowInserters<'tx> {
    symbol_annotations: CachedStatement<'tx>,
    identifiers: CachedStatement<'tx>,
    relationships: CachedStatement<'tx>,
    pending_relationships: CachedStatement<'tx>,
    type_facts: CachedStatement<'tx>,
    type_argument_usages: CachedStatement<'tx>,
    type_arguments: CachedStatement<'tx>,
    literals: CachedStatement<'tx>,
    parse_diagnostics: CachedStatement<'tx>,
}

impl<'tx> ChildRowInserters<'tx> {
    fn prepare(tx: &'tx Transaction<'_>) -> rusqlite::Result<Self> {
        let inserters = Self {
            symbol_annotations: tx.prepare_cached(
                "INSERT INTO symbol_annotations
                 (annotation_id, symbol_id, annotation, annotation_key, raw_text, carrier,
                  metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?,
            identifiers: tx.prepare_cached(
                "INSERT INTO identifiers
                 (identifier_id, file_id, path, language, name, kind, containing_symbol_id,
                  target_symbol_id, start_line, start_column, end_line, end_column, start_byte,
                  end_byte, confidence, code_context, metadata_json)
                 VALUES
                 (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            )?,
            relationships: tx.prepare_cached(
                "INSERT INTO relationships
                 (relationship_id, from_symbol_id, to_symbol_id, file_id, path, kind, start_line,
                  start_column, end_line, end_column, start_byte, end_byte, confidence,
                  metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            )?,
            pending_relationships: tx.prepare_cached(
                "INSERT INTO pending_relationships
                 (pending_relationship_id, from_symbol_id, caller_scope_symbol_id, file_id, path,
                  kind, target_display_name, target_terminal_name, target_receiver,
                  target_namespace_json, target_import_context, start_line, start_column,
                  end_line, end_column, start_byte, end_byte, confidence, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                         ?17, ?18, ?19)",
            )?,
            type_facts: tx.prepare_cached(
                "INSERT INTO type_facts
                 (type_fact_id, symbol_id, language, resolved_type, generic_params_json,
                  constraints_json, is_inferred, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?,
            type_argument_usages: tx.prepare_cached(
                "INSERT INTO type_argument_usages
                 (usage_id, identifier_id, file_id, path, language, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?,
            type_arguments: tx.prepare_cached(
                "INSERT INTO type_arguments
                 (type_argument_id, usage_id, parent_type_argument_id, ordinal, type_name)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?,
            literals: tx.prepare_cached(
                "INSERT INTO literals
                 (literal_id, file_id, path, language, literal_text, kind, carrier, arg_position,
                  containing_symbol_id, start_line, start_column, end_line, end_column, start_byte,
                  end_byte, confidence, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                         ?17)",
            )?,
            parse_diagnostics: tx.prepare_cached(
                "INSERT INTO parse_diagnostics
                 (diagnostic_id, file_id, path, language, kind, message, start_line, start_column,
                  end_line, end_column, start_byte, end_byte, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            )?,
        };
        #[cfg(test)]
        writer_prepare_metrics::record_child_row_inserter_prepare();
        Ok(inserters)
    }

    fn insert_child_rows(
        &mut self,
        file: &ArtifactFile,
        symbol_lookup: &SymbolLookup,
        counts: &mut RowCounts,
    ) -> rusqlite::Result<()> {
        counts.symbol_annotations +=
            insert_symbol_annotations(&mut self.symbol_annotations, file, symbol_lookup)?;
        counts.identifiers += insert_identifiers(&mut self.identifiers, file, symbol_lookup)?;
        let identifier_lookup = IdentifierLookup::from_file(file);
        counts.relationships += insert_relationships(&mut self.relationships, file, symbol_lookup)?;
        counts.pending_relationships +=
            insert_pending_relationships(&mut self.pending_relationships, file, symbol_lookup)?;
        counts.type_facts += insert_type_facts(&mut self.type_facts, file, symbol_lookup)?;
        counts.type_argument_usages +=
            insert_type_argument_usages(&mut self.type_argument_usages, file, &identifier_lookup)?;
        let usage_lookup = TypeArgumentUsageLookup::from_file(file, &identifier_lookup);
        counts.type_arguments += insert_type_arguments(
            &mut self.type_arguments,
            &file.type_arguments,
            &usage_lookup,
        )?;
        counts.literals += insert_literals(&mut self.literals, file, symbol_lookup)?;
        counts.parse_diagnostics +=
            insert_parse_diagnostics_rows(&mut self.parse_diagnostics, file)?;
        Ok(())
    }
}

fn insert_file_row(
    stmt: &mut CachedStatement<'_>,
    revision_id: i64,
    file: &ArtifactFile,
) -> rusqlite::Result<()> {
    stmt.execute(params![
        file.file_id,
        file.path,
        file.language,
        file.content_hash,
        file.content_bytes,
        file.line_count,
        file.indexed_at,
        revision_id,
        file.status.as_str(),
        file.metadata_json,
    ])?;
    Ok(())
}

fn is_preserved_failure(file: &ArtifactFile) -> bool {
    file.status == FileStatus::FailedPreserved
}

fn is_preserved_failure_update(file: &ArtifactFile, existing: Option<&ExistingFile>) -> bool {
    is_preserved_failure(file) && existing.is_some()
}

fn update_failed_preserved_file(
    tx: &Transaction<'_>,
    revision_id: i64,
    file: &ArtifactFile,
    existing_file_id: &str,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE files
         SET language = ?1,
             content_hash = ?2,
             content_bytes = ?3,
             line_count = ?4,
             indexed_at = ?5,
             last_revision_id = ?6,
             status = ?7,
             metadata_json = ?8
         WHERE file_id = ?9",
        params![
            file.language,
            file.content_hash,
            file.content_bytes,
            file.line_count,
            file.indexed_at,
            revision_id,
            file.status.as_str(),
            file.metadata_json,
            existing_file_id,
        ],
    )?;
    Ok(())
}

fn replace_parse_diagnostics(tx: &Transaction<'_>, file: &ArtifactFile) -> rusqlite::Result<i64> {
    tx.execute(
        "DELETE FROM parse_diagnostics WHERE file_id = ?1 OR path = ?2",
        params![file.file_id, file.path],
    )?;
    insert_parse_diagnostics(tx, file)
}

fn insert_symbol_rows(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
) -> rusqlite::Result<i64> {
    for symbol in &file.symbols {
        stmt.execute(params![
            symbol.symbol_id,
            file.file_id,
            file.path,
            file.language,
            symbol.name,
            symbol.kind,
            symbol.signature,
            symbol.doc_comment,
            symbol.visibility,
            symbol.start_line,
            symbol.start_column,
            symbol.end_line,
            symbol.end_column,
            symbol.start_byte,
            symbol.end_byte,
            symbol.body_start_line,
            symbol.body_start_column,
            symbol.body_end_line,
            symbol.body_end_column,
            symbol.body_start_byte,
            symbol.body_end_byte,
            symbol.body_hash,
            symbol.semantic_group,
            symbol.confidence,
            symbol.content_type,
            symbol.is_test,
            symbol.test_container,
            symbol.test_lifecycle,
            symbol.metadata_json,
        ])?;
    }

    Ok(file.symbols.len() as i64)
}

fn update_symbol_parent_rows<'a>(
    parent_update: &mut CachedStatement<'_>,
    files: impl IntoIterator<Item = &'a ArtifactFile>,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<()> {
    for file in files {
        for symbol in &file.symbols {
            if let Some(parent_symbol_id) = symbol.parent_symbol_id.as_deref()
                && symbol_lookup.contains(parent_symbol_id)
            {
                parent_update.execute(params![parent_symbol_id, symbol.symbol_id])?;
            }
        }
    }
    Ok(())
}

fn insert_symbol_annotations(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    let mut inserted = 0;
    for annotation in &file.symbol_annotations {
        if !symbol_lookup.contains(&annotation.symbol_id) {
            continue;
        }
        stmt.execute(params![
            annotation.annotation_id,
            annotation.symbol_id,
            annotation.annotation,
            annotation.annotation_key,
            annotation.raw_text,
            annotation.carrier,
            annotation.metadata_json,
        ])?;
        inserted += 1;
    }
    Ok(inserted)
}

fn insert_identifiers(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    // Resolve the symbol FKs inline at INSERT time. symbol_lookup is fully populated before any
    // child rows are written (all symbols for all files are inserted first), so the second
    // UPDATE pass that older revisions used was pure overhead — one extra statement per
    // identifier plus double index maintenance on idx_identifiers_containing/target. Unresolved
    // references bind as SQL NULL via valid_symbol_id, identical to the prior NULL columns.
    for identifier in &file.identifiers {
        let containing = valid_symbol_id(symbol_lookup, identifier.containing_symbol_id.as_deref());
        let target = valid_symbol_id(symbol_lookup, identifier.target_symbol_id.as_deref());
        stmt.execute(params![
            identifier.identifier_id,
            file.file_id,
            file.path,
            file.language,
            identifier.name,
            identifier.kind,
            containing,
            target,
            identifier.start_line,
            identifier.start_column,
            identifier.end_line,
            identifier.end_column,
            identifier.start_byte,
            identifier.end_byte,
            identifier.confidence,
            identifier.code_context,
            identifier.metadata_json,
        ])?;
    }

    Ok(file.identifiers.len() as i64)
}

fn insert_relationships(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    let mut inserted = 0;
    for relationship in &file.relationships {
        if !symbol_lookup.contains(&relationship.from_symbol_id)
            || !symbol_lookup.contains(&relationship.to_symbol_id)
        {
            continue;
        }
        stmt.execute(params![
            relationship.relationship_id,
            relationship.from_symbol_id,
            relationship.to_symbol_id,
            file.file_id,
            file.path,
            relationship.kind,
            relationship.start_line,
            relationship.start_column,
            relationship.end_line,
            relationship.end_column,
            relationship.start_byte,
            relationship.end_byte,
            relationship.confidence,
            relationship.metadata_json,
        ])?;
        inserted += 1;
    }
    Ok(inserted)
}

fn insert_pending_relationships(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    let mut inserted = 0;
    for pending in &file.pending_relationships {
        if !symbol_lookup.contains(&pending.from_symbol_id) {
            continue;
        }
        stmt.execute(params![
            pending.pending_relationship_id,
            pending.from_symbol_id,
            valid_symbol_id(symbol_lookup, pending.caller_scope_symbol_id.as_deref()),
            file.file_id,
            file.path,
            pending.kind,
            pending.target_display_name,
            pending.target_terminal_name,
            pending.target_receiver,
            pending.target_namespace_json,
            pending.target_import_context,
            pending.start_line,
            pending.start_column,
            pending.end_line,
            pending.end_column,
            pending.start_byte,
            pending.end_byte,
            pending.confidence,
            pending.metadata_json,
        ])?;
        inserted += 1;
    }
    Ok(inserted)
}

fn insert_type_facts(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    let mut inserted = 0;
    for fact in &file.type_facts {
        if !symbol_lookup.contains(&fact.symbol_id) {
            continue;
        }
        stmt.execute(params![
            fact.type_fact_id,
            fact.symbol_id,
            file.language,
            fact.resolved_type,
            fact.generic_params_json,
            fact.constraints_json,
            fact.is_inferred as i64,
            fact.metadata_json,
        ])?;
        inserted += 1;
    }
    Ok(inserted)
}

fn insert_type_argument_usages(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    identifier_lookup: &IdentifierLookup,
) -> rusqlite::Result<i64> {
    let mut inserted = 0;
    for usage in &file.type_argument_usages {
        if !identifier_lookup.contains(&usage.identifier_id) {
            continue;
        }
        stmt.execute(params![
            usage.usage_id,
            usage.identifier_id,
            file.file_id,
            file.path,
            file.language,
            usage.metadata_json,
        ])?;
        inserted += 1;
    }
    Ok(inserted)
}

fn insert_type_arguments(
    stmt: &mut CachedStatement<'_>,
    arguments: &[ArtifactTypeArgument],
    usage_lookup: &TypeArgumentUsageLookup,
) -> rusqlite::Result<i64> {
    let mut inserted = 0;
    for argument in arguments {
        if !usage_lookup.contains(&argument.usage_id) {
            continue;
        }
        stmt.execute(params![
            argument.type_argument_id,
            argument.usage_id,
            argument.parent_type_argument_id,
            argument.ordinal,
            argument.type_name,
        ])?;
        inserted += 1;
    }
    Ok(inserted)
}

fn insert_literals(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    for literal in &file.literals {
        stmt.execute(params![
            literal.literal_id,
            file.file_id,
            file.path,
            file.language,
            literal.literal_text,
            literal.kind,
            literal.carrier,
            literal.arg_position,
            valid_symbol_id(symbol_lookup, literal.containing_symbol_id.as_deref()),
            literal.start_line,
            literal.start_column,
            literal.end_line,
            literal.end_column,
            literal.start_byte,
            literal.end_byte,
            literal.confidence,
            literal.metadata_json,
        ])?;
    }
    Ok(file.literals.len() as i64)
}

fn insert_parse_diagnostics(tx: &Transaction<'_>, file: &ArtifactFile) -> rusqlite::Result<i64> {
    let mut stmt = tx.prepare_cached(
        "INSERT INTO parse_diagnostics
         (diagnostic_id, file_id, path, language, kind, message, start_line, start_column,
          end_line, end_column, start_byte, end_byte, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;
    insert_parse_diagnostics_rows(&mut stmt, file)
}

fn insert_parse_diagnostics_rows(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
) -> rusqlite::Result<i64> {
    for diagnostic in &file.parse_diagnostics {
        stmt.execute(params![
            diagnostic.diagnostic_id,
            file.file_id,
            file.path,
            file.language,
            diagnostic.kind,
            diagnostic.message,
            diagnostic.start_line,
            diagnostic.start_column,
            diagnostic.end_line,
            diagnostic.end_column,
            diagnostic.start_byte,
            diagnostic.end_byte,
            diagnostic.metadata_json,
        ])?;
    }
    Ok(file.parse_diagnostics.len() as i64)
}

#[derive(Default)]
struct SymbolLookup {
    ids: HashSet<String>,
}

impl SymbolLookup {
    fn contains(&self, symbol_id: &str) -> bool {
        self.ids.contains(symbol_id)
    }
}

fn load_symbol_lookup<'a>(
    tx: &Transaction<'_>,
    files: impl IntoIterator<Item = &'a ArtifactFile>,
) -> rusqlite::Result<SymbolLookup> {
    let mut requested = HashSet::new();
    let mut local_symbols = HashSet::new();
    for file in files {
        collect_requested_symbol_ids(file, &mut requested);
        collect_file_symbol_ids(file, &mut local_symbols);
    }

    load_symbol_lookup_for_requested_ids(tx, &requested, &local_symbols)
}

fn collect_file_symbol_ids(file: &ArtifactFile, ids: &mut HashSet<String>) {
    ids.extend(file.symbols.iter().map(|symbol| symbol.symbol_id.clone()));
}

fn collect_requested_symbol_ids(file: &ArtifactFile, requested: &mut HashSet<String>) {
    for symbol in &file.symbols {
        if let Some(parent_symbol_id) = symbol.parent_symbol_id.as_deref() {
            requested.insert(parent_symbol_id.to_string());
        }
    }
    for annotation in &file.symbol_annotations {
        requested.insert(annotation.symbol_id.clone());
    }
    for identifier in &file.identifiers {
        if let Some(containing_symbol_id) = identifier.containing_symbol_id.as_deref() {
            requested.insert(containing_symbol_id.to_string());
        }
        if let Some(target_symbol_id) = identifier.target_symbol_id.as_deref() {
            requested.insert(target_symbol_id.to_string());
        }
    }
    for relationship in &file.relationships {
        requested.insert(relationship.from_symbol_id.clone());
        requested.insert(relationship.to_symbol_id.clone());
    }
    for pending in &file.pending_relationships {
        requested.insert(pending.from_symbol_id.clone());
        if let Some(caller_scope_symbol_id) = pending.caller_scope_symbol_id.as_deref() {
            requested.insert(caller_scope_symbol_id.to_string());
        }
    }
    for fact in &file.type_facts {
        requested.insert(fact.symbol_id.clone());
    }
    for literal in &file.literals {
        if let Some(containing_symbol_id) = literal.containing_symbol_id.as_deref() {
            requested.insert(containing_symbol_id.to_string());
        }
    }
}

fn load_symbol_lookup_for_requested_ids(
    tx: &Transaction<'_>,
    requested: &HashSet<String>,
    local_symbols: &HashSet<String>,
) -> rusqlite::Result<SymbolLookup> {
    if requested.is_empty() {
        return Ok(SymbolLookup::default());
    }

    let mut ids = requested
        .intersection(local_symbols)
        .cloned()
        .collect::<HashSet<_>>();
    let unresolved = requested.difference(&ids).cloned().collect::<Vec<_>>();
    if !unresolved.is_empty() {
        load_existing_symbol_ids_for_requested_ids(tx, &unresolved, &mut ids)?;
    }

    Ok(SymbolLookup { ids })
}

fn load_existing_symbol_ids_for_requested_ids(
    tx: &Transaction<'_>,
    requested: &[String],
    ids: &mut HashSet<String>,
) -> rusqlite::Result<()> {
    tx.execute(DROP_SYMBOL_LOOKUP_TEMP_TABLE_SQL, [])?;
    let lookup_result = (|| -> rusqlite::Result<()> {
        tx.execute(CREATE_SYMBOL_LOOKUP_TEMP_TABLE_SQL, [])?;

        {
            let mut insert_requested = tx.prepare(
                "INSERT OR IGNORE INTO temp.julie_symbol_lookup_requested(symbol_id) VALUES (?1)",
            )?;
            for symbol_id in requested {
                insert_requested.execute(params![symbol_id])?;
            }
        }

        {
            let mut stmt = tx.prepare(
                "SELECT symbols.symbol_id \
                 FROM symbols \
                 INNER JOIN temp.julie_symbol_lookup_requested AS requested \
                    ON requested.symbol_id = symbols.symbol_id",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                ids.insert(row?);
            }
        }

        Ok(())
    })();
    let cleanup_result = tx.execute(DROP_SYMBOL_LOOKUP_TEMP_TABLE_SQL, []);
    match (lookup_result, cleanup_result) {
        (Ok(()), Ok(_)) => Ok(()),
        (Err(error), _) | (Ok(()), Err(error)) => Err(error),
    }
}

fn valid_symbol_id<'a>(
    symbol_lookup: &SymbolLookup,
    symbol_id: Option<&'a str>,
) -> Option<&'a str> {
    symbol_id.filter(|symbol_id| symbol_lookup.contains(symbol_id))
}

struct IdentifierLookup {
    ids: HashSet<String>,
}

impl IdentifierLookup {
    fn from_file(file: &ArtifactFile) -> Self {
        Self {
            ids: file
                .identifiers
                .iter()
                .map(|identifier| identifier.identifier_id.clone())
                .collect(),
        }
    }

    fn contains(&self, identifier_id: &str) -> bool {
        self.ids.contains(identifier_id)
    }
}

struct TypeArgumentUsageLookup {
    ids: HashSet<String>,
}

impl TypeArgumentUsageLookup {
    fn from_file(file: &ArtifactFile, identifier_lookup: &IdentifierLookup) -> Self {
        Self {
            ids: file
                .type_argument_usages
                .iter()
                .filter(|usage| identifier_lookup.contains(&usage.identifier_id))
                .map(|usage| usage.usage_id.clone())
                .collect(),
        }
    }

    fn contains(&self, usage_id: &str) -> bool {
        self.ids.contains(usage_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ArtifactIdentifier, ArtifactPendingRelationship, ArtifactSymbol, ArtifactTypeFact,
    };

    #[test]
    fn scan_prepares_batch_statement_sets_once_per_transaction() {
        writer_prepare_metrics::reset();
        let mut writer = ArtifactWriter::open_in_memory(test_metadata()).unwrap();
        let files = (0..5).map(test_file_with_child_rows).collect::<Vec<_>>();

        let result = writer.write_scan(test_revision(), &files).unwrap();

        assert_eq!(result.transactions_committed, 1);
        assert_eq!(result.rows_written.files, 5);
        assert_eq!(result.rows_written.symbols, 15);
        assert_eq!(result.rows_written.identifiers, 10);
        assert_eq!(result.rows_written.pending_relationships, 5);
        assert_eq!(result.rows_written.type_facts, 5);
        assert_eq!(writer_prepare_metrics::file_row_inserter_prepares(), 1);
        assert_eq!(writer_prepare_metrics::child_row_inserter_prepares(), 1);
    }

    #[test]
    fn symbol_lookup_uses_current_batch_symbols_without_sqlite_query() {
        let mut connection = Connection::open_in_memory().unwrap();
        create_schema(&connection).unwrap();
        let tx = connection.transaction().unwrap();
        let requested = (0..36_241)
            .map(|index| format!("symbol-{index}"))
            .collect::<HashSet<_>>();

        let lookup = load_symbol_lookup_for_requested_ids(&tx, &requested, &requested).unwrap();

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
            load_symbol_lookup_for_requested_ids(&tx, &requested, &HashSet::new()).unwrap();

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
            parse_diagnostics: Vec::new(),
        }
    }
}
