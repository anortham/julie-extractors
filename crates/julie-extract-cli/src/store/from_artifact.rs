use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::artifact_access::validate_current_artifact_output;
use julie_extract_artifact::model::{
    ArtifactComplexityMetric, ArtifactFile, ArtifactIdentifier, ArtifactLiteral,
    ArtifactParseDiagnostic, ArtifactPendingRelationship, ArtifactRelationship,
    ArtifactSourceRegion, ArtifactStructuralFact, ArtifactSymbol, ArtifactSymbolAnnotation,
    ArtifactTypeArgument, ArtifactTypeArgumentUsage, ArtifactTypeFact, FileStatus,
};
use julie_extract_artifact::resolution_store::{ResolutionStatus, read_resolution_metadata};
use julie_extract_artifact::store::{
    CoordinatorRequest, LeaseHolder, RequestKind, RequestState, ResolutionBaseReader,
    ResolutionBaseWriter, ResolutionFileIdentity, ResolutionIdentifierRow, ResolutionPendingRow,
    ResolutionValidationError, StoreCoordinator, StoreLayout, resolution_base_id,
};
use julie_extractors::EXTRACTION_IDENTITY_EPOCH;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use super::args::StoreImportArgs;
use super::executor::{
    ArtifactSourceIdentity, FromArtifactRequestPayload, IMPORT_PAYLOAD_MAX_BYTES,
    IMPORT_PLAN_MAX_FILES, PlannedArtifactFile, RequestedLevel, StoreRequestExecutor,
};
use super::import::{
    ImportClock, ImportPidLiveness, RequestReportSpec, StoreExecutionOutcome,
    absolute_runtime_path, classify_failure, drain_when_available, mint_request_id, now_millis,
    open_existing_store, preflight_store_capacity, report_request, root_scope_matches,
};
use super::report::{
    StoreOperation, StoreOutputFormat, StoreReport, StoreRequestState, StoreRequestedLevel,
    StoreResolutionState,
};

enum FromArtifactFailure {
    Incompatible,
    Operational(String),
}

pub(crate) fn run(args: StoreImportArgs) -> StoreExecutionOutcome {
    let format = if args.json {
        StoreOutputFormat::Json
    } else {
        StoreOutputFormat::Human
    };
    let request_id = args
        .request
        .request_id
        .clone()
        .unwrap_or_else(mint_request_id);
    let idempotency_key = args
        .request
        .idempotency_key
        .clone()
        .unwrap_or_else(|| request_id.clone());
    match execute(&args, &request_id, &idempotency_key) {
        Ok(report) => StoreExecutionOutcome::success(report, format),
        Err(failure) => {
            let report = base_report(&args, &request_id, &idempotency_key);
            match failure {
                FromArtifactFailure::Incompatible => {
                    StoreExecutionOutcome::incompatible(report, format)
                }
                FromArtifactFailure::Operational(message) => StoreExecutionOutcome::failure(
                    report.with_failure(classify_failure(&message), message),
                    format,
                ),
            }
        }
    }
}

fn execute(
    args: &StoreImportArgs,
    request_id: &str,
    idempotency_key: &str,
) -> Result<StoreReport, FromArtifactFailure> {
    let existing = if args.store.join("CURRENT").exists() {
        let existing = open_existing_store(&args.store, Some(&args.family))
            .map_err(FromArtifactFailure::Operational)?;
        let holder = lease_holder();
        let coordinator = open_coordinator(&existing.layout, holder)?;
        coordinator
            .request_by_idempotency_key(idempotency_key)
            .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?
            .map(|request| (existing.layout, coordinator, request))
    } else {
        None
    };

    let supplied_source = absolute_runtime_path(
        args.from_artifact
            .as_deref()
            .expect("from-artifact dispatch requires its path"),
    )
    .map_err(FromArtifactFailure::Operational)?;
    if let Some((layout, mut coordinator, request)) = existing {
        let validator =
            StoreRequestExecutor::new(layout.store_db().to_path_buf(), args.family.clone(), None);
        if request.kind != RequestKind::FromArtifact {
            return Err(FromArtifactFailure::Operational(
                "idempotency_conflict".to_string(),
            ));
        }
        let stored = validator
            .validate_from_artifact_payload_json(&request.payload_json)
            .map_err(FromArtifactFailure::Operational)?;
        if stored.family_id != args.family
            || stored.view_id != args.view
            || !root_scope_matches(&args.root, &stored.root)
            || stored.source.path != supplied_source
        {
            return Err(FromArtifactFailure::Operational(
                "idempotency_conflict".to_string(),
            ));
        }
        if !matches!(
            request.state,
            RequestState::Committed | RequestState::Acknowledged
        ) {
            let mut executor = StoreRequestExecutor::new(
                layout.store_db().to_path_buf(),
                args.family.clone(),
                None,
            );
            drain_when_available(&mut coordinator, &mut executor, &request)
                .map_err(FromArtifactFailure::Operational)?;
        }
        let observed = coordinator
            .request(&request.request_id)
            .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?;
        return report(&layout, &observed, &stored).map_err(FromArtifactFailure::Operational);
    }

    let payload = preflight_payload(args, &supplied_source)?;
    let source_bytes = payload.source.file_bytes.saturating_add(
        payload
            .files
            .iter()
            .map(|file| file.content_bytes)
            .fold(0_u64, u64::saturating_add),
    );
    preflight_store_capacity(&args.store, source_bytes)
        .map_err(FromArtifactFailure::Operational)?;
    let layout = StoreLayout::create(&args.store, &args.family, env!("CARGO_PKG_VERSION"))
        .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?;
    let holder = lease_holder();
    let mut coordinator = open_coordinator(&layout, holder.clone())?;
    let now = now_millis();
    let deadline = now.saturating_add(
        i64::try_from(args.request.request_timeout_seconds)
            .unwrap_or(i64::MAX)
            .saturating_mul(1_000),
    );
    let request = coordinator
        .enqueue(CoordinatorRequest::new(
            request_id,
            idempotency_key,
            RequestKind::FromArtifact,
            serde_json::to_string(&payload).expect("from-artifact payload is serializable"),
            holder.holder_id,
            deadline,
            now,
        ))
        .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?
        .request;
    let mut executor =
        StoreRequestExecutor::new(layout.store_db().to_path_buf(), args.family.clone(), None);
    drain_when_available(&mut coordinator, &mut executor, &request)
        .map_err(FromArtifactFailure::Operational)?;
    let observed = coordinator
        .request(&request.request_id)
        .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?;
    report(&layout, &observed, &payload).map_err(FromArtifactFailure::Operational)
}

fn preflight_payload(
    args: &StoreImportArgs,
    supplied_source: &str,
) -> Result<FromArtifactRequestPayload, FromArtifactFailure> {
    let artifact = Path::new(supplied_source);
    validate_current_artifact_output(artifact).map_err(|_| FromArtifactFailure::Incompatible)?;
    let root = args
        .root
        .canonicalize()
        .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?;
    let connection = Connection::open_with_flags(artifact, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?;
    validate_artifact_root(&connection, &root).map_err(FromArtifactFailure::Operational)?;
    validate_resolution(&connection).map_err(|error| {
        FromArtifactFailure::Operational(format!("resolution_input_incomplete: {error}"))
    })?;
    validate_capability_identity(&connection).map_err(FromArtifactFailure::Operational)?;
    let artifact_id =
        metadata_value(&connection, "artifact_id").map_err(FromArtifactFailure::Operational)?;
    let files = artifact_plan(&connection, &root).map_err(FromArtifactFailure::Operational)?;
    if files.len() > IMPORT_PLAN_MAX_FILES {
        return Err(FromArtifactFailure::Operational(
            "invalid_from_artifact_request_payload:too_many_files".to_string(),
        ));
    }
    let source = ArtifactSourceIdentity {
        path: supplied_source.to_string(),
        artifact_id,
        file_bytes: artifact
            .metadata()
            .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?
            .len(),
        file_sha256: sha256_file(artifact).map_err(FromArtifactFailure::Operational)?,
        extraction_epoch: EXTRACTION_IDENTITY_EPOCH,
        resolver_output_epoch: crate::resolution::RESOLUTION_VERSION,
    };
    let payload = FromArtifactRequestPayload {
        schema_version: 1,
        family_id: args.family.clone(),
        root: root.to_string_lossy().into_owned(),
        view_id: args.view.clone(),
        source,
        files,
    };
    if serde_json::to_vec(&payload)
        .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?
        .len()
        > IMPORT_PAYLOAD_MAX_BYTES
    {
        return Err(FromArtifactFailure::Operational(
            "invalid_from_artifact_request_payload:payload_too_large".to_string(),
        ));
    }
    Ok(payload)
}

fn artifact_plan(connection: &Connection, root: &Path) -> Result<Vec<PlannedArtifactFile>, String> {
    let mut statement = connection
        .prepare(
            "SELECT file_id,path,language,content_hash,content_bytes,indexed_at,status
             FROM files ORDER BY path",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(PlannedArtifactFile {
                file_id: row.get(0)?,
                path: row.get(1)?,
                language: row.get(2)?,
                content_hash: row.get(3)?,
                content_bytes: u64::try_from(row.get::<_, i64>(4)?).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Integer,
                        error.into(),
                    )
                })?,
                indexed_at: row.get(5)?,
                status: row.get(6)?,
            })
        })
        .map_err(|error| error.to_string())?;
    let mut files = Vec::new();
    for row in rows {
        let file = row.map_err(|error| error.to_string())?;
        if !valid_relative_path(root, &file.path)
            || !valid_blake3_hash(&file.content_hash)
            || file.file_id.is_empty()
            || file.language.is_empty()
            || !matches!(
                file.status.as_str(),
                "indexed" | "failed_preserved" | "unsupported"
            )
        {
            return Err(format!("invalid_from_artifact_file: {}", file.path));
        }
        files.push(file);
    }
    Ok(files)
}

pub(crate) fn load_artifact_file(
    source: &ArtifactSourceIdentity,
    planned: &PlannedArtifactFile,
) -> Result<ArtifactFile, String> {
    let connection = Connection::open_with_flags(&source.path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| error.to_string())?;
    let (
        file_id,
        path,
        language,
        content_hash,
        content_bytes,
        line_count,
        indexed_at,
        status,
        metadata_json,
    ) = connection
        .query_row(
            "SELECT file_id,path,language,content_hash,content_bytes,line_count,indexed_at,
                    status,metadata_json
             FROM files WHERE file_id=?1",
            [&planned.file_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
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
    if file_id != planned.file_id
        || path != planned.path
        || language != planned.language
        || content_hash != planned.content_hash
        || u64::try_from(content_bytes).ok() != Some(planned.content_bytes)
        || indexed_at != planned.indexed_at
        || status != planned.status
    {
        return Err(format!("from_artifact_source_changed:{}", planned.path));
    }
    let status = match status.as_str() {
        "indexed" => FileStatus::Indexed,
        "failed_preserved" => FileStatus::FailedPreserved,
        "unsupported" => FileStatus::Unsupported,
        _ => {
            return Err(format!(
                "invalid_from_artifact_file_status:{}",
                planned.path
            ));
        }
    };
    Ok(ArtifactFile {
        file_id,
        path,
        language,
        content_hash,
        content_bytes,
        line_count,
        indexed_at,
        status,
        metadata_json,
        symbols: load_child_rows::<ArtifactSymbol>(&connection, "symbols", &planned.file_id)?,
        symbol_annotations: load_child_rows::<ArtifactSymbolAnnotation>(
            &connection,
            "symbol_annotations",
            &planned.file_id,
        )?,
        identifiers: load_child_rows::<ArtifactIdentifier>(
            &connection,
            "identifiers",
            &planned.file_id,
        )?,
        relationships: load_child_rows::<ArtifactRelationship>(
            &connection,
            "relationships",
            &planned.file_id,
        )?,
        pending_relationships: load_child_rows::<ArtifactPendingRelationship>(
            &connection,
            "pending_relationships",
            &planned.file_id,
        )?,
        type_facts: load_child_rows::<ArtifactTypeFact>(
            &connection,
            "type_facts",
            &planned.file_id,
        )?,
        type_argument_usages: load_child_rows::<ArtifactTypeArgumentUsage>(
            &connection,
            "type_argument_usages",
            &planned.file_id,
        )?,
        type_arguments: load_child_rows::<ArtifactTypeArgument>(
            &connection,
            "type_arguments",
            &planned.file_id,
        )?,
        literals: load_child_rows::<ArtifactLiteral>(&connection, "literals", &planned.file_id)?,
        source_regions: load_child_rows::<ArtifactSourceRegion>(
            &connection,
            "source_regions",
            &planned.file_id,
        )?,
        structural_facts: load_child_rows::<ArtifactStructuralFact>(
            &connection,
            "structural_facts",
            &planned.file_id,
        )?,
        complexity_metrics: load_child_rows::<ArtifactComplexityMetric>(
            &connection,
            "complexity_metrics",
            &planned.file_id,
        )?,
        parse_diagnostics: load_child_rows::<ArtifactParseDiagnostic>(
            &connection,
            "parse_diagnostics",
            &planned.file_id,
        )?,
    })
}

pub(crate) fn verify_source_identity(payload: &FromArtifactRequestPayload) -> Result<(), String> {
    let source = &payload.source;
    let path = Path::new(&source.path);
    if path.canonicalize().map_err(|error| error.to_string())? != path {
        return Err("from_artifact_source_not_canonical".to_string());
    }
    if path.metadata().map_err(|error| error.to_string())?.len() != source.file_bytes
        || sha256_file(path)? != source.file_sha256
    {
        return Err("from_artifact_source_changed".to_string());
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| error.to_string())?;
    validate_current_artifact_output(path).map_err(|_| "store_incompatible".to_string())?;
    validate_artifact_root(&connection, Path::new(&payload.root))?;
    validate_resolution(&connection)
        .map_err(|error| format!("resolution_input_incomplete: {error}"))?;
    validate_capability_identity(&connection)?;
    if metadata_value(&connection, "artifact_id")? != source.artifact_id {
        return Err("from_artifact_source_changed".to_string());
    }
    Ok(())
}

fn load_child_rows<T: DeserializeOwned>(
    connection: &Connection,
    table: &str,
    file_id: &str,
) -> Result<Vec<T>, String> {
    let mut columns = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| error.to_string())?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let ordered = {
        let mut keys = columns
            .iter()
            .filter(|(_, _, ordinal)| *ordinal > 0)
            .cloned()
            .collect::<Vec<_>>();
        keys.sort_by_key(|(_, _, ordinal)| *ordinal);
        if keys.is_empty() {
            "t.rowid".to_string()
        } else {
            keys.into_iter()
                .map(|(name, _, _)| format!("t.{}", quote_identifier(&name)))
                .collect::<Vec<_>>()
                .join(",")
        }
    };
    columns.retain(|(name, _, _)| name != "file_id");
    let object = columns
        .iter()
        .flat_map(|(name, declared_type, _)| {
            let column = format!("t.{}", quote_identifier(name));
            let value = if declared_type.eq_ignore_ascii_case("REAL") {
                format!(
                    "CASE WHEN {column} IS NULL THEN NULL ELSE json(printf('%.17g',{column})) END"
                )
            } else {
                column
            };
            [format!("'{}'", name.replace('\'', "''")), value]
        })
        .collect::<Vec<_>>()
        .into_iter()
        .chain(
            matches!(
                table,
                "identifiers" | "relationships" | "pending_relationships"
            )
            .then_some([
                "'site_is_exact'".to_string(),
                "site.is_exact".to_string(),
                "'site_provenance'".to_string(),
                "site.provenance".to_string(),
            ])
            .into_iter()
            .flatten(),
        )
        .collect::<Vec<_>>()
        .join(",");
    let filter = match table {
        "symbol_annotations" | "type_facts" => {
            "t.symbol_id IN (SELECT symbol_id FROM symbols WHERE file_id=?1)"
        }
        "type_arguments" => {
            "t.usage_id IN (SELECT usage_id FROM type_argument_usages WHERE file_id=?1)"
        }
        _ => "t.file_id=?1",
    };
    let join = matches!(
        table,
        "identifiers" | "relationships" | "pending_relationships"
    )
    .then_some(" JOIN reference_sites AS site ON site.reference_site_id=t.reference_site_id")
    .unwrap_or_default();
    let sql = format!(
        "SELECT json_object({object}) FROM {} AS t{join} WHERE {filter} ORDER BY {ordered}",
        quote_identifier(table)
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([file_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    let mut values = Vec::new();
    for row in rows {
        let mut value =
            serde_json::from_str::<serde_json::Value>(&row.map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        normalize_sqlite_booleans(&mut value);
        values.push(serde_json::from_value(value).map_err(|error| error.to_string())?);
    }
    Ok(values)
}

fn normalize_sqlite_booleans(value: &mut serde_json::Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for key in [
        "is_test",
        "test_container",
        "test_lifecycle",
        "site_is_exact",
        "is_inferred",
    ] {
        if let Some(value) = object.get_mut(key)
            && let Some(number) = value.as_i64()
        {
            *value = serde_json::Value::Bool(number != 0);
        }
    }
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

pub(crate) struct ImportedResolutionBase {
    pub base_id: String,
    pub identity: ResolutionFileIdentity,
    pub already_ready: bool,
    /// True only when this call published a new final base file (hard-link from scratch).
    /// Callers clean this path on post-materialize quantum failure so a rolled-back
    /// catalog does not leave an orphan file. Reused existing files are not cleaned.
    pub published_new_file: bool,
}

/// Materialize a resolution base under the import/from_artifact quantum.
///
/// Architecture note (T8 multi-txn vs same-quantum): the coordinator quantum already
/// holds one IMMEDIATE writer on `store.db`. A nested second IMMEDIATE writer blocks
/// on that lock, so T8a (building) and T8d (ready) cannot commit as separate durable
/// store transactions inside this quantum. Steps T8a–T8d therefore share the outer
/// quantum transaction. Strongest in-quantum guarantees:
/// - building reclaim refuses live foreign owners (see reclaim path)
/// - new final files are cleaned on error after publish when CAS/later steps fail
/// - callers must clean `published_new_file` if later quantum work fails before commit
pub(crate) fn materialize_resolution_base(
    transaction: &Transaction<'_>,
    store_db: &Path,
    payload: &FromArtifactRequestPayload,
    generation: i64,
    manifest_hash: &str,
    request_id: &str,
    indexed_at: &str,
) -> Result<ImportedResolutionBase, String> {
    let source =
        Connection::open_with_flags(&payload.source.path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| error.to_string())?;
    let mut visible = transaction
        .prepare(
            "SELECT path,version_id FROM manifest_entries
             WHERE view_id=?1 AND generation=?2 AND version_id IS NOT NULL
             ORDER BY version_id",
        )
        .map_err(|error| error.to_string())?;
    let visible = visible
        .query_map(rusqlite::params![payload.view_id, generation], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let versions_by_path = visible.iter().cloned().collect::<BTreeMap<_, _>>();
    let source_versions = visible
        .iter()
        .map(|(_, version)| *version)
        .collect::<Vec<_>>();
    let file_ids = payload
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.file_id.as_str()))
        .collect::<BTreeMap<_, _>>();
    let base_id = resolution_base_id(manifest_hash, payload.source.resolver_output_epoch);
    let relative_path = format!("bases/{base_id}.db");
    let generation_dir = store_db
        .parent()
        .ok_or_else(|| "invalid_store_path".to_string())?;
    let family_root = generation_dir
        .parent()
        .ok_or_else(|| "invalid_store_path".to_string())?;
    let final_path = generation_dir.join(&relative_path);
    let scratch_path = family_root
        .join("scratch")
        .join(format!("resolution-{base_id}-{request_id}.partial.db"));

    // T8a: catalog state=building (or reuse ready identity hit). Same outer quantum txn.
    let existing = transaction
        .query_row(
            "SELECT state,manifest_hash,resolver_output_epoch,file_sha256,
                    identifier_count,pending_count,request_id
             FROM resolution_bases WHERE base_id=?1",
            [&base_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some((
        state,
        existing_manifest,
        existing_epoch,
        existing_sha,
        existing_identifiers,
        existing_pending,
        prior_request_id,
    )) = existing
    {
        if state == "ready" {
            if !final_path.exists() {
                return Err("resolution_base_ready_without_file".to_string());
            }
            let reader =
                ResolutionBaseReader::open(&final_path).map_err(|error| error.to_string())?;
            let identity = reader.file_identity().clone();
            if existing_manifest != manifest_hash
                || existing_epoch != payload.source.resolver_output_epoch
                || existing_sha.as_deref() != Some(identity.file_sha256.as_str())
                || existing_identifiers
                    != i64::try_from(identity.counts.identifiers)
                        .map_err(|_| "resolution_identifier_count_out_of_range".to_string())?
                || existing_pending
                    != i64::try_from(identity.counts.pending)
                        .map_err(|_| "resolution_pending_count_out_of_range".to_string())?
                || reader
                    .source_versions()
                    .map_err(|error| error.to_string())?
                    != source_versions
            {
                return Err("resolution_base_catalog_identity_mismatch".to_string());
            }
            return Ok(ImportedResolutionBase {
                base_id,
                identity,
                already_ready: true,
                published_new_file: false,
            });
        }
        if state == "building" {
            if prior_request_id != request_id
                && !prior_building_owner_is_reclaimable(family_root, &prior_request_id)?
            {
                return Err(format!(
                    "resolution_base_building_busy:{prior_request_id}"
                ));
            }
            transaction
                .execute(
                    "UPDATE resolution_bases
                     SET request_id=?1,updated_at=?2
                     WHERE base_id=?3 AND state='building'",
                    rusqlite::params![request_id, indexed_at, base_id],
                )
                .map_err(|error| error.to_string())?;
        } else {
            return Err(format!("resolution_base_unexpected_state:{state}"));
        }
    } else {
        transaction
            .execute(
                "INSERT INTO resolution_bases
                 (base_id,manifest_hash,resolver_output_epoch,state,relative_path,
                  identifier_count,pending_count,file_bytes,file_sha256,request_id,
                  created_at,updated_at)
                 VALUES (?1,?2,?3,'building',?4,0,0,NULL,NULL,?5,?6,?6)",
                rusqlite::params![
                    base_id,
                    manifest_hash,
                    payload.source.resolver_output_epoch,
                    relative_path,
                    request_id,
                    indexed_at,
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    for version_id in &source_versions {
        transaction
            .execute(
                "INSERT OR IGNORE INTO resolution_base_versions(base_id,version_id)
                 VALUES (?1,?2)",
                rusqlite::params![base_id, version_id],
            )
            .map_err(|error| error.to_string())?;
    }

    // T8b: materialize under scratch/partial path.
    if scratch_path.exists() {
        remove_base_file_set(&scratch_path)?;
    }
    if final_path.exists() {
        let reader = ResolutionBaseReader::open(&final_path).map_err(|error| error.to_string());
        match reader {
            Ok(reader)
                if reader.file_identity().manifest_hash == manifest_hash
                    && reader.file_identity().resolver_output_epoch
                        == payload.source.resolver_output_epoch
                    && reader
                        .source_versions()
                        .map_err(|error| error.to_string())?
                        == source_versions =>
            {
                let identity = reader.file_identity().clone();
                drop(reader);
                // T8d only: file already published with matching identity.
                cas_building_to_ready(transaction, &base_id, request_id, &identity, indexed_at)?;
                return Ok(ImportedResolutionBase {
                    base_id,
                    identity,
                    already_ready: false,
                    published_new_file: false,
                });
            }
            Ok(_) | Err(_) => {
                remove_base_file_set(&final_path)?;
            }
        }
    }

    fs::create_dir_all(
        scratch_path
            .parent()
            .ok_or_else(|| "invalid_scratch_path".to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut writer = ResolutionBaseWriter::new(
        &scratch_path,
        manifest_hash,
        payload.source.resolver_output_epoch,
    )
    .map_err(|error| error.to_string())?;
    for version_id in &source_versions {
        writer
            .push_source_version(*version_id)
            .map_err(|error| error.to_string())?;
    }
    for (path, version_id) in &visible {
        let file_id = file_ids
            .get(path.as_str())
            .ok_or_else(|| format!("from_artifact_file_missing:{path}"))?;
        push_identifier_rows(
            &source,
            &mut writer,
            *version_id,
            file_id,
            &versions_by_path,
        )?;
        push_pending_rows(
            &source,
            &mut writer,
            *version_id,
            file_id,
            &versions_by_path,
        )?;
    }
    let identity = writer
        .finish_with_target_lookup(|version_id, symbol_id| {
            transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM symbols WHERE version_id=?1 AND symbol_id=?2)",
                    rusqlite::params![version_id, symbol_id],
                    |row| row.get(0),
                )
                .map_err(ResolutionValidationError::Sqlite)
        })
        .map_err(|error| error.to_string())?;

    // T8c: durable publish into bases + fsync.
    // New final files are cleaned on subsequent errors until CAS succeeds.
    fs::create_dir_all(
        final_path
            .parent()
            .ok_or_else(|| "invalid_base_path".to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let mut published_new_file = false;
    match fs::hard_link(&scratch_path, &final_path) {
        Ok(()) => {
            published_new_file = true;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing =
                ResolutionBaseReader::open(&final_path).map_err(|error| error.to_string())?;
            if existing.file_identity() != &identity {
                return Err("from_artifact_resolution_base_identity_mismatch".to_string());
            }
        }
        Err(error) => return Err(error.to_string()),
    }
    let cleanup_published = |published: bool| {
        if published {
            let _ = remove_base_file_set(&final_path);
        }
    };
    if let Err(error) = File::open(
        final_path
            .parent()
            .ok_or_else(|| "invalid_base_path".to_string())?,
    )
    .and_then(|file| file.sync_all())
    {
        cleanup_published(published_new_file);
        return Err(error.to_string());
    }
    if let Err(error) = remove_base_file_set(&scratch_path) {
        cleanup_published(published_new_file);
        return Err(error);
    }

    // T8d: CAS building → ready only if identity matches.
    if let Err(error) =
        cas_building_to_ready(transaction, &base_id, request_id, &identity, indexed_at)
    {
        cleanup_published(published_new_file);
        return Err(error);
    }

    Ok(ImportedResolutionBase {
        base_id,
        identity,
        already_ready: false,
        published_new_file,
    })
}

/// Reclaim a building base only when the prior owner is the same request, missing,
/// terminal, or already receipted. Live queued/claimed owners keep exclusive ownership.
fn prior_building_owner_is_reclaimable(
    family_root: &Path,
    prior_request_id: &str,
) -> Result<bool, String> {
    let coordinator_db = family_root.join("coord.db");
    let connection = Connection::open_with_flags(
        &coordinator_db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    let state = connection
        .query_row(
            "SELECT state FROM requests WHERE request_id=?1",
            [prior_request_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    match state.as_deref() {
        None => Ok(true),
        Some("failed" | "committed" | "acknowledged") => Ok(true),
        Some(_) => {
            let receipted = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM request_receipts WHERE request_id=?1)",
                    [prior_request_id],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|error| error.to_string())?;
            Ok(receipted)
        }
    }
}

fn cas_building_to_ready(
    transaction: &Transaction<'_>,
    base_id: &str,
    request_id: &str,
    identity: &ResolutionFileIdentity,
    indexed_at: &str,
) -> Result<(), String> {
    let identifier_count = i64::try_from(identity.counts.identifiers)
        .map_err(|_| "resolution_identifier_count_out_of_range".to_string())?;
    let pending_count = i64::try_from(identity.counts.pending)
        .map_err(|_| "resolution_pending_count_out_of_range".to_string())?;
    let file_bytes = i64::try_from(identity.file_bytes)
        .map_err(|_| "resolution_file_size_out_of_range".to_string())?;
    let changed = transaction
        .execute(
            "UPDATE resolution_bases
             SET state='ready',identifier_count=?1,pending_count=?2,file_bytes=?3,
                 file_sha256=?4,updated_at=?5
             WHERE base_id=?6 AND state='building' AND request_id=?7",
            rusqlite::params![
                identifier_count,
                pending_count,
                file_bytes,
                identity.file_sha256,
                indexed_at,
                base_id,
                request_id,
            ],
        )
        .map_err(|error| error.to_string())?;
    if changed != 1 {
        return Err("resolution_base_ready_cas_lost".to_string());
    }
    Ok(())
}

pub(crate) fn remove_base_file_set_for_cleanup(path: &Path) -> Result<(), String> {
    remove_base_file_set(path)
}

fn remove_base_file_set(path: &Path) -> Result<(), String> {
    for suffix in ["", "-wal", "-shm"] {
        let candidate = if suffix.is_empty() {
            path.to_path_buf()
        } else {
            PathBuf::from(format!("{}{suffix}", path.display()))
        };
        match fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn push_identifier_rows(
    source: &Connection,
    writer: &mut ResolutionBaseWriter,
    version_id: i64,
    file_id: &str,
    versions_by_path: &BTreeMap<String, i64>,
) -> Result<(), String> {
    let mut statement = source
        .prepare(
            "SELECT resolution.identifier_id,target_file.path,resolution.target_symbol_id,
                    resolution.tier,resolution.confidence,resolution.method,resolution.outcome,
                    resolution.candidates
             FROM identifier_resolutions AS resolution
             JOIN identifiers AS identifier ON identifier.identifier_id=resolution.identifier_id
             LEFT JOIN symbols AS target ON target.symbol_id=resolution.target_symbol_id
             LEFT JOIN files AS target_file ON target_file.file_id=target.file_id
             WHERE identifier.file_id=?1 ORDER BY resolution.identifier_id",
        )
        .map_err(|error| error.to_string())?;
    let mut rows = statement
        .query([file_id])
        .map_err(|error| error.to_string())?;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        let target_path = row
            .get::<_, Option<String>>(1)
            .map_err(|error| error.to_string())?;
        let target_version_id = target_path
            .as_deref()
            .map(|path| {
                versions_by_path
                    .get(path)
                    .copied()
                    .ok_or_else(|| format!("from_artifact_resolution_target_not_visible:{path}"))
            })
            .transpose()?;
        writer
            .push_identifier_resolution(ResolutionIdentifierRow {
                version_id,
                identifier_id: row.get(0).map_err(|error| error.to_string())?,
                target_version_id,
                target_symbol_id: row.get(2).map_err(|error| error.to_string())?,
                tier: row.get(3).map_err(|error| error.to_string())?,
                confidence: row.get(4).map_err(|error| error.to_string())?,
                method: row.get(5).map_err(|error| error.to_string())?,
                outcome: row.get(6).map_err(|error| error.to_string())?,
                candidates: row.get(7).map_err(|error| error.to_string())?,
            })
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn push_pending_rows(
    source: &Connection,
    writer: &mut ResolutionBaseWriter,
    version_id: i64,
    file_id: &str,
    versions_by_path: &BTreeMap<String, i64>,
) -> Result<(), String> {
    let mut statement = source
        .prepare(
            "SELECT resolution.pending_relationship_id,target_file.path,
                    resolution.target_symbol_id,resolution.tier,resolution.confidence,
                    resolution.method
             FROM pending_resolutions AS resolution
             JOIN pending_relationships AS pending
               ON pending.pending_relationship_id=resolution.pending_relationship_id
             JOIN symbols AS target ON target.symbol_id=resolution.target_symbol_id
             JOIN files AS target_file ON target_file.file_id=target.file_id
             WHERE pending.file_id=?1 ORDER BY resolution.pending_relationship_id",
        )
        .map_err(|error| error.to_string())?;
    let mut rows = statement
        .query([file_id])
        .map_err(|error| error.to_string())?;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        let target_path = row.get::<_, String>(1).map_err(|error| error.to_string())?;
        let target_version_id = versions_by_path
            .get(&target_path)
            .copied()
            .ok_or_else(|| format!("from_artifact_resolution_target_not_visible:{target_path}"))?;
        writer
            .push_pending_resolution(ResolutionPendingRow {
                version_id,
                pending_relationship_id: row.get(0).map_err(|error| error.to_string())?,
                target_version_id,
                target_symbol_id: row.get(2).map_err(|error| error.to_string())?,
                tier: row.get(3).map_err(|error| error.to_string())?,
                confidence: row.get(4).map_err(|error| error.to_string())?,
                method: row.get(5).map_err(|error| error.to_string())?,
            })
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn report(
    layout: &StoreLayout,
    request: &julie_extract_artifact::store::CoordinatorRequest,
    payload: &FromArtifactRequestPayload,
) -> Result<StoreReport, String> {
    let mut report = report_request(
        layout,
        request,
        RequestReportSpec {
            operation: StoreOperation::FromArtifact,
            family_id: payload.family_id.clone(),
            view_id: payload.view_id.clone(),
            root: payload.root.clone(),
            requested_level: RequestedLevel::Full,
            l1_event_kind: "store_from_artifact_manifest_published",
        },
    )?;
    let connection = Connection::open(layout.store_db()).map_err(|error| error.to_string())?;
    if let Some((state, exact_at, base_id, delta_generation, current_generation)) = connection
        .query_row(
            "SELECT resolution_state,resolution_exact_at,resolution_base_id,
                    resolution_delta_generation,current_generation
             FROM views WHERE view_id=?1",
            [&payload.view_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
    {
        report.resolution.state = match state.as_str() {
            "exact" => StoreResolutionState::Exact,
            "converging" => StoreResolutionState::Converging,
            _ => StoreResolutionState::Unbound,
        };
        report.resolution.exact_at_matches = exact_at.is_some() && exact_at == current_generation;
        report.resolution.base_id = base_id;
        report.resolution.delta_generation =
            delta_generation.and_then(|value| value.try_into().ok());
        report.resolution.exact_at_generation = exact_at.and_then(|value| value.try_into().ok());
    }
    Ok(report)
}

fn lease_holder() -> LeaseHolder {
    LeaseHolder::new(
        format!("cli-{}", std::process::id()),
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
    )
}

fn open_coordinator(
    layout: &StoreLayout,
    holder: LeaseHolder,
) -> Result<StoreCoordinator, FromArtifactFailure> {
    StoreCoordinator::open_with_runtime(
        layout,
        holder,
        Arc::new(ImportClock),
        Arc::new(ImportPidLiveness),
    )
    .map_err(|error| FromArtifactFailure::Operational(error.to_string()))
}

fn base_report(args: &StoreImportArgs, request_id: &str, idempotency_key: &str) -> StoreReport {
    StoreReport::new(
        request_id,
        &args.family,
        &args.view,
        StoreRequestState::Failed,
    )
    .with_operation(StoreOperation::FromArtifact)
    .with_idempotency_key(idempotency_key)
    .with_root(args.root.to_string_lossy())
    .with_requested_level(StoreRequestedLevel::Full)
}

fn validate_resolution(connection: &Connection) -> Result<(), String> {
    let metadata = read_resolution_metadata(connection)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            "reference_resolution_status is missing from the source artifact".to_string()
        })?;
    if metadata.status != ResolutionStatus::Complete {
        return Err(format!(
            "reference_resolution_status must be complete, found {}",
            metadata.status.as_str()
        ));
    }
    if metadata.version != crate::resolution::RESOLUTION_VERSION {
        return Err(format!(
            "reference_resolution_version mismatch: expected {}, found {}",
            crate::resolution::RESOLUTION_VERSION,
            metadata.version
        ));
    }
    if metadata.last_full_revision <= 0 {
        return Err("reference_resolution_last_full_revision must be positive".to_string());
    }
    let missing_identifier: bool = connection
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM identifiers AS identifier
               LEFT JOIN identifier_resolutions AS resolution
                 ON resolution.identifier_id=identifier.identifier_id
               WHERE resolution.identifier_id IS NULL
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if missing_identifier {
        return Err("reference_resolution_status is incomplete for identifiers".to_string());
    }
    Ok(())
}

fn validate_capability_identity(connection: &Connection) -> Result<(), String> {
    let (parser, capability) = crate::capability_snapshot::current_capability_fingerprints();
    for (key, expected) in [
        ("parser_inventory_fingerprint", parser),
        ("capability_snapshot_fingerprint", capability),
    ] {
        let found = metadata_value(connection, key)?;
        if found != expected {
            return Err(format!("extraction_epoch_mismatch: {key}"));
        }
    }
    Ok(())
}

fn validate_artifact_root(connection: &Connection, requested_root: &Path) -> Result<(), String> {
    let stored_root = metadata_value(connection, "root_path")?;
    if Path::new(&stored_root) != requested_root {
        return Err("root_mismatch: artifact root does not match requested root".to_string());
    }
    Ok(())
}

fn metadata_value(connection: &Connection, key: &str) -> Result<String, String> {
    connection
        .query_row(
            "SELECT value FROM artifact_metadata WHERE key=?1",
            [key],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

fn valid_relative_path(root: &Path, path: &str) -> bool {
    !path.is_empty()
        && path.len() <= super::args::MAX_STORE_PATH_BYTES
        && !path.contains(['\\', ':', '\0'])
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && root.join(path).starts_with(root)
}

fn valid_blake3_hash(hash: &str) -> bool {
    hash.strip_prefix("blake3:").is_some_and(|value| {
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
