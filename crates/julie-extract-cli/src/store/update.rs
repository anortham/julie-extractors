use std::path::{Path, PathBuf};
use std::sync::Arc;

use julie_extract_artifact::store::{
    CoordinatorRequest, LeaseHolder, RequestKind, StoreCoordinator,
};

use crate::discovery::{DiscoveryExclusions, DiscoveryPolicy, FileSelection, UnsupportedReason};
use crate::paths::FileTarget;

use super::args::{StoreLevelArg, StoreUpdateArgs};
use super::executor::{
    DeleteRequestPayload, ImportScanControls, PlannedImportFile, RequestedLevel,
    StoreRequestExecutor, UpdateRequestPayload, frozen_chunk_versions_from_environment,
    validate_target_within_root,
};
use super::import::{
    ImportClock, ImportPidLiveness, RequestReportSpec, StoreExecutionOutcome,
    absolute_runtime_path, canonical_control_paths, classify_failure, drain_when_available,
    mint_request_id, normalize_root_relative, now_millis, open_existing_store,
    preflight_store_capacity, read_source_identity_or_missing, report_request,
    require_existing_view, root_scope_matches,
};
use super::report::{
    StoreOperation, StoreOutputFormat, StoreReport, StoreRequestState, StoreRequestedLevel,
    StoreUnsupportedReason, StoreUnsupportedReport,
};

pub(crate) fn run(args: StoreUpdateArgs) -> StoreExecutionOutcome {
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
    let mut failure_family_id = args.family.clone().unwrap_or_default();
    match execute_update(&args, &request_id, &idempotency_key, &mut failure_family_id) {
        Ok(report) => StoreExecutionOutcome::success(report, format),
        Err(message) => {
            let mut report = base_report(
                &args,
                &request_id,
                &idempotency_key,
                StoreRequestState::Failed,
            );
            report.family_id = failure_family_id;
            let report = report.with_failure(classify_failure(&message), message);
            StoreExecutionOutcome::failure(report, format)
        }
    }
}

fn execute_update(
    args: &StoreUpdateArgs,
    request_id: &str,
    idempotency_key: &str,
    failure_family_id: &mut String,
) -> Result<StoreReport, String> {
    let existing = open_existing_store(&args.store, args.family.as_deref())?;
    failure_family_id.clone_from(&existing.family_id);
    let layout = existing.layout;
    let family_id = existing.family_id;
    let holder = LeaseHolder::new(
        format!("cli-{}", std::process::id()),
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
    );
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        holder,
        Arc::new(ImportClock),
        Arc::new(ImportPidLiveness),
    )
    .map_err(|error| error.to_string())?;
    let existing_request = coordinator
        .request_by_idempotency_key(idempotency_key)
        .map_err(|error| error.to_string())?;
    let root_relative_path = normalize_root_relative(&args.file)?;
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
            .as_deref()
            .map(absolute_runtime_path)
            .transpose()?,
        progress_file: args
            .scan
            .progress_file
            .as_deref()
            .map(absolute_runtime_path)
            .transpose()?,
        l1_chunk_versions: 1,
        deep_chunk_versions: 1,
    };
    let canonical_request = if let Some(existing) = existing_request {
        let validator =
            StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id.clone(), None);
        match existing.kind {
            RequestKind::Update => {
                let payload = validator.validate_update_payload_json(&existing.payload_json)?;
                let controls_match = payload.controls.matches_runtime_controls(&controls);
                if payload.family_id != family_id
                    || !root_scope_matches(&args.root, &payload.root)
                    || payload.view_id != args.view
                    || payload.requested_level != requested_level
                    || payload.file.root_relative_path != root_relative_path
                    || !controls_match
                {
                    return Err("idempotency_conflict".to_string());
                }
                existing
            }
            // An update of a vanished file enqueues a delete under the
            // update's idempotency key, so a retry of that update must adopt
            // the delete row instead of reporting a conflict.
            RequestKind::Delete => {
                let payload = validator.validate_delete_payload_json(&existing.payload_json)?;
                if payload.family_id != family_id
                    || !root_scope_matches(&args.root, &payload.root)
                    || payload.view_id != args.view
                    || payload.files != [root_relative_path.clone()]
                {
                    return Err("idempotency_conflict".to_string());
                }
                existing
            }
            _ => return Err("idempotency_conflict".to_string()),
        }
    } else {
        let (l1_chunk_versions, deep_chunk_versions) = frozen_chunk_versions_from_environment()?;
        let controls = ImportScanControls {
            l1_chunk_versions,
            deep_chunk_versions,
            ..controls
        };
        let root_text = require_existing_view(&layout, &args.root, &args.view)?;
        let root = Path::new(&root_text);
        let target = FileTarget {
            absolute_path: root.join(&root_relative_path),
            root_relative_path: root_relative_path.clone(),
        };
        validate_target_within_root(root, &root_relative_path)?;
        if let Some(refusal) = discovery_refusal(root, layout.store_db(), &target, &controls)? {
            let mut report = base_report(
                args,
                request_id,
                idempotency_key,
                StoreRequestState::Unsupported,
            );
            report.family_id = family_id;
            report.root = root_text;
            return Ok(report.with_unsupported(refusal));
        }
        let identity = read_source_identity_or_missing(&target)?;
        let (request_kind, payload_json) = match identity {
            Some((content_hash, content_bytes)) => {
                preflight_store_capacity(&args.store, content_bytes)?;
                let payload = UpdateRequestPayload {
                    schema_version: 1,
                    family_id: family_id.clone(),
                    root: root_text,
                    view_id: args.view.clone(),
                    requested_level,
                    file: PlannedImportFile {
                        root_relative_path,
                        content_hash,
                        content_bytes,
                    },
                    controls,
                };
                let payload_json =
                    serde_json::to_string(&payload).expect("update payload is serializable");
                StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id.clone(), None)
                    .validate_update_payload_json(&payload_json)?;
                (RequestKind::Update, payload_json)
            }
            // The file vanished between the caller's delta enumeration and
            // this read. It genuinely does not exist, so the correct durable
            // outcome for this update is the same delete the caller would
            // have requested, not a failed request.
            None => {
                let payload = DeleteRequestPayload {
                    schema_version: 1,
                    family_id: family_id.clone(),
                    root: root_text,
                    view_id: args.view.clone(),
                    files: vec![root_relative_path],
                };
                let payload_json =
                    serde_json::to_string(&payload).expect("delete payload is serializable");
                StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id.clone(), None)
                    .validate_delete_payload_json(&payload_json)?;
                (RequestKind::Delete, payload_json)
            }
        };
        let now = now_millis();
        let deadline_delta = i64::try_from(args.request.request_timeout_seconds)
            .unwrap_or(i64::MAX)
            .saturating_mul(1_000);
        coordinator
            .enqueue(CoordinatorRequest::new(
                request_id,
                idempotency_key,
                request_kind,
                payload_json,
                format!("cli-{}", std::process::id()),
                now.saturating_add(deadline_delta),
                now,
            ))
            .map_err(|error| error.to_string())?
            .request
    };
    let adoption_now = now_millis();
    let adoption_deadline_delta = i64::try_from(args.request.request_timeout_seconds)
        .unwrap_or(i64::MAX)
        .saturating_mul(1_000);
    let canonical_request = coordinator
        .adopt_request(
            canonical_request,
            &format!("cli-{}", std::process::id()),
            adoption_now.saturating_add(adoption_deadline_delta),
            adoption_now,
        )
        .map_err(|error| error.to_string())?;
    let canonical_request_id = canonical_request.request_id.clone();
    let watchdog = args
        .scan
        .parent_pid
        .map(crate::watchdog::ParentWatchdog::start);
    let mut executor =
        StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id.clone(), watchdog);
    let warnings = drain_when_available(&mut coordinator, &mut executor, &canonical_request)?;
    let request = coordinator
        .request(&canonical_request_id)
        .map_err(|error| error.to_string())?;
    let validator = StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id, None);
    // The report keeps operation "update" either way: callers match the
    // report operation against the command they invoked.
    let spec = if request.kind == RequestKind::Delete {
        let payload = validator.validate_delete_payload_json(&request.payload_json)?;
        RequestReportSpec {
            operation: StoreOperation::Update,
            family_id: payload.family_id,
            view_id: payload.view_id,
            root: payload.root,
            requested_level,
            l1_event_kind: "store_delete_l1_published",
        }
    } else {
        let payload = validator.validate_update_payload_json(&request.payload_json)?;
        RequestReportSpec {
            operation: StoreOperation::Update,
            family_id: payload.family_id,
            view_id: payload.view_id,
            root: payload.root,
            requested_level: payload.requested_level,
            l1_event_kind: "store_update_l1_published",
        }
    };
    Ok(report_request(&layout, &request, spec)?.with_warnings(warnings))
}

/// The discovery decision `scan` applies, run before the update path reads,
/// hashes, or enqueues the file. A file `scan` refuses is refused here too, so
/// an oversized or hard-excluded path can never enter the request queue and
/// stall every later import that inherits it.
fn discovery_refusal(
    root: &Path,
    db_path: &Path,
    target: &FileTarget,
    controls: &ImportScanControls,
) -> Result<Option<StoreUnsupportedReport>, String> {
    let exclusions = DiscoveryExclusions {
        progress_path: controls.progress_file.as_deref().map(PathBuf::from),
        spool_dir: controls.spool_dir.as_deref().map(PathBuf::from),
    };
    let ignore_files: Vec<PathBuf> = controls.ignore_files.iter().map(PathBuf::from).collect();
    let policy = DiscoveryPolicy::build_excluding(root, db_path, exclusions, &ignore_files)
        .map_err(|error| format!("{error:?}"))?;
    let FileSelection::Unsupported { reason } = policy.select_file(target) else {
        return Ok(None);
    };
    Ok(Some(StoreUnsupportedReport {
        reason: match reason {
            UnsupportedReason::Ignored => StoreUnsupportedReason::Ignored,
            UnsupportedReason::HardExcluded => StoreUnsupportedReason::HardExcluded,
            UnsupportedReason::UnsupportedExtension => StoreUnsupportedReason::UnsupportedExtension,
            UnsupportedReason::Oversized => StoreUnsupportedReason::Oversized,
        },
        root_relative_path: target.root_relative_path.clone(),
        message: match reason {
            UnsupportedReason::Oversized => crate::limits::slow_file_skip_message(),
            _ => "file is ignored or unsupported; no store request was queued".to_string(),
        },
    }))
}

fn base_report(
    args: &StoreUpdateArgs,
    request_id: &str,
    idempotency_key: &str,
    state: StoreRequestState,
) -> StoreReport {
    StoreReport::new(
        request_id,
        args.family.as_deref().unwrap_or_default(),
        &args.view,
        state,
    )
    .with_operation(StoreOperation::Update)
    .with_idempotency_key(idempotency_key)
    .with_root(args.root.to_string_lossy())
    .with_requested_level(match args.level {
        StoreLevelArg::L1 => StoreRequestedLevel::L1,
        StoreLevelArg::Full => StoreRequestedLevel::Full,
    })
}
