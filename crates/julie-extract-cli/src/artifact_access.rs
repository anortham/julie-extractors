use std::collections::BTreeMap;
use std::path::Path;

use julie_extract_artifact::jsonl::{JSONL_RECORD_KINDS, JSONL_SCHEMA_VERSION};
use julie_extract_artifact::metadata::{ArtifactMetadata, REQUIRED_METADATA_KEYS, read_metadata};
use julie_extract_artifact::reports::{
    ArtifactReport, ReportCode, ReportDiagnostic, ReportFileRows, RowDomainCounts,
};
use julie_extract_artifact::resolution_store::{ResolutionStatus, read_resolution_metadata};
use julie_extract_artifact::schema::{EXTRACT_CONTRACT_VERSION, SQLITE_SCHEMA_VERSION};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;

use crate::reports::{CommandError, command_error, diagnostic, display_path};
use crate::resolution::RESOLUTION_VERSION;

pub(crate) struct OpenArtifact {
    pub(crate) connection: Connection,
    pub(crate) report: ArtifactReport,
    pub(crate) write_metadata: ArtifactMetadata,
    pub(crate) reference_resolution_version: Option<i64>,
    pub(crate) reference_resolution_ready: bool,
}

pub(crate) struct OpenInfoArtifact {
    pub(crate) connection: Connection,
    pub(crate) report: ArtifactReport,
    pub(crate) warnings: Vec<ReportDiagnostic>,
}

pub(crate) struct ExistingArtifact {
    pub(crate) write_metadata: ArtifactMetadata,
}

/// Upper bound for memory-mapped I/O on read-only artifact connections. SQLite
/// only maps up to the file size, so a large cap is lazy virtual address space
/// and never allocates the full amount resident. The GLM review flagged the
/// absence of `mmap_size` on reader paths; this bounds scan/export I/O for
/// large artifacts without changing the writer.
const READER_MMAP_SIZE_BYTES: i64 = 1024 * 1024 * 1024;

fn apply_reader_pragmas(connection: &Connection, db_path: &Path) -> Result<(), CommandError> {
    connection
        .pragma_update(None, "mmap_size", READER_MMAP_SIZE_BYTES)
        .map_err(|error| {
            command_error(
                1,
                ReportCode::DbOpenFailed,
                format!("could not apply reader mmap_size pragma: {error}"),
                Some(display_path(db_path)),
                None,
                false,
                json!({}),
            )
        })?;
    Ok(())
}

pub(crate) fn artifact_report_from_connection(
    db: &Path,
    connection: &Connection,
) -> Result<ArtifactReport, CommandError> {
    let metadata = read_metadata(connection).map_err(|error| {
        command_error(
            3,
            ReportCode::SchemaIncompatible,
            format!("artifact metadata could not be read: {error}"),
            Some(display_path(db)),
            None,
            false,
            json!({}),
        )
    })?;
    artifact_report(db, &metadata, Some(JSONL_SCHEMA_VERSION))
}

pub(crate) fn open_artifact(
    db_path: &Path,
    strict_schema: bool,
    jsonl_schema_version: Option<i64>,
) -> Result<OpenArtifact, CommandError> {
    let connection = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| {
            command_error(
                1,
                ReportCode::DbOpenFailed,
                format!("could not open SQLite artifact: {error}"),
                Some(display_path(db_path)),
                None,
                true,
                json!({}),
            )
        })?;
    apply_reader_pragmas(&connection, db_path)?;
    let metadata = read_metadata(&connection).map_err(|error| {
        command_error(
            3,
            ReportCode::SchemaIncompatible,
            format!("artifact metadata could not be read: {error}"),
            Some(display_path(db_path)),
            None,
            false,
            json!({}),
        )
    })?;
    check_versions(&metadata, strict_schema)?;
    let resolution_metadata = read_resolution_metadata(&connection).ok().flatten();
    let reference_resolution_version = resolution_metadata
        .as_ref()
        .map(|resolution| resolution.version);
    let reference_resolution_ready = resolution_metadata.is_some_and(|resolution| {
        matches!(
            resolution.status,
            ResolutionStatus::Complete | ResolutionStatus::Partial
        )
    });
    let write_metadata = artifact_metadata_from_rows(&metadata)?;
    let report = artifact_report(db_path, &metadata, jsonl_schema_version)?;
    Ok(OpenArtifact {
        connection,
        report,
        write_metadata,
        reference_resolution_version,
        reference_resolution_ready,
    })
}

pub(crate) fn open_artifact_for_info(
    db_path: &Path,
    strict_schema: bool,
    jsonl_schema_version: Option<i64>,
) -> Result<OpenInfoArtifact, CommandError> {
    let connection = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| {
            command_error(
                1,
                ReportCode::DbOpenFailed,
                format!("could not open SQLite artifact: {error}"),
                Some(display_path(db_path)),
                None,
                true,
                json!({}),
            )
        })?;
    apply_reader_pragmas(&connection, db_path)?;
    let metadata = read_metadata(&connection).map_err(|error| {
        command_error(
            3,
            ReportCode::SchemaIncompatible,
            format!("artifact metadata could not be read: {error}"),
            Some(display_path(db_path)),
            None,
            false,
            json!({}),
        )
    })?;
    check_versions(&metadata, strict_schema)?;
    let report = artifact_report(db_path, &metadata, jsonl_schema_version)?;
    let warnings = missing_metadata_warnings(&metadata);
    Ok(OpenInfoArtifact {
        connection,
        report,
        warnings,
    })
}

pub(crate) fn load_existing_content_hashes(
    connection: &Connection,
) -> Result<BTreeMap<String, String>, CommandError> {
    let mut statement = connection
        .prepare("SELECT path, content_hash FROM files")
        .map_err(|error| {
            command_error(
                3,
                ReportCode::SchemaIncompatible,
                format!("existing file hashes could not be prepared: {error}"),
                None,
                None,
                false,
                json!({}),
            )
        })?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| {
            command_error(
                3,
                ReportCode::SchemaIncompatible,
                format!("existing file hashes could not be read: {error}"),
                None,
                None,
                false,
                json!({}),
            )
        })?;

    let mut hashes = BTreeMap::new();
    for row in rows {
        let (path, content_hash) = row.map_err(|error| {
            command_error(
                3,
                ReportCode::SchemaIncompatible,
                format!("existing file hash row could not be read: {error}"),
                None,
                None,
                false,
                json!({}),
            )
        })?;
        hashes.insert(path, content_hash);
    }
    Ok(hashes)
}

pub(crate) fn existing_artifact_for_root(
    db_path: &Path,
    strict_schema: bool,
    jsonl_schema_version: Option<i64>,
    root: &Path,
) -> Result<Option<ExistingArtifact>, CommandError> {
    if !db_path.exists() {
        return Ok(None);
    }

    let artifact = open_artifact(db_path, strict_schema, jsonl_schema_version)?;
    if artifact.report.root_path != display_path(root) {
        return Err(command_error(
            3,
            ReportCode::RootMismatch,
            "artifact root does not match requested root",
            Some(display_path(db_path)),
            None,
            false,
            json!({
                "artifact_root": artifact.report.root_path,
                "requested_root": display_path(root),
            }),
        ));
    }
    if artifact.reference_resolution_version != Some(RESOLUTION_VERSION)
        || !artifact.reference_resolution_ready
    {
        return Err(command_error(
            3,
            ReportCode::SchemaMigrationRequired,
            "artifact reference evidence requires a full scan before single-file operations",
            Some(display_path(db_path)),
            None,
            true,
            json!({
                "artifact_reference_resolution_version": artifact.reference_resolution_version,
                "required_reference_resolution_version": RESOLUTION_VERSION,
                "action": "julie-extract scan",
            }),
        ));
    }

    Ok(Some(ExistingArtifact {
        write_metadata: artifact.write_metadata,
    }))
}

pub(crate) fn open_artifact_for_root(
    db_path: &Path,
    strict_schema: bool,
    jsonl_schema_version: Option<i64>,
    root: &Path,
) -> Result<OpenArtifact, CommandError> {
    let artifact = open_artifact(db_path, strict_schema, jsonl_schema_version)?;
    if artifact.report.root_path != display_path(root) {
        return Err(command_error(
            3,
            ReportCode::RootMismatch,
            "artifact root does not match requested root",
            Some(display_path(db_path)),
            None,
            false,
            json!({
                "artifact_root": artifact.report.root_path,
                "requested_root": display_path(root),
            }),
        ));
    }
    Ok(artifact)
}

fn check_versions(
    metadata: &BTreeMap<String, String>,
    strict_schema: bool,
) -> Result<(), CommandError> {
    let sqlite_schema_version = metadata_i64(metadata, "sqlite_schema_version")?;
    let schema_version = metadata_i64(metadata, "schema_version")?;
    let extract_contract_version = metadata_i64(metadata, "extract_contract_version")?;

    if sqlite_schema_version > SQLITE_SCHEMA_VERSION || schema_version > SQLITE_SCHEMA_VERSION {
        return Err(command_error(
            3,
            ReportCode::SchemaIncompatible,
            "artifact schema version is newer than this binary supports",
            None,
            None,
            false,
            json!({
                "supported_sqlite_schema_version": SQLITE_SCHEMA_VERSION,
                "artifact_sqlite_schema_version": sqlite_schema_version,
                "artifact_schema_version": schema_version,
            }),
        ));
    }
    if strict_schema
        && (sqlite_schema_version != SQLITE_SCHEMA_VERSION
            || schema_version != SQLITE_SCHEMA_VERSION)
    {
        return Err(command_error(
            3,
            ReportCode::SchemaMigrationRequired,
            "artifact schema migration is required",
            None,
            None,
            true,
            json!({
                "required_sqlite_schema_version": SQLITE_SCHEMA_VERSION,
                "artifact_sqlite_schema_version": sqlite_schema_version,
                "artifact_schema_version": schema_version,
            }),
        ));
    }
    if extract_contract_version != EXTRACT_CONTRACT_VERSION {
        return Err(command_error(
            3,
            ReportCode::ContractIncompatible,
            "artifact extraction contract version is incompatible",
            None,
            None,
            false,
            json!({
                "supported_extract_contract_version": EXTRACT_CONTRACT_VERSION,
                "artifact_extract_contract_version": extract_contract_version,
            }),
        ));
    }
    Ok(())
}

fn artifact_report(
    db_path: &Path,
    metadata: &BTreeMap<String, String>,
    jsonl_schema_version: Option<i64>,
) -> Result<ArtifactReport, CommandError> {
    Ok(ArtifactReport {
        db_path: display_path(db_path),
        root_path: metadata_string(metadata, "root_path")?,
        artifact_id: metadata_string(metadata, "artifact_id")?,
        schema_version: metadata_i64(metadata, "schema_version")?,
        extract_contract_version: metadata_i64(metadata, "extract_contract_version")?,
        sqlite_schema_version: metadata_i64(metadata, "sqlite_schema_version")?,
        jsonl_schema_version,
        hash_algorithm: metadata_string(metadata, "hash_algorithm")?,
        parser_inventory_fingerprint: metadata_string(metadata, "parser_inventory_fingerprint")?,
        capability_snapshot_fingerprint: metadata_string(
            metadata,
            "capability_snapshot_fingerprint",
        )?,
    })
}

fn artifact_metadata_from_rows(
    metadata: &BTreeMap<String, String>,
) -> Result<ArtifactMetadata, CommandError> {
    Ok(ArtifactMetadata {
        artifact_id: metadata_string(metadata, "artifact_id")?,
        root_path: metadata_string(metadata, "root_path")?,
        binary_version: metadata_string(metadata, "binary_version")?,
        hash_algorithm: metadata_string(metadata, "hash_algorithm")?,
        parser_inventory_fingerprint: metadata_string(metadata, "parser_inventory_fingerprint")?,
        capability_snapshot_fingerprint: metadata_string(
            metadata,
            "capability_snapshot_fingerprint",
        )?,
        created_at: metadata_string(metadata, "created_at")?,
        updated_at: metadata_string(metadata, "updated_at")?,
    })
}

fn missing_metadata_warnings(metadata: &BTreeMap<String, String>) -> Vec<ReportDiagnostic> {
    REQUIRED_METADATA_KEYS
        .iter()
        .filter(|key| !metadata.contains_key(**key))
        .map(|key| {
            diagnostic(
                ReportCode::MetadataMissing,
                format!("artifact is missing metadata key {key}"),
                None,
                None,
                true,
                json!({"missing_key": key}),
            )
        })
        .collect()
}

fn metadata_string(
    metadata: &BTreeMap<String, String>,
    key: &'static str,
) -> Result<String, CommandError> {
    metadata.get(key).cloned().ok_or_else(|| {
        command_error(
            3,
            ReportCode::SchemaIncompatible,
            format!("artifact is missing metadata key {key}"),
            None,
            None,
            false,
            json!({"missing_key": key}),
        )
    })
}

fn metadata_i64(
    metadata: &BTreeMap<String, String>,
    key: &'static str,
) -> Result<i64, CommandError> {
    let value = metadata_string(metadata, key)?;
    value.parse::<i64>().map_err(|error| {
        command_error(
            3,
            ReportCode::SchemaIncompatible,
            format!("artifact metadata key {key} is not an integer: {error}"),
            None,
            None,
            false,
            json!({"key": key, "value": value}),
        )
    })
}

pub(crate) fn table_totals(connection: &Connection) -> RowDomainCounts {
    RowDomainCounts {
        artifact_metadata: table_count(connection, "artifact_metadata"),
        parser_inventory: table_count(connection, "parser_inventory"),
        language_capabilities: table_count(connection, "language_capabilities"),
        language_capability_fixtures: table_count(connection, "language_capability_fixtures"),
        language_capability_gaps: table_count(connection, "language_capability_gaps"),
        extraction_revisions: table_count(connection, "extraction_revisions"),
        revision_file_changes: table_count(connection, "revision_file_changes"),
        files: table_count(connection, "files"),
        symbols: table_count(connection, "symbols"),
        symbol_annotations: table_count(connection, "symbol_annotations"),
        identifiers: table_count(connection, "identifiers"),
        relationships: table_count(connection, "relationships"),
        pending_relationships: table_count(connection, "pending_relationships"),
        type_facts: table_count(connection, "type_facts"),
        type_argument_usages: table_count(connection, "type_argument_usages"),
        type_arguments: table_count(connection, "type_arguments"),
        literals: table_count(connection, "literals"),
        source_regions: table_count(connection, "source_regions"),
        structural_facts: table_count(connection, "structural_facts"),
        complexity_metrics: table_count(connection, "complexity_metrics"),
        parse_diagnostics: table_count(connection, "parse_diagnostics"),
        pending_resolutions: table_count(connection, "pending_resolutions"),
        identifier_resolutions: table_count(connection, "identifier_resolutions"),
    }
}

#[derive(Debug, Default)]
pub(crate) struct FileRowAttribution {
    pub(crate) rows: Vec<ReportFileRows>,
    pub(crate) truncated: bool,
}

#[derive(Debug)]
struct FileRowAccumulator {
    path: String,
    language: String,
    status: String,
    rows: RowDomainCounts,
}

type CountSetter = fn(&mut RowDomainCounts, i64);
type CountQuery = (&'static str, CountSetter);

pub(crate) fn file_row_attribution(
    connection: &Connection,
    limit: Option<usize>,
) -> FileRowAttribution {
    let mut files = match file_row_accumulators(connection) {
        Ok(files) => files,
        Err(_) => return FileRowAttribution::default(),
    };

    let count_queries: [CountQuery; 13] = [
        (
            "SELECT file_id, COUNT(*) FROM symbols GROUP BY file_id",
            set_symbols,
        ),
        (
            "SELECT s.file_id, COUNT(*)
             FROM symbol_annotations a
             JOIN symbols s ON s.symbol_id = a.symbol_id
             GROUP BY s.file_id",
            set_symbol_annotations,
        ),
        (
            "SELECT file_id, COUNT(*) FROM identifiers GROUP BY file_id",
            set_identifiers,
        ),
        (
            "SELECT file_id, COUNT(*) FROM relationships GROUP BY file_id",
            set_relationships,
        ),
        (
            "SELECT file_id, COUNT(*) FROM pending_relationships GROUP BY file_id",
            set_pending_relationships,
        ),
        (
            "SELECT s.file_id, COUNT(*)
             FROM type_facts t
             JOIN symbols s ON s.symbol_id = t.symbol_id
             GROUP BY s.file_id",
            set_type_facts,
        ),
        (
            "SELECT file_id, COUNT(*) FROM type_argument_usages GROUP BY file_id",
            set_type_argument_usages,
        ),
        (
            "SELECT u.file_id, COUNT(*)
             FROM type_arguments a
             JOIN type_argument_usages u ON u.usage_id = a.usage_id
             GROUP BY u.file_id",
            set_type_arguments,
        ),
        (
            "SELECT file_id, COUNT(*) FROM literals GROUP BY file_id",
            set_literals,
        ),
        (
            "SELECT file_id, COUNT(*) FROM source_regions GROUP BY file_id",
            set_source_regions,
        ),
        (
            "SELECT file_id, COUNT(*) FROM structural_facts GROUP BY file_id",
            set_structural_facts,
        ),
        (
            "SELECT file_id, COUNT(*) FROM complexity_metrics GROUP BY file_id",
            set_complexity_metrics,
        ),
        (
            "SELECT file_id, COUNT(*) FROM parse_diagnostics GROUP BY file_id",
            set_parse_diagnostics,
        ),
    ];

    for (sql, setter) in count_queries {
        if add_grouped_counts(connection, sql, &mut files, setter).is_err() {
            return FileRowAttribution::default();
        }
    }

    let mut rows = files
        .into_values()
        .map(|file| {
            let total_rows = file_attributed_total(&file.rows);
            ReportFileRows {
                path: file.path,
                language: file.language,
                status: file.status,
                total_rows,
                rows: file.rows,
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .total_rows
            .cmp(&left.total_rows)
            .then_with(|| left.path.cmp(&right.path))
    });

    let truncated = limit.is_some_and(|limit| rows.len() > limit);
    if let Some(limit) = limit {
        rows.truncate(limit);
    }
    FileRowAttribution { rows, truncated }
}

fn table_count(connection: &Connection, table: &str) -> i64 {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    connection
        .query_row(&sql, [], |row| row.get(0))
        .unwrap_or(0)
}

fn file_row_accumulators(
    connection: &Connection,
) -> rusqlite::Result<BTreeMap<String, FileRowAccumulator>> {
    let mut stmt = connection.prepare("SELECT file_id, path, language, status FROM files")?;
    let mut rows = stmt.query([])?;
    let mut files = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let file_id: String = row.get(0)?;
        files.insert(
            file_id,
            FileRowAccumulator {
                path: row.get(1)?,
                language: row.get(2)?,
                status: row.get(3)?,
                rows: RowDomainCounts {
                    files: 1,
                    ..RowDomainCounts::default()
                },
            },
        );
    }
    Ok(files)
}

fn add_grouped_counts(
    connection: &Connection,
    sql: &str,
    files: &mut BTreeMap<String, FileRowAccumulator>,
    setter: fn(&mut RowDomainCounts, i64),
) -> rusqlite::Result<()> {
    let mut stmt = connection.prepare(sql)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let file_id: String = row.get(0)?;
        let count: i64 = row.get(1)?;
        if let Some(file) = files.get_mut(&file_id) {
            setter(&mut file.rows, count);
        }
    }
    Ok(())
}

fn file_attributed_total(rows: &RowDomainCounts) -> i64 {
    rows.files
        + rows.symbols
        + rows.symbol_annotations
        + rows.identifiers
        + rows.relationships
        + rows.pending_relationships
        + rows.type_facts
        + rows.type_argument_usages
        + rows.type_arguments
        + rows.literals
        + rows.source_regions
        + rows.structural_facts
        + rows.complexity_metrics
        + rows.parse_diagnostics
}

fn set_symbols(rows: &mut RowDomainCounts, count: i64) {
    rows.symbols = count;
}

fn set_symbol_annotations(rows: &mut RowDomainCounts, count: i64) {
    rows.symbol_annotations = count;
}

fn set_identifiers(rows: &mut RowDomainCounts, count: i64) {
    rows.identifiers = count;
}

fn set_relationships(rows: &mut RowDomainCounts, count: i64) {
    rows.relationships = count;
}

fn set_pending_relationships(rows: &mut RowDomainCounts, count: i64) {
    rows.pending_relationships = count;
}

fn set_type_facts(rows: &mut RowDomainCounts, count: i64) {
    rows.type_facts = count;
}

fn set_type_argument_usages(rows: &mut RowDomainCounts, count: i64) {
    rows.type_argument_usages = count;
}

fn set_type_arguments(rows: &mut RowDomainCounts, count: i64) {
    rows.type_arguments = count;
}

fn set_literals(rows: &mut RowDomainCounts, count: i64) {
    rows.literals = count;
}

fn set_source_regions(rows: &mut RowDomainCounts, count: i64) {
    rows.source_regions = count;
}

fn set_structural_facts(rows: &mut RowDomainCounts, count: i64) {
    rows.structural_facts = count;
}

fn set_complexity_metrics(rows: &mut RowDomainCounts, count: i64) {
    rows.complexity_metrics = count;
}

fn set_parse_diagnostics(rows: &mut RowDomainCounts, count: i64) {
    rows.parse_diagnostics = count;
}

pub(crate) fn latest_revision_id(connection: &Connection) -> Option<i64> {
    connection
        .query_row(
            "SELECT MAX(revision_id) FROM extraction_revisions",
            [],
            |row| row.get(0),
        )
        .unwrap_or(None)
}

pub(crate) fn jsonl_counts(records_by_kind: &BTreeMap<&'static str, usize>) -> RowDomainCounts {
    let mut counts = RowDomainCounts::default();
    for kind in JSONL_RECORD_KINDS {
        let count = records_by_kind.get(kind).copied().unwrap_or(0) as i64;
        match *kind {
            "artifact" => counts.artifact_metadata = count,
            "parser_inventory" => counts.parser_inventory = count,
            "language_capability" => counts.language_capabilities = count,
            "language_capability_fixture" => counts.language_capability_fixtures = count,
            "language_capability_gap" => counts.language_capability_gaps = count,
            "revision" => counts.extraction_revisions = count,
            "revision_file_change" => counts.revision_file_changes = count,
            "file" => counts.files = count,
            "symbol" => counts.symbols = count,
            "symbol_annotation" => counts.symbol_annotations = count,
            "identifier" => counts.identifiers = count,
            "relationship" => counts.relationships = count,
            "pending_relationship" => counts.pending_relationships = count,
            "type_fact" => counts.type_facts = count,
            "type_argument_usage" => counts.type_argument_usages = count,
            "type_argument" => counts.type_arguments = count,
            "literal" => counts.literals = count,
            "source_region" => counts.source_regions = count,
            "structural_fact" => counts.structural_facts = count,
            "complexity_metric" => counts.complexity_metrics = count,
            "parse_diagnostic" => counts.parse_diagnostics = count,
            _ => {}
        }
    }
    counts
}
