use std::sync::Arc;

use julie_extract_artifact::store::{
    CoordinatorRequest, LeaseHolder, RequestKind, StoreCoordinator,
};

use super::args::StoreDeleteArgs;
use super::executor::{DeleteRequestPayload, RequestedLevel, StoreRequestExecutor};
use super::import::{
    ImportClock, ImportPidLiveness, RequestReportSpec, StoreExecutionOutcome, classify_failure,
    drain_when_available, mint_request_id, normalize_root_relative, now_millis,
    open_existing_store, report_request, require_existing_view, root_scope_matches,
};
use super::report::{
    StoreOperation, StoreOutputFormat, StoreReport, StoreRequestState, StoreRequestedLevel,
};

pub(crate) fn run(args: StoreDeleteArgs) -> StoreExecutionOutcome {
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
    match execute_delete(&args, &request_id, &idempotency_key, &mut failure_family_id) {
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

fn execute_delete(
    args: &StoreDeleteArgs,
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
    let mut files = args
        .files
        .iter()
        .map(|path| normalize_root_relative(path))
        .collect::<Result<Vec<_>, _>>()?;
    files.sort();
    files.dedup();
    let canonical_request = if let Some(existing) = existing_request {
        if existing.kind != RequestKind::Delete {
            return Err("idempotency_conflict".to_string());
        }
        let validator =
            StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id.clone(), None);
        let payload = validator.validate_delete_payload_json(&existing.payload_json)?;
        if payload.family_id != family_id
            || !root_scope_matches(&args.root, &payload.root)
            || payload.view_id != args.view
            || payload.files != files
        {
            return Err("idempotency_conflict".to_string());
        }
        existing
    } else {
        let root_text = require_existing_view(&layout, &args.root, &args.view)?;
        let payload = DeleteRequestPayload {
            schema_version: 1,
            family_id: family_id.clone(),
            root: root_text,
            view_id: args.view.clone(),
            files,
        };
        let payload_json = serde_json::to_string(&payload).expect("delete payload is serializable");
        StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id.clone(), None)
            .validate_delete_payload_json(&payload_json)?;
        let now = now_millis();
        let deadline_delta = i64::try_from(args.request.request_timeout_seconds)
            .unwrap_or(i64::MAX)
            .saturating_mul(1_000);
        coordinator
            .enqueue(CoordinatorRequest::new(
                request_id,
                idempotency_key,
                RequestKind::Delete,
                payload_json,
                format!("cli-{}", std::process::id()),
                now.saturating_add(deadline_delta),
                now,
            ))
            .map_err(|error| error.to_string())?
            .request
    };
    let canonical_request_id = canonical_request.request_id.clone();
    let mut executor =
        StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id.clone(), None);
    let warnings = drain_when_available(&mut coordinator, &mut executor, &canonical_request)?;
    let request = coordinator
        .request(&canonical_request_id)
        .map_err(|error| error.to_string())?;
    let payload = StoreRequestExecutor::new(layout.store_db().to_path_buf(), family_id, None)
        .validate_delete_payload_json(&request.payload_json)?;
    Ok(report_request(
        &layout,
        &request,
        RequestReportSpec {
            operation: StoreOperation::Delete,
            family_id: payload.family_id,
            view_id: payload.view_id,
            root: payload.root,
            requested_level: RequestedLevel::L1,
            l1_event_kind: "store_delete_l1_published",
        },
    )?
    .with_warnings(warnings))
}

fn base_report(
    args: &StoreDeleteArgs,
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
    .with_operation(StoreOperation::Delete)
    .with_idempotency_key(idempotency_key)
    .with_root(args.root.to_string_lossy())
    .with_requested_level(StoreRequestedLevel::L1)
}
