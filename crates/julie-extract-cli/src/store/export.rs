use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use julie_extract_artifact::metadata::{ArtifactMetadata, initialize_metadata, write_index_level};
use julie_extract_artifact::schema::{
    EXTRACT_CONTRACT_VERSION, SQLITE_SCHEMA_VERSION, create_schema,
};
use julie_extract_artifact::store::StoreConnectionFactory;
use julie_extractors::EXTRACTION_IDENTITY_EPOCH;
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params_from_iter};

use crate::artifact_access::validate_current_artifact_output;

use super::args::StoreExportArgs;
use super::common::*;
use super::import::{
    StoreExecutionOutcome, absolute_runtime_path, classify_failure, mint_request_id,
    open_existing_store,
};
use super::report::{
    StoreCoordinatorDisposition, StoreExportDisposition, StoreExportReport, StoreLevelCompletion,
    StoreManifestReport, StoreOperation, StoreOutputFormat, StoreReport, StoreRequestState,
    StoreRequestedLevel, StoreRowCounts,
};
const LOCAL_ID_COLUMNS: &[&str] = &[
    "symbol_id",
    "parent_symbol_id",
    "annotation_id",
    "reference_site_id",
    "identifier_id",
    "relationship_id",
    "from_symbol_id",
    "to_symbol_id",
    "pending_relationship_id",
    "caller_scope_symbol_id",
    "type_fact_id",
    "usage_id",
    "type_argument_id",
    "parent_type_argument_id",
    "literal_id",
    "containing_symbol_id",
    "source_region_id",
    "structural_fact_id",
    "complexity_metric_id",
    "diagnostic_id",
];
const VERSION_TABLES: &[(&str, &str)] = &[
    ("symbols", "symbols"),
    ("symbol_annotations", "symbol_annotations"),
    ("reference_sites", "reference_sites"),
    ("identifiers", "identifiers"),
    ("relationships", "relationships"),
    ("pending_relationships", "pending_relationships"),
    ("type_facts", "type_facts"),
    ("type_argument_usages", "type_argument_usages"),
    ("type_arguments", "type_arguments"),
    ("literals", "literals"),
    ("source_regions", "source_regions"),
    ("structural_facts", "structural_facts"),
    ("complexity_metrics", "complexity_metrics"),
    ("parse_diagnostics", "parse_diagnostics"),
];
const GLOBAL_TABLES: &[&str] = &[
    "parser_inventory",
    "language_capabilities",
    "language_capability_fixtures",
    "language_capability_gaps",
];

#[derive(Debug, Clone)]
struct ExportIdentity {
    family_id: String,
    view_id: String,
    root: String,
    manifest_generation: i64,
    manifest_hash: String,
    created_at: String,
}

struct ExportLock {
    path: PathBuf,
}

pub(crate) fn run(args: StoreExportArgs) -> StoreExecutionOutcome {
    let format = if args.json {
        StoreOutputFormat::Json
    } else {
        StoreOutputFormat::Human
    };
    let request_id = mint_request_id();
    let mut family_id = args.family.clone().unwrap_or_default();
    match execute_export(&args, &request_id, &mut family_id) {
        Ok(report) => StoreExecutionOutcome::success(report, format),
        Err(message) => {
            let report =
                StoreReport::new(request_id, family_id, &args.view, StoreRequestState::Failed)
                    .with_operation(StoreOperation::Export)
                    .with_requested_level(StoreRequestedLevel::NotApplicable)
                    .with_failure(classify_failure(&message), message);
            StoreExecutionOutcome::failure(report, format)
        }
    }
}

fn execute_export(
    args: &StoreExportArgs,
    request_id: &str,
    failure_family: &mut String,
) -> Result<StoreReport, String> {
    let existing = open_existing_store(&args.store, args.family.as_deref())?;
    failure_family.clone_from(&existing.family_id);
    let output_path = PathBuf::from(absolute_runtime_path(&args.out)?);
    let factory = StoreConnectionFactory::new(
        existing.layout.clone(),
        &existing.family_id,
        env!("CARGO_PKG_VERSION"),
    );
    let export_lock = acquire_export_lock(&output_path, &partial_path(&output_path))?;
    let result = (|| {
        let source = factory.open_reader().map_err(|error| error.to_string())?;
        source
            .execute_batch("BEGIN")
            .map_err(|error| error.to_string())?;
        let finish_snapshot = |source: &Connection, commit: bool| {
            let _ = source.execute_batch(if commit { "COMMIT" } else { "ROLLBACK" });
        };
        let identity = match current_view_identity(&source, &existing.family_id, &args.view) {
            Ok(identity) => identity,
            Err(error) => {
                finish_snapshot(&source, false);
                return Err(error);
            }
        };
        export_test_pause_after_snapshot()?;
        match fs::symlink_metadata(&output_path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                finish_snapshot(&source, false);
                return Err("output_identity_mismatch".to_string());
            }
            Ok(_) => {
                if let Err(error) = validate_existing_output(&output_path, &identity) {
                    finish_snapshot(&source, false);
                    return Err(error);
                }
                finish_snapshot(&source, true);
                return Ok((identity, StoreExportDisposition::Reused));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                finish_snapshot(&source, false);
                return Err(error.to_string());
            }
        }
        let partial_path = partial_path(&output_path);
        if let Err(error) = remove_stale_partial(&partial_path) {
            finish_snapshot(&source, false);
            return Err(error);
        }
        if let Err(error) = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial_path)
            .map(|_| ())
            .map_err(|error| error.to_string())
        {
            finish_snapshot(&source, false);
            return Err(error);
        }
        if let Err(error) = materialize_export(&source, &identity, &partial_path) {
            finish_snapshot(&source, false);
            let _ = fs::remove_file(&partial_path);
            return Err(error);
        }
        finish_snapshot(&source, true);
        if let Err(error) = validate_existing_output(&partial_path, &identity) {
            let _ = fs::remove_file(&partial_path);
            return Err(error);
        }
        if let Err(error) = publish_partial(&partial_path, &output_path) {
            let _ = fs::remove_file(&partial_path);
            return Err(error);
        }
        Ok((identity, StoreExportDisposition::Created))
    })();
    export_lock.release()?;
    let (identity, disposition) = result?;
    export_report(request_id, &identity, &output_path, disposition)
}

fn current_view_identity(
    source: &Connection,
    family_id: &str,
    view_id: &str,
) -> Result<ExportIdentity, String> {
    source
        .query_row(
            "SELECT view.root,view.current_generation,manifest.manifest_hash,manifest.created_at
             FROM views AS view
             JOIN manifests AS manifest
               ON manifest.view_id=view.view_id AND manifest.generation=view.current_generation
             WHERE view.view_id=?1",
            [view_id],
            |row| {
                Ok(ExportIdentity {
                    family_id: family_id.to_string(),
                    view_id: view_id.to_string(),
                    root: row.get(0)?,
                    manifest_generation: row.get(1)?,
                    manifest_hash: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "view_not_found".to_string())
}

fn materialize_export(
    source: &Connection,
    identity: &ExportIdentity,
    partial_path: &Path,
) -> Result<(), String> {
    let mut output = Connection::open(partial_path).map_err(|error| error.to_string())?;
    output
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=OFF;")
        .map_err(|error| error.to_string())?;
    create_schema(&output).map_err(|error| error.to_string())?;
    output
        .execute_batch("PRAGMA foreign_keys=OFF;")
        .map_err(|error| error.to_string())?;
    let transaction = output.transaction().map_err(|error| error.to_string())?;
    initialize_export_metadata(&transaction, identity)?;
    insert_revision(&transaction, identity)?;
    copy_files(source, &transaction, identity).map_err(|error| format!("copy_files:{error}"))?;
    for &(source_table, target_table) in VERSION_TABLES {
        copy_version_table(source, &transaction, identity, source_table, target_table)
            .map_err(|error| format!("copy_version_table:{source_table}:{error}"))?;
    }
    copy_global_tables(source, &transaction, identity)
        .map_err(|error| format!("copy_global_tables:{error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("export_commit:{error}"))?;
    output
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA foreign_keys=ON;")
        .map_err(|error| error.to_string())?;
    let foreign_key_failure = output
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
        .optional()
        .map_err(|error| error.to_string())?;
    if foreign_key_failure.is_some() {
        return Err("artifact_foreign_key_check_failed".to_string());
    }
    output.close().map_err(|(_, error)| error.to_string())?;
    OpenOptions::new()
        .write(true)
        .open(partial_path)
        .and_then(|file| file.sync_all())
        .map_err(|error| error.to_string())
}

fn initialize_export_metadata(
    transaction: &Transaction<'_>,
    identity: &ExportIdentity,
) -> Result<(), String> {
    let (parser_inventory_fingerprint, capability_snapshot_fingerprint) =
        crate::capability_snapshot::current_capability_fingerprints();
    let metadata = ArtifactMetadata {
        artifact_id: format!(
            "store:{}:{}:{}:{}",
            identity.family_id,
            identity.view_id,
            identity.manifest_generation,
            identity.manifest_hash
        ),
        root_path: identity.root.clone(),
        binary_version: env!("CARGO_PKG_VERSION").to_string(),
        hash_algorithm: "blake3".to_string(),
        parser_inventory_fingerprint,
        capability_snapshot_fingerprint,
        created_at: identity.created_at.clone(),
        updated_at: identity.created_at.clone(),
    };
    initialize_metadata(transaction, &metadata).map_err(|error| error.to_string())?;
    write_index_level(transaction, "full").map_err(|error| error.to_string())?;
    for (key, value) in export_identity_rows(identity) {
        transaction
            .execute(
                "INSERT INTO artifact_metadata(key,value) VALUES (?1,?2)",
                rusqlite::params![key, value],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn export_identity_rows(identity: &ExportIdentity) -> [(&'static str, String); 4] {
    [
        ("store_family_id", identity.family_id.clone()),
        ("store_view_id", identity.view_id.clone()),
        (
            "store_manifest_generation",
            identity.manifest_generation.to_string(),
        ),
        ("store_manifest_hash", identity.manifest_hash.clone()),
    ]
}

fn insert_revision(transaction: &Transaction<'_>, identity: &ExportIdentity) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO extraction_revisions
             (revision_id,parent_revision_id,operation,mode,started_at,completed_at,
              binary_version,extract_contract_version,sqlite_schema_version,input_root,counts_json)
             VALUES (1,NULL,'store_export','full',?1,?1,?2,?3,?4,?5,'{}')",
            rusqlite::params![
                identity.created_at,
                env!("CARGO_PKG_VERSION"),
                EXTRACT_CONTRACT_VERSION,
                SQLITE_SCHEMA_VERSION,
                identity.root,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn copy_files(
    source: &Connection,
    target: &Transaction<'_>,
    identity: &ExportIdentity,
) -> Result<(), String> {
    let mut statement = source
        .prepare(
            "SELECT version.version_id,entry.path,entry.language,version.content_hash,
                    version.content_bytes,version.line_count,entry.indexed_at,entry.status,
                    version.metadata_json
             FROM manifest_entries AS entry
             JOIN file_versions AS version ON version.version_id=entry.version_id
             WHERE entry.view_id=?1 AND entry.generation=?2
               AND entry.status IN ('indexed','failed_preserved')
             ORDER BY entry.path",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params![identity.view_id, identity.manifest_generation],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    for row in rows {
        let (version_id, path, language, hash, bytes, lines, indexed_at, status, metadata) =
            row.map_err(|error| error.to_string())?;
        let file_id = version_file_id(version_id);
        target
            .execute(
                "INSERT INTO files
                 (file_id,path,language,content_hash,content_bytes,line_count,indexed_at,
                  last_revision_id,status,metadata_json)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,?9)",
                rusqlite::params![
                    file_id, path, language, hash, bytes, lines, indexed_at, status, metadata
                ],
            )
            .map_err(|error| error.to_string())?;
        target
            .execute(
                "INSERT INTO revision_file_changes(revision_id,file_id,path,change_kind)
                 VALUES (1,?1,?2,'added')",
                rusqlite::params![version_file_id(version_id), path],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn copy_version_table(
    source: &Connection,
    target: &Transaction<'_>,
    identity: &ExportIdentity,
    source_table: &str,
    target_table: &str,
) -> Result<(), String> {
    let source_columns = table_columns(source, source_table)?;
    let target_columns = table_columns(target, target_table)?;
    let selected = target_columns
        .iter()
        .filter(|column| column.as_str() != "file_id")
        .map(|column| {
            if source_columns.contains(column) {
                Ok(format!("row.{}", quote_identifier(column)))
            } else {
                Err(format!(
                    "store export column mismatch: {source_table}.{column}"
                ))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let query = format!(
        "SELECT row.version_id,{} FROM {} AS row
         JOIN manifest_entries AS entry ON entry.version_id=row.version_id
         WHERE entry.view_id=?1 AND entry.generation=?2
           AND entry.status IN ('indexed','failed_preserved')
         ORDER BY row.version_id",
        selected.join(","),
        quote_identifier(source_table),
    );
    let insert = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        quote_identifier(target_table),
        target_columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(","),
        (1..=target_columns.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(","),
    );
    let mut statement = source.prepare(&query).map_err(|error| error.to_string())?;
    let mut rows = statement
        .query(rusqlite::params![
            identity.view_id,
            identity.manifest_generation
        ])
        .map_err(|error| error.to_string())?;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        let version_id = row.get::<_, i64>(0).map_err(|error| error.to_string())?;
        let mut source_index = 1;
        let mut values = Vec::with_capacity(target_columns.len());
        for column in &target_columns {
            if column == "file_id" {
                values.push(Value::Text(version_file_id(version_id)));
                continue;
            }
            let mut value = row
                .get::<_, Value>(source_index)
                .map_err(|error| error.to_string())?;
            source_index += 1;
            if LOCAL_ID_COLUMNS.contains(&column.as_str())
                && let Value::Text(local_id) = value
            {
                value = Value::Text(version_local_id(version_id, &local_id));
            }
            values.push(value);
        }
        target
            .execute(&insert, params_from_iter(values))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn copy_global_tables(
    source: &Connection,
    target: &Transaction<'_>,
    identity: &ExportIdentity,
) -> Result<(), String> {
    let epoch = source
        .query_row(
            "SELECT MAX(version.extraction_epoch)
             FROM manifest_entries AS entry
             JOIN file_versions AS version ON version.version_id=entry.version_id
             WHERE entry.view_id=?1 AND entry.generation=?2",
            rusqlite::params![identity.view_id, identity.manifest_generation],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(|error| error.to_string())?
        .unwrap_or(i64::from(EXTRACTION_IDENTITY_EPOCH));
    let mut copied_capability_rows = false;
    for table in GLOBAL_TABLES {
        let target_columns = table_columns(target, table)?;
        let select = target_columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(",");
        let query = if *table == "language_capability_gaps" {
            format!(
                "SELECT {select} FROM {} WHERE extraction_epoch=?1
                 AND capability NOT LIKE 'reference_resolution.%'",
                quote_identifier(table)
            )
        } else {
            format!(
                "SELECT {select} FROM {} WHERE extraction_epoch=?1",
                quote_identifier(table)
            )
        };
        let insert = format!(
            "INSERT INTO {} ({select}) VALUES ({})",
            quote_identifier(table),
            (1..=target_columns.len())
                .map(|index| format!("?{index}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let mut statement = source.prepare(&query).map_err(|error| error.to_string())?;
        let mut rows = statement
            .query([epoch])
            .map_err(|error| error.to_string())?;
        while let Some(row) = rows.next().map_err(|error| error.to_string())? {
            let values = (0..target_columns.len())
                .map(|index| row.get::<_, Value>(index))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            target
                .execute(&insert, params_from_iter(values))
                .map_err(|error| error.to_string())?;
            if *table == "language_capabilities" {
                copied_capability_rows = true;
            }
        }
    }
    if !copied_capability_rows {
        write_current_capability_rows(target)?;
    }
    Ok(())
}

fn write_current_capability_rows(target: &Transaction<'_>) -> Result<(), String> {
    let snapshot = crate::capability_snapshot::artifact_capability_snapshot();
    for row in &snapshot.parser_inventory {
        let metadata_json = row
            .metadata
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| error.to_string())?;
        target
            .execute(
                "INSERT INTO parser_inventory
                 (language, parser_package, parser_version, grammar_version, source, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    row.language,
                    row.parser_package,
                    row.parser_version,
                    row.grammar_version,
                    row.source,
                    metadata_json,
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    for row in &snapshot.languages {
        let extensions_json =
            serde_json::to_string(&row.extensions).map_err(|error| error.to_string())?;
        let kind_coverage_json =
            serde_json::to_string(&row.kind_coverage).map_err(|error| error.to_string())?;
        target
            .execute(
                "INSERT INTO language_capabilities
                 (language, parser_package, extensions_json, dependency_status,
                  target_symbols, target_relationships, target_pending_relationships,
                  target_identifiers, target_types, actual_symbols, actual_relationships,
                  actual_pending_relationships, actual_identifiers, actual_types,
                  kind_coverage_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                rusqlite::params![
                    row.language,
                    row.parser_package,
                    extensions_json,
                    row.dependency_status,
                    i64::from(row.target_capabilities.symbols),
                    i64::from(row.target_capabilities.relationships),
                    i64::from(row.target_capabilities.pending_relationships),
                    i64::from(row.target_capabilities.identifiers),
                    i64::from(row.target_capabilities.types),
                    i64::from(row.actual_capabilities.symbols),
                    i64::from(row.actual_capabilities.relationships),
                    i64::from(row.actual_capabilities.pending_relationships),
                    i64::from(row.actual_capabilities.identifiers),
                    i64::from(row.actual_capabilities.types),
                    kind_coverage_json,
                ],
            )
            .map_err(|error| error.to_string())?;
        for fixture in &row.fixtures {
            target
                .execute(
                    "INSERT INTO language_capability_fixtures
                     (language, fixture_name, source_path, expected_path)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        row.language,
                        fixture.fixture_name,
                        fixture.source_path,
                        fixture.expected_path,
                    ],
                )
                .map_err(|error| error.to_string())?;
        }
        for gap in &row.gaps {
            if gap.capability.starts_with("reference_resolution.") {
                continue;
            }
            let evidence_json =
                serde_json::to_string(&gap.evidence).map_err(|error| error.to_string())?;
            target
                .execute(
                    "INSERT INTO language_capability_gaps
                     (gap_id, language, capability, status, reason, required_closure, evidence_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        gap.gap_id,
                        row.language,
                        gap.capability,
                        gap.status.as_str(),
                        gap.reason,
                        gap.required_closure,
                        evidence_json,
                    ],
                )
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, String> {
    let query = format!("PRAGMA table_info({})", quote_identifier(table));
    connection
        .prepare(&query)
        .map_err(|error| error.to_string())?
        .query_map([], |row| row.get(1))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

fn version_file_id(version_id: i64) -> String {
    format!("store-version:{version_id}")
}

fn version_local_id(version_id: i64, local_id: &str) -> String {
    format!("store-version:{version_id}:{local_id}")
}

fn validate_existing_output(path: &Path, identity: &ExportIdentity) -> Result<(), String> {
    validate_current_artifact_output(path)
        .map_err(|error| format!("output_identity_mismatch: {error}"))?;
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("output_identity_mismatch: {error}"))?;
    let found = connection
        .prepare(
            "SELECT key,value FROM artifact_metadata
             WHERE key LIKE 'store_%' ORDER BY key",
        )
        .and_then(|mut statement| {
            statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<BTreeMap<_, _>, _>>()
        })
        .map_err(|error| format!("output_identity_mismatch: {error}"))?;
    let expected = export_identity_rows(identity)
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect::<BTreeMap<_, _>>();
    if found != expected {
        return Err("output_identity_mismatch".to_string());
    }
    Ok(())
}

fn export_report(
    request_id: &str,
    identity: &ExportIdentity,
    output: &Path,
    disposition: StoreExportDisposition,
) -> Result<StoreReport, String> {
    let row_counts = exported_row_counts(output)?;
    let report = StoreReport::new(
        request_id,
        &identity.family_id,
        &identity.view_id,
        StoreRequestState::Committed,
    )
    .with_operation(StoreOperation::Export)
    .with_root(&identity.root)
    .with_requested_level(StoreRequestedLevel::NotApplicable)
    .with_completion(StoreLevelCompletion {
        l1: true,
        l2: true,
        l3: true,
    })
    .with_manifest(StoreManifestReport {
        generation: u64::try_from(identity.manifest_generation).ok(),
        hash: Some(identity.manifest_hash.clone()),
        disposition: Default::default(),
    })
    .with_coordinator(StoreCoordinatorDisposition::NotStarted)
    .with_row_counts(row_counts)
    .with_export(StoreExportReport {
        output: output.to_string_lossy().into_owned(),
        disposition,
    });
    Ok(report)
}

fn exported_row_counts(output: &Path) -> Result<StoreRowCounts, String> {
    let connection = Connection::open_with_flags(output, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| error.to_string())?;
    let counts = connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM files),
               (SELECT COUNT(*) FROM symbols)
                 + (SELECT COUNT(*) FROM symbol_annotations)
                 + (SELECT COUNT(*) FROM reference_sites AS site WHERE EXISTS(
                     SELECT 1 FROM relationships WHERE reference_site_id=site.reference_site_id
                     UNION ALL
                     SELECT 1 FROM pending_relationships WHERE reference_site_id=site.reference_site_id))
                 + (SELECT COUNT(*) FROM relationships)
                 + (SELECT COUNT(*) FROM pending_relationships)
                 + (SELECT COUNT(*) FROM type_facts)
                 + (SELECT COUNT(*) FROM complexity_metrics)
                 + (SELECT COUNT(*) FROM parse_diagnostics),
               (SELECT COUNT(*) FROM identifiers)
                 + (SELECT COUNT(*) FROM reference_sites AS site WHERE NOT EXISTS(
                     SELECT 1 FROM relationships WHERE reference_site_id=site.reference_site_id
                     UNION ALL
                     SELECT 1 FROM pending_relationships WHERE reference_site_id=site.reference_site_id)),
               (SELECT COUNT(*) FROM type_arguments)
                 + (SELECT COUNT(*) FROM type_argument_usages)
                 + (SELECT COUNT(*) FROM literals)
                 + (SELECT COUNT(*) FROM source_regions)
                 + (SELECT COUNT(*) FROM structural_facts)",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    Ok(StoreRowCounts {
        file_versions: u64::try_from(counts.0).map_err(|_| "invalid_row_count")?,
        l1: u64::try_from(counts.1).map_err(|_| "invalid_row_count")?,
        l2: u64::try_from(counts.2).map_err(|_| "invalid_row_count")?,
        l3: u64::try_from(counts.3).map_err(|_| "invalid_row_count")?,
    })
}

fn export_test_pause_after_snapshot() -> Result<(), String> {
    let Some(directory) = std::env::var_os("JULIE_EXTRACT_STORE_EXPORT_TEST_PAUSE_DIR") else {
        return Ok(());
    };
    let directory = PathBuf::from(directory);
    fs::write(directory.join("ready"), b"ready").map_err(|error| error.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !directory.join("continue").exists() {
        if std::time::Instant::now() >= deadline {
            return Err("export test pause timed out".to_string());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Ok(())
}

fn partial_path(output: &Path) -> PathBuf {
    let mut name = OsString::from(output.as_os_str());
    name.push(".partial");
    PathBuf::from(name)
}

fn remove_stale_partial(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => {
            fs::remove_file(path).map_err(|error| error.to_string())
        }
        Ok(_) => Err("output_identity_mismatch: invalid export partial path".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn acquire_export_lock(output: &Path, partial: &Path) -> Result<ExportLock, String> {
    let lock_path = partial_lock_path(output);
    loop {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(mut file) => {
                writeln!(file, "{}", std::process::id()).map_err(|error| error.to_string())?;
                file.sync_all().map_err(|error| error.to_string())?;
                return Ok(ExportLock { path: lock_path });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata =
                    fs::symlink_metadata(&lock_path).map_err(|error| error.to_string())?;
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err("output_identity_mismatch: invalid export lock".to_string());
                }
                let contents = fs::read_to_string(&lock_path).map_err(|error| error.to_string())?;
                let pid = contents
                    .lines()
                    .next()
                    .and_then(|value| value.parse::<u32>().ok());
                if pid.is_none_or(process_is_alive) {
                    return Err("busy: export output is active".to_string());
                }
                match fs::symlink_metadata(partial) {
                    Ok(partial_metadata) if partial_metadata.is_file() => {
                        fs::remove_file(partial).map_err(|error| error.to_string())?;
                    }
                    Ok(_) => {
                        return Err(
                            "output_identity_mismatch: invalid export partial path".to_string()
                        );
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.to_string()),
                }
                fs::remove_file(&lock_path).map_err(|error| error.to_string())?;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

impl ExportLock {
    fn release(self) -> Result<(), String> {
        match fs::remove_file(self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

fn partial_lock_path(output: &Path) -> PathBuf {
    let mut name = OsString::from(output.as_os_str());
    name.push(".partial.lock");
    PathBuf::from(name)
}

/// Delegates to the one liveness probe in the artifact crate. Only a proven-dead
/// process releases the partial lock: `Unknown` must read as alive here, because
/// stealing the lock from a live writer would corrupt its output.
fn process_is_alive(pid: u32) -> bool {
    julie_extract_artifact::store::process_status(pid)
        != julie_extract_artifact::store::PidStatus::Dead
}

fn publish_partial(partial: &Path, output: &Path) -> Result<(), String> {
    fs::hard_link(partial, output).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            "output_identity_mismatch".to_string()
        } else {
            error.to_string()
        }
    })?;
    fs::remove_file(partial).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    if let Some(parent) = output.parent() {
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}
