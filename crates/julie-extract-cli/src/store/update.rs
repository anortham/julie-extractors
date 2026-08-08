use std::sync::Arc;

use julie_extract_artifact::store::{
    CoordinatorRequest, LeaseHolder, RequestKind, StoreCoordinator,
};

use super::args::{StoreLevelArg, StoreUpdateArgs};
use super::executor::{
    ImportScanControls, PlannedImportFile, RequestedLevel, StoreRequestExecutor,
    UpdateRequestPayload, frozen_chunk_versions_from_environment, validate_target_within_root,
};
use super::import::{
    ImportClock, ImportPidLiveness, RequestReportSpec, StoreExecutionOutcome,
    absolute_runtime_path, canonical_control_paths, classify_failure, drain_when_available,
    mint_request_id, normalize_root_relative, now_millis, open_existing_store, report_request,
    require_existing_view, root_scope_matches,
};
use super::report::{
    StoreOperation, StoreOutputFormat, StoreReport, StoreRequestState, StoreRequestedLevel,
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
        if existing.kind != RequestKind::Update {
            return Err("idempotency_conflict".to_string());
        }
        let validator =
            StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id.clone(), None);
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
    } else {
        let (l1_chunk_versions, deep_chunk_versions) = frozen_chunk_versions_from_environment()?;
        let controls = ImportScanControls {
            l1_chunk_versions,
            deep_chunk_versions,
            ..controls
        };
        let root_text = require_existing_view(&layout, &args.root, &args.view)?;
        let root = std::path::Path::new(&root_text);
        let target = crate::paths::FileTarget {
            absolute_path: root.join(&root_relative_path),
            root_relative_path: root_relative_path.clone(),
        };
        validate_target_within_root(root, &root_relative_path)?;
        let (content_hash, content_bytes) =
            crate::extraction::read_source_identity(&target).map_err(|error| error.message)?;
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
        let payload_json = serde_json::to_string(&payload).expect("update payload is serializable");
        StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id.clone(), None)
            .validate_update_payload_json(&payload_json)?;
        let now = now_millis();
        let deadline_delta = i64::try_from(args.request.request_timeout_seconds)
            .unwrap_or(i64::MAX)
            .saturating_mul(1_000);
        coordinator
            .enqueue(CoordinatorRequest::new(
                request_id,
                idempotency_key,
                RequestKind::Update,
                payload_json,
                format!("cli-{}", std::process::id()),
                now.saturating_add(deadline_delta),
                now,
            ))
            .map_err(|error| error.to_string())?
            .request
    };
    let canonical_request_id = canonical_request.request_id.clone();
    let watchdog = args
        .scan
        .parent_pid
        .map(crate::watchdog::ParentWatchdog::start);
    let mut executor =
        StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id.clone(), watchdog);
    drain_when_available(&mut coordinator, &mut executor, &canonical_request)?;
    let request = coordinator
        .request(&canonical_request_id)
        .map_err(|error| error.to_string())?;
    let payload = StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id, None)
        .validate_update_payload_json(&request.payload_json)?;
    report_request(
        &layout,
        &request,
        RequestReportSpec {
            operation: StoreOperation::Update,
            family_id: payload.family_id,
            view_id: payload.view_id,
            root: payload.root,
            requested_level: payload.requested_level,
            l1_event_kind: "store_update_l1_published",
        },
    )
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
