use rusqlite::{CachedStatement, OptionalExtension, Transaction, params};

use crate::model::ArtifactCapabilitySnapshot;

use super::{StoreFileVersion, StoreLevel, StoreRowCounts};

#[derive(Debug)]
pub(super) enum CapabilityWriteError {
    Sqlite(rusqlite::Error),
    Conflict,
}

impl From<rusqlite::Error> for CapabilityWriteError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

#[derive(Debug, Default)]
pub(super) struct StatementPreparationCounter {
    count: usize,
}

impl StatementPreparationCounter {
    pub(super) fn prepare_cached<'tx>(
        &mut self,
        tx: &'tx Transaction<'_>,
        sql: &str,
    ) -> rusqlite::Result<CachedStatement<'tx>> {
        let statement = tx.prepare_cached(sql)?;
        self.count += 1;
        Ok(statement)
    }

    pub(super) fn count(&self) -> usize {
        self.count
    }
}

pub(super) fn delete_level_rows(
    tx: &Transaction<'_>,
    version_id: i64,
    level: StoreLevel,
) -> rusqlite::Result<()> {
    match level {
        StoreLevel::L1 => {
            for table in [
                "relationships",
                "pending_relationships",
                "symbol_annotations",
                "type_facts",
                "complexity_metrics",
                "parse_diagnostics",
                "symbols",
            ] {
                tx.execute(
                    &format!("DELETE FROM {table} WHERE version_id = ?1"),
                    [version_id],
                )?;
            }
            tx.execute(
                "DELETE FROM reference_sites WHERE version_id = ?1 AND level = 1",
                [version_id],
            )?;
        }
        StoreLevel::L2 => {
            tx.execute(
                "DELETE FROM identifiers WHERE version_id = ?1",
                [version_id],
            )?;
            tx.execute(
                "DELETE FROM reference_sites WHERE version_id = ?1 AND level = 2",
                [version_id],
            )?;
        }
        StoreLevel::L3 => {
            for table in [
                "type_arguments",
                "type_argument_usages",
                "literals",
                "source_regions",
                "structural_facts",
            ] {
                tx.execute(
                    &format!("DELETE FROM {table} WHERE version_id = ?1"),
                    [version_id],
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn insert_level_rows(
    tx: &Transaction<'_>,
    version_id: i64,
    version: &StoreFileVersion,
    level: StoreLevel,
    preparations: &mut StatementPreparationCounter,
) -> rusqlite::Result<StoreRowCounts> {
    match level {
        StoreLevel::L1 => insert_l1_rows(tx, version_id, version, preparations),
        StoreLevel::L2 => insert_l2_rows(tx, version_id, version, preparations),
        StoreLevel::L3 => insert_l3_rows(tx, version_id, version, preparations),
    }
}

fn insert_l1_rows(
    tx: &Transaction<'_>,
    version_id: i64,
    version: &StoreFileVersion,
    preparations: &mut StatementPreparationCounter,
) -> rusqlite::Result<StoreRowCounts> {
    let file = version.artifact_file();
    let mut counts = StoreRowCounts::default();
    let mut symbols = preparations.prepare_cached(
        tx,
        "INSERT INTO symbols
         (version_id, symbol_id, path, language, name, kind, signature, doc_comment, visibility,
          parent_symbol_id, start_line, start_column, end_line, end_column, start_byte, end_byte,
          body_start_line, body_start_column, body_end_line, body_end_column, body_start_byte,
          body_end_byte, body_hash, semantic_group, confidence, content_type, is_test,
          test_container, test_lifecycle, metadata_json)
         VALUES
         (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
          ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30)",
    )?;
    let mut annotations = preparations.prepare_cached(
        tx,
        "INSERT INTO symbol_annotations
         (version_id, annotation_id, symbol_id, annotation, annotation_key, raw_text, carrier,
          metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    let mut reference_sites = prepare_reference_sites(tx, preparations)?;
    let mut relationships = preparations.prepare_cached(
        tx,
        "INSERT INTO relationships
         (version_id, relationship_id, reference_site_id, from_symbol_id, to_symbol_id, path,
          kind, start_line, start_column, end_line, end_column, start_byte, end_byte, confidence,
          metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
    )?;
    let mut pending = preparations.prepare_cached(
        tx,
        "INSERT INTO pending_relationships
         (version_id, pending_relationship_id, reference_site_id, from_symbol_id,
          caller_scope_symbol_id, path, kind, target_display_name, target_terminal_name,
          target_receiver, target_namespace_json, target_import_context, start_line, start_column,
          end_line, end_column, start_byte, end_byte, confidence, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 ?16, ?17, ?18, ?19, ?20)",
    )?;
    let mut type_facts = preparations.prepare_cached(
        tx,
        "INSERT INTO type_facts
         (version_id, type_fact_id, symbol_id, language, resolved_type, generic_params_json,
          constraints_json, is_inferred, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    let mut complexity = preparations.prepare_cached(
        tx,
        "INSERT INTO complexity_metrics
         (version_id, complexity_metric_id, path, language, scope, symbol_id, algorithm_id,
          covered_lines, covered_bytes, decision_count, loop_count, max_nesting_depth,
          parameter_count, start_line, start_column, end_line, end_column, start_byte, end_byte,
          metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 ?16, ?17, ?18, ?19, ?20)",
    )?;
    let mut diagnostics = preparations.prepare_cached(
        tx,
        "INSERT INTO parse_diagnostics
         (version_id, diagnostic_id, path, language, kind, message, start_line, start_column,
          end_line, end_column, start_byte, end_byte, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;

    for symbol in &file.symbols {
        counts.symbols += symbols.execute(params![
            version_id,
            symbol.symbol_id,
            file.path,
            file.language,
            symbol.name,
            symbol.kind,
            symbol.signature,
            symbol.doc_comment,
            symbol.visibility,
            symbol.parent_symbol_id,
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
            bool_integer(symbol.is_test),
            bool_integer(symbol.test_container),
            bool_integer(symbol.test_lifecycle),
            symbol.metadata_json,
        ])? as i64;
    }
    for annotation in &file.symbol_annotations {
        counts.symbol_annotations += annotations.execute(params![
            version_id,
            annotation.annotation_id,
            annotation.symbol_id,
            annotation.annotation,
            annotation.annotation_key,
            annotation.raw_text,
            annotation.carrier,
            annotation.metadata_json,
        ])? as i64;
    }
    counts.reference_sites += insert_reference_sites(
        &mut reference_sites,
        version_id,
        version.reference_sites(StoreLevel::L1),
    )?;
    for relationship in &file.relationships {
        counts.relationships += relationships.execute(params![
            version_id,
            relationship.relationship_id,
            relationship.reference_site_id,
            relationship.from_symbol_id,
            relationship.to_symbol_id,
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
        ])? as i64;
    }
    for row in &file.pending_relationships {
        counts.pending_relationships += pending.execute(params![
            version_id,
            row.pending_relationship_id,
            row.reference_site_id,
            row.from_symbol_id,
            row.caller_scope_symbol_id,
            file.path,
            row.kind,
            row.target_display_name,
            row.target_terminal_name,
            row.target_receiver,
            row.target_namespace_json,
            row.target_import_context,
            row.start_line,
            row.start_column,
            row.end_line,
            row.end_column,
            row.start_byte,
            row.end_byte,
            row.confidence,
            row.metadata_json,
        ])? as i64;
    }
    for row in &file.type_facts {
        counts.type_facts += type_facts.execute(params![
            version_id,
            row.type_fact_id,
            row.symbol_id,
            file.language,
            row.resolved_type,
            row.generic_params_json,
            row.constraints_json,
            bool_integer(row.is_inferred),
            row.metadata_json,
        ])? as i64;
    }
    for row in &file.complexity_metrics {
        counts.complexity_metrics += complexity.execute(params![
            version_id,
            row.complexity_metric_id,
            file.path,
            file.language,
            row.scope,
            row.symbol_id,
            row.algorithm_id,
            row.covered_lines,
            row.covered_bytes,
            row.decision_count,
            row.loop_count,
            row.max_nesting_depth,
            row.parameter_count,
            row.start_line,
            row.start_column,
            row.end_line,
            row.end_column,
            row.start_byte,
            row.end_byte,
            row.metadata_json,
        ])? as i64;
    }
    for row in &file.parse_diagnostics {
        counts.parse_diagnostics += diagnostics.execute(params![
            version_id,
            row.diagnostic_id,
            file.path,
            file.language,
            row.kind,
            row.message,
            row.start_line,
            row.start_column,
            row.end_line,
            row.end_column,
            row.start_byte,
            row.end_byte,
            row.metadata_json,
        ])? as i64;
    }
    Ok(counts)
}

fn insert_l2_rows(
    tx: &Transaction<'_>,
    version_id: i64,
    version: &StoreFileVersion,
    preparations: &mut StatementPreparationCounter,
) -> rusqlite::Result<StoreRowCounts> {
    let file = version.artifact_file();
    let mut counts = StoreRowCounts::default();
    let mut reference_sites = prepare_reference_sites(tx, preparations)?;
    let mut identifiers = preparations.prepare_cached(
        tx,
        "INSERT INTO identifiers
         (version_id, identifier_id, reference_site_id, path, language, name, kind,
          containing_symbol_id, start_line, start_column, end_line, end_column, start_byte,
          end_byte, confidence, code_context, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 ?16, ?17)",
    )?;
    counts.reference_sites += insert_reference_sites(
        &mut reference_sites,
        version_id,
        version.reference_sites(StoreLevel::L2),
    )?;
    for row in &file.identifiers {
        counts.identifiers += identifiers.execute(params![
            version_id,
            row.identifier_id,
            row.reference_site_id,
            file.path,
            file.language,
            row.name,
            row.kind,
            row.containing_symbol_id,
            row.start_line,
            row.start_column,
            row.end_line,
            row.end_column,
            row.start_byte,
            row.end_byte,
            row.confidence,
            row.code_context,
            row.metadata_json,
        ])? as i64;
    }
    Ok(counts)
}

fn insert_l3_rows(
    tx: &Transaction<'_>,
    version_id: i64,
    version: &StoreFileVersion,
    preparations: &mut StatementPreparationCounter,
) -> rusqlite::Result<StoreRowCounts> {
    let file = version.artifact_file();
    let mut counts = StoreRowCounts::default();
    let mut usages = preparations.prepare_cached(
        tx,
        "INSERT INTO type_argument_usages
         (version_id, usage_id, identifier_id, path, language, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    let mut arguments = preparations.prepare_cached(
        tx,
        "INSERT INTO type_arguments
         (version_id, type_argument_id, usage_id, parent_type_argument_id, ordinal, type_name)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    let mut literals = preparations.prepare_cached(
        tx,
        "INSERT INTO literals
         (version_id, literal_id, path, language, literal_text, kind, carrier, arg_position,
          containing_symbol_id, start_line, start_column, end_line, end_column, start_byte,
          end_byte, confidence, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 ?16, ?17)",
    )?;
    let mut regions = preparations.prepare_cached(
        tx,
        "INSERT INTO source_regions
         (version_id, source_region_id, path, language, kind, containing_symbol_id, start_line,
          start_column, end_line, end_column, start_byte, end_byte, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;
    let mut facts = preparations.prepare_cached(
        tx,
        "INSERT INTO structural_facts
         (version_id, structural_fact_id, path, language, pattern_id, capture_name, node_kind,
          containing_symbol_id, start_line, start_column, end_line, end_column, start_byte,
          end_byte, confidence, metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 ?16)",
    )?;
    for row in &file.type_argument_usages {
        counts.type_argument_usages += usages.execute(params![
            version_id,
            row.usage_id,
            row.identifier_id,
            file.path,
            file.language,
            row.metadata_json,
        ])? as i64;
    }
    for row in &file.type_arguments {
        counts.type_arguments += arguments.execute(params![
            version_id,
            row.type_argument_id,
            row.usage_id,
            row.parent_type_argument_id,
            row.ordinal,
            row.type_name,
        ])? as i64;
    }
    for row in &file.literals {
        counts.literals += literals.execute(params![
            version_id,
            row.literal_id,
            file.path,
            file.language,
            row.literal_text,
            row.kind,
            row.carrier,
            row.arg_position,
            row.containing_symbol_id,
            row.start_line,
            row.start_column,
            row.end_line,
            row.end_column,
            row.start_byte,
            row.end_byte,
            row.confidence,
            row.metadata_json,
        ])? as i64;
    }
    for row in &file.source_regions {
        counts.source_regions += regions.execute(params![
            version_id,
            row.source_region_id,
            file.path,
            file.language,
            row.kind,
            row.containing_symbol_id,
            row.start_line,
            row.start_column,
            row.end_line,
            row.end_column,
            row.start_byte,
            row.end_byte,
            row.metadata_json,
        ])? as i64;
    }
    for row in &file.structural_facts {
        counts.structural_facts += facts.execute(params![
            version_id,
            row.structural_fact_id,
            file.path,
            file.language,
            row.pattern_id,
            row.capture_name,
            row.node_kind,
            row.containing_symbol_id,
            row.start_line,
            row.start_column,
            row.end_line,
            row.end_column,
            row.start_byte,
            row.end_byte,
            row.confidence,
            row.metadata_json,
        ])? as i64;
    }
    Ok(counts)
}

fn prepare_reference_sites<'tx>(
    tx: &'tx Transaction<'_>,
    preparations: &mut StatementPreparationCounter,
) -> rusqlite::Result<rusqlite::CachedStatement<'tx>> {
    preparations.prepare_cached(
        tx,
        "INSERT OR IGNORE INTO reference_sites
         (version_id, reference_site_id, path, language, containing_symbol_id, start_line,
          start_column, end_line, end_column, start_byte, end_byte, is_exact, provenance, level)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )
}

fn insert_reference_sites(
    statement: &mut rusqlite::CachedStatement<'_>,
    version_id: i64,
    sites: &[super::StoreReferenceSite],
) -> rusqlite::Result<i64> {
    let mut inserted = 0;
    for site in sites {
        inserted += statement.execute(params![
            version_id,
            site.reference_site_id,
            site.path,
            site.language,
            site.containing_symbol_id,
            site.start_line,
            site.start_column,
            site.end_line,
            site.end_column,
            site.start_byte,
            site.end_byte,
            bool_integer(site.is_exact),
            site.provenance,
            site.level,
        ])? as i64;
    }
    Ok(inserted)
}

pub(super) fn sync_capability_snapshot(
    tx: &Transaction<'_>,
    extraction_epoch: u32,
    snapshot: &ArtifactCapabilitySnapshot,
    preparations: &mut StatementPreparationCounter,
) -> Result<StoreRowCounts, CapabilityWriteError> {
    let mut counts = StoreRowCounts::default();
    let mut parser_inventory = preparations.prepare_cached(
        tx,
        "INSERT OR IGNORE INTO parser_inventory
         (extraction_epoch, language, parser_package, parser_version, grammar_version, source,
          metadata_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    let mut languages = preparations.prepare_cached(
        tx,
        "INSERT OR IGNORE INTO language_capabilities
         (extraction_epoch, language, parser_package, extensions_json, dependency_status,
          target_symbols, target_relationships, target_pending_relationships, target_identifiers,
          target_types, actual_symbols, actual_relationships, actual_pending_relationships,
          actual_identifiers, actual_types, kind_coverage_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                 ?16)",
    )?;
    let mut fixtures = preparations.prepare_cached(
        tx,
        "INSERT OR IGNORE INTO language_capability_fixtures
         (extraction_epoch, language, fixture_name, source_path, expected_path)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    let mut gaps = preparations.prepare_cached(
        tx,
        "INSERT OR IGNORE INTO language_capability_gaps
         (extraction_epoch, gap_id, language, capability, status, reason, required_closure,
          evidence_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    for row in &snapshot.parser_inventory {
        let metadata_json = row.metadata.as_ref().map(|value| value.to_string());
        counts.parser_inventory += parser_inventory.execute(params![
            extraction_epoch,
            row.language,
            row.parser_package,
            row.parser_version,
            row.grammar_version,
            row.source,
            metadata_json,
        ])? as i64;
    }
    for row in &snapshot.languages {
        let extensions_json =
            serde_json::to_string(&row.extensions).expect("capability extensions are serializable");
        let kind_coverage_json = row.kind_coverage.to_string();
        counts.language_capabilities += languages.execute(params![
            extraction_epoch,
            row.language,
            row.parser_package,
            extensions_json,
            row.dependency_status,
            bool_integer(row.target_capabilities.symbols),
            bool_integer(row.target_capabilities.relationships),
            bool_integer(row.target_capabilities.pending_relationships),
            bool_integer(row.target_capabilities.identifiers),
            bool_integer(row.target_capabilities.types),
            bool_integer(row.actual_capabilities.symbols),
            bool_integer(row.actual_capabilities.relationships),
            bool_integer(row.actual_capabilities.pending_relationships),
            bool_integer(row.actual_capabilities.identifiers),
            bool_integer(row.actual_capabilities.types),
            kind_coverage_json,
        ])? as i64;
        for fixture in &row.fixtures {
            counts.language_capability_fixtures += fixtures.execute(params![
                extraction_epoch,
                row.language,
                fixture.fixture_name,
                fixture.source_path,
                fixture.expected_path,
            ])? as i64;
        }
        for gap in &row.gaps {
            counts.language_capability_gaps += gaps.execute(params![
                extraction_epoch,
                gap.gap_id,
                row.language,
                gap.capability,
                gap.status.as_str(),
                gap.reason,
                gap.required_closure,
                gap.evidence.to_string(),
            ])? as i64;
        }
    }

    if !capability_snapshot_matches(tx, extraction_epoch, snapshot, preparations)? {
        return Err(CapabilityWriteError::Conflict);
    }
    Ok(counts)
}

pub(super) fn capability_epoch_initialized(
    tx: &Transaction<'_>,
    extraction_epoch: u32,
    preparations: &mut StatementPreparationCounter,
) -> rusqlite::Result<bool> {
    let mut initialized = preparations.prepare_cached(
        tx,
        "SELECT EXISTS(
             SELECT 1 FROM language_capabilities WHERE extraction_epoch = ?1
         )",
    )?;
    initialized.query_row([extraction_epoch], |row| row.get(0))
}

pub(super) fn capability_snapshot_matches(
    tx: &Transaction<'_>,
    extraction_epoch: u32,
    snapshot: &ArtifactCapabilitySnapshot,
    preparations: &mut StatementPreparationCounter,
) -> rusqlite::Result<bool> {
    let expected_fixtures = snapshot
        .languages
        .iter()
        .map(|row| row.fixtures.len() as i64)
        .sum::<i64>();
    let expected_gaps = snapshot
        .languages
        .iter()
        .map(|row| row.gaps.len() as i64)
        .sum::<i64>();
    let mut parser_count = preparations.prepare_cached(
        tx,
        "SELECT COUNT(*) FROM parser_inventory WHERE extraction_epoch = ?1",
    )?;
    let mut language_count = preparations.prepare_cached(
        tx,
        "SELECT COUNT(*) FROM language_capabilities WHERE extraction_epoch = ?1",
    )?;
    let mut fixture_count = preparations.prepare_cached(
        tx,
        "SELECT COUNT(*) FROM language_capability_fixtures WHERE extraction_epoch = ?1",
    )?;
    let mut gap_count = preparations.prepare_cached(
        tx,
        "SELECT COUNT(*) FROM language_capability_gaps WHERE extraction_epoch = ?1",
    )?;
    for (statement, expected) in [
        (&mut parser_count, snapshot.parser_inventory.len() as i64),
        (&mut language_count, snapshot.languages.len() as i64),
        (&mut fixture_count, expected_fixtures),
        (&mut gap_count, expected_gaps),
    ] {
        let count = statement.query_row([extraction_epoch], |row| row.get::<_, i64>(0))?;
        if count != expected {
            return Ok(false);
        }
    }
    let mut parser_match = preparations.prepare_cached(
        tx,
        "SELECT 1 FROM parser_inventory
         WHERE extraction_epoch = ?1 AND language = ?2 AND parser_package = ?3
           AND parser_version IS ?4 AND grammar_version IS ?5 AND source IS ?6
           AND metadata_json IS ?7",
    )?;
    let mut language_match = preparations.prepare_cached(
        tx,
        "SELECT 1 FROM language_capabilities
         WHERE extraction_epoch = ?1 AND language = ?2 AND parser_package = ?3
           AND extensions_json = ?4 AND dependency_status = ?5
           AND target_symbols = ?6 AND target_relationships = ?7
           AND target_pending_relationships = ?8 AND target_identifiers = ?9
           AND target_types = ?10 AND actual_symbols = ?11 AND actual_relationships = ?12
           AND actual_pending_relationships = ?13 AND actual_identifiers = ?14
           AND actual_types = ?15 AND kind_coverage_json = ?16",
    )?;
    let mut fixture_match = preparations.prepare_cached(
        tx,
        "SELECT 1 FROM language_capability_fixtures
         WHERE extraction_epoch = ?1 AND language = ?2 AND fixture_name = ?3
           AND source_path = ?4 AND expected_path = ?5",
    )?;
    let mut gap_match = preparations.prepare_cached(
        tx,
        "SELECT 1 FROM language_capability_gaps
         WHERE extraction_epoch = ?1 AND gap_id = ?2 AND language = ?3
           AND capability = ?4 AND status = ?5 AND reason = ?6
           AND required_closure = ?7 AND evidence_json = ?8",
    )?;
    for row in &snapshot.parser_inventory {
        let metadata_json = row.metadata.as_ref().map(|value| value.to_string());
        let matched = parser_match
            .query_row(
                params![
                    extraction_epoch,
                    row.language,
                    row.parser_package,
                    row.parser_version,
                    row.grammar_version,
                    row.source,
                    metadata_json,
                ],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !matched {
            return Ok(false);
        }
    }
    for row in &snapshot.languages {
        let extensions_json =
            serde_json::to_string(&row.extensions).expect("capability extensions are serializable");
        let matched = language_match
            .query_row(
                params![
                    extraction_epoch,
                    row.language,
                    row.parser_package,
                    extensions_json,
                    row.dependency_status,
                    bool_integer(row.target_capabilities.symbols),
                    bool_integer(row.target_capabilities.relationships),
                    bool_integer(row.target_capabilities.pending_relationships),
                    bool_integer(row.target_capabilities.identifiers),
                    bool_integer(row.target_capabilities.types),
                    bool_integer(row.actual_capabilities.symbols),
                    bool_integer(row.actual_capabilities.relationships),
                    bool_integer(row.actual_capabilities.pending_relationships),
                    bool_integer(row.actual_capabilities.identifiers),
                    bool_integer(row.actual_capabilities.types),
                    row.kind_coverage.to_string(),
                ],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !matched {
            return Ok(false);
        }
        for fixture in &row.fixtures {
            let matched = fixture_match
                .query_row(
                    params![
                        extraction_epoch,
                        row.language,
                        fixture.fixture_name,
                        fixture.source_path,
                        fixture.expected_path,
                    ],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !matched {
                return Ok(false);
            }
        }
        for gap in &row.gaps {
            let matched = gap_match
                .query_row(
                    params![
                        extraction_epoch,
                        gap.gap_id,
                        row.language,
                        gap.capability,
                        gap.status.as_str(),
                        gap.reason,
                        gap.required_closure,
                        gap.evidence.to_string(),
                    ],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !matched {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn bool_integer(value: bool) -> i64 {
    if value { 1 } else { 0 }
}
