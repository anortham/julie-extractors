use std::collections::HashSet;

use rusqlite::{Transaction, params};

use crate::model::{
    ArtifactCapabilitySnapshot, ArtifactLanguageCapabilityFixtureRow,
    ArtifactLanguageCapabilityGapRow, ArtifactLanguageCapabilityRow, ArtifactParserInventoryRow,
};
use crate::reports::RowDomainCounts;

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

pub(super) fn sync_optional_capability_snapshot_in_tx(
    tx: &Transaction<'_>,
    snapshot: Option<&ArtifactCapabilitySnapshot>,
) -> rusqlite::Result<RowDomainCounts> {
    match snapshot {
        Some(snapshot) => sync_capability_snapshot_in_tx(tx, snapshot),
        None => Ok(RowDomainCounts::default()),
    }
}

pub(super) fn sync_capability_snapshot_in_tx(
    tx: &Transaction<'_>,
    snapshot: &ArtifactCapabilitySnapshot,
) -> rusqlite::Result<RowDomainCounts> {
    let mut counts = RowDomainCounts::default();

    let parser_keys = snapshot
        .parser_inventory
        .iter()
        .map(|row| (row.language.clone(), row.parser_package.clone()))
        .collect::<HashSet<_>>();
    for (language, parser_package) in load_parser_inventory_keys(tx)? {
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

    for (language, fixture_name) in load_language_capability_fixture_keys(tx)? {
        if !fixture_keys.contains(&(language.clone(), fixture_name.clone())) {
            counts.language_capability_fixtures += tx.execute(
                "DELETE FROM language_capability_fixtures
                 WHERE language = ?1 AND fixture_name = ?2",
                params![language, fixture_name],
            )? as i64;
        }
    }
    for gap_id in load_language_capability_gap_keys(tx)? {
        if !gap_keys.contains(&gap_id) {
            counts.language_capability_gaps += tx.execute(
                "DELETE FROM language_capability_gaps WHERE gap_id = ?1",
                [gap_id],
            )? as i64;
        }
    }
    for language in load_language_capability_keys(tx)? {
        if !language_keys.contains(&language) {
            counts.language_capabilities += tx.execute(
                "DELETE FROM language_capabilities WHERE language = ?1",
                [language],
            )? as i64;
        }
    }

    for row in &snapshot.parser_inventory {
        counts.parser_inventory += upsert_parser_inventory(tx, row)? as i64;
    }
    for row in &snapshot.languages {
        counts.language_capabilities += upsert_language_capability(tx, row)? as i64;
        for fixture in &row.fixtures {
            counts.language_capability_fixtures +=
                upsert_language_capability_fixture(tx, &row.language, fixture)? as i64;
        }
        for gap in &row.gaps {
            counts.language_capability_gaps +=
                upsert_language_capability_gap(tx, &row.language, gap)? as i64;
        }
    }

    Ok(counts)
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
