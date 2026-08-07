use std::collections::{HashMap, HashSet};

use rusqlite::{CachedStatement, ToSql, Transaction, limits::Limit, params, params_from_iter};

use crate::model::{
    ArtifactComplexityMetric, ArtifactFile, ArtifactPendingRelationship, ArtifactRelationship,
    ArtifactSourceRegion, ArtifactStructuralFact, ArtifactTypeArgument, FileStatus,
    ReferenceSiteConflictFile, ReferenceSiteConflictSite, ReferenceSiteConflicts,
    RevisionChangeKind, RowCounts,
};

use super::ExistingFile;

/// How many conflicting files and per-file sites the write result samples for the
/// report. A pathological producer must not turn one warning per site into a
/// multi-megabyte report; `total`/`files_affected` still carry the true counts.
const MAX_REPORTED_CONFLICT_FILES: usize = 32;
const MAX_REPORTED_CONFLICT_SITES_PER_FILE: usize = 5;

/// Max `file_id` placeholders per chunk when collecting existing symbol names.
/// The effective chunk is capped at the connection's runtime variable limit so a
/// low limit never blows the host-parameter budget.
const SYMBOL_NAME_QUERY_MAX_CHUNK: usize = 500;

/// Collect the `symbols.name` values currently stored under any of `file_ids`.
///
/// Seeds the resolution hook's `touched_symbol_names` with the OLD names of files
/// about to be deleted or rewritten: the incoming file set cannot supply a name
/// that a rewrite removed or a delete dropped, so these must be read from the DB
/// **before** `delete_file_rows` runs (design §"Incremental correctness",
/// round-3 note).
pub(super) fn collect_existing_symbol_names(
    tx: &Transaction<'_>,
    file_ids: &[&str],
) -> rusqlite::Result<HashSet<String>> {
    let mut names = HashSet::new();
    if file_ids.is_empty() {
        return Ok(names);
    }
    let variable_limit = tx.limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER)?.max(1) as usize;
    let chunk_size = variable_limit.clamp(1, SYMBOL_NAME_QUERY_MAX_CHUNK);
    for chunk in file_ids.chunks(chunk_size) {
        let placeholders = vec!["?"; chunk.len()].join(", ");
        let sql = format!("SELECT name FROM symbols WHERE file_id IN ({placeholders})");
        let mut stmt = tx.prepare_cached(&sql)?;
        let rows = stmt.query_map(params_from_iter(chunk.iter()), |row| {
            row.get::<_, String>(0)
        })?;
        for name in rows {
            names.insert(name?);
        }
    }
    Ok(names)
}

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

/// Maximum rows per multi-row `INSERT` for `structural_facts` (16 columns per
/// row). The effective chunk is `min(this, variable_limit / 16)`, so a low
/// runtime limit (the writer-contract test sets 64) is always honored. Only
/// files with at least the effective chunk size take the multi-row path;
/// smaller files fall back to the cached single-row statement, so typical
/// small files pay no overhead.
const STRUCTURAL_FACT_MAX_CHUNK: usize = 256;

/// Same shape as `STRUCTURAL_FACT_MAX_CHUNK` for `source_regions` (13 columns
/// per row).
const SOURCE_REGION_MAX_CHUNK: usize = 256;

/// Same shape as `STRUCTURAL_FACT_MAX_CHUNK` for `complexity_metrics` (20
/// columns per row).
const COMPLEXITY_METRIC_MAX_CHUNK: usize = 256;

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
        parents: ParentBinding,
    ) -> rusqlite::Result<i64> {
        insert_symbol_rows(&mut self.symbols, file, parents)
    }

    pub(super) fn update_symbol_parents<'a>(
        &mut self,
        files: impl IntoIterator<Item = &'a ArtifactFile>,
    ) -> rusqlite::Result<()> {
        update_symbol_parent_rows(&mut self.symbol_parent_update, files)
    }
}

pub(super) struct ChildRowInserters<'tx> {
    symbol_annotations: CachedStatement<'tx>,
    reference_sites: CachedStatement<'tx>,
    identifiers: CachedStatement<'tx>,
    relationships: CachedStatement<'tx>,
    pending_relationships: CachedStatement<'tx>,
    type_facts: CachedStatement<'tx>,
    type_argument_usages: CachedStatement<'tx>,
    type_arguments: CachedStatement<'tx>,
    literals: CachedStatement<'tx>,
    source_regions: CachedStatement<'tx>,
    source_regions_multi: CachedStatement<'tx>,
    source_region_chunk: usize,
    structural_facts: CachedStatement<'tx>,
    structural_facts_multi: CachedStatement<'tx>,
    structural_fact_chunk: usize,
    structural_fact_ids: HashSet<String>,
    complexity_metrics: CachedStatement<'tx>,
    complexity_metrics_multi: CachedStatement<'tx>,
    complexity_metric_chunk: usize,
    parse_diagnostics: CachedStatement<'tx>,
    reference_site_conflicts: ReferenceSiteConflicts,
}

impl<'tx> ChildRowInserters<'tx> {
    pub(super) fn prepare(tx: &'tx Transaction<'_>) -> rusqlite::Result<Self> {
        let structural_fact_chunk = structural_fact_chunk_size(tx)?;
        let source_region_chunk = source_region_chunk_size(tx)?;
        let complexity_metric_chunk = complexity_metric_chunk_size(tx)?;
        let inserters = Self {
            symbol_annotations: tx.prepare_cached(
                "INSERT INTO symbol_annotations
                 (annotation_id, symbol_id, annotation, annotation_key, raw_text, carrier,
                  metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?,
            reference_sites: tx.prepare_cached(
                "INSERT OR IGNORE INTO reference_sites
                 (reference_site_id, file_id, path, language, containing_symbol_id,
                  start_line, start_column, end_line, end_column, start_byte, end_byte,
                  is_exact, provenance)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            )?,
            identifiers: tx.prepare_cached(
                "INSERT INTO identifiers
                 (identifier_id, reference_site_id, file_id, path, language, name, kind,
                  containing_symbol_id, target_symbol_id, start_line, start_column, end_line,
                  end_column, start_byte, end_byte, confidence, code_context, metadata_json)
                 VALUES
                 (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                  ?17, ?18)",
            )?,
            relationships: tx.prepare_cached(
                "INSERT INTO relationships
                 (relationship_id, reference_site_id, from_symbol_id, to_symbol_id, file_id, path,
                  kind, start_line, start_column, end_line, end_column, start_byte, end_byte,
                  confidence, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            )?,
            pending_relationships: tx.prepare_cached(
                "INSERT INTO pending_relationships
                 (pending_relationship_id, reference_site_id, from_symbol_id,
                  caller_scope_symbol_id, file_id, path, kind, target_display_name,
                  target_terminal_name, target_receiver,
                  target_namespace_json, target_import_context, start_line, start_column,
                  end_line, end_column, start_byte, end_byte, confidence, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20)",
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
            source_regions_multi: tx
                .prepare_cached(&source_regions_multi_insert_sql(source_region_chunk))?,
            source_region_chunk,
            structural_facts: tx.prepare_cached(
                "INSERT INTO structural_facts
                 (structural_fact_id, file_id, path, language, pattern_id, capture_name,
                  node_kind, containing_symbol_id, start_line, start_column, end_line,
                  end_column, start_byte, end_byte, confidence, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            )?,
            structural_facts_multi: tx
                .prepare_cached(&structural_facts_multi_insert_sql(structural_fact_chunk))?,
            structural_fact_chunk,
            structural_fact_ids: HashSet::new(),
            complexity_metrics: tx.prepare_cached(
                "INSERT INTO complexity_metrics
                 (complexity_metric_id, file_id, path, language, scope, symbol_id, algorithm_id,
                  covered_lines, covered_bytes, decision_count, loop_count, max_nesting_depth,
                  parameter_count, start_line, start_column, end_line, end_column, start_byte,
                  end_byte, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                         ?17, ?18, ?19, ?20)",
            )?,
            complexity_metrics_multi: tx.prepare_cached(&complexity_metrics_multi_insert_sql(
                complexity_metric_chunk,
            ))?,
            complexity_metric_chunk,
            parse_diagnostics: tx.prepare_cached(
                "INSERT INTO parse_diagnostics
                 (diagnostic_id, file_id, path, language, kind, message, start_line, start_column,
                  end_line, end_column, start_byte, end_byte, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            )?,
            reference_site_conflicts: ReferenceSiteConflicts::default(),
        };
        #[cfg(test)]
        writer_prepare_metrics::record_child_row_inserter_prepare();
        Ok(inserters)
    }

    pub(super) fn take_reference_site_conflicts(&mut self) -> ReferenceSiteConflicts {
        std::mem::take(&mut self.reference_site_conflicts)
    }

    pub(super) fn insert_child_rows(
        &mut self,
        file: &ArtifactFile,
        counts: &mut RowCounts,
    ) -> rusqlite::Result<()> {
        let symbol_lookup = &SymbolLookup::from_file(file);
        counts.symbol_annotations +=
            insert_symbol_annotations(&mut self.symbol_annotations, file, symbol_lookup)?;
        counts.reference_sites += insert_reference_sites(
            &mut self.reference_sites,
            file,
            symbol_lookup,
            &mut self.reference_site_conflicts,
        )?;
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
        counts.source_regions += insert_source_regions(
            &mut self.source_regions,
            &mut self.source_regions_multi,
            self.source_region_chunk,
            file,
            symbol_lookup,
        )?;
        counts.structural_facts += insert_structural_facts(
            &mut self.structural_facts,
            &mut self.structural_facts_multi,
            self.structural_fact_chunk,
            &mut self.structural_fact_ids,
            file,
            symbol_lookup,
        )?;
        counts.complexity_metrics += insert_complexity_metrics(
            &mut self.complexity_metrics,
            &mut self.complexity_metrics_multi,
            self.complexity_metric_chunk,
            file,
            symbol_lookup,
        )?;
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
    parents: ParentBinding,
) -> rusqlite::Result<i64> {
    let parent_lookup = match parents {
        ParentBinding::ResolvedInFile => Some(SymbolLookup::from_file(file)),
        ParentBinding::Deferred => None,
    };
    for symbol in &file.symbols {
        let parent = parent_lookup
            .as_ref()
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
) -> rusqlite::Result<()> {
    for file in files {
        let symbol_lookup = SymbolLookup::from_file(file);
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
    symbol_lookup: &SymbolLookup<'_>,
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

/// The reference-site columns the identity guard compares, minus the ones that
/// are constant for every row of one file (`file_id`, `path`, `language`).
#[derive(PartialEq, Eq)]
struct SitePayload {
    containing_symbol_id: Option<String>,
    start_line: Option<i64>,
    start_column: Option<i64>,
    end_line: Option<i64>,
    end_column: Option<i64>,
    start_byte: Option<i64>,
    end_byte: Option<i64>,
    is_exact: bool,
    provenance: &'static str,
}

impl SitePayload {
    fn diverging_fields(&self, other: &Self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        let mut check = |diverges: bool, name: &'static str| {
            if diverges {
                fields.push(name);
            }
        };
        check(
            self.containing_symbol_id != other.containing_symbol_id,
            "containing_symbol_id",
        );
        check(self.start_line != other.start_line, "start_line");
        check(self.start_column != other.start_column, "start_column");
        check(self.end_line != other.end_line, "end_line");
        check(self.end_column != other.end_column, "end_column");
        check(self.start_byte != other.start_byte, "start_byte");
        check(self.end_byte != other.end_byte, "end_byte");
        check(self.is_exact != other.is_exact, "is_exact");
        check(self.provenance != other.provenance, "provenance");
        fields
    }
}

/// First-write-wins arbitration for one file's reference sites.
///
/// Returns `true` when the row must reach SQLite. A repeat of an identical
/// payload is skipped (the statement's `INSERT OR IGNORE` would drop it anyway);
/// a divergent repeat is skipped AND recorded, so the disagreement is reported
/// instead of aborting the import.
fn claim_reference_site<'a>(
    seen: &mut HashMap<&'a str, SitePayload>,
    conflicts: &mut ReferenceSiteConflicts,
    file: &ArtifactFile,
    reference_site_id: &'a str,
    payload: SitePayload,
) -> bool {
    let Some(existing) = seen.get(reference_site_id) else {
        seen.insert(reference_site_id, payload);
        return true;
    };
    let fields = existing.diverging_fields(&payload);
    if !fields.is_empty() {
        record_reference_site_conflict(conflicts, file, reference_site_id, fields);
    }
    false
}

fn record_reference_site_conflict(
    conflicts: &mut ReferenceSiteConflicts,
    file: &ArtifactFile,
    reference_site_id: &str,
    fields: Vec<&'static str>,
) {
    conflicts.total += 1;
    match conflicts
        .files
        .iter_mut()
        .find(|entry| entry.path == file.path)
    {
        Some(entry) => {
            entry.conflicts += 1;
            if entry.sites.len() < MAX_REPORTED_CONFLICT_SITES_PER_FILE {
                entry.sites.push(ReferenceSiteConflictSite {
                    reference_site_id: reference_site_id.to_string(),
                    fields,
                });
            }
        }
        None => {
            conflicts.files_affected += 1;
            if conflicts.files.len() < MAX_REPORTED_CONFLICT_FILES {
                conflicts.files.push(ReferenceSiteConflictFile {
                    path: file.path.clone(),
                    language: file.language.clone(),
                    conflicts: 1,
                    sites: vec![ReferenceSiteConflictSite {
                        reference_site_id: reference_site_id.to_string(),
                        fields,
                    }],
                });
            }
        }
    }
}

fn insert_reference_sites(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup<'_>,
    conflicts: &mut ReferenceSiteConflicts,
) -> rusqlite::Result<i64> {
    let mut inserted = 0;
    let mut seen: HashMap<&str, SitePayload> = HashMap::new();

    for identifier in &file.identifiers {
        let span = identifier.site_is_exact.then_some((
            identifier.start_line,
            identifier.start_column,
            identifier.end_line,
            identifier.end_column,
            identifier.start_byte,
            identifier.end_byte,
        ));
        let payload = SitePayload {
            containing_symbol_id: valid_symbol_id(
                symbol_lookup,
                identifier.containing_symbol_id.as_deref(),
            )
            .map(ToOwned::to_owned),
            start_line: span.map(|span| span.0),
            start_column: span.map(|span| span.1),
            end_line: span.map(|span| span.2),
            end_column: span.map(|span| span.3),
            start_byte: span.map(|span| span.4),
            end_byte: span.map(|span| span.5),
            is_exact: identifier.site_is_exact,
            provenance: identifier.site_provenance.as_str(),
        };
        if !claim_reference_site(
            &mut seen,
            conflicts,
            file,
            identifier.reference_site_id.as_str(),
            payload,
        ) {
            continue;
        }
        inserted += stmt.execute(params![
            identifier.reference_site_id,
            file.file_id,
            file.path,
            file.language,
            valid_symbol_id(symbol_lookup, identifier.containing_symbol_id.as_deref()),
            span.map(|span| span.0),
            span.map(|span| span.1),
            span.map(|span| span.2),
            span.map(|span| span.3),
            span.map(|span| span.4),
            span.map(|span| span.5),
            identifier.site_is_exact,
            identifier.site_provenance.as_str(),
        ])? as i64;
    }

    for relationship in &file.relationships {
        if !relationship_is_insertable(relationship, symbol_lookup) {
            continue;
        }
        let span = relationship.site_is_exact.then_some((
            relationship.start_line,
            relationship.start_column,
            relationship.end_line,
            relationship.end_column,
            relationship.start_byte,
            relationship.end_byte,
        ));
        let payload = SitePayload {
            containing_symbol_id: valid_symbol_id(
                symbol_lookup,
                Some(relationship.from_symbol_id.as_str()),
            )
            .map(ToOwned::to_owned),
            start_line: span.and_then(|span| span.0),
            start_column: span.and_then(|span| span.1),
            end_line: span.and_then(|span| span.2),
            end_column: span.and_then(|span| span.3),
            start_byte: span.and_then(|span| span.4),
            end_byte: span.and_then(|span| span.5),
            is_exact: relationship.site_is_exact,
            provenance: relationship.site_provenance.as_str(),
        };
        if !claim_reference_site(
            &mut seen,
            conflicts,
            file,
            relationship.reference_site_id.as_str(),
            payload,
        ) {
            continue;
        }
        inserted += stmt.execute(params![
            relationship.reference_site_id,
            file.file_id,
            file.path,
            file.language,
            valid_symbol_id(symbol_lookup, Some(relationship.from_symbol_id.as_str())),
            span.and_then(|span| span.0),
            span.and_then(|span| span.1),
            span.and_then(|span| span.2),
            span.and_then(|span| span.3),
            span.and_then(|span| span.4),
            span.and_then(|span| span.5),
            relationship.site_is_exact,
            relationship.site_provenance.as_str(),
        ])? as i64;
    }

    for pending in &file.pending_relationships {
        if !pending_relationship_is_insertable(pending, symbol_lookup) {
            continue;
        }
        let containing_symbol_id = pending
            .caller_scope_symbol_id
            .as_deref()
            .or(Some(pending.from_symbol_id.as_str()));
        let span = pending.site_is_exact.then_some((
            pending.start_line,
            pending.start_column,
            pending.end_line,
            pending.end_column,
            pending.start_byte,
            pending.end_byte,
        ));
        let payload = SitePayload {
            containing_symbol_id: valid_symbol_id(symbol_lookup, containing_symbol_id)
                .map(ToOwned::to_owned),
            start_line: span.map(|span| span.0),
            start_column: span.and_then(|span| span.1),
            end_line: span.and_then(|span| span.2),
            end_column: span.and_then(|span| span.3),
            start_byte: span.and_then(|span| span.4),
            end_byte: span.and_then(|span| span.5),
            is_exact: pending.site_is_exact,
            provenance: pending.site_provenance.as_str(),
        };
        if !claim_reference_site(
            &mut seen,
            conflicts,
            file,
            pending.reference_site_id.as_str(),
            payload,
        ) {
            continue;
        }
        inserted += stmt.execute(params![
            pending.reference_site_id,
            file.file_id,
            file.path,
            file.language,
            valid_symbol_id(symbol_lookup, containing_symbol_id),
            span.map(|span| span.0),
            span.and_then(|span| span.1),
            span.and_then(|span| span.2),
            span.and_then(|span| span.3),
            span.and_then(|span| span.4),
            span.and_then(|span| span.5),
            pending.site_is_exact,
            pending.site_provenance.as_str(),
        ])? as i64;
    }

    Ok(inserted)
}

fn insert_identifiers(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup<'_>,
) -> rusqlite::Result<i64> {
    // Every symbol row of this file exists before its child rows are written, so the FKs bind
    // inline; the second UPDATE pass older revisions ran cost one extra statement per identifier
    // plus double index maintenance on idx_identifiers_containing/target.
    for identifier in &file.identifiers {
        let containing = valid_symbol_id(symbol_lookup, identifier.containing_symbol_id.as_deref());
        let target = valid_symbol_id(symbol_lookup, identifier.target_symbol_id.as_deref());
        stmt.execute(params![
            identifier.identifier_id,
            identifier.reference_site_id,
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
    symbol_lookup: &SymbolLookup<'_>,
) -> rusqlite::Result<i64> {
    let mut inserted = 0;
    for relationship in &file.relationships {
        if !relationship_is_insertable(relationship, symbol_lookup) {
            continue;
        }
        stmt.execute(params![
            relationship.relationship_id,
            relationship.reference_site_id,
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
    symbol_lookup: &SymbolLookup<'_>,
) -> rusqlite::Result<i64> {
    let mut inserted = 0;
    for pending in &file.pending_relationships {
        if !pending_relationship_is_insertable(pending, symbol_lookup) {
            continue;
        }
        stmt.execute(params![
            pending.pending_relationship_id,
            pending.reference_site_id,
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

fn relationship_is_insertable(
    relationship: &ArtifactRelationship,
    symbol_lookup: &SymbolLookup<'_>,
) -> bool {
    symbol_lookup.contains(&relationship.from_symbol_id)
        && symbol_lookup.contains(&relationship.to_symbol_id)
}

fn pending_relationship_is_insertable(
    pending: &ArtifactPendingRelationship,
    symbol_lookup: &SymbolLookup<'_>,
) -> bool {
    symbol_lookup.contains(&pending.from_symbol_id)
}

fn insert_type_facts(
    stmt: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup<'_>,
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
    symbol_lookup: &SymbolLookup<'_>,
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
    single: &mut CachedStatement<'_>,
    multi: &mut CachedStatement<'_>,
    chunk_size: usize,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup<'_>,
) -> rusqlite::Result<i64> {
    let mut inserted = 0i64;
    let mut chunk_rows: Vec<&ArtifactSourceRegion> = Vec::with_capacity(256);
    let mut chunk_valid: Vec<Option<&str>> = Vec::with_capacity(256);

    for region in &file.source_regions {
        let valid_id = valid_symbol_id(symbol_lookup, region.containing_symbol_id.as_deref());
        chunk_rows.push(region);
        chunk_valid.push(valid_id);
        if chunk_rows.len() == chunk_size {
            flush_source_region_chunk(multi, file, &chunk_rows, &chunk_valid)?;
            inserted += chunk_size as i64;
            chunk_rows.clear();
            chunk_valid.clear();
        }
    }

    for (region, valid_id) in chunk_rows.into_iter().zip(chunk_valid) {
        single.execute(params![
            region.source_region_id,
            file.file_id,
            file.path,
            file.language,
            region.kind,
            valid_id,
            region.start_line,
            region.start_column,
            region.end_line,
            region.end_column,
            region.start_byte,
            region.end_byte,
            region.metadata_json,
        ])?;
        inserted += 1;
    }
    Ok(inserted)
}

fn insert_structural_facts(
    single: &mut CachedStatement<'_>,
    multi: &mut CachedStatement<'_>,
    chunk_size: usize,
    seen_ids: &mut HashSet<String>,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup<'_>,
) -> rusqlite::Result<i64> {
    let mut inserted = 0i64;
    // Buffer capacity is bounded independently of the flush threshold so a
    // large runtime chunk size does not trigger a huge per-file allocation.
    let mut chunk_facts: Vec<&ArtifactStructuralFact> = Vec::with_capacity(256);
    let mut chunk_valid_ids: Vec<Option<&str>> = Vec::with_capacity(256);

    for fact in &file.structural_facts {
        if !seen_ids.insert(fact.structural_fact_id.clone()) {
            continue;
        }
        let valid_id = valid_symbol_id(symbol_lookup, fact.containing_symbol_id.as_deref());
        chunk_facts.push(fact);
        chunk_valid_ids.push(valid_id);
        if chunk_facts.len() == chunk_size {
            flush_structural_fact_chunk(multi, file, &chunk_facts, &chunk_valid_ids)?;
            inserted += chunk_size as i64;
            chunk_facts.clear();
            chunk_valid_ids.clear();
        }
    }

    // Tail smaller than a full chunk: reuse the cached single-row statement so
    // no variable-length SQL is compiled. Small files never hit the multi-row
    // path, so they pay no overhead.
    for (fact, valid_id) in chunk_facts.into_iter().zip(chunk_valid_ids) {
        single.execute(params![
            fact.structural_fact_id,
            file.file_id,
            file.path,
            file.language,
            fact.pattern_id,
            fact.capture_name,
            fact.node_kind,
            valid_id,
            fact.start_line,
            fact.start_column,
            fact.end_line,
            fact.end_column,
            fact.start_byte,
            fact.end_byte,
            fact.confidence,
            fact.metadata_json,
        ])?;
        inserted += 1;
    }
    Ok(inserted)
}

/// Generic full-chunk flush: builds one `Vec<&dyn ToSql>` of
/// `chunk.len() * columns_per_row` references (no per-row boxing or per-row
/// allocation) and executes the cached multi-row statement once. `push_params`
/// references the row's fields and the pre-computed `valid_id` (stored in the
/// caller's buffer, which outlives this call), so the `&dyn ToSql` pointers
/// stay valid for the execute.
///
/// A single lifetime `'a` unifies the row, file, and `valid_id` references so
/// they can coexist in one invariant `Vec<&'a dyn ToSql>`. Indexing (rather
/// than `.iter()`) keeps the borrows at `'a` instead of the loop's local
/// lifetime.
fn flush_chunk<'a, R, F>(
    multi: &mut CachedStatement<'_>,
    file: &'a ArtifactFile,
    chunk_rows: &[&'a R],
    chunk_valid: &'a [Option<&'a str>],
    columns_per_row: usize,
    push_params: F,
) -> rusqlite::Result<()>
where
    F: Fn(&mut Vec<&'a dyn ToSql>, &'a R, &'a ArtifactFile, &'a Option<&'a str>),
{
    debug_assert_eq!(chunk_rows.len(), chunk_valid.len());
    let mut params: Vec<&'a dyn ToSql> = Vec::with_capacity(chunk_rows.len() * columns_per_row);
    for index in 0..chunk_rows.len() {
        push_params(&mut params, chunk_rows[index], file, &chunk_valid[index]);
    }
    multi.execute(params_from_iter(params))?;
    Ok(())
}

fn flush_structural_fact_chunk(
    multi: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    chunk_facts: &[&ArtifactStructuralFact],
    chunk_valid_ids: &[Option<&str>],
) -> rusqlite::Result<()> {
    flush_chunk(
        multi,
        file,
        chunk_facts,
        chunk_valid_ids,
        16,
        |params, fact, file, valid_id| {
            params.push(&fact.structural_fact_id);
            params.push(&file.file_id);
            params.push(&file.path);
            params.push(&file.language);
            params.push(&fact.pattern_id);
            params.push(&fact.capture_name);
            params.push(&fact.node_kind);
            params.push(valid_id);
            params.push(&fact.start_line);
            params.push(&fact.start_column);
            params.push(&fact.end_line);
            params.push(&fact.end_column);
            params.push(&fact.start_byte);
            params.push(&fact.end_byte);
            params.push(&fact.confidence);
            params.push(&fact.metadata_json);
        },
    )
}

fn flush_source_region_chunk(
    multi: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    chunk_rows: &[&ArtifactSourceRegion],
    chunk_valid: &[Option<&str>],
) -> rusqlite::Result<()> {
    flush_chunk(
        multi,
        file,
        chunk_rows,
        chunk_valid,
        13,
        |params, region, file, valid_id| {
            params.push(&region.source_region_id);
            params.push(&file.file_id);
            params.push(&file.path);
            params.push(&file.language);
            params.push(&region.kind);
            params.push(valid_id);
            params.push(&region.start_line);
            params.push(&region.start_column);
            params.push(&region.end_line);
            params.push(&region.end_column);
            params.push(&region.start_byte);
            params.push(&region.end_byte);
            params.push(&region.metadata_json);
        },
    )
}

fn flush_complexity_metric_chunk(
    multi: &mut CachedStatement<'_>,
    file: &ArtifactFile,
    chunk_rows: &[&ArtifactComplexityMetric],
    chunk_valid: &[Option<&str>],
) -> rusqlite::Result<()> {
    flush_chunk(
        multi,
        file,
        chunk_rows,
        chunk_valid,
        20,
        |params, metric, file, valid_id| {
            params.push(&metric.complexity_metric_id);
            params.push(&file.file_id);
            params.push(&file.path);
            params.push(&file.language);
            params.push(&metric.scope);
            params.push(valid_id);
            params.push(&metric.algorithm_id);
            params.push(&metric.covered_lines);
            params.push(&metric.covered_bytes);
            params.push(&metric.decision_count);
            params.push(&metric.loop_count);
            params.push(&metric.max_nesting_depth);
            params.push(&metric.parameter_count);
            params.push(&metric.start_line);
            params.push(&metric.start_column);
            params.push(&metric.end_line);
            params.push(&metric.end_column);
            params.push(&metric.start_byte);
            params.push(&metric.end_byte);
            params.push(&metric.metadata_json);
        },
    )
}

fn structural_fact_chunk_size(tx: &Transaction<'_>) -> rusqlite::Result<usize> {
    chunk_size_for_columns(tx, STRUCTURAL_FACT_MAX_CHUNK, 16)
}

fn source_region_chunk_size(tx: &Transaction<'_>) -> rusqlite::Result<usize> {
    chunk_size_for_columns(tx, SOURCE_REGION_MAX_CHUNK, 13)
}

fn complexity_metric_chunk_size(tx: &Transaction<'_>) -> rusqlite::Result<usize> {
    chunk_size_for_columns(tx, COMPLEXITY_METRIC_MAX_CHUNK, 20)
}

/// Rows per multi-row chunk for a table with `columns_per_row` columns, capped
/// so a full chunk never exceeds the runtime host-parameter limit. `max(1)`
/// keeps at least one row when the limit is pathologically low.
fn chunk_size_for_columns(
    tx: &Transaction<'_>,
    max_chunk: usize,
    columns_per_row: usize,
) -> rusqlite::Result<usize> {
    let limit = tx
        .limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER)?
        .max(columns_per_row as i32) as usize;
    Ok(max_chunk.min(limit / columns_per_row).max(1))
}

fn structural_facts_multi_insert_sql(rows: usize) -> String {
    multi_row_insert_sql(
        "INSERT INTO structural_facts
         (structural_fact_id, file_id, path, language, pattern_id, capture_name,
          node_kind, containing_symbol_id, start_line, start_column, end_line,
          end_column, start_byte, end_byte, confidence, metadata_json)",
        "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        rows,
    )
}

fn source_regions_multi_insert_sql(rows: usize) -> String {
    multi_row_insert_sql(
        "INSERT INTO source_regions
         (source_region_id, file_id, path, language, kind, containing_symbol_id,
          start_line, start_column, end_line, end_column, start_byte, end_byte,
          metadata_json)",
        "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        rows,
    )
}

fn complexity_metrics_multi_insert_sql(rows: usize) -> String {
    multi_row_insert_sql(
        "INSERT INTO complexity_metrics
         (complexity_metric_id, file_id, path, language, scope, symbol_id, algorithm_id,
          covered_lines, covered_bytes, decision_count, loop_count, max_nesting_depth,
          parameter_count, start_line, start_column, end_line, end_column, start_byte,
          end_byte, metadata_json)",
        "(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        rows,
    )
}

fn multi_row_insert_sql(prefix: &str, group: &str, rows: usize) -> String {
    let groups = std::iter::repeat_n(group, rows)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{prefix} VALUES {groups}")
}

fn insert_complexity_metrics(
    single: &mut CachedStatement<'_>,
    multi: &mut CachedStatement<'_>,
    chunk_size: usize,
    file: &ArtifactFile,
    symbol_lookup: &SymbolLookup<'_>,
) -> rusqlite::Result<i64> {
    let mut inserted = 0i64;
    let mut chunk_rows: Vec<&ArtifactComplexityMetric> = Vec::with_capacity(256);
    let mut chunk_valid: Vec<Option<&str>> = Vec::with_capacity(256);

    for metric in &file.complexity_metrics {
        let valid_id = valid_symbol_id(symbol_lookup, metric.symbol_id.as_deref());
        chunk_rows.push(metric);
        chunk_valid.push(valid_id);
        if chunk_rows.len() == chunk_size {
            flush_complexity_metric_chunk(multi, file, &chunk_rows, &chunk_valid)?;
            inserted += chunk_size as i64;
            chunk_rows.clear();
            chunk_valid.clear();
        }
    }

    for (metric, valid_id) in chunk_rows.into_iter().zip(chunk_valid) {
        single.execute(params![
            metric.complexity_metric_id,
            file.file_id,
            file.path,
            file.language,
            metric.scope,
            valid_id,
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
        inserted += 1;
    }
    Ok(inserted)
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

/// The symbol ids one file owns.
///
/// Extraction-table rows resolve their symbol FKs against the file they were
/// extracted from and nothing else, so extracting a file in isolation writes the
/// same rows as extracting it beside the whole repository. Cross-file targets are
/// the resolution store's job, not the extraction pass's.
pub(super) struct SymbolLookup<'a> {
    ids: HashSet<&'a str>,
}

impl<'a> SymbolLookup<'a> {
    fn from_file(file: &'a ArtifactFile) -> Self {
        Self {
            ids: file
                .symbols
                .iter()
                .map(|symbol| symbol.symbol_id.as_str())
                .collect(),
        }
    }

    fn contains(&self, symbol_id: &str) -> bool {
        self.ids.contains(symbol_id)
    }
}

/// Whether `insert_symbols` binds `parent_symbol_id` as it writes each row.
///
/// A file may list a child before its parent, so inline binding needs a
/// transaction that defers foreign keys (the spooled path). Elsewhere the column
/// binds NULL and `update_symbol_parents` fills it once every symbol row exists.
pub(super) enum ParentBinding {
    Deferred,
    ResolvedInFile,
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

fn valid_symbol_id<'a>(
    symbol_lookup: &SymbolLookup<'_>,
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
