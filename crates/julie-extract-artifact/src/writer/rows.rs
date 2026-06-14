use std::collections::HashSet;

use rusqlite::{CachedStatement, Transaction, params};

use crate::model::{ArtifactFile, ArtifactTypeArgument, FileStatus, RevisionChangeKind, RowCounts};

use super::ExistingFile;

#[cfg(test)]
pub(super) mod writer_prepare_metrics {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static FILE_ROW_INSERTER_PREPARES: AtomicUsize = AtomicUsize::new(0);
    static CHILD_ROW_INSERTER_PREPARES: AtomicUsize = AtomicUsize::new(0);

    pub(in crate::writer) fn reset() {
        FILE_ROW_INSERTER_PREPARES.store(0, Ordering::SeqCst);
        CHILD_ROW_INSERTER_PREPARES.store(0, Ordering::SeqCst);
    }

    pub(in crate::writer) fn record_file_row_inserter_prepare() {
        FILE_ROW_INSERTER_PREPARES.fetch_add(1, Ordering::SeqCst);
    }

    pub(in crate::writer) fn record_child_row_inserter_prepare() {
        CHILD_ROW_INSERTER_PREPARES.fetch_add(1, Ordering::SeqCst);
    }

    pub(in crate::writer) fn file_row_inserter_prepares() -> usize {
        FILE_ROW_INSERTER_PREPARES.load(Ordering::SeqCst)
    }

    pub(in crate::writer) fn child_row_inserter_prepares() -> usize {
        CHILD_ROW_INSERTER_PREPARES.load(Ordering::SeqCst)
    }
}

const DROP_SYMBOL_LOOKUP_TEMP_TABLE_SQL: &str =
    "DROP TABLE IF EXISTS temp.julie_symbol_lookup_requested";
const CREATE_SYMBOL_LOOKUP_TEMP_TABLE_SQL: &str = "
CREATE TEMP TABLE julie_symbol_lookup_requested (
    symbol_id TEXT PRIMARY KEY
) WITHOUT ROWID
";

pub(super) struct FileRowInserters<'tx> {
    files: CachedStatement<'tx>,
    revision_file_changes: CachedStatement<'tx>,
    symbols: CachedStatement<'tx>,
    symbol_parent_update: CachedStatement<'tx>,
}

impl<'tx> FileRowInserters<'tx> {
    pub(super) fn prepare(tx: &'tx Transaction<'_>) -> rusqlite::Result<Self> {
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
                 (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                  ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30)",
            )?,
            symbol_parent_update: tx
                .prepare_cached("UPDATE symbols SET parent_symbol_id = ?1 WHERE symbol_id = ?2")?,
        };
        #[cfg(test)]
        writer_prepare_metrics::record_file_row_inserter_prepare();
        Ok(inserters)
    }

    pub(super) fn insert_file(
        &mut self,
        revision_id: i64,
        file: &ArtifactFile,
    ) -> rusqlite::Result<()> {
        insert_file_row(&mut self.files, revision_id, file)
    }

    pub(super) fn insert_revision_file_change(
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

    pub(super) fn insert_symbols(
        &mut self,
        file: &ArtifactFile,
        parent_lookup: Option<&SymbolLookup>,
    ) -> rusqlite::Result<i64> {
        insert_symbol_rows(&mut self.symbols, file, parent_lookup)
    }

    pub(super) fn update_symbol_parents<'a>(
        &mut self,
        files: impl IntoIterator<Item = &'a ArtifactFile>,
        symbol_lookup: &SymbolLookup,
    ) -> rusqlite::Result<()> {
        update_symbol_parent_rows(&mut self.symbol_parent_update, files, symbol_lookup)
    }
}

pub(super) struct ChildRowInserters<'tx> {
    symbol_annotations: CachedStatement<'tx>,
    identifiers: CachedStatement<'tx>,
    relationships: CachedStatement<'tx>,
    pending_relationships: CachedStatement<'tx>,
    type_facts: CachedStatement<'tx>,
    type_argument_usages: CachedStatement<'tx>,
    type_arguments: CachedStatement<'tx>,
    literals: CachedStatement<'tx>,
    source_regions: CachedStatement<'tx>,
    structural_facts: CachedStatement<'tx>,
    complexity_metrics: CachedStatement<'tx>,
    parse_diagnostics: CachedStatement<'tx>,
}

impl<'tx> ChildRowInserters<'tx> {
    pub(super) fn prepare(tx: &'tx Transaction<'_>) -> rusqlite::Result<Self> {
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
            source_regions: tx.prepare_cached(
                "INSERT INTO source_regions
                 (source_region_id, file_id, path, language, kind, containing_symbol_id,
                  start_line, start_column, end_line, end_column, start_byte, end_byte,
                  metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            )?,
            structural_facts: tx.prepare_cached(
                "INSERT INTO structural_facts
                 (structural_fact_id, file_id, path, language, pattern_id, capture_name,
                  node_kind, containing_symbol_id, start_line, start_column, end_line,
                  end_column, start_byte, end_byte, confidence, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            )?,
            complexity_metrics: tx.prepare_cached(
                "INSERT INTO complexity_metrics
                 (complexity_metric_id, file_id, path, language, scope, symbol_id, algorithm_id,
                  covered_lines, covered_bytes, decision_count, loop_count, max_nesting_depth,
                  parameter_count, start_line, start_column, end_line, end_column, start_byte,
                  end_byte, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                         ?17, ?18, ?19, ?20)",
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

    pub(super) fn insert_child_rows(
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
        counts.source_regions +=
            insert_source_regions(&mut self.source_regions, file, symbol_lookup)?;
        counts.structural_facts +=
            insert_structural_facts(&mut self.structural_facts, file, symbol_lookup)?;
        counts.complexity_metrics +=
            insert_complexity_metrics(&mut self.complexity_metrics, file, symbol_lookup)?;
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

pub(super) fn insert_revision_file_change_row(
    stmt: &mut CachedStatement<'_>,
    revision_id: i64,
    file_id: &str,
    path: &str,
    change_kind: RevisionChangeKind,
) -> rusqlite::Result<i64> {
    stmt.execute(params![revision_id, file_id, path, change_kind.as_str()])?;
    Ok(1)
}

pub(super) fn is_preserved_failure(file: &ArtifactFile) -> bool {
    file.status == FileStatus::FailedPreserved
}

pub(super) fn is_preserved_failure_update(
    file: &ArtifactFile,
    existing: Option<&ExistingFile>,
) -> bool {
    is_preserved_failure(file) && existing.is_some()
}

pub(super) fn update_failed_preserved_file(
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

pub(super) fn replace_parse_diagnostics(
    tx: &Transaction<'_>,
    file: &ArtifactFile,
) -> rusqlite::Result<i64> {
    tx.execute(
        "DELETE FROM parse_diagnostics WHERE file_id = ?1 OR path = ?2",
        params![file.file_id, file.path],
    )?;
    insert_parse_diagnostics(tx, file)
}

fn insert_symbol_rows(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    parent_lookup: Option<&SymbolLookup>,
) -> rusqlite::Result<i64> {
    for symbol in &file.symbols {
        // When a lookup is supplied (spooled path), resolve parent_symbol_id inline so the
        // separate parent UPDATE pass is unnecessary. Without one (in-memory path), bind NULL and
        // let update_symbol_parents resolve it afterward — identical to the prior NULL literal.
        let parent = parent_lookup
            .and_then(|lookup| valid_symbol_id(lookup, symbol.parent_symbol_id.as_deref()));
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
            parent,
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

fn insert_source_regions(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    for region in &file.source_regions {
        stmt.execute(params![
            region.source_region_id,
            file.file_id,
            file.path,
            file.language,
            region.kind,
            valid_symbol_id(symbol_lookup, region.containing_symbol_id.as_deref()),
            region.start_line,
            region.start_column,
            region.end_line,
            region.end_column,
            region.start_byte,
            region.end_byte,
            region.metadata_json,
        ])?;
    }
    Ok(file.source_regions.len() as i64)
}

fn insert_structural_facts(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    for fact in &file.structural_facts {
        stmt.execute(params![
            fact.structural_fact_id,
            file.file_id,
            file.path,
            file.language,
            fact.pattern_id,
            fact.capture_name,
            fact.node_kind,
            valid_symbol_id(symbol_lookup, fact.containing_symbol_id.as_deref()),
            fact.start_line,
            fact.start_column,
            fact.end_line,
            fact.end_column,
            fact.start_byte,
            fact.end_byte,
            fact.confidence,
            fact.metadata_json,
        ])?;
    }
    Ok(file.structural_facts.len() as i64)
}

fn insert_complexity_metrics(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup,
) -> rusqlite::Result<i64> {
    for metric in &file.complexity_metrics {
        stmt.execute(params![
            metric.complexity_metric_id,
            file.file_id,
            file.path,
            file.language,
            metric.scope,
            valid_symbol_id(symbol_lookup, metric.symbol_id.as_deref()),
            metric.algorithm_id,
            metric.covered_lines,
            metric.covered_bytes,
            metric.decision_count,
            metric.loop_count,
            metric.max_nesting_depth,
            metric.parameter_count,
            metric.start_line,
            metric.start_column,
            metric.end_line,
            metric.end_column,
            metric.start_byte,
            metric.end_byte,
            metric.metadata_json,
        ])?;
    }
    Ok(file.complexity_metrics.len() as i64)
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
pub(super) struct SymbolLookup {
    pub(super) ids: HashSet<String>,
}

impl SymbolLookup {
    fn contains(&self, symbol_id: &str) -> bool {
        self.ids.contains(symbol_id)
    }
}

pub(super) fn load_symbol_lookup<'a>(
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

pub(super) fn collect_file_symbol_ids(file: &ArtifactFile, ids: &mut HashSet<String>) {
    ids.extend(file.symbols.iter().map(|symbol| symbol.symbol_id.clone()));
}

pub(super) fn collect_requested_symbol_ids(file: &ArtifactFile, requested: &mut HashSet<String>) {
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
    for region in &file.source_regions {
        if let Some(containing_symbol_id) = region.containing_symbol_id.as_deref() {
            requested.insert(containing_symbol_id.to_string());
        }
    }
    for fact in &file.structural_facts {
        if let Some(containing_symbol_id) = fact.containing_symbol_id.as_deref() {
            requested.insert(containing_symbol_id.to_string());
        }
    }
    for metric in &file.complexity_metrics {
        if let Some(symbol_id) = metric.symbol_id.as_deref() {
            requested.insert(symbol_id.to_string());
        }
    }
}

pub(super) fn load_symbol_lookup_for_requested_ids(
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
