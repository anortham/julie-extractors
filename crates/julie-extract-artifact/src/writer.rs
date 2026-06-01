use std::{collections::HashSet, path::Path};

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::metadata::{ArtifactMetadata, initialize_metadata};
use crate::model::{
    ArtifactFile, ArtifactTypeArgument, FileStatus, RevisionChangeKind, RevisionInput, RowCounts,
    WriteMode, WriteOperation, WriteResult,
};
use crate::schema::{EXTRACT_CONTRACT_VERSION, SQLITE_SCHEMA_VERSION, create_schema};

pub type ArtifactWriteResult<T> = Result<T, ArtifactWriteError>;

#[derive(Debug)]
pub enum ArtifactWriteError {
    Sqlite(rusqlite::Error),
    DataLossGuard {
        path: String,
        existing_symbols: i64,
        reason: String,
    },
}

impl std::fmt::Display for ArtifactWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactWriteError::Sqlite(error) => write!(f, "{error}"),
            ArtifactWriteError::DataLossGuard {
                path,
                existing_symbols,
                reason,
            } => write!(
                f,
                "refusing to replace {path}: {reason}; existing symbol rows: {existing_symbols}"
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

    pub fn write_scan(
        &mut self,
        revision: RevisionInput,
        files: &[ArtifactFile],
    ) -> ArtifactWriteResult<WriteResult> {
        debug_assert_eq!(revision.operation, WriteOperation::Scan);
        self.write_scan_snapshot(revision, files)
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

        let mut row_counts = RowCounts::default();
        row_counts.revision_file_changes = insert_revision_file_change(
            &tx,
            revision_id,
            &existing.file_id,
            path,
            RevisionChangeKind::Deleted,
        )?;
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
            files_changed: planned.len() + deleted.len(),
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
        FileStatus::Indexed if file.symbols.is_empty() => Some("parser returned zero symbols"),
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

fn insert_symbols(tx: &Transaction<'_>, file: &ArtifactFile) -> rusqlite::Result<i64> {
    let mut stmt = tx.prepare(
        "INSERT INTO symbols
         (symbol_id, file_id, path, language, name, kind, signature, doc_comment, visibility,
          parent_symbol_id, start_line, start_column, end_line, end_column, start_byte, end_byte,
          body_start_line, body_start_column, body_end_line, body_end_column, body_start_byte,
          body_end_byte, body_hash, semantic_group, confidence, content_type, metadata_json)
         VALUES
         (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
          ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
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
            if let Some(parent_symbol_id) = symbol.parent_symbol_id.as_deref() {
                if symbol_lookup.contains(parent_symbol_id) {
                    parent_update.execute(params![parent_symbol_id, symbol.symbol_id])?;
                }
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

    if requested.is_empty() {
        return Ok(SymbolLookup::default());
    }

    let bind_marks = std::iter::repeat("?")
        .take(requested.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT symbol_id FROM symbols WHERE symbol_id IN ({bind_marks})");
    let mut stmt = tx.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(requested.iter().map(String::as_str)),
        |row| row.get::<_, String>(0),
    )?;

    let mut ids = HashSet::new();
    for row in rows {
        ids.insert(row?);
    }

    Ok(SymbolLookup { ids })
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
