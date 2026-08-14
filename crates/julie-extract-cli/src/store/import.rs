use std::io::{self, Write};
use std::path::{Component, Path};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::store::{
    CoordinatorError, CoordinatorPolicy, CoordinatorRequest, LeaseHolder, RequestKind,
    RequestState, StoreCoordinator, StoreLayout,
};
use rusqlite::{Connection, OptionalExtension, params};

use super::args::{StoreImportArgs, StoreLevelArg};
use super::executor::{
    ImportRequestPayload, ImportScanControls, PlannedImportFile, RequestedLevel,
    StoreRequestExecutor, frozen_chunk_versions_from_environment,
};
use super::report::{
    StoreCommandOutcome, StoreCoordinatorDisposition, StoreErrorReport, StoreFailureClass,
    StoreLevelCompletion, StoreManifestDisposition, StoreOperation, StoreOutputFormat,
    StoreOutputStream, StoreReport, StoreRequestState, StoreRequestedLevel, StoreResolutionState,
    StoreRowCounts,
};

pub struct StoreExecutionOutcome {
    exit_code: u8,
    rendered: String,
    stream: StoreOutputStream,
}

impl StoreExecutionOutcome {
    pub(crate) fn success(report: StoreReport, format: StoreOutputFormat) -> Self {
        let outcome = if report.failure_class == StoreFailureClass::RequestTimeout {
            StoreCommandOutcome::observed_incomplete(report)
        } else {
            StoreCommandOutcome::queued(report)
        };
        Self::request(outcome, format)
    }

    pub(crate) fn failure(report: StoreReport, format: StoreOutputFormat) -> Self {
        Self::request(StoreCommandOutcome::failed(report), format)
    }

    pub(crate) fn incompatible(report: StoreReport, format: StoreOutputFormat) -> Self {
        Self::request(StoreCommandOutcome::incompatible(report), format)
    }

    fn request(outcome: StoreCommandOutcome, format: StoreOutputFormat) -> Self {
        let exit_code = outcome.exit_code();
        let rendered = outcome.render(format);
        let stream = outcome
            .output_plan(format == StoreOutputFormat::Json)
            .stream;
        Self {
            exit_code,
            rendered,
            stream,
        }
    }

    pub(crate) fn rendered(exit_code: u8, rendered: String, stream: StoreOutputStream) -> Self {
        Self {
            exit_code,
            rendered,
            stream,
        }
    }

    pub fn exit_code(&self) -> u8 {
        self.exit_code
    }

    pub fn write(&self) {
        match self.stream {
            StoreOutputStream::Stdout => {
                let _ = io::stdout().lock().write_all(self.rendered.as_bytes());
            }
            StoreOutputStream::Stderr => {
                let _ = io::stderr().lock().write_all(self.rendered.as_bytes());
            }
        }
    }
}

pub(crate) struct ExistingStoreContext {
    pub layout: StoreLayout,
    pub family_id: String,
}

pub(crate) fn open_existing_store(
    store: &Path,
    requested_family: Option<&str>,
) -> Result<ExistingStoreContext, String> {
    if !store.join("CURRENT").exists() {
        return Err("store_not_found".to_string());
    }
    let layout = StoreLayout::open(store).map_err(|error| error.to_string())?;
    let family_id = trusted_store_family(&layout)?;
    if requested_family.is_some_and(|requested| family_id != requested) {
        return Err("family_mismatch".to_string());
    }
    Ok(ExistingStoreContext { layout, family_id })
}

pub(crate) fn require_existing_view(
    layout: &StoreLayout,
    requested_root: &Path,
    view_id: &str,
) -> Result<String, String> {
    let root = requested_root
        .canonicalize()
        .map_err(|error| error.to_string())?
        .to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "path_is_not_utf8".to_string())?;
    let connection =
        rusqlite::Connection::open(layout.store_db()).map_err(|error| error.to_string())?;
    let stored = connection
        .query_row(
            "SELECT root FROM views WHERE view_id = ?1",
            [view_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "view_not_found".to_string())?;
    if stored != root {
        return Err("view_root_mismatch".to_string());
    }
    Ok(root)
}

pub(crate) fn normalize_root_relative(path: &Path) -> Result<String, String> {
    if path.is_absolute() {
        return Err("file_must_be_root_relative".to_string());
    }
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            return Err("invalid_file_path".to_string());
        };
        parts.push(
            part.to_str()
                .ok_or_else(|| "path_is_not_utf8".to_string())?
                .to_string(),
        );
    }
    if parts.is_empty() {
        return Err("invalid_file_path".to_string());
    }
    Ok(parts.join("/"))
}

pub(crate) struct RequestReportSpec {
    pub operation: StoreOperation,
    pub family_id: String,
    pub view_id: String,
    pub root: String,
    pub requested_level: RequestedLevel,
    pub l1_event_kind: &'static str,
}

pub(crate) fn report_request(
    layout: &StoreLayout,
    request: &CoordinatorRequest,
    spec: RequestReportSpec,
) -> Result<StoreReport, String> {
    let committed = matches!(
        request.state,
        RequestState::Committed | RequestState::Acknowledged
    );
    let report_state = match request.state {
        RequestState::Queued => StoreRequestState::Queued,
        RequestState::Claimed => StoreRequestState::Claimed,
        RequestState::Committed | RequestState::Acknowledged => StoreRequestState::Committed,
        RequestState::Failed => StoreRequestState::Failed,
    };
    let mut report = StoreReport::new(
        &request.request_id,
        &spec.family_id,
        &spec.view_id,
        report_state,
    )
    .with_operation(spec.operation)
    .with_idempotency_key(&request.idempotency_key)
    .with_root(&spec.root)
    .with_requested_level(match spec.requested_level {
        RequestedLevel::L1 => StoreRequestedLevel::L1,
        RequestedLevel::Full => StoreRequestedLevel::Full,
    });
    if !committed {
        populate_durable_projection(
            &mut report,
            layout,
            &spec.view_id,
            &request.request_id,
            spec.l1_event_kind,
            spec.requested_level,
            false,
        )?;
        if request.state == RequestState::Failed {
            let message = request
                .error_json
                .clone()
                .unwrap_or_else(|| "store_request_failed".to_string());
            return Ok(report.with_failure(classify_failure(&message), message));
        }
        report.failure_class = StoreFailureClass::RequestTimeout;
        report.error = Some(StoreErrorReport {
            class: StoreFailureClass::RequestTimeout,
            message: "request_timeout".to_string(),
        });
        return Ok(report);
    }
    let result: serde_json::Value = serde_json::from_str(
        request
            .result_json
            .as_deref()
            .ok_or("missing_store_result")?,
    )
    .map_err(|error| error.to_string())?;
    report.coordinator = StoreCoordinatorDisposition::Committed;
    report.manifest.generation = result["manifest_generation"].as_u64();
    report.manifest.hash = result["manifest_hash"].as_str().map(ToOwned::to_owned);
    report.manifest.disposition = match result["manifest_disposition"].as_str() {
        Some("created") => StoreManifestDisposition::Created,
        Some("reused") => StoreManifestDisposition::Reused,
        _ => StoreManifestDisposition::NotPublished,
    };
    report.completion = StoreLevelCompletion {
        l1: result["l1"].as_bool().unwrap_or(false),
        l2: result["l2"].as_bool().unwrap_or(false),
        l3: result["l3"].as_bool().unwrap_or(false),
    };
    report.row_counts = result
        .get("row_counts")
        .and_then(|counts| {
            Some(StoreRowCounts {
                file_versions: counts.get("file_versions")?.as_u64()?,
                l1: counts.get("l1")?.as_u64()?,
                l2: counts.get("l2")?.as_u64()?,
                l3: counts.get("l3")?.as_u64()?,
            })
        })
        .ok_or("invalid_terminal_row_counts")?;
    populate_durable_projection(
        &mut report,
        layout,
        &spec.view_id,
        &request.request_id,
        spec.l1_event_kind,
        spec.requested_level,
        true,
    )?;
    Ok(report)
}

pub(crate) fn run(args: StoreImportArgs) -> StoreExecutionOutcome {
    if args.from_artifact.is_some() {
        return super::from_artifact::run(args);
    }
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
    match execute_import(&args, &request_id, &idempotency_key) {
        Ok(report) => StoreExecutionOutcome::success(report, format),
        Err(message) => {
            let report = base_report(
                &args,
                &request_id,
                &idempotency_key,
                StoreRequestState::Failed,
            )
            .with_failure(classify_failure(&message), message);
            StoreExecutionOutcome::failure(report, format)
        }
    }
}

fn execute_import(
    args: &StoreImportArgs,
    request_id: &str,
    idempotency_key: &str,
) -> Result<StoreReport, String> {
    let observe_started = now_millis();
    let existing_store = args.store.join("CURRENT").exists();
    let existing = if existing_store {
        let layout = StoreLayout::open(&args.store).map_err(|error| error.to_string())?;
        let holder = LeaseHolder::new(
            format!("cli-{}", std::process::id()),
            env!("CARGO_PKG_VERSION"),
            std::process::id(),
        );
        let coordinator = StoreCoordinator::open_with_runtime(
            &layout,
            holder,
            std::sync::Arc::new(ImportClock),
            std::sync::Arc::new(ImportPidLiveness),
        )
        .map_err(|error| error.to_string())?;
        coordinator
            .request_by_idempotency_key(idempotency_key)
            .map_err(|error| error.to_string())?
            .map(|request| (layout, coordinator, request))
    } else {
        None
    };

    let requested_level = match args.level {
        StoreLevelArg::L1 => RequestedLevel::L1,
        StoreLevelArg::Full => RequestedLevel::Full,
    };
    let controls = ImportScanControls {
        jobs: args.scan.jobs,
        ignore_files: canonical_control_paths(&args.scan.ignore_files)?,
        spool_dir: args
            .scan
            .spool_dir
            .as_ref()
            .map(|path| absolute_runtime_path(path))
            .transpose()?,
        progress_file: args
            .scan
            .progress_file
            .as_ref()
            .map(|path| absolute_runtime_path(path))
            .transpose()?,
        l1_chunk_versions: 1,
        deep_chunk_versions: 1,
    };

    let (layout, mut coordinator, canonical_request) =
        if let Some((layout, coordinator, existing)) = existing {
            let trusted_family = trusted_store_family(&layout)?;
            let validator =
                StoreRequestExecutor::new(layout.store_db().to_path_buf(), trusted_family, None);
            let existing_payload = validator.validate_payload_json(&existing.payload_json)?;
            let controls_match = existing_payload
                .controls
                .matches_runtime_controls(&controls);
            if existing.kind != RequestKind::Import
                || existing_payload.schema_version != 1
                || existing_payload.family_id != args.family
                || !root_scope_matches(&args.root, &existing_payload.root)
                || existing_payload.view_id != args.view
                || existing_payload.requested_level != requested_level
                || !controls_match
            {
                return Err("idempotency_conflict".to_string());
            }
            (layout, coordinator, existing)
        } else {
            let (l1_chunk_versions, deep_chunk_versions) =
                frozen_chunk_versions_from_environment()?;
            let controls = ImportScanControls {
                l1_chunk_versions,
                deep_chunk_versions,
                ..controls
            };
            let root = args
                .root
                .canonicalize()
                .map_err(|error| error.to_string())?;
            let root_text = root.to_string_lossy().into_owned();
            let exclusions = crate::discovery::DiscoveryExclusions {
                progress_path: args.scan.progress_file.clone(),
                spool_dir: args.scan.spool_dir.clone(),
            };
            let discovery = crate::discovery::DiscoveryPolicy::build_excluding(
                &root,
                &args.store.join("gen-001").join("store.db"),
                exclusions,
                &args.scan.ignore_files,
            )
            .map_err(|error| format!("{error:?}"))?
            .discover_with_progress(None);
            if let Some(error) = discovery.errors.first() {
                return Err(error.message.clone());
            }
            let mut files = discovery
                .supported_files
                .into_iter()
                .map(|target| {
                    let (content_hash, content_bytes) =
                        crate::extraction::read_source_identity(&target)
                            .map_err(|error| error.message)?;
                    Ok(PlannedImportFile {
                        root_relative_path: target.root_relative_path,
                        content_hash,
                        content_bytes,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            files.sort_by(|left, right| left.root_relative_path.cmp(&right.root_relative_path));
            let source_bytes = files
                .iter()
                .map(|file| file.content_bytes)
                .fold(0_u64, u64::saturating_add);
            preflight_store_capacity(&args.store, source_bytes)?;
            let layout = StoreLayout::create(&args.store, &args.family, env!("CARGO_PKG_VERSION"))
                .map_err(|error| error.to_string())?;
            let progress = args
                .scan
                .progress_file
                .as_deref()
                .map(|path| {
                    crate::progress::ScanProgress::create_for_artifact(path, layout.store_db())
                })
                .transpose()
                .map_err(|error| format!("{error:?}"))?
                .map(Arc::new);
            if let Some(progress) = progress.as_deref() {
                progress.enter_phase("discovery");
            }
            let planned_payload = ImportRequestPayload {
                schema_version: 1,
                family_id: args.family.clone(),
                root: root_text.clone(),
                view_id: args.view.clone(),
                requested_level,
                files,
                controls,
            };
            let payload =
                serde_json::to_string(&planned_payload).expect("import payload is serializable");
            StoreRequestExecutor::new(layout.store_db().to_path_buf(), args.family.clone(), None)
                .validate_payload_json(&payload)?;
            let now = now_millis();
            let deadline_delta = i64::try_from(args.request.request_timeout_seconds)
                .unwrap_or(i64::MAX)
                .saturating_mul(1_000);
            let request = CoordinatorRequest::new(
                request_id,
                idempotency_key,
                RequestKind::Import,
                payload,
                format!("cli-{}", std::process::id()),
                now.saturating_add(deadline_delta),
                now,
            );
            let holder = LeaseHolder::new(
                format!("cli-{}", std::process::id()),
                env!("CARGO_PKG_VERSION"),
                std::process::id(),
            );
            let mut coordinator = StoreCoordinator::open_with_runtime(
                &layout,
                holder,
                std::sync::Arc::new(ImportClock),
                std::sync::Arc::new(ImportPidLiveness),
            )
            .map_err(|error| error.to_string())?;
            let canonical_request = coordinator
                .enqueue(request)
                .map_err(|error| error.to_string())?
                .request;
            (layout, coordinator, canonical_request)
        };
    let canonical_request_id = canonical_request.request_id.clone();
    let watchdog = args
        .scan
        .parent_pid
        .map(crate::watchdog::ParentWatchdog::start);
    let mut executor = StoreRequestExecutor::new(
        layout.store_db().to_path_buf(),
        args.family.clone(),
        watchdog,
    );
    let policy = CoordinatorPolicy {
        own_request_id: Some(canonical_request_id.clone()),
        ..CoordinatorPolicy::default()
    };
    // Re-attempt the drain for as long as the lease is held elsewhere. Waiting
    // passively for another process to finish burned the whole request budget
    // whenever the lease was released without our request being executed — the
    // crash-resume case. A retry inherits the ORIGINAL requester deadline, so a
    // resumed request gets what remains of the crashed process's window.
    loop {
        match coordinator.drain(&mut executor, &policy) {
            Ok(_) => break,
            Err(CoordinatorError::LeaseUnavailable) => {}
            Err(error) => return Err(error.to_string()),
        }
        let observed = coordinator
            .request(&canonical_request_id)
            .map_err(|error| error.to_string())?;
        if matches!(
            observed.state,
            RequestState::Committed | RequestState::Acknowledged | RequestState::Failed
        ) {
            break;
        }
        if now_millis()
            >= canonical_request
                .requester_deadline
                .unwrap_or(observe_started)
        {
            let canonical_payload: ImportRequestPayload =
                serde_json::from_str(&observed.payload_json)
                    .map_err(|_| "invalid_import_request".to_string())?;
            let state = match observed.state {
                RequestState::Queued => StoreRequestState::Queued,
                RequestState::Claimed => StoreRequestState::Claimed,
                _ => StoreRequestState::Queued,
            };
            let mut report = StoreReport::new(
                &observed.request_id,
                &canonical_payload.family_id,
                &canonical_payload.view_id,
                state,
            )
            .with_idempotency_key(&observed.idempotency_key)
            .with_root(&canonical_payload.root)
            .with_requested_level(match canonical_payload.requested_level {
                RequestedLevel::L1 => StoreRequestedLevel::L1,
                RequestedLevel::Full => StoreRequestedLevel::Full,
            })
            .with_failure(StoreFailureClass::RequestTimeout, "request_timeout");
            report.state = state;
            report.coordinator = match state {
                StoreRequestState::Claimed => StoreCoordinatorDisposition::Claimed,
                _ => StoreCoordinatorDisposition::Queued,
            };
            populate_durable_projection(
                &mut report,
                &layout,
                &canonical_payload.view_id,
                &observed.request_id,
                "store_import_l1_published",
                canonical_payload.requested_level,
                false,
            )?;
            populate_current_resolution(&layout, &canonical_payload.view_id, &mut report)?;
            return Ok(report);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let request = coordinator
        .request(&canonical_request_id)
        .map_err(|error| error.to_string())?;
    let canonical_payload: ImportRequestPayload = serde_json::from_str(&request.payload_json)
        .map_err(|_| "invalid_import_request".to_string())?;
    let committed = matches!(
        request.state,
        RequestState::Committed | RequestState::Acknowledged
    );
    let mut report = StoreReport::new(
        &request.request_id,
        &canonical_payload.family_id,
        &canonical_payload.view_id,
        if committed {
            StoreRequestState::Committed
        } else {
            StoreRequestState::Failed
        },
    )
    .with_idempotency_key(&request.idempotency_key)
    .with_root(&canonical_payload.root)
    .with_requested_level(match canonical_payload.requested_level {
        RequestedLevel::L1 => StoreRequestedLevel::L1,
        RequestedLevel::Full => StoreRequestedLevel::Full,
    });
    if !committed {
        let message = request
            .error_json
            .unwrap_or_else(|| "store_import_failed".to_string());
        populate_durable_projection(
            &mut report,
            &layout,
            &canonical_payload.view_id,
            &request.request_id,
            "store_import_l1_published",
            canonical_payload.requested_level,
            false,
        )?;
        populate_current_resolution(&layout, &canonical_payload.view_id, &mut report)?;
        return Ok(report.with_failure(classify_failure(&message), message));
    }
    let result: serde_json::Value = serde_json::from_str(
        request
            .result_json
            .as_deref()
            .ok_or("missing_import_result")?,
    )
    .map_err(|error| error.to_string())?;
    report.coordinator = StoreCoordinatorDisposition::Committed;
    report.manifest.generation = result["manifest_generation"].as_u64();
    report.manifest.hash = result["manifest_hash"].as_str().map(ToOwned::to_owned);
    report.manifest.disposition = match result["manifest_disposition"].as_str() {
        Some("created") => StoreManifestDisposition::Created,
        Some("reused") => StoreManifestDisposition::Reused,
        _ => StoreManifestDisposition::NotPublished,
    };
    report.completion = StoreLevelCompletion {
        l1: result["l1"].as_bool().unwrap_or(false),
        l2: result["l2"].as_bool().unwrap_or(false),
        l3: result["l3"].as_bool().unwrap_or(false),
    };
    report.row_counts = result
        .get("row_counts")
        .and_then(|counts| {
            Some(StoreRowCounts {
                file_versions: counts.get("file_versions")?.as_u64()?,
                l1: counts.get("l1")?.as_u64()?,
                l2: counts.get("l2")?.as_u64()?,
                l3: counts.get("l3")?.as_u64()?,
            })
        })
        .ok_or("invalid_terminal_row_counts")?;
    populate_durable_projection(
        &mut report,
        &layout,
        &canonical_payload.view_id,
        &request.request_id,
        "store_import_l1_published",
        canonical_payload.requested_level,
        true,
    )?;
    populate_current_resolution(&layout, &canonical_payload.view_id, &mut report)?;
    Ok(report)
}

pub(crate) fn populate_durable_projection(
    report: &mut StoreReport,
    layout: &StoreLayout,
    view_id: &str,
    request_id: &str,
    l1_event_kind: &str,
    requested_level: RequestedLevel,
    preserve_terminal_snapshot: bool,
) -> Result<(), String> {
    let connection =
        rusqlite::Connection::open(layout.store_db()).map_err(|error| error.to_string())?;
    if report.manifest.generation.is_none() {
        let progress_payload = connection
            .query_row(
                "SELECT payload_json FROM store_log
                 WHERE request_id = ?1 AND event_kind = ?2
                 ORDER BY sequence DESC LIMIT 1",
                rusqlite::params![request_id, l1_event_kind],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some(progress_payload) = progress_payload {
            let progress: serde_json::Value =
                serde_json::from_str(&progress_payload).map_err(|error| error.to_string())?;
            report.manifest.generation = progress["generation"].as_u64();
            report.manifest.hash = progress["manifest_hash"].as_str().map(ToOwned::to_owned);
            report.manifest.disposition = match progress["manifest_disposition"].as_str() {
                Some("created") => StoreManifestDisposition::Created,
                Some("reused") => StoreManifestDisposition::Reused,
                _ => StoreManifestDisposition::NotPublished,
            };
        }
    }
    let Some(report_generation) = report.manifest.generation else {
        return Ok(());
    };
    let report_generation =
        i64::try_from(report_generation).map_err(|_| "invalid_manifest_generation")?;
    let manifest = connection
        .query_row(
            "SELECT generation, manifest_hash, request_id
             FROM manifests WHERE view_id = ?1 AND generation = ?2",
            rusqlite::params![view_id, report_generation],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some((generation, hash, publisher)) = manifest {
        report.manifest.generation =
            Some(u64::try_from(generation).map_err(|_| "invalid_manifest_generation")?);
        report.manifest.hash = Some(hash);
        if report.manifest.disposition == StoreManifestDisposition::NotPublished {
            report.manifest.disposition = if publisher == request_id {
                StoreManifestDisposition::Created
            } else {
                StoreManifestDisposition::Reused
            };
        }
        if !preserve_terminal_snapshot {
            report.completion.l1 = true;
        }
        if !preserve_terminal_snapshot && requested_level == RequestedLevel::Full {
            let incomplete: (i64, i64) = connection
                .query_row(
                    "SELECT
                       COUNT(*) FILTER (WHERE me.version_id IS NOT NULL AND fv.complete_l2 IS NULL),
                       COUNT(*) FILTER (WHERE me.version_id IS NOT NULL AND fv.complete_l3 IS NULL)
                     FROM manifest_entries me
                     LEFT JOIN file_versions fv ON fv.version_id = me.version_id
                     WHERE me.view_id = ?1 AND me.generation = ?2",
                    rusqlite::params![view_id, report_generation],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|error| error.to_string())?;
            report.completion.l2 = incomplete.0 == 0;
            report.completion.l3 = incomplete.1 == 0;
        }
    }
    if preserve_terminal_snapshot {
        return Ok(());
    }
    let counts: (i64, i64, i64, i64) = connection
        .query_row(
            "WITH current_versions AS (
               SELECT DISTINCT me.version_id
               FROM manifest_entries me
               WHERE me.view_id = ?1 AND me.generation = ?2 AND me.version_id IS NOT NULL
             )
             SELECT
               (SELECT COUNT(*) FROM current_versions),
               (SELECT COUNT(*) FROM symbols WHERE version_id IN current_versions)
                 + (SELECT COUNT(*) FROM symbol_annotations WHERE version_id IN current_versions)
                 + (SELECT COUNT(*) FROM reference_sites WHERE level = 1 AND version_id IN current_versions)
                 + (SELECT COUNT(*) FROM relationships WHERE version_id IN current_versions)
                 + (SELECT COUNT(*) FROM pending_relationships WHERE version_id IN current_versions)
                 + (SELECT COUNT(*) FROM type_facts WHERE version_id IN current_versions)
                 + (SELECT COUNT(*) FROM complexity_metrics WHERE version_id IN current_versions)
                 + (SELECT COUNT(*) FROM parse_diagnostics WHERE version_id IN current_versions),
               (SELECT COUNT(*) FROM identifiers WHERE version_id IN current_versions)
                 + (SELECT COUNT(*) FROM reference_sites WHERE level = 2 AND version_id IN current_versions),
               (SELECT COUNT(*) FROM type_arguments WHERE version_id IN current_versions)
                 + (SELECT COUNT(*) FROM type_argument_usages WHERE version_id IN current_versions)
                 + (SELECT COUNT(*) FROM literals WHERE version_id IN current_versions)
                 + (SELECT COUNT(*) FROM source_regions WHERE version_id IN current_versions)
                 + (SELECT COUNT(*) FROM structural_facts WHERE version_id IN current_versions)",
            rusqlite::params![view_id, report_generation],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|error| error.to_string())?;
    report.row_counts = StoreRowCounts {
        file_versions: u64::try_from(counts.0).map_err(|_| "invalid_row_count")?,
        l1: u64::try_from(counts.1).map_err(|_| "invalid_row_count")?,
        l2: u64::try_from(counts.2).map_err(|_| "invalid_row_count")?,
        l3: u64::try_from(counts.3).map_err(|_| "invalid_row_count")?,
    };
    Ok(())
}

fn populate_current_resolution(
    layout: &StoreLayout,
    view_id: &str,
    report: &mut StoreReport,
) -> Result<(), String> {
    let connection = Connection::open(layout.store_db()).map_err(|error| error.to_string())?;
    let row = connection
        .query_row(
            "SELECT resolution_state,resolution_base_id,resolution_delta_generation,
                    resolution_exact_at,current_generation
             FROM views WHERE view_id=?1",
            [view_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((state, base_id, delta_generation, exact_at, current_generation)) = row else {
        return Ok(());
    };
    report.resolution.state = match state.as_str() {
        "converging" => StoreResolutionState::Converging,
        "exact" => StoreResolutionState::Exact,
        _ => StoreResolutionState::Unbound,
    };
    report.resolution.base_id = base_id;
    report.resolution.delta_generation =
        delta_generation.and_then(|value| u64::try_from(value).ok());
    report.resolution.exact_at_generation = exact_at.and_then(|value| u64::try_from(value).ok());
    report.resolution.exact_at_matches = exact_at.is_some() && exact_at == current_generation;
    if let Some(delta_generation) = delta_generation {
        let (gap_rows, gap_files) = connection
            .query_row(
                "SELECT exact_gap_rows,exact_gap_files FROM resolution_deltas
                 WHERE view_id=?1 AND delta_generation=?2",
                params![view_id, delta_generation],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|error| error.to_string())?;
        if report.resolution.state == StoreResolutionState::Exact {
            report.resolution.exact_gap_rows = u64::try_from(gap_rows).ok();
            report.resolution.exact_gap_files = u64::try_from(gap_files).ok();
        } else {
            report.resolution.gap_lower_bound = u64::try_from(gap_rows).ok();
        }
    }
    Ok(())
}

pub(crate) fn root_scope_matches(requested: &std::path::Path, stored: &str) -> bool {
    if requested == std::path::Path::new(stored) {
        return true;
    }
    if let Ok(canonical) = requested.canonicalize() {
        return canonical == std::path::Path::new(stored);
    }
    let Some(name) = requested.file_name() else {
        return false;
    };
    requested
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .is_some_and(|parent| parent.join(name) == std::path::Path::new(stored))
}

pub(crate) fn absolute_runtime_path(path: &std::path::Path) -> Result<String, String> {
    let absolute = std::path::absolute(path).map_err(|error| error.to_string())?;
    let mut existing = absolute.as_path();
    let mut missing = Vec::new();
    while !existing.exists() {
        let leaf = existing
            .file_name()
            .ok_or_else(|| "path_has_no_existing_ancestor".to_string())?;
        missing.push(leaf.to_os_string());
        existing = existing
            .parent()
            .ok_or_else(|| "path_has_no_existing_ancestor".to_string())?;
    }
    let mut canonical = existing.canonicalize().map_err(|error| error.to_string())?;
    for leaf in missing.into_iter().rev() {
        canonical.push(leaf);
    }
    canonical
        .to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| "path_is_not_utf8".to_string())
}

pub(crate) fn canonical_control_paths(paths: &[std::path::PathBuf]) -> Result<Vec<String>, String> {
    let mut paths = paths
        .iter()
        .map(|path| absolute_runtime_path(path))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

pub(crate) fn trusted_store_family(layout: &StoreLayout) -> Result<String, String> {
    rusqlite::Connection::open(layout.store_db())
        .map_err(|error| error.to_string())?
        .query_row(
            "SELECT value FROM store_meta WHERE key = 'family_id'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
}

pub(crate) fn classify_failure(message: &str) -> StoreFailureClass {
    if message.contains("capacity_insufficient") {
        StoreFailureClass::CapacityInsufficient
    } else if message.contains("store_not_found") {
        StoreFailureClass::StoreNotFound
    } else if message.contains("view_not_found") || message.contains("ViewNotFound") {
        StoreFailureClass::ViewNotFound
    } else if message.contains("family_mismatch") || message.contains("FamilyMismatch") {
        StoreFailureClass::FamilyMismatch
    } else if message.contains("invalid_file_path")
        || message.contains("file_must_be_root_relative")
        || message.contains("path_is_not_utf8")
    {
        StoreFailureClass::InvalidPath
    } else if message.contains("idempotency") || message.contains("IdempotencyConflict") {
        StoreFailureClass::IdempotencyConflict
    } else if message.contains("l1_projection_mismatch") {
        StoreFailureClass::L1ProjectionMismatch
    } else if message.contains("changed_between_waves") {
        StoreFailureClass::ChangedBetweenWaves
    } else if message.contains("root")
        && (message.contains("mismatch") || message.contains("does not match"))
    {
        StoreFailureClass::ViewRootMismatch
    } else if message.contains("request_timeout") {
        StoreFailureClass::RequestTimeout
    } else if message.contains("resolution_input_incomplete") {
        StoreFailureClass::ResolutionInputIncomplete
    } else if message.contains("resolution_not_exact") {
        StoreFailureClass::ResolutionNotExact
    } else if message.contains("output_identity_mismatch") {
        StoreFailureClass::OutputIdentityMismatch
    } else if message.contains("resolution_failed") {
        StoreFailureClass::ResolutionFailed
    } else if message.contains("lease") || message.contains("busy:") {
        StoreFailureClass::Busy
    } else {
        StoreFailureClass::Internal
    }
}

fn base_report(
    args: &StoreImportArgs,
    request_id: &str,
    idempotency_key: &str,
    state: StoreRequestState,
) -> StoreReport {
    StoreReport::new(request_id, &args.family, &args.view, state)
        .with_idempotency_key(idempotency_key)
        .with_root(args.root.to_string_lossy())
        .with_requested_level(match args.level {
            StoreLevelArg::L1 => StoreRequestedLevel::L1,
            StoreLevelArg::Full => StoreRequestedLevel::Full,
        })
}

pub(crate) fn mint_request_id() -> String {
    format!("request-{}-{}", std::process::id(), now_millis())
}

pub(crate) fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}

const STORE_CAPACITY_HEADROOM_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) fn required_store_capacity_bytes(source_bytes: u64) -> u64 {
    source_bytes
        .saturating_mul(2)
        .saturating_add(super::executor::estimate_projected_wal_bytes(source_bytes))
        .saturating_add(STORE_CAPACITY_HEADROOM_BYTES)
}

pub(crate) fn ensure_store_capacity(
    available_bytes: u64,
    required_bytes: u64,
) -> Result<(), String> {
    if available_bytes >= required_bytes {
        Ok(())
    } else {
        Err(format!(
            "capacity_insufficient:required_bytes={required_bytes}:available_bytes={available_bytes}"
        ))
    }
}

pub(crate) fn preflight_store_capacity(path: &Path, source_bytes: u64) -> Result<(), String> {
    let mut probe = path;
    while !probe.exists() {
        probe = probe
            .parent()
            .ok_or_else(|| "capacity_probe_has_no_existing_ancestor".to_string())?;
    }
    let available = super::maintenance::filesystem_free_bytes(probe)
        .map_err(|error| format!("capacity_probe_failed:{error}"))?;
    ensure_store_capacity(available, required_store_capacity_bytes(source_bytes))
}

#[cfg(test)]
mod capacity_tests {
    use super::{ensure_store_capacity, preflight_store_capacity, required_store_capacity_bytes};

    #[test]
    fn capacity_preflight_rejects_a_peak_that_does_not_fit() {
        let required = required_store_capacity_bytes(8 * 1024 * 1024);

        let error = ensure_store_capacity(required - 1, required).unwrap_err();

        assert!(error.starts_with("capacity_insufficient:"));
    }

    #[test]
    fn capacity_preflight_accepts_the_exact_peak() {
        let required = required_store_capacity_bytes(8 * 1024 * 1024);

        ensure_store_capacity(required, required).unwrap();
    }

    #[test]
    fn capacity_preflight_checks_the_filesystem_without_creating_the_store() {
        let temp = tempfile::tempdir().unwrap();
        let store = temp.path().join("missing").join("new-store");

        let error = preflight_store_capacity(&store, u64::MAX).unwrap_err();

        assert!(error.starts_with("capacity_insufficient:"));
        assert!(!store.exists());
    }

    #[test]
    fn capacity_probe_reads_a_real_temporary_directory() {
        let temp = tempfile::tempdir().unwrap();

        let available = super::super::maintenance::filesystem_free_bytes(temp.path()).unwrap();

        assert!(available > 0);
    }
}

pub(crate) fn drain_when_available(
    coordinator: &mut StoreCoordinator,
    executor: &mut StoreRequestExecutor,
    request: &CoordinatorRequest,
) -> Result<(), String> {
    let policy = CoordinatorPolicy {
        own_request_id: Some(request.request_id.clone()),
        ..CoordinatorPolicy::default()
    };
    loop {
        match coordinator.drain(executor, &policy) {
            Ok(_) => return Ok(()),
            Err(CoordinatorError::LeaseUnavailable) => {
                if now_millis() >= request.requester_deadline.unwrap_or(i64::MAX) {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ImportClock;

impl julie_extract_artifact::store::UnixMillisClock for ImportClock {
    fn now_ms(&self) -> i64 {
        now_millis()
    }
}

#[derive(Debug)]
pub(crate) struct ImportPidLiveness;

impl julie_extract_artifact::store::PidLiveness for ImportPidLiveness {
    fn status(&self, pid: u32) -> julie_extract_artifact::store::PidStatus {
        crate::watchdog::process_status(pid)
    }
}
