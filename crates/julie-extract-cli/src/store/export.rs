use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use julie_extract_artifact::metadata::{ArtifactMetadata, initialize_metadata, write_index_level};
use julie_extract_artifact::resolution_store::{ResolutionStatus, write_resolution_metadata};
use julie_extract_artifact::schema::{
    EXTRACT_CONTRACT_VERSION, SQLITE_SCHEMA_VERSION, create_schema,
};
use julie_extract_artifact::store::{
    ResolutionBaseReader, ResolutionBindingStore, ResolutionIdentifierDeltaRecord,
    ResolutionIdentifierRow, ResolutionPendingDeltaRecord, ResolutionPendingOperation,
    ResolutionPendingRow, ResolutionPinOwnerKind, StoreConnectionFactory, StoreLayout,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params_from_iter};

use crate::artifact_access::validate_current_artifact_output;

use super::args::StoreExportArgs;
use super::import::{
    StoreExecutionOutcome, absolute_runtime_path, classify_failure, mint_request_id,
    open_existing_store,
};
use super::report::{
    StoreCoordinatorDisposition, StoreExportDisposition, StoreExportReport, StoreLevelCompletion,
    StoreManifestReport, StoreOperation, StoreOutputFormat, StoreReport, StoreRequestState,
    StoreRequestedLevel, StoreResolutionState, StoreRowCounts,
};

const EXPORT_WINDOW_SIZE: usize = 300;
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
    base_id: String,
    delta_generation: i64,
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
    let layout = existing.layout.clone();
    let factory = StoreConnectionFactory::new(
        layout.clone(),
        &existing.family_id,
        env!("CARGO_PKG_VERSION"),
    );
    let now = store_timestamp(layout.store_db(), "now")?;
    cleanup_expired_pins(&factory, &now)?;
    current_exact_identity(&factory, &existing.family_id, &args.view)?;
    let binding = ResolutionBindingStore::new(factory.clone());
    let expires_at = store_timestamp(layout.store_db(), export_pin_expiry_modifier())?;
    let pin_id = format!("export-{request_id}");
    let owner_id = request_id;
    let pin = binding
        .open_pin(
            &pin_id,
            ResolutionPinOwnerKind::Reader,
            owner_id,
            &args.view,
            &expires_at,
            &now,
        )
        .map_err(|error| format!("resolution_not_exact: {error}"))?;
    if pin.delta_generation.is_none() {
        let _ = binding.release_pin(&pin_id, ResolutionPinOwnerKind::Reader, owner_id);
        return Err("resolution_not_exact".to_string());
    }
    let export_lock = match acquire_export_lock(
        &output_path,
        &partial_path(&output_path),
        &factory,
        &pin_id,
        owner_id,
    ) {
        Ok(export_lock) => export_lock,
        Err(error) => {
            binding
                .release_pin(&pin_id, ResolutionPinOwnerKind::Reader, owner_id)
                .map_err(|release| release.to_string())?;
            return Err(error);
        }
    };
    let pinned_identity = match pinned_identity(&factory, &existing.family_id, &pin) {
        Ok(identity) => identity,
        Err(error) => {
            release_export_resources(&binding, &pin_id, owner_id, export_lock)?;
            return Err(error);
        }
    };
    let partial_path = partial_path(&output_path);
    let mut partial_owned = false;
    let result = (|| {
        export_test_pause_after_pin()?;
        match fs::symlink_metadata(&output_path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err("output_identity_mismatch".to_string());
            }
            Ok(_) => {
                validate_existing_output(&output_path, &pinned_identity)?;
                return Ok(StoreExportDisposition::Reused);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        remove_stale_partial(&partial_path)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial_path)
            .map_err(|error| error.to_string())?;
        partial_owned = true;
        materialize_export(
            &factory,
            &layout,
            &binding,
            &pin_id,
            &now,
            &pinned_identity,
            &partial_path,
        )?;
        export_test_crash("before_validation");
        validate_existing_output(&partial_path, &pinned_identity)?;
        export_test_crash("after_validation");
        publish_partial(&partial_path, &output_path)?;
        export_test_crash("after_rename");
        Ok(StoreExportDisposition::Created)
    })();
    let release = release_export_resources(&binding, &pin_id, owner_id, export_lock);
    let disposition = match result {
        Ok(disposition) => disposition,
        Err(error) => {
            if partial_owned {
                let _ = fs::remove_file(&partial_path);
            }
            release?;
            return Err(error);
        }
    };
    release?;
    export_report(request_id, &pinned_identity, &output_path, disposition)
}

fn release_export_resources(
    binding: &ResolutionBindingStore,
    pin_id: &str,
    owner_id: &str,
    export_lock: ExportLock,
) -> Result<(), String> {
    let pin_result = binding
        .release_pin(pin_id, ResolutionPinOwnerKind::Reader, owner_id)
        .map(|_| ())
        .map_err(|error| error.to_string());
    let lock_result = export_lock.release();
    match (pin_result, lock_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(pin), Ok(())) => Err(pin),
        (Ok(()), Err(lock)) => Err(lock),
        (Err(pin), Err(lock)) => Err(format!("{pin}; export lock release failed: {lock}")),
    }
}

fn current_exact_identity(
    factory: &StoreConnectionFactory,
    family_id: &str,
    view_id: &str,
) -> Result<ExportIdentity, String> {
    let connection = factory.open_reader().map_err(|error| error.to_string())?;
    connection
        .query_row(
            "SELECT view.root,view.current_generation,manifest.manifest_hash,
                    view.resolution_base_id,view.resolution_delta_generation,
                    manifest.created_at,view.resolution_state,view.resolution_exact_at
             FROM views AS view
             JOIN manifests AS manifest
               ON manifest.view_id=view.view_id AND manifest.generation=view.current_generation
             WHERE view.view_id=?1",
            [view_id],
            |row| {
                let generation = row.get::<_, i64>(1)?;
                let state = row.get::<_, String>(6)?;
                let exact_at = row.get::<_, Option<i64>>(7)?;
                if state != "exact" || exact_at != Some(generation) {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                Ok(ExportIdentity {
                    family_id: family_id.to_string(),
                    view_id: view_id.to_string(),
                    root: row.get(0)?,
                    manifest_generation: generation,
                    manifest_hash: row.get(2)?,
                    base_id: row.get(3)?,
                    delta_generation: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|error| {
            if matches!(error, rusqlite::Error::InvalidQuery) {
                "resolution_not_exact".to_string()
            } else {
                error.to_string()
            }
        })?
        .ok_or_else(|| "view_not_found".to_string())
}

fn pinned_identity(
    factory: &StoreConnectionFactory,
    family_id: &str,
    pin: &julie_extract_artifact::store::ResolutionPinRecord,
) -> Result<ExportIdentity, String> {
    let connection = factory.open_reader().map_err(|error| error.to_string())?;
    connection
        .query_row(
            "SELECT view.root,manifest.manifest_hash,manifest.created_at
             FROM views AS view
             JOIN manifests AS manifest ON manifest.view_id=view.view_id AND manifest.generation=?2
             WHERE view.view_id=?1",
            rusqlite::params![pin.view_id, pin.manifest_generation],
            |row| {
                Ok(ExportIdentity {
                    family_id: family_id.to_string(),
                    view_id: pin.view_id.clone(),
                    root: row.get(0)?,
                    manifest_generation: pin.manifest_generation,
                    manifest_hash: row.get(1)?,
                    base_id: pin.base_id.clone(),
                    delta_generation: pin.delta_generation.expect("exact pin has delta"),
                    created_at: row.get(2)?,
                })
            },
        )
        .map_err(|error| error.to_string())
}

fn materialize_export(
    factory: &StoreConnectionFactory,
    layout: &StoreLayout,
    binding: &ResolutionBindingStore,
    pin_id: &str,
    now: &str,
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
    let source = factory.open_reader().map_err(|error| error.to_string())?;
    let transaction = output.transaction().map_err(|error| error.to_string())?;
    initialize_export_metadata(&transaction, identity)?;
    insert_revision(&transaction, identity)?;
    copy_files(&source, &transaction, identity).map_err(|error| format!("copy_files:{error}"))?;
    for &(source_table, target_table) in VERSION_TABLES {
        copy_version_table(&source, &transaction, identity, source_table, target_table)
            .map_err(|error| format!("copy_version_table:{source_table}:{error}"))?;
    }
    copy_global_tables(&source, &transaction, identity)
        .map_err(|error| format!("copy_global_tables:{error}"))?;
    prepare_visible_versions(&source, &transaction, identity)
        .map_err(|error| format!("prepare_visible_versions:{error}"))?;
    copy_resolution(
        factory,
        layout,
        binding,
        pin_id,
        now,
        identity,
        &transaction,
    )
    .map_err(|error| format!("copy_resolution:{error}"))?;
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
    write_resolution_metadata(
        transaction,
        ResolutionStatus::Complete,
        crate::resolution::RESOLUTION_VERSION,
        1,
    )
    .map_err(|error| error.to_string())?;
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

fn export_identity_rows(identity: &ExportIdentity) -> [(&'static str, String); 6] {
    [
        ("store_family_id", identity.family_id.clone()),
        ("store_view_id", identity.view_id.clone()),
        (
            "store_manifest_generation",
            identity.manifest_generation.to_string(),
        ),
        ("store_manifest_hash", identity.manifest_hash.clone()),
        ("store_resolution_base_id", identity.base_id.clone()),
        (
            "store_resolution_delta_generation",
            identity.delta_generation.to_string(),
        ),
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
        .ok_or_else(|| "store export has no visible extraction epoch".to_string())?;
    for table in GLOBAL_TABLES {
        let target_columns = table_columns(target, table)?;
        let select = target_columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(",");
        let query = format!(
            "SELECT {select} FROM {} WHERE extraction_epoch=?1",
            quote_identifier(table)
        );
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
        }
    }
    Ok(())
}

fn copy_resolution(
    factory: &StoreConnectionFactory,
    layout: &StoreLayout,
    binding: &ResolutionBindingStore,
    pin_id: &str,
    now: &str,
    identity: &ExportIdentity,
    target: &Transaction<'_>,
) -> Result<(), String> {
    let source = factory.open_reader().map_err(|error| error.to_string())?;
    let relative_path: String = source
        .query_row(
            "SELECT relative_path FROM resolution_bases WHERE base_id=?1 AND state='ready'",
            [&identity.base_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?;
    let base_path = layout.generation_dir().join(relative_path);
    let base = ResolutionBaseReader::open(base_path).map_err(|error| error.to_string())?;
    let mut after = None::<(i64, String)>;
    loop {
        let page = base
            .identifier_window(
                after.as_ref().map(|(version, id)| (*version, id.as_str())),
                EXPORT_WINDOW_SIZE,
            )
            .map_err(|error| error.to_string())?;
        if page.is_empty() {
            break;
        }
        for row in &page {
            if version_visible(target, row.version_id)? {
                upsert_identifier_resolution(target, row)?;
            }
        }
        after = page
            .last()
            .map(|row| (row.version_id, row.identifier_id.clone()));
    }
    let mut after = None::<(i64, String)>;
    loop {
        let page = base
            .pending_window(
                after.as_ref().map(|(version, id)| (*version, id.as_str())),
                EXPORT_WINDOW_SIZE,
            )
            .map_err(|error| error.to_string())?;
        if page.is_empty() {
            break;
        }
        for row in &page {
            if version_visible(target, row.version_id)? {
                upsert_pending_resolution(target, row)?;
            }
        }
        after = page
            .last()
            .map(|row| (row.version_id, row.pending_relationship_id.clone()));
    }

    let mut after = None::<(i64, String)>;
    loop {
        let page = binding
            .pinned_identifier_delta_window(
                pin_id,
                now,
                after.as_ref().map(|(version, id)| (*version, id.as_str())),
                EXPORT_WINDOW_SIZE,
            )
            .map_err(|error| error.to_string())?;
        if page.is_empty() {
            break;
        }
        for row in &page {
            if version_visible(target, row.version_id)? {
                upsert_identifier_delta(target, row)?;
            }
        }
        after = page
            .last()
            .map(|row| (row.version_id, row.identifier_id.clone()));
    }
    let mut after = None::<(i64, String)>;
    loop {
        let page = binding
            .pinned_pending_delta_window(
                pin_id,
                now,
                after.as_ref().map(|(version, id)| (*version, id.as_str())),
                EXPORT_WINDOW_SIZE,
            )
            .map_err(|error| error.to_string())?;
        if page.is_empty() {
            break;
        }
        for row in &page {
            if !version_visible(target, row.version_id)? {
                continue;
            }
            match row.operation {
                ResolutionPendingOperation::Replace => upsert_pending_delta(target, row)?,
                ResolutionPendingOperation::Tombstone => {
                    target
                        .execute(
                            "DELETE FROM pending_resolutions WHERE pending_relationship_id=?1",
                            [version_local_id(
                                row.version_id,
                                &row.pending_relationship_id,
                            )],
                        )
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        after = page
            .last()
            .map(|row| (row.version_id, row.pending_relationship_id.clone()));
    }
    Ok(())
}

fn prepare_visible_versions(
    source: &Connection,
    target: &Transaction<'_>,
    identity: &ExportIdentity,
) -> Result<(), String> {
    target
        .execute_batch(
            "CREATE TEMP TABLE export_visible_versions(
               version_id INTEGER PRIMARY KEY
             ) WITHOUT ROWID;",
        )
        .map_err(|error| error.to_string())?;
    let mut statement = source
        .prepare(
            "SELECT version_id FROM manifest_entries
             WHERE view_id=?1 AND generation=?2 AND version_id IS NOT NULL
             ORDER BY version_id",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(
            rusqlite::params![identity.view_id, identity.manifest_generation],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())?;
    for version_id in rows {
        target
            .execute(
                "INSERT INTO export_visible_versions(version_id) VALUES (?1)",
                [version_id.map_err(|error| error.to_string())?],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn version_visible(target: &Transaction<'_>, version_id: i64) -> Result<bool, String> {
    target
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM export_visible_versions WHERE version_id=?1)",
            [version_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn upsert_identifier_resolution(
    target: &Transaction<'_>,
    row: &ResolutionIdentifierRow,
) -> Result<(), String> {
    insert_identifier_resolution(
        target,
        row.version_id,
        &row.identifier_id,
        row.target_version_id,
        row.target_symbol_id.as_deref(),
        row.tier,
        row.confidence,
        row.method.as_deref(),
        &row.outcome,
        row.candidates,
    )
}

fn upsert_identifier_delta(
    target: &Transaction<'_>,
    row: &ResolutionIdentifierDeltaRecord,
) -> Result<(), String> {
    insert_identifier_resolution(
        target,
        row.version_id,
        &row.identifier_id,
        row.target_version_id,
        row.target_symbol_id.as_deref(),
        row.tier,
        row.confidence,
        row.method.as_deref(),
        &row.outcome,
        row.candidates,
    )
}

#[allow(clippy::too_many_arguments)]
fn insert_identifier_resolution(
    target: &Transaction<'_>,
    version_id: i64,
    identifier_id: &str,
    target_version_id: Option<i64>,
    target_symbol_id: Option<&str>,
    tier: Option<i64>,
    confidence: Option<f64>,
    method: Option<&str>,
    outcome: &str,
    candidates: Option<i64>,
) -> Result<(), String> {
    let target_symbol_id = target_version_id
        .zip(target_symbol_id)
        .map(|(version, symbol)| version_local_id(version, symbol));
    target
        .execute(
            "INSERT INTO identifier_resolutions
             (identifier_id,target_symbol_id,tier,confidence,method,outcome,candidates,resolved_at_revision)
             VALUES (?1,?2,?3,?4,?5,?6,?7,1)
             ON CONFLICT(identifier_id) DO UPDATE SET
               target_symbol_id=excluded.target_symbol_id,tier=excluded.tier,
               confidence=excluded.confidence,method=excluded.method,
               outcome=excluded.outcome,candidates=excluded.candidates,resolved_at_revision=1",
            rusqlite::params![
                version_local_id(version_id, identifier_id),
                target_symbol_id,
                tier,
                confidence,
                method,
                outcome,
                candidates,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn upsert_pending_resolution(
    target: &Transaction<'_>,
    row: &ResolutionPendingRow,
) -> Result<(), String> {
    insert_pending_resolution(
        target,
        row.version_id,
        &row.pending_relationship_id,
        row.target_version_id,
        &row.target_symbol_id,
        row.tier,
        row.confidence,
        &row.method,
    )
}

fn upsert_pending_delta(
    target: &Transaction<'_>,
    row: &ResolutionPendingDeltaRecord,
) -> Result<(), String> {
    insert_pending_resolution(
        target,
        row.version_id,
        &row.pending_relationship_id,
        row.target_version_id
            .ok_or_else(|| "invalid pending replacement".to_string())?,
        row.target_symbol_id
            .as_deref()
            .ok_or_else(|| "invalid pending replacement".to_string())?,
        row.tier
            .ok_or_else(|| "invalid pending replacement".to_string())?,
        row.confidence
            .ok_or_else(|| "invalid pending replacement".to_string())?,
        row.method
            .as_deref()
            .ok_or_else(|| "invalid pending replacement".to_string())?,
    )
}

#[allow(clippy::too_many_arguments)]
fn insert_pending_resolution(
    target: &Transaction<'_>,
    version_id: i64,
    pending_id: &str,
    target_version_id: i64,
    target_symbol_id: &str,
    tier: i64,
    confidence: f64,
    method: &str,
) -> Result<(), String> {
    target
        .execute(
            "INSERT INTO pending_resolutions
             (pending_relationship_id,target_symbol_id,tier,confidence,method,resolved_at_revision)
             VALUES (?1,?2,?3,?4,?5,1)
             ON CONFLICT(pending_relationship_id) DO UPDATE SET
               target_symbol_id=excluded.target_symbol_id,tier=excluded.tier,
               confidence=excluded.confidence,method=excluded.method,resolved_at_revision=1",
            rusqlite::params![
                version_local_id(version_id, pending_id),
                version_local_id(target_version_id, target_symbol_id),
                tier,
                confidence,
                method,
            ],
        )
        .map_err(|error| error.to_string())?;
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

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
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
    let mut report = StoreReport::new(
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
    report.resolution.state = StoreResolutionState::Exact;
    report.resolution.exact_at_matches = true;
    report.resolution.base_id = Some(identity.base_id.clone());
    report.resolution.delta_generation = u64::try_from(identity.delta_generation).ok();
    report.resolution.exact_at_generation = u64::try_from(identity.manifest_generation).ok();
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

fn store_timestamp(store_db: &Path, modifier: &str) -> Result<String, String> {
    let connection = Connection::open(store_db).map_err(|error| error.to_string())?;
    if modifier == "now" {
        connection
            .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |row| {
                row.get(0)
            })
            .map_err(|error| error.to_string())
    } else {
        connection
            .query_row(
                "SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now',?1)",
                [modifier],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())
    }
}

fn cleanup_expired_pins(factory: &StoreConnectionFactory, now: &str) -> Result<(), String> {
    factory
        .open_writer()
        .map_err(|error| error.to_string())?
        .execute(
            "DELETE FROM resolution_pins WHERE julianday(expires_at)<=julianday(?1)",
            [now],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(feature = "test-store-resolution-contract")]
fn export_pin_expiry_modifier() -> &'static str {
    if std::env::var_os("JULIE_EXTRACT_STORE_EXPORT_TEST_SHORT_PIN").is_some() {
        "+1 second"
    } else {
        "+1 hour"
    }
}

#[cfg(not(feature = "test-store-resolution-contract"))]
fn export_pin_expiry_modifier() -> &'static str {
    "+1 hour"
}

#[cfg(feature = "test-store-resolution-contract")]
fn export_test_crash(boundary: &str) {
    if std::env::var("JULIE_EXTRACT_STORE_EXPORT_TEST_CRASH_AT").as_deref() == Ok(boundary) {
        std::process::abort();
    }
}

#[cfg(not(feature = "test-store-resolution-contract"))]
fn export_test_crash(_boundary: &str) {}

#[cfg(feature = "test-store-resolution-contract")]
fn export_test_pause_after_pin() -> Result<(), String> {
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

#[cfg(not(feature = "test-store-resolution-contract"))]
fn export_test_pause_after_pin() -> Result<(), String> {
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

fn acquire_export_lock(
    output: &Path,
    partial: &Path,
    factory: &StoreConnectionFactory,
    pin_id: &str,
    owner_id: &str,
) -> Result<ExportLock, String> {
    let lock_path = partial_lock_path(output);
    loop {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(mut file) => {
                writeln!(file, "{}", std::process::id()).map_err(|error| error.to_string())?;
                writeln!(file, "{pin_id}").map_err(|error| error.to_string())?;
                writeln!(file, "{owner_id}").map_err(|error| error.to_string())?;
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
                let mut lines = contents.lines();
                let pid = lines.next().and_then(|value| value.parse::<u32>().ok());
                let stale_pin_id = lines.next();
                let stale_owner_id = lines.next();
                if pid.is_none_or(process_is_alive) {
                    return Err("busy: export output is active".to_string());
                }
                if let (Some(stale_pin_id), Some(stale_owner_id)) = (stale_pin_id, stale_owner_id) {
                    factory
                        .open_writer()
                        .map_err(|error| error.to_string())?
                        .execute(
                            "DELETE FROM resolution_pins
                             WHERE pin_id=?1 AND owner_kind='reader' AND owner_id=?2",
                            rusqlite::params![stale_pin_id, stale_owner_id],
                        )
                        .map_err(|error| error.to_string())?;
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

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let pid = pid.to_string();
    std::process::Command::new("kill")
        .args(["-0", &pid])
        .status()
        .is_ok_and(|status| status.success())
        || std::process::Command::new("ps")
            .args(["-p", &pid, "-o", "pid="])
            .output()
            .is_ok_and(|output| output.status.success() && !output.stdout.is_empty())
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    let pid = pid.to_string();
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .map_or(true, |output| {
            !output.status.success()
                || String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                    line.split(',')
                        .nth(1)
                        .is_some_and(|value| value.trim_matches('"') == pid)
                })
        })
}

#[cfg(not(any(unix, windows)))]
fn process_is_alive(_pid: u32) -> bool {
    true
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
