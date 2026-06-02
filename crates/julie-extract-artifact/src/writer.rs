use std::{
    collections::HashSet,
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OptionalExtension, Transaction, limits::Limit, params};

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
const SQLITE_PREPARE_SAFE_VARIABLE_LIMIT: usize = 32_000;

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

        for (file, _, change_kind) in &planned {
            insert_file(&tx, revision_id, file)?;
            row_counts.files += 1;
            row_counts.revision_file_changes += insert_revision_file_change(
                &tx,
                revision_id,
                &file.file_id,
                &file.path,
                *change_kind,
            )?;
        }

        for (file, _, _) in &planned {
            row_counts.symbols += insert_symbols(&tx, file)?;
        }
        let symbol_lookup = load_symbol_lookup(&tx, planned.iter().map(|(file, _, _)| *file))?;
        update_symbol_parents(
            &tx,
            planned.iter().map(|(file, _, _)| *file),
            &symbol_lookup,
        )?;

        for (file, _, _) in &planned {
            insert_child_rows(&tx, file, &symbol_lookup, &mut row_counts)?;
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

        for existing in &deleted {
            delete_file_rows(&tx, &existing.file_id, &existing.path)?;
            row_counts.revision_file_changes += insert_revision_file_change(
                &tx,
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
                insert_file(&tx, revision_id, file)?;
                row_counts.files += 1;
            }
            row_counts.revision_file_changes += insert_revision_file_change(
                &tx,
                revision_id,
                &file.file_id,
                &file.path,
                *change_kind,
            )?;
        }

        let rewritten_files = planned
            .iter()
            .filter(|(file, existing, _)| !is_preserved_failure_update(file, existing.as_ref()))
            .map(|(file, _, _)| *file)
            .collect::<Vec<_>>();

        for file in &rewritten_files {
            row_counts.symbols += insert_symbols(&tx, file)?;
        }
        let symbol_lookup = load_symbol_lookup(&tx, rewritten_files.iter().copied())?;
        update_symbol_parents(&tx, rewritten_files.iter().copied(), &symbol_lookup)?;

        for file in &rewritten_files {
            insert_child_rows(&tx, file, &symbol_lookup, &mut row_counts)?;
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
        let mut planned_paths = HashSet::new();
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
            planned_paths.insert(file.path);
        }

        let deleted = load_existing_files(&tx)?
            .into_iter()
            .filter(|existing| !snapshot_paths.contains(existing.path.as_str()))
            .collect::<Vec<_>>();

        if planned_paths.is_empty() && deleted.is_empty() {
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

        for file in spool.iter()? {
            let file = file?;
            if !planned_paths.contains(file.path.as_str()) {
                continue;
            }

            let existing = load_existing_file(&tx, &file.path)?;
            let change_kind = file_change_kind(&file, existing.as_ref());
            if is_preserved_failure_update(&file, existing.as_ref()) {
                if let Some(existing) = existing.as_ref() {
                    update_failed_preserved_file(&tx, revision_id, &file, &existing.file_id)?;
                    row_counts.files += 1;
                    row_counts.parse_diagnostics += replace_parse_diagnostics(&tx, &file)?;
                }
            } else {
                if let Some(existing) = existing.as_ref() {
                    delete_file_rows(&tx, &existing.file_id, &file.path)?;
                }
                insert_file(&tx, revision_id, &file)?;
                row_counts.files += 1;
                row_counts.symbols += insert_symbols(&tx, &file)?;
                rewritten_file_ids.insert(file.file_id.clone());
            }
            row_counts.revision_file_changes += insert_revision_file_change(
                &tx,
                revision_id,
                &file.file_id,
                &file.path,
                change_kind,
            )?;
        }

        for existing in &deleted {
            delete_file_rows(&tx, &existing.file_id, &existing.path)?;
            row_counts.revision_file_changes += insert_revision_file_change(
                &tx,
                revision_id,
                &existing.file_id,
                &existing.path,
                RevisionChangeKind::Deleted,
            )?;
        }

        let mut requested_symbol_ids = HashSet::new();
        for file in spool.iter()? {
            let file = file?;
            if rewritten_file_ids.contains(&file.file_id) {
                collect_requested_symbol_ids(&file, &mut requested_symbol_ids);
            }
        }
        let symbol_lookup = load_symbol_lookup_for_requested_ids(&tx, &requested_symbol_ids)?;

        for file in spool.iter()? {
            let file = file?;
            if !rewritten_file_ids.contains(&file.file_id) {
                continue;
            }
            update_symbol_parents(&tx, std::iter::once(&file), &symbol_lookup)?;
            insert_child_rows(&tx, &file, &symbol_lookup, &mut row_counts)?;
        }

        update_revision_counts(&tx, revision_id, &row_counts)?;
        tx.commit()?;

        Ok(WriteResult {
            revision_id: Some(revision_id),
            files_changed: planned_paths.len() + deleted.len(),
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
    tx.query_row(
        "SELECT file_id, content_hash FROM files WHERE path = ?1",
        [path],
        |row| {
            Ok(ExistingFile {
                file_id: row.get(0)?,
                path: path.to_string(),
                content_hash: row.get(1)?,
            })
        },
    )
    .optional()
}

fn load_existing_files(tx: &Transaction<'_>) -> rusqlite::Result<Vec<ExistingFile>> {
    let mut statement = tx.prepare("SELECT file_id, path, content_hash FROM files")?;
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
    tx.execute(
        "INSERT INTO revision_file_changes (revision_id, file_id, path, change_kind)
         VALUES (?1, ?2, ?3, ?4)",
        params![revision_id, file_id, path, change_kind.as_str()],
    )?;
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

fn insert_child_rows(
    tx: &Transaction<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
    counts: &mut RowCounts,
) -> rusqlite::Result<()> {
    counts.symbol_annotations += insert_symbol_annotations(tx, file, symbol_lookup)?;
    counts.identifiers += insert_identifiers(tx, file, symbol_lookup)?;
    let identifier_lookup = IdentifierLookup::from_file(file);
    counts.relationships += insert_relationships(tx, file, symbol_lookup)?;
    counts.pending_relationships += insert_pending_relationships(tx, file, symbol_lookup)?;
    counts.type_facts += insert_type_facts(tx, file, symbol_lookup)?;
    counts.type_argument_usages += insert_type_argument_usages(tx, file, &identifier_lookup)?;
    let usage_lookup = TypeArgumentUsageLookup::from_file(file, &identifier_lookup);
    counts.type_arguments += insert_type_arguments(tx, &file.type_arguments, &usage_lookup)?;
    counts.literals += insert_literals(tx, file, symbol_lookup)?;
    counts.parse_diagnostics += insert_parse_diagnostics(tx, file)?;
    Ok(())
}

fn insert_file(
    tx: &Transaction<'_>,
    revision_id: i64,
    file: &ArtifactFile,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO files
         (file_id, path, language, content_hash, content_bytes, line_count, indexed_at,
          last_revision_id, status, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
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
        ],
    )?;
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

fn insert_symbols(tx: &Transaction<'_>, file: &ArtifactFile) -> rusqlite::Result<i64> {
    let mut stmt = tx.prepare(
        "INSERT INTO symbols
	     (symbol_id, file_id, path, language, name, kind, signature, doc_comment, visibility,
	      parent_symbol_id, start_line, start_column, end_line, end_column, start_byte, end_byte,
	      body_start_line, body_start_column, body_end_line, body_end_column, body_start_byte,
	      body_end_byte, body_hash, semantic_group, confidence, content_type, is_test,
	      test_container, test_lifecycle, metadata_json)
	     VALUES
	     (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
	      ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29)",
    )?;
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
    drop(stmt);

    Ok(file.symbols.len() as i64)
}

fn update_symbol_parents<'a>(
    tx: &Transaction<'_>,
    files: impl IntoIterator<Item = &'a ArtifactFile>,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<()> {
    let mut parent_update =
        tx.prepare("UPDATE symbols SET parent_symbol_id = ?1 WHERE symbol_id = ?2")?;
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
    tx: &Transaction<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    let mut stmt = tx.prepare(
        "INSERT INTO symbol_annotations
         (annotation_id, symbol_id, annotation, annotation_key, raw_text, carrier, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
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
    tx: &Transaction<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    let mut stmt = tx.prepare(
        "INSERT INTO identifiers
         (identifier_id, file_id, path, language, name, kind, containing_symbol_id,
          target_symbol_id, start_line, start_column, end_line, end_column, start_byte, end_byte,
          confidence, code_context, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
    )?;
    for identifier in &file.identifiers {
        stmt.execute(params![
            identifier.identifier_id,
            file.file_id,
            file.path,
            file.language,
            identifier.name,
            identifier.kind,
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
    drop(stmt);

    let mut ref_update = tx.prepare(
        "UPDATE identifiers
         SET containing_symbol_id = ?1, target_symbol_id = ?2
         WHERE identifier_id = ?3",
    )?;
    for identifier in &file.identifiers {
        let containing = valid_symbol_id(symbol_lookup, identifier.containing_symbol_id.as_deref());
        let target = valid_symbol_id(symbol_lookup, identifier.target_symbol_id.as_deref());
        if containing.is_some() || target.is_some() {
            ref_update.execute(params![containing, target, identifier.identifier_id])?;
        }
    }

    Ok(file.identifiers.len() as i64)
}

fn insert_relationships(
    tx: &Transaction<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    let mut stmt = tx.prepare(
        "INSERT INTO relationships
         (relationship_id, from_symbol_id, to_symbol_id, file_id, path, kind, start_line,
          start_column, end_line, end_column, start_byte, end_byte, confidence, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
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
    tx: &Transaction<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    let mut stmt = tx.prepare(
        "INSERT INTO pending_relationships
         (pending_relationship_id, from_symbol_id, caller_scope_symbol_id, file_id, path, kind,
          target_display_name, target_terminal_name, target_receiver, target_namespace_json,
          target_import_context, start_line, start_column, end_line, end_column, start_byte,
          end_byte, confidence, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                 ?18, ?19)",
    )?;
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
    tx: &Transaction<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    let mut stmt = tx.prepare(
        "INSERT INTO type_facts
         (type_fact_id, symbol_id, language, resolved_type, generic_params_json,
          constraints_json, is_inferred, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
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
    tx: &Transaction<'_>,
    file: &ArtifactFile,
    identifier_lookup: &IdentifierLookup,
) -> rusqlite::Result<i64> {
    let mut stmt = tx.prepare(
        "INSERT INTO type_argument_usages
         (usage_id, identifier_id, file_id, path, language, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
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
    tx: &Transaction<'_>,
    arguments: &[ArtifactTypeArgument],
    usage_lookup: &TypeArgumentUsageLookup,
) -> rusqlite::Result<i64> {
    let mut stmt = tx.prepare(
        "INSERT INTO type_arguments
         (type_argument_id, usage_id, parent_type_argument_id, ordinal, type_name)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
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
    tx: &Transaction<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    let mut stmt = tx.prepare(
        "INSERT INTO literals
         (literal_id, file_id, path, language, literal_text, kind, carrier, arg_position,
          containing_symbol_id, start_line, start_column, end_line, end_column, start_byte,
          end_byte, confidence, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
    )?;
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
    let mut stmt = tx.prepare(
        "INSERT INTO parse_diagnostics
         (diagnostic_id, file_id, path, language, kind, message, start_line, start_column,
          end_line, end_column, start_byte, end_byte, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;
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
    for file in files {
        collect_requested_symbol_ids(file, &mut requested);
    }

    load_symbol_lookup_for_requested_ids(tx, &requested)
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
) -> rusqlite::Result<SymbolLookup> {
    if requested.is_empty() {
        return Ok(SymbolLookup::default());
    }

    let mut ids = HashSet::new();
    let bind_limit = tx.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER)? as usize;
    let chunk_size = symbol_lookup_chunk_size(bind_limit);
    let requested = requested.iter().map(String::as_str).collect::<Vec<_>>();
    for chunk in requested.chunks(chunk_size) {
        let bind_marks = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("SELECT symbol_id FROM symbols WHERE symbol_id IN ({bind_marks})");
        let mut stmt = tx.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter().copied()), |row| {
            row.get::<_, String>(0)
        })?;

        for row in rows {
            ids.insert(row?);
        }
    }

    Ok(SymbolLookup { ids })
}

fn symbol_lookup_chunk_size(reported_bind_limit: usize) -> usize {
    reported_bind_limit.clamp(1, SQLITE_PREPARE_SAFE_VARIABLE_LIMIT)
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

    #[test]
    fn symbol_lookup_chunks_above_sqlite_prepare_variable_limit() {
        let mut connection = Connection::open_in_memory().unwrap();
        create_schema(&connection).unwrap();
        let tx = connection.transaction().unwrap();
        let requested = (0..36_241)
            .map(|index| format!("symbol-{index}"))
            .collect::<HashSet<_>>();

        let lookup = load_symbol_lookup_for_requested_ids(&tx, &requested).unwrap();

        assert!(lookup.ids.is_empty());
    }

    #[test]
    fn symbol_lookup_chunk_size_clamps_reported_limit_to_prepare_safe_bound() {
        assert_eq!(symbol_lookup_chunk_size(0), 1);
        assert_eq!(symbol_lookup_chunk_size(64), 64);
        assert_eq!(symbol_lookup_chunk_size(500_000), 32_000);
    }
}
