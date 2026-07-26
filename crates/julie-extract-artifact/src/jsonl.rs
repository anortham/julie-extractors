use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::metadata::{REQUIRED_METADATA_KEYS, read_metadata};
use crate::resolution_store::{
    KEY_RESOLUTION_LAST_FULL_REVISION, KEY_RESOLUTION_STATUS, KEY_RESOLUTION_VERSION,
};
use crate::schema::EXTRACT_CONTRACT_VERSION;

pub const JSONL_SCHEMA_VERSION: i64 = 4;

pub const JSONL_RECORD_KINDS: &[&str] = &[
    "artifact",
    "parser_inventory",
    "language_capability",
    "language_capability_fixture",
    "language_capability_gap",
    "revision",
    "revision_file_change",
    "file",
    "symbol",
    "symbol_annotation",
    "reference_site",
    "identifier",
    "relationship",
    "pending_relationship",
    "type_fact",
    "type_argument_usage",
    "type_argument",
    "literal",
    "source_region",
    "complexity_metric",
    "structural_fact",
    "parse_diagnostic",
];

const JSONL_EXPORT_BUFFER_BYTES: usize = 64 * 1024;

pub type JsonlExportResult<T> = Result<T, JsonlExportError>;

#[derive(Debug)]
pub enum JsonlExportError {
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Json {
        column: &'static str,
        source: serde_json::Error,
    },
    InvalidJsonShape {
        column: &'static str,
        expected: &'static str,
    },
    MissingMetadata {
        key: &'static str,
    },
    InvalidMetadata {
        key: &'static str,
        value: String,
    },
}

impl std::fmt::Display for JsonlExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JsonlExportError::Io(error) => write!(f, "{error}"),
            JsonlExportError::Sqlite(error) => write!(f, "{error}"),
            JsonlExportError::Json { column, source } => {
                write!(f, "invalid JSON in {column}: {source}")
            }
            JsonlExportError::InvalidJsonShape { column, expected } => {
                write!(f, "invalid JSON shape in {column}: expected {expected}")
            }
            JsonlExportError::MissingMetadata { key } => {
                write!(f, "missing required artifact metadata key {key}")
            }
            JsonlExportError::InvalidMetadata { key, value } => {
                write!(f, "invalid artifact metadata value for {key}: {value}")
            }
        }
    }
}

impl std::error::Error for JsonlExportError {}

impl From<io::Error> for JsonlExportError {
    fn from(value: io::Error) -> Self {
        JsonlExportError::Io(value)
    }
}

impl From<rusqlite::Error> for JsonlExportError {
    fn from(value: rusqlite::Error) -> Self {
        JsonlExportError::Sqlite(value)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JsonlExportSummary {
    pub total_records: usize,
    pub records_by_kind: BTreeMap<&'static str, usize>,
}

impl JsonlExportSummary {
    fn record_written(&mut self, kind: &'static str) {
        self.total_records += 1;
        *self.records_by_kind.entry(kind).or_insert(0) += 1;
    }
}

pub fn export_jsonl<W: Write>(
    conn: &Connection,
    writer: W,
) -> JsonlExportResult<JsonlExportSummary> {
    let mut writer = BufWriter::with_capacity(JSONL_EXPORT_BUFFER_BYTES, writer);
    let metadata = load_required_metadata(conn)?;
    let artifact_id = required_metadata(&metadata, "artifact_id")?;
    let mut summary = JsonlExportSummary::default();

    export_artifact(&mut writer, &metadata, artifact_id, &mut summary)?;
    export_parser_inventory(conn, &mut writer, artifact_id, &mut summary)?;
    export_language_capabilities(conn, &mut writer, artifact_id, &mut summary)?;
    export_language_capability_fixtures(conn, &mut writer, artifact_id, &mut summary)?;
    export_language_capability_gaps(conn, &mut writer, artifact_id, &mut summary)?;
    export_revisions(conn, &mut writer, artifact_id, &mut summary)?;
    export_revision_file_changes(conn, &mut writer, artifact_id, &mut summary)?;
    export_files(conn, &mut writer, artifact_id, &mut summary)?;
    export_symbols(conn, &mut writer, artifact_id, &mut summary)?;
    export_symbol_annotations(conn, &mut writer, artifact_id, &mut summary)?;
    export_reference_sites(conn, &mut writer, artifact_id, &mut summary)?;
    export_identifiers(conn, &mut writer, artifact_id, &mut summary)?;
    export_relationships(conn, &mut writer, artifact_id, &mut summary)?;
    export_pending_relationships(conn, &mut writer, artifact_id, &mut summary)?;
    export_type_facts(conn, &mut writer, artifact_id, &mut summary)?;
    export_type_argument_usages(conn, &mut writer, artifact_id, &mut summary)?;
    export_type_arguments(conn, &mut writer, artifact_id, &mut summary)?;
    export_literals(conn, &mut writer, artifact_id, &mut summary)?;
    export_source_regions(conn, &mut writer, artifact_id, &mut summary)?;
    export_complexity_metrics(conn, &mut writer, artifact_id, &mut summary)?;
    export_structural_facts(conn, &mut writer, artifact_id, &mut summary)?;
    export_parse_diagnostics(conn, &mut writer, artifact_id, &mut summary)?;

    writer.flush()?;
    Ok(summary)
}

pub fn export_jsonl_to_path(
    conn: &Connection,
    output_path: impl AsRef<Path>,
) -> JsonlExportResult<JsonlExportSummary> {
    let output_path = output_path.as_ref();
    let temp_path = unique_temp_output_path(output_path);
    let result = File::create(&temp_path)
        .map_err(JsonlExportError::Io)
        .and_then(|file| export_jsonl(conn, file));

    match result {
        Ok(summary) => {
            if let Err(error) = fs::rename(&temp_path, output_path) {
                let _ = fs::remove_file(&temp_path);
                return Err(JsonlExportError::Io(error));
            }
            Ok(summary)
        }
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            Err(error)
        }
    }
}

fn unique_temp_output_path(output_path: &Path) -> PathBuf {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = output_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("julie-extract-export");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    parent.join(format!(".{file_name}.{}.{nanos}.tmp", std::process::id()))
}

fn load_required_metadata(conn: &Connection) -> JsonlExportResult<BTreeMap<String, String>> {
    let metadata = read_metadata(conn)?;
    for key in REQUIRED_METADATA_KEYS {
        if !metadata.contains_key(*key) {
            return Err(JsonlExportError::MissingMetadata { key });
        }
    }
    Ok(metadata)
}

fn export_artifact<W: Write>(
    writer: &mut W,
    metadata: &BTreeMap<String, String>,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let record = json!({
        "artifact_id": artifact_id,
        "root_path": required_metadata(metadata, "root_path")?,
        "schema_version": metadata_i64(metadata, "schema_version")?,
        "extract_contract_version": metadata_i64(metadata, "extract_contract_version")?,
        "sqlite_schema_version": metadata_i64(metadata, "sqlite_schema_version")?,
        "binary_version": required_metadata(metadata, "binary_version")?,
        "hash_algorithm": required_metadata(metadata, "hash_algorithm")?,
        "parser_inventory_fingerprint": required_metadata(metadata, "parser_inventory_fingerprint")?,
        "capability_snapshot_fingerprint": required_metadata(metadata, "capability_snapshot_fingerprint")?,
        "created_at": required_metadata(metadata, "created_at")?,
        "updated_at": required_metadata(metadata, "updated_at")?,
        "reference_resolution_status": metadata.get(KEY_RESOLUTION_STATUS),
        "reference_resolution_version": optional_metadata_i64(metadata, KEY_RESOLUTION_VERSION)?,
        "reference_resolution_last_full_revision":
            optional_metadata_i64(metadata, KEY_RESOLUTION_LAST_FULL_REVISION)?,
    });
    write_record(
        writer,
        artifact_id,
        "artifact",
        artifact_id,
        record,
        summary,
    )
}

fn export_parser_inventory<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT language, parser_package, parser_version, grammar_version, source, metadata_json
         FROM parser_inventory
         ORDER BY language, parser_package",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    for row in rows {
        let (language, parser_package, parser_version, grammar_version, source, metadata_json) =
            row?;
        let record_id = format!("{language}:{parser_package}");
        let record = json!({
            "language": language,
            "parser_package": parser_package,
            "parser_version": parser_version,
            "grammar_version": grammar_version,
            "source": source,
            "metadata": optional_object("parser_inventory.metadata_json", metadata_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "parser_inventory",
            &record_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_language_capabilities<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT language, parser_package, extensions_json, dependency_status,
                target_symbols, target_relationships, target_pending_relationships,
                target_identifiers, target_types, actual_symbols, actual_relationships,
                actual_pending_relationships, actual_identifiers, actual_types, kind_coverage_json
         FROM language_capabilities
         ORDER BY language",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, i64>(12)?,
            row.get::<_, i64>(13)?,
            row.get::<_, String>(14)?,
        ))
    })?;
    for row in rows {
        let (
            language,
            parser_package,
            extensions_json,
            dependency_status,
            target_symbols,
            target_relationships,
            target_pending_relationships,
            target_identifiers,
            target_types,
            actual_symbols,
            actual_relationships,
            actual_pending_relationships,
            actual_identifiers,
            actual_types,
            kind_coverage_json,
        ) = row?;
        let record = json!({
            "language": language,
            "parser_package": parser_package,
            "extensions": required_array("language_capabilities.extensions_json", extensions_json)?,
            "dependency_status": dependency_status,
            "target_capabilities": capability_flags(
                target_symbols,
                target_relationships,
                target_pending_relationships,
                target_identifiers,
                target_types,
            ),
            "actual_capabilities": capability_flags(
                actual_symbols,
                actual_relationships,
                actual_pending_relationships,
                actual_identifiers,
                actual_types,
            ),
            "kind_coverage": required_object("language_capabilities.kind_coverage_json", kind_coverage_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "language_capability",
            &language,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_language_capability_fixtures<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT language, fixture_name, source_path, expected_path
         FROM language_capability_fixtures
         ORDER BY language, fixture_name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (language, fixture_name, source_path, expected_path) = row?;
        let record_id = format!("{language}:{fixture_name}");
        let record = json!({
            "language": language,
            "fixture_name": fixture_name,
            "source_path": source_path,
            "expected_path": expected_path,
        });
        write_record(
            writer,
            artifact_id,
            "language_capability_fixture",
            &record_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_language_capability_gaps<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT gap_id, language, capability, status, reason, required_closure, evidence_json
         FROM language_capability_gaps
         ORDER BY gap_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
        ))
    })?;
    for row in rows {
        let (gap_id, language, capability, status, reason, required_closure, evidence_json) = row?;
        let record = json!({
            "gap_id": gap_id,
            "language": language,
            "capability": capability,
            "status": status,
            "reason": reason,
            "required_closure": required_closure,
            "evidence": required_object("language_capability_gaps.evidence_json", evidence_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "language_capability_gap",
            &gap_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_revisions<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT revision_id, parent_revision_id, operation, mode, started_at, completed_at,
                binary_version, extract_contract_version, sqlite_schema_version, input_root,
                counts_json
         FROM extraction_revisions
         ORDER BY revision_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<i64>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, String>(10)?,
        ))
    })?;
    for row in rows {
        let (
            revision_id,
            parent_revision_id,
            operation,
            mode,
            started_at,
            completed_at,
            binary_version,
            extract_contract_version,
            sqlite_schema_version,
            input_root,
            counts_json,
        ) = row?;
        let record = json!({
            "revision_id": revision_id,
            "parent_revision_id": parent_revision_id,
            "operation": operation,
            "mode": mode,
            "started_at": started_at,
            "completed_at": completed_at,
            "binary_version": binary_version,
            "extract_contract_version": extract_contract_version,
            "sqlite_schema_version": sqlite_schema_version,
            "input_root": input_root,
            "counts": required_object("extraction_revisions.counts_json", counts_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "revision",
            &revision_id.to_string(),
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_revision_file_changes<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT revision_id, file_id, path, change_kind
         FROM revision_file_changes
         ORDER BY revision_id, file_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (revision_id, file_id, path, change_kind) = row?;
        let record_id = format!("{revision_id}:{file_id}");
        let record = json!({
            "revision_id": revision_id,
            "file_id": file_id,
            "path": path,
            "change_kind": change_kind,
        });
        write_record(
            writer,
            artifact_id,
            "revision_file_change",
            &record_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_files<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT file_id, path, language, content_hash, content_bytes, line_count, indexed_at,
                last_revision_id, status, metadata_json
         FROM files
         ORDER BY file_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, Option<String>>(9)?,
        ))
    })?;
    for row in rows {
        let (
            file_id,
            path,
            language,
            content_hash,
            content_bytes,
            line_count,
            indexed_at,
            last_revision_id,
            status,
            metadata_json,
        ) = row?;
        let record = json!({
            "file_id": file_id,
            "path": path,
            "language": language,
            "content_hash": content_hash,
            "content_bytes": content_bytes,
            "line_count": line_count,
            "indexed_at": indexed_at,
            "last_revision_id": last_revision_id,
            "status": status,
            "metadata": object_or_empty("files.metadata_json", metadata_json)?,
        });
        write_record(writer, artifact_id, "file", &file_id, record, summary)?;
    }
    Ok(())
}

fn export_symbols<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT symbol_id, file_id, path, language, name, kind, signature, doc_comment,
                visibility, parent_symbol_id, start_line, start_column, end_line, end_column,
                start_byte, end_byte, body_start_line, body_start_column, body_end_line,
                body_end_column, body_start_byte, body_end_byte, body_hash, semantic_group,
                confidence, content_type, is_test, test_container, test_lifecycle, metadata_json
         FROM symbols
         ORDER BY symbol_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, i64>(12)?,
            row.get::<_, i64>(13)?,
            row.get::<_, i64>(14)?,
            row.get::<_, i64>(15)?,
            row.get::<_, Option<i64>>(16)?,
            row.get::<_, Option<i64>>(17)?,
            row.get::<_, Option<i64>>(18)?,
            row.get::<_, Option<i64>>(19)?,
            row.get::<_, Option<i64>>(20)?,
            row.get::<_, Option<i64>>(21)?,
            row.get::<_, Option<String>>(22)?,
            row.get::<_, Option<String>>(23)?,
            row.get::<_, Option<f64>>(24)?,
            row.get::<_, Option<String>>(25)?,
            row.get::<_, bool>(26)?,
            row.get::<_, bool>(27)?,
            row.get::<_, bool>(28)?,
            row.get::<_, Option<String>>(29)?,
        ))
    })?;
    for row in rows {
        let (
            symbol_id,
            file_id,
            path,
            language,
            name,
            kind,
            signature,
            doc_comment,
            visibility,
            parent_symbol_id,
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
            body_start_line,
            body_start_column,
            body_end_line,
            body_end_column,
            body_start_byte,
            body_end_byte,
            body_hash,
            semantic_group,
            confidence,
            content_type,
            is_test,
            test_container,
            test_lifecycle,
            metadata_json,
        ) = row?;
        let record = json!({
            "symbol_id": symbol_id,
            "file_id": file_id,
            "path": path,
            "language": language,
            "name": name,
            "kind": kind,
            "signature": signature,
            "doc_comment": doc_comment,
            "visibility": visibility,
            "parent_symbol_id": parent_symbol_id,
            "span": span(start_line, start_column, end_line, end_column, start_byte, end_byte),
            "body_span": optional_complete_span(
                body_start_line,
                body_start_column,
                body_end_line,
                body_end_column,
                body_start_byte,
                body_end_byte,
            ),
            "body_hash": body_hash,
            "semantic_group": semantic_group,
            "confidence": confidence,
            "content_type": content_type,
            "is_test": is_test,
            "test_container": test_container,
            "test_lifecycle": test_lifecycle,
            "metadata": object_or_empty("symbols.metadata_json", metadata_json)?,
        });
        write_record(writer, artifact_id, "symbol", &symbol_id, record, summary)?;
    }
    Ok(())
}

fn export_symbol_annotations<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT annotation_id, symbol_id, annotation, annotation_key, raw_text, carrier,
                metadata_json
         FROM symbol_annotations
         ORDER BY annotation_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
        ))
    })?;
    for row in rows {
        let (
            annotation_id,
            symbol_id,
            annotation,
            annotation_key,
            raw_text,
            carrier,
            metadata_json,
        ) = row?;
        let record = json!({
            "annotation_id": annotation_id,
            "symbol_id": symbol_id,
            "annotation": annotation,
            "annotation_key": annotation_key,
            "raw_text": raw_text,
            "carrier": carrier,
            "metadata": optional_object("symbol_annotations.metadata_json", metadata_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "symbol_annotation",
            &annotation_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_reference_sites<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT reference_site_id, file_id, path, language, containing_symbol_id,
                start_line, start_column, end_line, end_column, start_byte, end_byte,
                is_exact, provenance
         FROM reference_sites
         ORDER BY reference_site_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<i64>>(7)?,
            row.get::<_, Option<i64>>(8)?,
            row.get::<_, Option<i64>>(9)?,
            row.get::<_, Option<i64>>(10)?,
            row.get::<_, bool>(11)?,
            row.get::<_, String>(12)?,
        ))
    })?;
    for row in rows {
        let (
            reference_site_id,
            file_id,
            path,
            language,
            containing_symbol_id,
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
            is_exact,
            provenance,
        ) = row?;
        let record = json!({
            "reference_site_id": reference_site_id,
            "file_id": file_id,
            "path": path,
            "language": language,
            "containing_symbol_id": containing_symbol_id,
            "span": reference_span(
                start_line,
                start_column,
                end_line,
                end_column,
                start_byte,
                end_byte,
            ),
            "is_exact": is_exact,
            "provenance": provenance,
        });
        write_record(
            writer,
            artifact_id,
            "reference_site",
            &reference_site_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_identifiers<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT identifier_id, reference_site_id, file_id, path, language, name, kind, containing_symbol_id,
                target_symbol_id, start_line, start_column, end_line, end_column, start_byte,
                end_byte, confidence, code_context, metadata_json
         FROM identifiers
         ORDER BY identifier_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, i64>(12)?,
            row.get::<_, i64>(13)?,
            row.get::<_, i64>(14)?,
            row.get::<_, f64>(15)?,
            row.get::<_, Option<String>>(16)?,
            row.get::<_, Option<String>>(17)?,
        ))
    })?;
    for row in rows {
        let (
            identifier_id,
            reference_site_id,
            file_id,
            path,
            language,
            name,
            kind,
            containing_symbol_id,
            target_symbol_id,
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
            confidence,
            code_context,
            metadata_json,
        ) = row?;
        let record = json!({
            "identifier_id": identifier_id,
            "reference_site_id": reference_site_id,
            "file_id": file_id,
            "path": path,
            "language": language,
            "name": name,
            "kind": kind,
            "containing_symbol_id": containing_symbol_id,
            "target_symbol_id": target_symbol_id,
            "span": span(start_line, start_column, end_line, end_column, start_byte, end_byte),
            "confidence": confidence,
            "code_context": code_context,
            "metadata": optional_object("identifiers.metadata_json", metadata_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "identifier",
            &identifier_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_relationships<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT relationship_id, reference_site_id, from_symbol_id, to_symbol_id, file_id, path, kind, start_line,
                start_column, end_line, end_column, start_byte, end_byte, confidence,
                metadata_json
         FROM relationships
         ORDER BY relationship_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<i64>>(7)?,
            row.get::<_, Option<i64>>(8)?,
            row.get::<_, Option<i64>>(9)?,
            row.get::<_, Option<i64>>(10)?,
            row.get::<_, Option<i64>>(11)?,
            row.get::<_, Option<i64>>(12)?,
            row.get::<_, f64>(13)?,
            row.get::<_, Option<String>>(14)?,
        ))
    })?;
    for row in rows {
        let (
            relationship_id,
            reference_site_id,
            from_symbol_id,
            to_symbol_id,
            file_id,
            path,
            kind,
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
            confidence,
            metadata_json,
        ) = row?;
        let record = json!({
            "relationship_id": relationship_id,
            "reference_site_id": reference_site_id,
            "from_symbol_id": from_symbol_id,
            "to_symbol_id": to_symbol_id,
            "file_id": file_id,
            "path": path,
            "kind": kind,
            "span": optional_complete_span(
                start_line,
                start_column,
                end_line,
                end_column,
                start_byte,
                end_byte,
            ),
            "confidence": confidence,
            "metadata": optional_object("relationships.metadata_json", metadata_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "relationship",
            &relationship_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_pending_relationships<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT pending_relationship_id, reference_site_id, from_symbol_id, caller_scope_symbol_id, file_id, path,
                kind, target_display_name, target_terminal_name, target_receiver,
                target_namespace_json, target_import_context, start_line, start_column,
                end_line, end_column, start_byte, end_byte, confidence, metadata_json
         FROM pending_relationships
         ORDER BY pending_relationship_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, String>(10)?,
            row.get::<_, Option<String>>(11)?,
            row.get::<_, i64>(12)?,
            row.get::<_, Option<i64>>(13)?,
            row.get::<_, Option<i64>>(14)?,
            row.get::<_, Option<i64>>(15)?,
            row.get::<_, Option<i64>>(16)?,
            row.get::<_, Option<i64>>(17)?,
            row.get::<_, f64>(18)?,
            row.get::<_, Option<String>>(19)?,
        ))
    })?;
    for row in rows {
        let (
            pending_relationship_id,
            reference_site_id,
            from_symbol_id,
            caller_scope_symbol_id,
            file_id,
            path,
            kind,
            target_display_name,
            target_terminal_name,
            target_receiver,
            target_namespace_json,
            target_import_context,
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
            confidence,
            metadata_json,
        ) = row?;
        let record = json!({
            "pending_relationship_id": pending_relationship_id,
            "reference_site_id": reference_site_id,
            "from_symbol_id": from_symbol_id,
            "caller_scope_symbol_id": caller_scope_symbol_id,
            "file_id": file_id,
            "path": path,
            "kind": kind,
            "target": {
                "display_name": target_display_name,
                "terminal_name": target_terminal_name,
                "receiver": target_receiver,
                "namespace": required_array(
                    "pending_relationships.target_namespace_json",
                    target_namespace_json,
                )?,
                "import_context": target_import_context,
            },
            "site": partial_span(
                start_line,
                start_column,
                end_line,
                end_column,
                start_byte,
                end_byte,
            ),
            "confidence": confidence,
            "metadata": optional_object("pending_relationships.metadata_json", metadata_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "pending_relationship",
            &pending_relationship_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_type_facts<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT type_fact_id, symbol_id, language, resolved_type, generic_params_json,
                constraints_json, is_inferred, metadata_json
         FROM type_facts
         ORDER BY type_fact_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;
    for row in rows {
        let (
            type_fact_id,
            symbol_id,
            language,
            resolved_type,
            generic_params_json,
            constraints_json,
            is_inferred,
            metadata_json,
        ) = row?;
        let record = json!({
            "type_fact_id": type_fact_id,
            "symbol_id": symbol_id,
            "language": language,
            "resolved_type": resolved_type,
            "generic_params": optional_array("type_facts.generic_params_json", generic_params_json)?,
            "constraints": optional_array("type_facts.constraints_json", constraints_json)?,
            "is_inferred": is_inferred != 0,
            "metadata": optional_object("type_facts.metadata_json", metadata_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "type_fact",
            &type_fact_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_type_argument_usages<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT usage_id, identifier_id, file_id, path, language, metadata_json
         FROM type_argument_usages
         ORDER BY usage_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    for row in rows {
        let (usage_id, identifier_id, file_id, path, language, metadata_json) = row?;
        let record = json!({
            "usage_id": usage_id,
            "identifier_id": identifier_id,
            "file_id": file_id,
            "path": path,
            "language": language,
            "metadata": optional_object("type_argument_usages.metadata_json", metadata_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "type_argument_usage",
            &usage_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_type_arguments<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT type_argument_id, usage_id, parent_type_argument_id, ordinal, type_name
         FROM type_arguments
         ORDER BY type_argument_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    for row in rows {
        let (type_argument_id, usage_id, parent_type_argument_id, ordinal, type_name) = row?;
        let record = json!({
            "type_argument_id": type_argument_id,
            "usage_id": usage_id,
            "parent_type_argument_id": parent_type_argument_id,
            "ordinal": ordinal,
            "type_name": type_name,
        });
        write_record(
            writer,
            artifact_id,
            "type_argument",
            &type_argument_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_literals<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT literal_id, file_id, path, language, literal_text, kind, carrier, arg_position,
                containing_symbol_id, start_line, start_column, end_line, end_column, start_byte,
                end_byte, confidence, metadata_json
         FROM literals
         ORDER BY literal_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, i64>(12)?,
            row.get::<_, i64>(13)?,
            row.get::<_, i64>(14)?,
            row.get::<_, f64>(15)?,
            row.get::<_, Option<String>>(16)?,
        ))
    })?;
    for row in rows {
        let (
            literal_id,
            file_id,
            path,
            language,
            literal_text,
            kind,
            carrier,
            arg_position,
            containing_symbol_id,
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
            confidence,
            metadata_json,
        ) = row?;
        let record = json!({
            "literal_id": literal_id,
            "file_id": file_id,
            "path": path,
            "language": language,
            "literal_text": literal_text,
            "kind": kind,
            "carrier": carrier,
            "arg_position": arg_position,
            "containing_symbol_id": containing_symbol_id,
            "span": span(start_line, start_column, end_line, end_column, start_byte, end_byte),
            "confidence": confidence,
            "metadata": optional_object("literals.metadata_json", metadata_json)?,
        });
        write_record(writer, artifact_id, "literal", &literal_id, record, summary)?;
    }
    Ok(())
}

fn export_source_regions<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT source_region_id, file_id, path, language, kind, containing_symbol_id,
                start_line, start_column, end_line, end_column, start_byte, end_byte,
                metadata_json
         FROM source_regions
         ORDER BY path, start_byte, end_byte, kind, source_region_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, Option<String>>(12)?,
        ))
    })?;
    for row in rows {
        let (
            source_region_id,
            file_id,
            path,
            language,
            kind,
            containing_symbol_id,
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
            metadata_json,
        ) = row?;
        let record = json!({
            "source_region_id": source_region_id,
            "file_id": file_id,
            "path": path,
            "language": language,
            "kind": kind,
            "containing_symbol_id": containing_symbol_id,
            "span": span(start_line, start_column, end_line, end_column, start_byte, end_byte),
            "metadata": optional_object("source_regions.metadata_json", metadata_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "source_region",
            &source_region_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_structural_facts<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT structural_fact_id, file_id, path, language, pattern_id, capture_name,
                node_kind, containing_symbol_id, start_line, start_column, end_line,
                end_column, start_byte, end_byte, confidence, metadata_json
         FROM structural_facts
         ORDER BY path, start_byte, end_byte, pattern_id, capture_name, structural_fact_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, i64>(12)?,
            row.get::<_, i64>(13)?,
            row.get::<_, f64>(14)?,
            row.get::<_, Option<String>>(15)?,
        ))
    })?;
    for row in rows {
        let (
            structural_fact_id,
            file_id,
            path,
            language,
            pattern_id,
            capture_name,
            node_kind,
            containing_symbol_id,
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
            confidence,
            metadata_json,
        ) = row?;
        let metadata_json = optional_raw_object("structural_facts.metadata_json", metadata_json)?;
        write_record_raw_object(
            writer,
            artifact_id,
            "structural_fact",
            &structural_fact_id,
            summary,
            |writer| {
                let mut first = true;
                writer.write_all(b"{")?;
                write_json_field(
                    writer,
                    "structural_fact_id",
                    &structural_fact_id,
                    &mut first,
                )?;
                write_json_field(writer, "file_id", &file_id, &mut first)?;
                write_json_field(writer, "path", &path, &mut first)?;
                write_json_field(writer, "language", &language, &mut first)?;
                write_json_field(writer, "pattern_id", &pattern_id, &mut first)?;
                write_json_field(writer, "capture_name", &capture_name, &mut first)?;
                write_json_field(writer, "node_kind", &node_kind, &mut first)?;
                write_json_field(
                    writer,
                    "containing_symbol_id",
                    &containing_symbol_id,
                    &mut first,
                )?;
                write_json_field(
                    writer,
                    "span",
                    &span(
                        start_line,
                        start_column,
                        end_line,
                        end_column,
                        start_byte,
                        end_byte,
                    ),
                    &mut first,
                )?;
                write_json_field(writer, "confidence", &confidence, &mut first)?;
                write_raw_json_field(writer, "metadata", metadata_json.as_deref(), &mut first)?;
                writer.write_all(b"}")?;
                Ok(())
            },
        )?;
    }
    Ok(())
}

fn export_complexity_metrics<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT complexity_metric_id, file_id, path, language, scope, symbol_id, algorithm_id,
                covered_lines, covered_bytes, decision_count, loop_count, max_nesting_depth,
                parameter_count, start_line, start_column, end_line, end_column, start_byte,
                end_byte, metadata_json
         FROM complexity_metrics
         ORDER BY path, start_byte, end_byte, scope, symbol_id, complexity_metric_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, Option<i64>>(12)?,
            row.get::<_, i64>(13)?,
            row.get::<_, i64>(14)?,
            row.get::<_, i64>(15)?,
            row.get::<_, i64>(16)?,
            row.get::<_, i64>(17)?,
            row.get::<_, i64>(18)?,
            row.get::<_, Option<String>>(19)?,
        ))
    })?;
    for row in rows {
        let (
            complexity_metric_id,
            file_id,
            path,
            language,
            scope,
            symbol_id,
            algorithm_id,
            covered_lines,
            covered_bytes,
            decision_count,
            loop_count,
            max_nesting_depth,
            parameter_count,
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
            metadata_json,
        ) = row?;
        let record = json!({
            "complexity_metric_id": complexity_metric_id,
            "file_id": file_id,
            "path": path,
            "language": language,
            "scope": scope,
            "symbol_id": symbol_id,
            "algorithm_id": algorithm_id,
            "covered_lines": covered_lines,
            "covered_bytes": covered_bytes,
            "decision_count": decision_count,
            "loop_count": loop_count,
            "max_nesting_depth": max_nesting_depth,
            "parameter_count": parameter_count,
            "span": span(start_line, start_column, end_line, end_column, start_byte, end_byte),
            "metadata": optional_object("complexity_metrics.metadata_json", metadata_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "complexity_metric",
            &complexity_metric_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn export_parse_diagnostics<W: Write>(
    conn: &Connection,
    writer: &mut W,
    artifact_id: &str,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let mut stmt = conn.prepare(
        "SELECT diagnostic_id, file_id, path, language, kind, message, start_line, start_column,
                end_line, end_column, start_byte, end_byte, metadata_json
         FROM parse_diagnostics
         ORDER BY diagnostic_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, i64>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
            row.get::<_, Option<String>>(12)?,
        ))
    })?;
    for row in rows {
        let (
            diagnostic_id,
            file_id,
            path,
            language,
            kind,
            message,
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
            metadata_json,
        ) = row?;
        let record = json!({
            "diagnostic_id": diagnostic_id,
            "file_id": file_id,
            "path": path,
            "language": language,
            "kind": kind,
            "message": message,
            "span": span(start_line, start_column, end_line, end_column, start_byte, end_byte),
            "metadata": optional_object("parse_diagnostics.metadata_json", metadata_json)?,
        });
        write_record(
            writer,
            artifact_id,
            "parse_diagnostic",
            &diagnostic_id,
            record,
            summary,
        )?;
    }
    Ok(())
}

fn write_record<W: Write>(
    writer: &mut W,
    artifact_id: &str,
    kind: &'static str,
    record_id: &str,
    record: Value,
    summary: &mut JsonlExportSummary,
) -> JsonlExportResult<()> {
    let envelope = json!({
        "jsonl_schema_version": JSONL_SCHEMA_VERSION,
        "extract_contract_version": EXTRACT_CONTRACT_VERSION,
        "kind": kind,
        "op": "snapshot",
        "artifact_id": artifact_id,
        "record_id": record_id,
        "record": record,
    });
    serde_json::to_writer(&mut *writer, &envelope).map_err(|source| JsonlExportError::Json {
        column: "jsonl.envelope",
        source,
    })?;
    writer.write_all(b"\n")?;
    summary.record_written(kind);
    Ok(())
}

fn write_record_raw_object<W: Write>(
    writer: &mut W,
    artifact_id: &str,
    kind: &'static str,
    record_id: &str,
    summary: &mut JsonlExportSummary,
    write_record: impl FnOnce(&mut W) -> JsonlExportResult<()>,
) -> JsonlExportResult<()> {
    writer.write_all(b"{\"jsonl_schema_version\":")?;
    write_json_value(writer, "jsonl.envelope", &JSONL_SCHEMA_VERSION)?;
    writer.write_all(b",\"extract_contract_version\":")?;
    write_json_value(writer, "jsonl.envelope", &EXTRACT_CONTRACT_VERSION)?;
    writer.write_all(b",\"kind\":")?;
    write_json_value(writer, "jsonl.envelope", &kind)?;
    writer.write_all(b",\"op\":\"snapshot\",\"artifact_id\":")?;
    write_json_value(writer, "jsonl.envelope", &artifact_id)?;
    writer.write_all(b",\"record_id\":")?;
    write_json_value(writer, "jsonl.envelope", &record_id)?;
    writer.write_all(b",\"record\":")?;
    write_record(writer)?;
    writer.write_all(b"}\n")?;
    summary.record_written(kind);
    Ok(())
}

fn write_json_field<W: Write, T: Serialize>(
    writer: &mut W,
    key: &'static str,
    value: &T,
    first: &mut bool,
) -> JsonlExportResult<()> {
    write_field_prefix(writer, key, first)?;
    write_json_value(writer, key, value)
}

fn write_raw_json_field<W: Write>(
    writer: &mut W,
    key: &'static str,
    value: Option<&str>,
    first: &mut bool,
) -> JsonlExportResult<()> {
    write_field_prefix(writer, key, first)?;
    match value {
        Some(value) => writer.write_all(value.as_bytes())?,
        None => writer.write_all(b"null")?,
    }
    Ok(())
}

fn write_field_prefix<W: Write>(
    writer: &mut W,
    key: &'static str,
    first: &mut bool,
) -> JsonlExportResult<()> {
    if *first {
        *first = false;
    } else {
        writer.write_all(b",")?;
    }
    write_json_value(writer, "jsonl.field", &key)?;
    writer.write_all(b":")?;
    Ok(())
}

fn write_json_value<W: Write, T: Serialize>(
    writer: &mut W,
    column: &'static str,
    value: &T,
) -> JsonlExportResult<()> {
    serde_json::to_writer(&mut *writer, value)
        .map_err(|source| JsonlExportError::Json { column, source })
}

fn required_metadata<'a>(
    metadata: &'a BTreeMap<String, String>,
    key: &'static str,
) -> JsonlExportResult<&'a str> {
    metadata
        .get(key)
        .map(String::as_str)
        .ok_or(JsonlExportError::MissingMetadata { key })
}

fn metadata_i64(metadata: &BTreeMap<String, String>, key: &'static str) -> JsonlExportResult<i64> {
    let value = required_metadata(metadata, key)?;
    value
        .parse()
        .map_err(|_| JsonlExportError::InvalidMetadata {
            key,
            value: value.to_string(),
        })
}

/// Absent key exports as `null` — the resolution metadata keys exist only once
/// a resolution pass has run (absent = resolution status `absent`).
fn optional_metadata_i64(
    metadata: &BTreeMap<String, String>,
    key: &'static str,
) -> JsonlExportResult<Option<i64>> {
    metadata
        .get(key)
        .map(|value| {
            value
                .parse()
                .map_err(|_| JsonlExportError::InvalidMetadata {
                    key,
                    value: value.clone(),
                })
        })
        .transpose()
}

fn capability_flags(
    symbols: i64,
    relationships: i64,
    pending_relationships: i64,
    identifiers: i64,
    types: i64,
) -> Value {
    json!({
        "symbols": symbols != 0,
        "relationships": relationships != 0,
        "pending_relationships": pending_relationships != 0,
        "identifiers": identifiers != 0,
        "types": types != 0,
    })
}

fn span(
    start_line: i64,
    start_column: i64,
    end_line: i64,
    end_column: i64,
    start_byte: i64,
    end_byte: i64,
) -> Value {
    json!({
        "start_line": start_line,
        "start_column": start_column,
        "end_line": end_line,
        "end_column": end_column,
        "start_byte": start_byte,
        "end_byte": end_byte,
    })
}

fn optional_complete_span(
    start_line: Option<i64>,
    start_column: Option<i64>,
    end_line: Option<i64>,
    end_column: Option<i64>,
    start_byte: Option<i64>,
    end_byte: Option<i64>,
) -> Value {
    match (
        start_line,
        start_column,
        end_line,
        end_column,
        start_byte,
        end_byte,
    ) {
        (
            Some(start_line),
            Some(start_column),
            Some(end_line),
            Some(end_column),
            Some(start_byte),
            Some(end_byte),
        ) => span(
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
        ),
        _ => Value::Null,
    }
}

fn partial_span(
    start_line: i64,
    start_column: Option<i64>,
    end_line: Option<i64>,
    end_column: Option<i64>,
    start_byte: Option<i64>,
    end_byte: Option<i64>,
) -> Value {
    json!({
        "start_line": start_line,
        "start_column": start_column,
        "end_line": end_line,
        "end_column": end_column,
        "start_byte": start_byte,
        "end_byte": end_byte,
    })
}

fn reference_span(
    start_line: Option<i64>,
    start_column: Option<i64>,
    end_line: Option<i64>,
    end_column: Option<i64>,
    start_byte: Option<i64>,
    end_byte: Option<i64>,
) -> Value {
    start_line.map_or(Value::Null, |start_line| {
        partial_span(
            start_line,
            start_column,
            end_line,
            end_column,
            start_byte,
            end_byte,
        )
    })
}

fn object_or_empty(column: &'static str, value: Option<String>) -> JsonlExportResult<Value> {
    match value {
        Some(value) => required_object(column, value),
        None => Ok(Value::Object(Map::new())),
    }
}

fn optional_object(column: &'static str, value: Option<String>) -> JsonlExportResult<Value> {
    match value {
        Some(value) => required_object(column, value),
        None => Ok(Value::Null),
    }
}

fn optional_raw_object(
    column: &'static str,
    value: Option<String>,
) -> JsonlExportResult<Option<String>> {
    match value {
        Some(value) => {
            validate_object(column, &value)?;
            Ok(Some(compact_json_text(&value)))
        }
        None => Ok(None),
    }
}

fn compact_json_text(value: &str) -> String {
    let mut compacted = String::with_capacity(value.len());
    let mut in_string = false;
    let mut escaped = false;

    for character in value.chars() {
        if in_string {
            compacted.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
        } else if character == '"' {
            in_string = true;
            compacted.push(character);
        } else if !character.is_ascii_whitespace() {
            compacted.push(character);
        }
    }

    compacted
}

fn required_object(column: &'static str, value: String) -> JsonlExportResult<Value> {
    let value = parse_json(column, &value)?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(JsonlExportError::InvalidJsonShape {
            column,
            expected: "object",
        })
    }
}

fn validate_object(column: &'static str, value: &str) -> JsonlExportResult<()> {
    let value = parse_json(column, value)?;
    if value.is_object() {
        Ok(())
    } else {
        Err(JsonlExportError::InvalidJsonShape {
            column,
            expected: "object",
        })
    }
}

fn optional_array(column: &'static str, value: Option<String>) -> JsonlExportResult<Value> {
    match value {
        Some(value) => required_array(column, value),
        None => Ok(Value::Null),
    }
}

fn required_array(column: &'static str, value: String) -> JsonlExportResult<Value> {
    let value = parse_json(column, &value)?;
    if value.is_array() {
        Ok(value)
    } else {
        Err(JsonlExportError::InvalidJsonShape {
            column,
            expected: "array",
        })
    }
}

fn parse_json(column: &'static str, value: &str) -> JsonlExportResult<Value> {
    serde_json::from_str(value).map_err(|source| JsonlExportError::Json { column, source })
}
