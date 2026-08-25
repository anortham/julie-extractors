use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};
use std::sync::Arc;

use crate::artifact_access::validate_current_artifact_output;
use julie_extract_artifact::model::{
    ArtifactComplexityMetric, ArtifactFile, ArtifactIdentifier, ArtifactLiteral,
    ArtifactParseDiagnostic, ArtifactPendingRelationship, ArtifactRelationship,
    ArtifactSourceRegion, ArtifactStructuralFact, ArtifactSymbol, ArtifactSymbolAnnotation,
    ArtifactTypeArgument, ArtifactTypeArgumentUsage, ArtifactTypeFact, FileStatus,
};
use julie_extract_artifact::store::{
    CoordinatorRequest, LeaseHolder, RequestKind, RequestState, StoreCoordinator, StoreLayout,
};
use julie_extractors::EXTRACTION_IDENTITY_EPOCH;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
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
            let warnings = drain_when_available(&mut coordinator, &mut executor, &request)
                .map_err(FromArtifactFailure::Operational)?;
            let observed = coordinator
                .request(&request.request_id)
                .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?;
            return report(&layout, &observed, &stored)
                .map(|report| report.with_warnings(warnings))
                .map_err(FromArtifactFailure::Operational);
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
    let payload_json =
        serde_json::to_string(&payload).expect("from-artifact payload is serializable");
    let reuse = reusable_from_artifact_manifest(
        &layout,
        &coordinator,
        request_id,
        &payload.view_id,
        &payload_json,
    )?;
    let request = coordinator
        .enqueue(CoordinatorRequest::new(
            request_id,
            idempotency_key,
            RequestKind::FromArtifact,
            payload_json,
            holder.holder_id,
            deadline,
            now,
        ))
        .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?
        .request;
    let mut executor =
        StoreRequestExecutor::new(layout.store_db().to_path_buf(), args.family.clone(), None)
            .with_from_artifact_reuse(reuse);
    let warnings = drain_when_available(&mut coordinator, &mut executor, &request)
        .map_err(FromArtifactFailure::Operational)?;
    let observed = coordinator
        .request(&request.request_id)
        .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?;
    report(&layout, &observed, &payload)
        .map(|report| report.with_warnings(warnings))
        .map_err(FromArtifactFailure::Operational)
}

fn reusable_from_artifact_manifest(
    layout: &StoreLayout,
    coordinator: &StoreCoordinator,
    request_id: &str,
    view_id: &str,
    payload_json: &str,
) -> Result<Option<super::executor::FromArtifactReuse>, FromArtifactFailure> {
    let connection = Connection::open(layout.store_db())
        .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?;
    let current = connection
        .query_row(
            "SELECT view.current_generation,manifest.manifest_hash,manifest.request_id
             FROM views AS view JOIN manifests AS manifest
               ON manifest.view_id=view.view_id
              AND manifest.generation=view.current_generation
             WHERE view.view_id=?1",
            [view_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| FromArtifactFailure::Operational(error.to_string()))?;
    let Some((generation, manifest_hash, origin_request_id)) = current else {
        return Ok(None);
    };
    let Ok(origin) = coordinator.request(&origin_request_id) else {
        return Ok(None);
    };
    if origin.kind != RequestKind::FromArtifact
        || !matches!(
            origin.state,
            RequestState::Committed | RequestState::Acknowledged
        )
        || origin.payload_json != payload_json
    {
        return Ok(None);
    }
    Ok(Some(super::executor::FromArtifactReuse {
        request_id: request_id.to_string(),
        generation: u64::try_from(generation)
            .map_err(|_| FromArtifactFailure::Operational("invalid_manifest_generation".into()))?,
        manifest_hash,
    }))
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
        resolver_output_epoch: 0,
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

fn report(
    layout: &StoreLayout,
    request: &julie_extract_artifact::store::CoordinatorRequest,
    payload: &FromArtifactRequestPayload,
) -> Result<StoreReport, String> {
    report_request(
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
    )
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
