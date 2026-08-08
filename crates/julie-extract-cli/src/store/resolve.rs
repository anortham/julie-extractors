use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use julie_extract_artifact::store::{
    CoordinatorRequest, LeaseDisposition, LeaseHolder, RequestKind, RequestState,
    ResolutionBaseBegin, ResolutionBaseCatalog, ResolutionBaseReader, ResolutionBaseRecovery,
    ResolutionBindingStore, ResolutionConvergenceBegin, ResolutionExactPublish, ResolutionGapFact,
    ResolutionPinOwnerKind, ResolutionPublicationFence, ResolutionScratchReader,
    ResolutionViewBinding, StoreConnectionFactory, StoreCoordinator, StoreLayout, StoreLog,
    StoreLogEntry, ViewResolutionState, stream_resolution_diff,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::args::StoreResolveArgs;
use super::import::{
    ImportClock, ImportPidLiveness, StoreExecutionOutcome, classify_failure, mint_request_id,
    now_millis, open_existing_store,
};
use super::report::{
    StoreCoordinatorDisposition, StoreOperation, StoreOutputFormat, StoreReport, StoreRequestState,
    StoreRequestedLevel, StoreResolutionState,
};
use super::resolution_session::{StoreManifestIdentity, StoreScratchResolutionSession};

const RESOLVE_CLAIM_STALE_MS: i64 = 5_000;
const RESOLUTION_WINDOW_SIZE: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ResolveRequestPayload {
    schema_version: i64,
    family_id: String,
    view_id: String,
    resolver_output_epoch: i64,
}

pub(crate) fn run(args: StoreResolveArgs) -> StoreExecutionOutcome {
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
    let mut failure_family = args.family.clone().unwrap_or_default();
    match execute_resolve(&args, &request_id, &idempotency_key, &mut failure_family) {
        Ok(report) => StoreExecutionOutcome::success(report, format),
        Err(message) => {
            let report = StoreReport::new(
                &request_id,
                failure_family,
                &args.view,
                StoreRequestState::Failed,
            )
            .with_operation(StoreOperation::Resolve)
            .with_requested_level(StoreRequestedLevel::NotApplicable)
            .with_idempotency_key(idempotency_key)
            .with_failure(classify_failure(&message), message);
            StoreExecutionOutcome::failure(report, format)
        }
    }
}

fn execute_resolve(
    args: &StoreResolveArgs,
    request_id: &str,
    idempotency_key: &str,
    failure_family: &mut String,
) -> Result<StoreReport, String> {
    let existing = open_existing_store(&args.store, args.family.as_deref())?;
    failure_family.clone_from(&existing.family_id);
    let layout = existing.layout;
    let family_id = existing.family_id;
    let holder = LeaseHolder::new(
        format!("cli-{}", std::process::id()),
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
    );
    let mut coordinator = StoreCoordinator::open_with_runtime(
        &layout,
        holder.clone(),
        Arc::new(ImportClock),
        Arc::new(ImportPidLiveness),
    )
    .map_err(|error| error.to_string())?;
    let payload = ResolveRequestPayload {
        schema_version: 1,
        family_id: family_id.clone(),
        view_id: args.view.clone(),
        resolver_output_epoch: crate::resolution::RESOLUTION_VERSION,
    };
    let canonical = if let Some(existing) = coordinator
        .request_by_idempotency_key(idempotency_key)
        .map_err(|error| error.to_string())?
    {
        if existing.kind != RequestKind::Resolve {
            return Err("idempotency_conflict".to_string());
        }
        let stored = parse_payload(&existing.payload_json)?;
        if stored != payload {
            return Err("idempotency_conflict".to_string());
        }
        existing
    } else {
        require_view_identity(&layout, &args.view, false)?;
        let now = now_millis();
        let deadline = now.saturating_add(
            i64::try_from(args.request.request_timeout_seconds)
                .unwrap_or(i64::MAX)
                .saturating_mul(1_000),
        );
        coordinator
            .enqueue(CoordinatorRequest::new(
                request_id,
                idempotency_key,
                RequestKind::Resolve,
                serde_json::to_string(&payload).expect("resolve payload is serializable"),
                holder.holder_id.clone(),
                deadline,
                now,
            ))
            .map_err(|error| error.to_string())?
            .request
    };
    if matches!(
        canonical.state,
        RequestState::Committed | RequestState::Acknowledged
    ) {
        normalize_dead_writer_lease(&layout, &mut coordinator, &holder)?;
        return report_resolve(&layout, &canonical, &payload);
    }
    let reconciliation = coordinator
        .reconcile(&canonical.request_id)
        .map_err(|error| error.to_string())?;
    if reconciliation.committed_in_fact {
        normalize_dead_writer_lease(&layout, &mut coordinator, &holder)?;
        let request = coordinator
            .request(&canonical.request_id)
            .map_err(|error| error.to_string())?;
        return report_resolve(&layout, &request, &payload);
    }
    if canonical.state == RequestState::Failed {
        return report_resolve(&layout, &canonical, &payload);
    }

    claim_until_deadline(&coordinator, &canonical, &holder.holder_id)?;
    let heartbeat = ResolveHeartbeat::start(
        layout.clone(),
        canonical.request_id.clone(),
        holder.holder_id.clone(),
    );
    #[cfg(feature = "test-store-resolution-contract")]
    pause_after_claim_for_test()?;
    let result = resolve_claimed(
        &layout,
        &family_id,
        &mut coordinator,
        &holder,
        &canonical,
        &payload,
        &heartbeat,
    );
    let heartbeat_current = heartbeat.stop();
    if !heartbeat_current {
        return Err("resolution_failed: resolve claim lost".to_string());
    }
    if let Err(message) = result {
        if coordinator
            .reconcile(&canonical.request_id)
            .map_err(|error| error.to_string())?
            .committed_in_fact
        {
            let request = coordinator
                .request(&canonical.request_id)
                .map_err(|error| error.to_string())?;
            return report_resolve(&layout, &request, &payload);
        }
        if !coordinator
            .fail_resolve(
                &canonical.request_id,
                &holder.holder_id,
                &message,
                now_millis(),
            )
            .map_err(|error| error.to_string())?
        {
            return Err("resolution_failed: resolve claim lost".to_string());
        }
        let request = coordinator
            .request(&canonical.request_id)
            .map_err(|error| error.to_string())?;
        return report_resolve(&layout, &request, &payload);
    }
    coordinator
        .commit_resolve(&canonical.request_id, &holder.holder_id)
        .map_err(|error| error.to_string())?;
    #[cfg(feature = "test-store-resolution-contract")]
    julie_extract_artifact::store::test_hooks::crash_if("resolution_coord_after_commit");
    let request = coordinator
        .request(&canonical.request_id)
        .map_err(|error| error.to_string())?;
    report_resolve(&layout, &request, &payload)
}

fn resolve_claimed(
    layout: &StoreLayout,
    family_id: &str,
    coordinator: &mut StoreCoordinator,
    holder: &LeaseHolder,
    request: &CoordinatorRequest,
    payload: &ResolveRequestPayload,
    heartbeat: &ResolveHeartbeat,
) -> Result<(), String> {
    heartbeat.ensure_current(coordinator, request, holder)?;
    let deadline = request.requester_deadline.unwrap_or(i64::MAX);
    let factory = StoreConnectionFactory::new(layout.clone(), family_id, env!("CARGO_PKG_VERSION"));
    let identity = require_view_identity(layout, &payload.view_id, true)?;
    let catalog = ResolutionBaseCatalog::new(factory.clone());
    let bindings = ResolutionBindingStore::new(factory.clone());
    let has_ready_base: bool = factory
        .open_reader()
        .map_err(|error| format!("resolution_failed: {error}"))?
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM resolution_bases
               WHERE state='ready' AND resolver_output_epoch=?1
             )",
            [payload.resolver_output_epoch],
            |row| row.get(0),
        )
        .map_err(|error| format!("resolution_failed: {error}"))?;
    if !has_ready_base {
        let created_at = store_timestamp(layout, "now")?;
        let begin = with_writer_lease(coordinator, holder, deadline, |_| {
            ensure_resolve_claim(layout, request, holder)?;
            catalog
                .begin_build(
                    &identity.manifest_hash,
                    payload.resolver_output_epoch,
                    &request.request_id,
                    &created_at,
                )
                .map_err(|error| error.to_string())
        })?;
        let build = match begin {
            ResolutionBaseBegin::Build(build) => Some(build),
            ResolutionBaseBegin::Ready(_) => None,
            ResolutionBaseBegin::Building(_) => {
                let recovery = with_writer_lease(coordinator, holder, deadline, |_| {
                    heartbeat.ensure_live()?;
                    ensure_resolve_claim(layout, request, holder)?;
                    catalog
                        .recover(
                            &identity.manifest_hash,
                            payload.resolver_output_epoch,
                            &request.request_id,
                            false,
                            &store_timestamp(layout, "now")?,
                        )
                        .map_err(|error| format!("resolution_failed: {error}"))
                })?;
                match recovery {
                    ResolutionBaseRecovery::Ready(_) => None,
                    ResolutionBaseRecovery::Rebuild(build) => Some(build),
                    ResolutionBaseRecovery::LiveOwner(_) => {
                        return Err(
                            "resolution_failed: a live base builder owns this identity".to_string()
                        );
                    }
                }
            }
        };
        if let Some(build) = build {
            heartbeat.ensure_current(coordinator, request, holder)?;
            let mut session = StoreScratchResolutionSession::new(
                factory.clone(),
                identity.clone(),
                &build.scratch_path,
                RESOLUTION_WINDOW_SIZE,
                payload.resolver_output_epoch,
            )
            .map_err(classify_resolution_error)?;
            crate::resolution::run_resolution_session(&mut session, true, true)
                .map_err(classify_resolution_error)?;
            session.finish_exact().map_err(classify_resolution_error)?;
            heartbeat.ensure_current(coordinator, request, holder)?;
            catalog
                .publish_scratch(&build)
                .map_err(|error| format!("resolution_failed: {error}"))?;
            heartbeat.ensure_current(coordinator, request, holder)?;
            with_writer_lease(coordinator, holder, deadline, |_| {
                heartbeat.ensure_live()?;
                ensure_resolve_claim(layout, request, holder)?;
                catalog
                    .mark_ready(&build, &store_timestamp(layout, "now")?)
                    .map(|_| ())
                    .map_err(|error| format!("resolution_failed: {error}"))
            })?;
        }
    }

    let pin_id = format!("resolve-{}-{}", request.request_id, std::process::id());
    let convergence = ResolutionConvergenceBegin {
        view_id: payload.view_id.clone(),
        resolver_output_epoch: payload.resolver_output_epoch,
        request_id: request.request_id.clone(),
        pin_id: pin_id.clone(),
        owner_id: holder.holder_id.clone(),
        expires_at: store_timestamp(layout, "+1 hour")?,
        created_at: store_timestamp(layout, "now")?,
    };
    heartbeat.ensure_current(coordinator, request, holder)?;
    let (binding, _) = with_writer_lease(coordinator, holder, deadline, |_| {
        heartbeat.ensure_live()?;
        ensure_resolve_claim(layout, request, holder)?;
        bindings
            .begin_convergence(&convergence)
            .map_err(|error| format!("resolution_failed: {error}"))
    })?;
    if binding.state == ViewResolutionState::Exact {
        heartbeat.ensure_current(coordinator, request, holder)?;
        with_writer_lease(coordinator, holder, deadline, |token| {
            heartbeat.ensure_live()?;
            ensure_resolve_claim(layout, request, holder)?;
            append_resolution_terminal(layout, request, &binding, None, None)?;
            bindings
                .release_pin(&pin_id, ResolutionPinOwnerKind::Resolve, &holder.holder_id)
                .map_err(|error| format!("resolution_failed: {error}"))?;
            let _ = token;
            Ok(())
        })?;
        return Ok(());
    }

    heartbeat.ensure_current(coordinator, request, holder)?;
    let exact_path = layout
        .scratch_dir()
        .join(format!("resolve-exact-{}.db", request.request_id));
    remove_if_exists(&exact_path)?;
    let mut exact_session = StoreScratchResolutionSession::new(
        factory.clone(),
        identity,
        &exact_path,
        RESOLUTION_WINDOW_SIZE,
        payload.resolver_output_epoch,
    )
    .map_err(classify_resolution_error)?;
    crate::resolution::run_resolution_session(&mut exact_session, true, true)
        .map_err(classify_resolution_error)?;
    exact_session
        .finish_exact()
        .map_err(classify_resolution_error)?;
    heartbeat.ensure_current(coordinator, request, holder)?;

    let base_relative_path = factory
        .open_reader()
        .map_err(|error| format!("resolution_failed: {error}"))?
        .query_row(
            "SELECT relative_path FROM resolution_bases WHERE base_id=?1 AND state='ready'",
            [&binding.base_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| format!("resolution_failed: {error}"))?;
    let base = ResolutionBaseReader::open(layout.generation_dir().join(base_relative_path))
        .map_err(|error| format!("resolution_failed: {error}"))?;
    let exact = ResolutionBaseReader::open(&exact_path)
        .map_err(|error| format!("resolution_failed: {error}"))?;
    let delta_path = layout
        .scratch_dir()
        .join(format!("resolve-delta-{}.db", request.request_id));
    remove_if_exists(&delta_path)?;
    let mut gaps = Vec::<ResolutionGapFact>::new();
    stream_resolution_diff(&base, &exact, &delta_path, RESOLUTION_WINDOW_SIZE, |gap| {
        gaps.push(gap);
        Ok(())
    })
    .map_err(|error| format!("resolution_failed: {error}"))?;
    let scratch = ResolutionScratchReader::open(&delta_path)
        .map_err(|error| format!("resolution_failed: {error}"))?;
    heartbeat.ensure_current(coordinator, request, holder)?;
    with_writer_lease(coordinator, holder, deadline, |fencing_token| {
        heartbeat.ensure_live()?;
        ensure_resolve_claim(layout, request, holder)?;
        let publication = ResolutionExactPublish {
            view_id: payload.view_id.clone(),
            manifest_generation: binding.manifest_generation,
            manifest_hash: binding.manifest_hash.clone(),
            base_id: binding.base_id.clone(),
            previous_delta_generation: binding.delta_generation,
            resolver_output_epoch: payload.resolver_output_epoch,
            request_id: request.request_id.clone(),
            created_at: store_timestamp(layout, "now")?,
        };
        let fence = ResolutionPublicationFence {
            claim_owner: holder.holder_id.clone(),
            holder_id: holder.holder_id.clone(),
            holder_pid: holder.holder_pid,
            fencing_token,
            now_ms: now_millis(),
        };
        #[cfg(feature = "test-store-resolution-contract")]
        julie_extract_artifact::store::test_hooks::crash_if("resolution_before_exact_publish");
        let published = bindings
            .publish_exact(
                &publication,
                &fence,
                &scratch,
                &gaps,
                RESOLUTION_WINDOW_SIZE,
            )
            .map_err(|error| format!("resolution_failed: {error}"))?;
        #[cfg(feature = "test-store-resolution-contract")]
        julie_extract_artifact::store::test_hooks::crash_if("resolution_exact_after_store_commit");
        append_resolution_terminal(layout, request, &published, Some(&scratch), Some(&gaps))?;
        bindings
            .release_pin(&pin_id, ResolutionPinOwnerKind::Resolve, &holder.holder_id)
            .map_err(|error| format!("resolution_failed: {error}"))?;
        Ok(())
    })?;
    drop(scratch);
    drop(exact);
    drop(base);
    remove_if_exists(&delta_path)?;
    remove_if_exists(&exact_path)?;
    Ok(())
}

fn with_writer_lease<T>(
    coordinator: &mut StoreCoordinator,
    holder: &LeaseHolder,
    deadline: i64,
    operation: impl FnOnce(i64) -> Result<T, String>,
) -> Result<T, String> {
    let fencing_token = loop {
        match coordinator
            .try_acquire_or_takeover(holder.clone(), now_millis())
            .map_err(|error| error.to_string())?
        {
            LeaseDisposition::Acquired { fencing_token } => break fencing_token,
            LeaseDisposition::HeldByOther if now_millis() < deadline => {
                thread::sleep(Duration::from_millis(25));
            }
            LeaseDisposition::HeldByOther => return Err("request_timeout".to_string()),
        }
    };
    let result = operation(fencing_token);
    let release = coordinator
        .release_lease(holder, fencing_token)
        .map_err(|error| error.to_string());
    match (result, release) {
        (Ok(value), Ok(true)) => Ok(value),
        (Ok(_), Ok(false)) => Err("resolution_failed: writer lease was lost".to_string()),
        (Ok(_), Err(error)) | (Err(error), _) => Err(error),
    }
}

fn normalize_dead_writer_lease(
    layout: &StoreLayout,
    coordinator: &mut StoreCoordinator,
    holder: &LeaseHolder,
) -> Result<(), String> {
    let lease_exists: bool = Connection::open(layout.coordinator_db())
        .map_err(|error| error.to_string())?
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM writer_lease WHERE resource='store-writer')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !lease_exists {
        return Ok(());
    }
    match coordinator
        .try_acquire_or_takeover(holder.clone(), now_millis())
        .map_err(|error| error.to_string())?
    {
        LeaseDisposition::Acquired { fencing_token } => {
            if !coordinator
                .release_lease(holder, fencing_token)
                .map_err(|error| error.to_string())?
            {
                return Err("resolution_failed: writer lease was lost".to_string());
            }
        }
        LeaseDisposition::HeldByOther => {}
    }
    Ok(())
}

fn append_resolution_terminal(
    layout: &StoreLayout,
    request: &CoordinatorRequest,
    binding: &ResolutionViewBinding,
    scratch: Option<&ResolutionScratchReader>,
    gaps: Option<&[ResolutionGapFact]>,
) -> Result<(), String> {
    let mut connection = Connection::open(layout.store_db()).map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    if StoreLog::committed_in_fact(&transaction, &request.request_id)
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(());
    }
    let delta = transaction
        .query_row(
            "SELECT exact_gap_rows,exact_gap_files FROM resolution_deltas
             WHERE view_id=?1 AND delta_generation=?2",
            params![binding.view_id, binding.delta_generation],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let result_json = serde_json::json!({
        "base_id": binding.base_id,
        "delta_generation": binding.delta_generation,
        "exact_at_generation": binding.exact_at,
        "exact_gap_files": delta.1,
        "exact_gap_rows": delta.0,
        "gap_lower_bound": gaps.map_or(0, |facts| facts.len()),
        "identifier_replacements": scratch.map_or(0, |value| value.semantic_counts().identifier_replacements),
        "manifest_generation": binding.manifest_generation,
        "manifest_hash": binding.manifest_hash,
        "pending_replacements": scratch.map_or(0, |value| value.semantic_counts().pending_replacements),
        "pending_tombstones": scratch.map_or(0, |value| value.semantic_counts().pending_tombstones),
        "resolution_state": binding.state.as_str(),
    })
    .to_string();
    let created_at = transaction
        .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?;
    StoreLog::append_terminal(
        &transaction,
        &StoreLogEntry::new(
            &request.request_id,
            "store_resolve_completed",
            result_json,
            created_at,
        )
        .with_view(&binding.view_id)
        .with_generation(
            u64::try_from(binding.manifest_generation)
                .map_err(|_| "resolution_failed: invalid manifest generation".to_string())?,
        ),
    )
    .map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    #[cfg(feature = "test-store-resolution-contract")]
    julie_extract_artifact::store::test_hooks::crash_if("resolution_terminal_after_store_commit");
    Ok(())
}

fn claim_until_deadline(
    coordinator: &StoreCoordinator,
    request: &CoordinatorRequest,
    owner_id: &str,
) -> Result<(), String> {
    loop {
        let now = now_millis();
        if coordinator
            .claim_resolve(&request.request_id, owner_id, now, RESOLVE_CLAIM_STALE_MS)
            .map_err(|error| error.to_string())?
        {
            return Ok(());
        }
        if request
            .requester_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            return Err("request_timeout".to_string());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn ensure_resolve_claim(
    layout: &StoreLayout,
    request: &CoordinatorRequest,
    holder: &LeaseHolder,
) -> Result<(), String> {
    if StoreCoordinator::open(layout)
        .map_err(|error| error.to_string())?
        .resolve_claim_is_current(&request.request_id, &holder.holder_id)
        .map_err(|error| error.to_string())?
    {
        Ok(())
    } else {
        Err("resolution_failed: resolve claim lost".to_string())
    }
}

fn parse_payload(payload_json: &str) -> Result<ResolveRequestPayload, String> {
    let payload: ResolveRequestPayload =
        serde_json::from_str(payload_json).map_err(|_| "invalid_resolve_payload".to_string())?;
    if payload.schema_version != 1
        || payload.family_id.is_empty()
        || payload.view_id.is_empty()
        || payload.resolver_output_epoch <= 0
    {
        return Err("invalid_resolve_payload".to_string());
    }
    Ok(payload)
}

fn require_view_identity(
    layout: &StoreLayout,
    view_id: &str,
    require_l2: bool,
) -> Result<StoreManifestIdentity, String> {
    let connection = Connection::open(layout.store_db()).map_err(|error| error.to_string())?;
    let family_id = connection
        .query_row(
            "SELECT value FROM store_meta WHERE key='family_id'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| error.to_string())?;
    let (generation, manifest_hash) = connection
        .query_row(
            "SELECT view.current_generation,manifest.manifest_hash
             FROM views AS view
             LEFT JOIN manifests AS manifest
               ON manifest.view_id=view.view_id AND manifest.generation=view.current_generation
             WHERE view.view_id=?1",
            [view_id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "view_not_found".to_string())?;
    let (Some(generation), Some(manifest_hash)) = (generation, manifest_hash) else {
        return Err("resolution_input_incomplete".to_string());
    };
    if require_l2 {
        let incomplete: Option<(String, i64)> = connection
            .query_row(
                "SELECT entry.path,entry.version_id
                 FROM manifest_entries AS entry
                 JOIN file_versions AS version ON version.version_id=entry.version_id
                 WHERE entry.view_id=?1 AND entry.generation=?2
                   AND entry.status IN ('indexed','failed_preserved')
                   AND version.complete_l2 IS NULL
                 ORDER BY entry.path COLLATE BINARY LIMIT 1",
                params![view_id, generation],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if let Some((path, version_id)) = incomplete {
            return Err(format!(
                "resolution_input_incomplete: {path} version {version_id} has no L2 stamp"
            ));
        }
    }
    Ok(StoreManifestIdentity {
        family_id,
        view_id: view_id.to_string(),
        generation,
        manifest_hash,
    })
}

fn report_resolve(
    layout: &StoreLayout,
    request: &CoordinatorRequest,
    payload: &ResolveRequestPayload,
) -> Result<StoreReport, String> {
    let state = match request.state {
        RequestState::Queued => StoreRequestState::Queued,
        RequestState::Claimed => StoreRequestState::Claimed,
        RequestState::Committed => StoreRequestState::Committed,
        RequestState::Acknowledged => StoreRequestState::Acknowledged,
        RequestState::Failed => StoreRequestState::Failed,
    };
    let mut report = StoreReport::new(
        &request.request_id,
        &payload.family_id,
        &payload.view_id,
        state,
    )
    .with_operation(StoreOperation::Resolve)
    .with_requested_level(StoreRequestedLevel::NotApplicable)
    .with_idempotency_key(&request.idempotency_key);
    report.coordinator = match request.state {
        RequestState::Queued => StoreCoordinatorDisposition::Queued,
        RequestState::Claimed => StoreCoordinatorDisposition::Claimed,
        RequestState::Committed => StoreCoordinatorDisposition::Committed,
        RequestState::Acknowledged => StoreCoordinatorDisposition::Acknowledged,
        RequestState::Failed => StoreCoordinatorDisposition::Failed,
    };
    if let Some(result_json) = &request.result_json {
        let result: serde_json::Value =
            serde_json::from_str(result_json).map_err(|error| error.to_string())?;
        report.resolution.state = parse_resolution_state(&result)?;
        report.resolution.exact_at_matches = report.resolution.state == StoreResolutionState::Exact;
        report.resolution.base_id = result["base_id"].as_str().map(ToOwned::to_owned);
        report.resolution.delta_generation = result["delta_generation"].as_u64();
        report.resolution.exact_at_generation = result["exact_at_generation"].as_u64();
        report.resolution.gap_lower_bound = result["gap_lower_bound"].as_u64();
        report.resolution.exact_gap_rows = result["exact_gap_rows"].as_u64();
        report.resolution.exact_gap_files = result["exact_gap_files"].as_u64();
        report.manifest.generation = result["manifest_generation"].as_u64();
        report.manifest.hash = result["manifest_hash"].as_str().map(ToOwned::to_owned);
    } else {
        populate_current_resolution(layout, &payload.view_id, &mut report)?;
    }
    if request.state == RequestState::Failed {
        let message = request
            .error_json
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .and_then(|value| value["message"].as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "resolution_failed".to_string());
        return Ok(report.with_failure(classify_failure(&message), message));
    }
    Ok(report)
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

fn parse_resolution_state(value: &serde_json::Value) -> Result<StoreResolutionState, String> {
    match value["resolution_state"].as_str() {
        Some("unbound") => Ok(StoreResolutionState::Unbound),
        Some("converging") => Ok(StoreResolutionState::Converging),
        Some("exact") => Ok(StoreResolutionState::Exact),
        _ => Err("resolution_failed: invalid terminal resolution state".to_string()),
    }
}

fn store_timestamp(layout: &StoreLayout, modifier: &str) -> Result<String, String> {
    let connection = Connection::open(layout.store_db()).map_err(|error| error.to_string())?;
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

fn classify_resolution_error(error: impl std::fmt::Display) -> String {
    let detail = error.to_string();
    if detail.contains("not L2-complete") || detail.contains("input") {
        format!("resolution_input_incomplete: {detail}")
    } else {
        format!("resolution_failed: {detail}")
    }
}

fn remove_if_exists(path: &std::path::Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(feature = "test-store-resolution-contract")]
fn pause_after_claim_for_test() -> Result<(), String> {
    let Ok(ready_path) = std::env::var("JULIE_EXTRACT_STORE_RESOLUTION_PAUSE_FILE") else {
        return Ok(());
    };
    let ready_path = std::path::PathBuf::from(ready_path);
    fs::write(&ready_path, b"claimed").map_err(|error| error.to_string())?;
    let resume_path = ready_path.with_extension("resume");
    while !resume_path.exists() {
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

struct ResolveHeartbeat {
    stop: mpsc::Sender<()>,
    current: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ResolveHeartbeat {
    fn start(layout: StoreLayout, request_id: String, owner_id: String) -> Self {
        let (stop, receiver) = mpsc::channel();
        let current = Arc::new(AtomicBool::new(true));
        let worker_current = current.clone();
        let worker = thread::spawn(move || {
            let coordinator = match StoreCoordinator::open(&layout) {
                Ok(coordinator) => coordinator,
                Err(_) => {
                    worker_current.store(false, Ordering::Release);
                    return;
                }
            };
            loop {
                match receiver.recv_timeout(Duration::from_millis(250)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                match coordinator.heartbeat_resolve(&request_id, &owner_id, now_millis()) {
                    Ok(true) => {}
                    _ => {
                        worker_current.store(false, Ordering::Release);
                        return;
                    }
                }
            }
        });
        Self {
            stop,
            current,
            worker: Some(worker),
        }
    }

    fn ensure_current(
        &self,
        coordinator: &StoreCoordinator,
        request: &CoordinatorRequest,
        holder: &LeaseHolder,
    ) -> Result<(), String> {
        if !self.current.load(Ordering::Acquire)
            || !coordinator
                .resolve_claim_is_current(&request.request_id, &holder.holder_id)
                .map_err(|error| error.to_string())?
        {
            return Err("resolution_failed: resolve claim lost".to_string());
        }
        Ok(())
    }

    fn ensure_live(&self) -> Result<(), String> {
        if self.current.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err("resolution_failed: resolve claim lost".to_string())
        }
    }

    fn stop(mut self) -> bool {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.current.load(Ordering::Acquire)
    }
}
