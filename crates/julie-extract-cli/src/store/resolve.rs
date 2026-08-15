use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use julie_extract_artifact::store::{
    CoordinatorRequest, GenerationFence, LeaseDisposition, LeaseHolder, RequestKind, RequestState,
    ResolutionBaseBegin, ResolutionBaseBuild, ResolutionBaseCatalog, ResolutionBaseReader,
    ResolutionBaseRecovery, ResolutionBaseWriter, ResolutionBindingStore,
    ResolutionConvergenceBegin, ResolutionExactPublish, ResolutionGapFact, ResolutionPinOwnerKind,
    ResolutionPublicationFence, ResolutionScratchReader, ResolutionViewBinding,
    StoreConnectionFactory, StoreCoordinator, StoreLayout, StoreLog, StoreLogEntry,
    ViewResolutionState, apply_base_delta, renew_writer_lease_with_retry, stream_resolution_diff,
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
use crate::resolution_session::{ResolutionWorklists, SemanticVersionId};

const RESOLVE_CLAIM_STALE_MS: i64 = 5_000;
const RESOLUTION_WINDOW_SIZE: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ResolveRequestPayload {
    schema_version: i64,
    family_id: String,
    view_id: String,
    resolver_output_epoch: i64,
    #[serde(default)]
    resolution_delta_enabled: bool,
}

#[derive(Debug, Clone)]
struct ResolutionExecutionTelemetry {
    resolution_mode: &'static str,
    scope_file_count: u64,
    scope_name_count: u64,
    scope_row_count: u64,
    fallback_reason: Option<String>,
    phase_timings_ms: BTreeMap<String, u64>,
}

impl ResolutionExecutionTelemetry {
    fn forced_full(fallback_reason: impl Into<String>) -> Self {
        Self {
            resolution_mode: "full",
            scope_file_count: 0,
            scope_name_count: 0,
            scope_row_count: 0,
            fallback_reason: Some(fallback_reason.into()),
            phase_timings_ms: BTreeMap::new(),
        }
    }

    fn durable_payload(&self) -> serde_json::Value {
        serde_json::json!({
            "resolution_mode": self.resolution_mode,
            "scope_file_count": self.scope_file_count,
            "scope_name_count": self.scope_name_count,
            "scope_row_count": self.scope_row_count,
            "fallback_reason": self.fallback_reason,
            "phase_timings_ms": self.phase_timings_ms,
        })
    }

    fn from_durable_payload(payload: &serde_json::Value) -> Result<Self, String> {
        let resolution_mode = match payload["resolution_mode"].as_str() {
            Some("full") => "full",
            Some("scoped") => "scoped",
            _ => return Err("resolution_failed: invalid durable resolution mode".to_string()),
        };
        let fallback_reason = payload["fallback_reason"].as_str().map(ToOwned::to_owned);
        let phase_timings_ms = payload["phase_timings_ms"]
            .as_object()
            .ok_or_else(|| "resolution_failed: invalid durable phase timings".to_string())?
            .iter()
            .map(|(phase, value)| {
                value
                    .as_u64()
                    .map(|value| (phase.clone(), value))
                    .ok_or_else(|| "resolution_failed: invalid durable phase timing".to_string())
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            resolution_mode,
            scope_file_count: payload["scope_file_count"]
                .as_u64()
                .ok_or_else(|| "resolution_failed: invalid durable scope file count".to_string())?,
            scope_name_count: payload["scope_name_count"]
                .as_u64()
                .ok_or_else(|| "resolution_failed: invalid durable scope name count".to_string())?,
            scope_row_count: payload["scope_row_count"]
                .as_u64()
                .ok_or_else(|| "resolution_failed: invalid durable scope row count".to_string())?,
            fallback_reason,
            phase_timings_ms,
        })
    }
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
    let resolution_delta_enabled = resolution_delta_enabled()?;
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
        resolution_delta_enabled,
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
        remove_resolution_request_scratch(&layout, &canonical.request_id)?;
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
        remove_resolution_request_scratch(&layout, &canonical.request_id)?;
        return report_resolve(&layout, &request, &payload);
    }
    if canonical.state == RequestState::Failed {
        remove_resolution_request_scratch(&layout, &canonical.request_id)?;
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
    heartbeat.stop()?;
    if let Err(message) = result {
        if coordinator
            .reconcile(&canonical.request_id)
            .map_err(|error| error.to_string())?
            .committed_in_fact
        {
            let request = coordinator
                .request(&canonical.request_id)
                .map_err(|error| error.to_string())?;
            remove_resolution_request_scratch(&layout, &canonical.request_id)?;
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
        remove_resolution_request_scratch(&layout, &canonical.request_id)?;
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
    let has_ready_base: bool = {
        let ready_base_connection = factory
            .open_reader()
            .map_err(|error| format!("resolution_failed: {error}"))?;
        ready_base_connection
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM resolution_bases
                   WHERE state='ready' AND resolver_output_epoch=?1
                 )",
                [payload.resolver_output_epoch],
                |row| row.get(0),
            )
            .map_err(|error| format!("resolution_failed: {error}"))?
    };
    if !has_ready_base {
        let created_at = store_timestamp(layout, "now")?;
        let begin = with_writer_lease(
            layout,
            coordinator,
            holder,
            deadline,
            |_coordinator, fencing_token| {
                ensure_resolve_claim(layout, request, holder)?;
                ResolutionBaseCatalog::new(fenced_factory(&factory, layout, holder, fencing_token))
                    .begin_build(
                        &identity.manifest_hash,
                        payload.resolver_output_epoch,
                        &request.request_id,
                        &created_at,
                    )
                    .map_err(|error| error.to_string())
            },
        )?;
        let build = match begin {
            ResolutionBaseBegin::Build(build) => Some(build),
            ResolutionBaseBegin::Ready(_) => None,
            ResolutionBaseBegin::Building(_) => {
                let recovery = with_writer_lease(
                    layout,
                    coordinator,
                    holder,
                    deadline,
                    |_coordinator, fencing_token| {
                        heartbeat.ensure_live()?;
                        ensure_resolve_claim(layout, request, holder)?;
                        ResolutionBaseCatalog::new(fenced_factory(
                            &factory,
                            layout,
                            holder,
                            fencing_token,
                        ))
                        .recover(
                            &identity.manifest_hash,
                            payload.resolver_output_epoch,
                            &request.request_id,
                            false,
                            &store_timestamp(layout, "now")?,
                        )
                        .map_err(|error| format!("resolution_failed: {error}"))
                    },
                )?;
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
            session.force_full_without_prior_state();
            crate::resolution::run_resolution_session(&mut session, true, true)
                .map_err(classify_resolution_error)?;
            session.finish_exact().map_err(classify_resolution_error)?;
            heartbeat.ensure_current(coordinator, request, holder)?;
            catalog
                .publish_scratch(&build)
                .map_err(|error| format!("resolution_failed: {error}"))?;
            heartbeat.ensure_current(coordinator, request, holder)?;
            with_writer_lease(
                layout,
                coordinator,
                holder,
                deadline,
                |_coordinator, fencing_token| {
                    heartbeat.ensure_live()?;
                    ensure_resolve_claim(layout, request, holder)?;
                    ResolutionBaseCatalog::new(fenced_factory(
                        &factory,
                        layout,
                        holder,
                        fencing_token,
                    ))
                    .mark_ready(&build, &store_timestamp(layout, "now")?)
                    .map(|_| ())
                    .map_err(|error| format!("resolution_failed: {error}"))
                },
            )?;
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
    let (binding, _, validated_base) = with_writer_lease(
        layout,
        coordinator,
        holder,
        deadline,
        |_coordinator, fencing_token| {
            heartbeat.ensure_live()?;
            ensure_resolve_claim(layout, request, holder)?;
            ResolutionBindingStore::new(fenced_factory(&factory, layout, holder, fencing_token))
                .begin_convergence_with_proof(&convergence)
                .map_err(|error| format!("resolution_failed: {error}"))
        },
    )?;
    let mut pin_guard =
        ResolvePinGuard::armed(factory.clone(), pin_id.clone(), holder.holder_id.clone());
    let mut telemetry =
        ResolutionExecutionTelemetry::forced_full(if payload.resolution_delta_enabled {
            "resolution_already_exact"
        } else {
            "incremental_resolution_disabled"
        });
    let exact_path = layout
        .scratch_dir()
        .join(format!("resolve-exact-{}.db", request.request_id));
    let delta_path = layout
        .scratch_dir()
        .join(format!("resolve-delta-{}.db", request.request_id));
    if binding.state == ViewResolutionState::Exact {
        let rebase_published = resolution_rebase_published(layout, &request.request_id)?;
        if let Some(durable) = ResolutionBindingStore::new(factory.clone())
            .exact_publication_telemetry(
                &request.request_id,
                &binding.view_id,
                binding.manifest_generation,
            )
            .map_err(|error| format!("resolution_failed: {error}"))?
        {
            telemetry = ResolutionExecutionTelemetry::from_durable_payload(&durable)?;
        }
        heartbeat.ensure_current(coordinator, request, holder)?;
        with_writer_lease(
            layout,
            coordinator,
            holder,
            deadline,
            |_coordinator, fencing_token| {
                heartbeat.ensure_live()?;
                ensure_resolve_claim(layout, request, holder)?;
                let fenced_bindings = ResolutionBindingStore::new(fenced_factory(
                    &factory,
                    layout,
                    holder,
                    fencing_token,
                ));
                fenced_bindings
                    .release_pin(
                        &rebase_pin_id(&request.request_id),
                        ResolutionPinOwnerKind::Resolve,
                        &request.request_id,
                    )
                    .map_err(|error| format!("resolution_failed: {error}"))?;
                fenced_bindings
                    .release_pin(&pin_id, ResolutionPinOwnerKind::Resolve, &holder.holder_id)
                    .map_err(|error| format!("resolution_failed: {error}"))?;
                pin_guard.disarm();
                if rebase_published {
                    fenced_bindings
                        .cleanup_superseded_deltas(
                            &binding.view_id,
                            &store_timestamp(layout, "now")?,
                        )
                        .map_err(|error| format!("resolution_failed: {error}"))?;
                }
                append_resolution_terminal(
                    &factory,
                    layout,
                    holder,
                    fencing_token,
                    request,
                    &binding,
                    &telemetry,
                )?;
                Ok(())
            },
        )?;
        remove_sqlite_if_exists(&delta_path)?;
        remove_sqlite_if_exists(&exact_path)?;
        return Ok(());
    }

    heartbeat.ensure_current(coordinator, request, holder)?;
    remove_resolution_scratch_if_exists(&exact_path)?;
    let mut exact_session = StoreScratchResolutionSession::new(
        factory.clone(),
        identity.clone(),
        &exact_path,
        RESOLUTION_WINDOW_SIZE,
        payload.resolver_output_epoch,
    )
    .map_err(classify_resolution_error)?;
    exact_session.set_validated_base(validated_base.clone());
    if !payload.resolution_delta_enabled {
        exact_session.force_full_without_prior_state();
    }
    let resolution_started = Instant::now();
    crate::resolution::run_resolution_session(
        &mut exact_session,
        !payload.resolution_delta_enabled,
        true,
    )
    .map_err(classify_resolution_error)?;
    telemetry
        .phase_timings_ms
        .insert("resolution".to_string(), elapsed_millis(resolution_started));
    let decision = exact_session
        .decision_telemetry()
        .cloned()
        .ok_or_else(|| "resolution_failed: resolution decision telemetry missing".to_string())?;
    apply_scope_counts(&factory, &identity, &decision.worklists, &mut telemetry)?;
    telemetry.resolution_mode = if decision.effective_full {
        "full"
    } else {
        "scoped"
    };
    telemetry.fallback_reason = decision.fallback_reason.map(|reason| {
        if reason == "resolution_requested_full" && !payload.resolution_delta_enabled {
            "incremental_resolution_disabled".to_string()
        } else if reason == "resolution_requested_full" {
            "resolution_prior_overlay_unavailable".to_string()
        } else {
            reason.to_string()
        }
    });
    telemetry
        .phase_timings_ms
        .insert("scope".to_string(), decision.elapsed_millis);
    let base_relative_path = {
        let base_path_connection = factory
            .open_reader()
            .map_err(|error| format!("resolution_failed: {error}"))?;
        base_path_connection
            .query_row(
                "SELECT relative_path FROM resolution_bases WHERE base_id=?1 AND state='ready'",
                [&binding.base_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| format!("resolution_failed: {error}"))?
    };
    let base_path = layout.generation_dir().join(base_relative_path);
    remove_sqlite_if_exists(&delta_path)?;
    let mut gaps = Vec::<ResolutionGapFact>::new();
    let mut base_reader = None;
    let mut exact_reader = None;
    if decision.effective_full {
        #[cfg(feature = "test-store-resolution-contract")]
        fail_before_exact_finalize_for_test()?;
        exact_session
            .finish_exact()
            .map_err(classify_resolution_error)?;
        heartbeat.ensure_current(coordinator, request, holder)?;
        let base = ResolutionBaseReader::open(&base_path)
            .map_err(|error| format!("resolution_failed: {error}"))?;
        let exact = ResolutionBaseReader::open(&exact_path)
            .map_err(|error| format!("resolution_failed: {error}"))?;
        let diff_started = Instant::now();
        stream_resolution_diff(&base, &exact, &delta_path, RESOLUTION_WINDOW_SIZE, |gap| {
            gaps.push(gap);
            Ok(())
        })
        .map_err(|error| format!("resolution_failed: {error}"))?;
        telemetry
            .phase_timings_ms
            .insert("diff".to_string(), elapsed_millis(diff_started));
        base_reader = Some(base);
        exact_reader = Some(exact);
    } else {
        let diff_started = Instant::now();
        exact_session
            .finish_scoped_delta(&delta_path, |gap| {
                gaps.push(gap);
                Ok(())
            })
            .map_err(classify_resolution_error)?;
        telemetry
            .phase_timings_ms
            .insert("diff".to_string(), elapsed_millis(diff_started));
        heartbeat.ensure_current(coordinator, request, holder)?;
    }
    let scratch = ResolutionScratchReader::open(&delta_path)
        .map_err(|error| format!("resolution_failed: {error}"))?;
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
    let rebase_required = ResolutionBindingStore::new(factory.clone())
        .exact_rebase_required_with_proof(&publication, &scratch, &gaps, &validated_base)
        .map_err(|error| format!("resolution_failed: {error}"))?;
    heartbeat.ensure_current(coordinator, request, holder)?;
    #[cfg(feature = "test-store-resolution-contract")]
    pause_before_exact_publish_for_test()?;
    #[cfg(feature = "test-store-resolution-contract")]
    julie_extract_artifact::store::test_hooks::crash_if("resolution_before_exact_publish");
    if rebase_required {
        drop(scratch);
        drop(exact_reader.take());
        drop(base_reader.take());
        if !exact_path.exists() {
            materialize_exact_for_rebase(
                &factory,
                &identity,
                payload.resolver_output_epoch,
                &base_path,
                &delta_path,
                &exact_path,
                RESOLUTION_WINDOW_SIZE,
            )?;
        }
        remove_sqlite_if_exists(&delta_path)?;
        let (rebased_base_id, mut rebased_pin_guard) = prepare_rebased_base(
            layout,
            &factory,
            coordinator,
            holder,
            request,
            heartbeat,
            deadline,
            &publication,
            &exact_path,
        )?;
        let rebased_pin_id = rebased_pin_guard.pin_id.clone();
        with_writer_lease(
            layout,
            coordinator,
            holder,
            deadline,
            |coordinator, fencing_token| {
                heartbeat.ensure_live()?;
                ensure_resolve_claim(layout, request, holder)?;
                let fence = ResolutionPublicationFence {
                    claim_owner: holder.holder_id.clone(),
                    holder_id: holder.holder_id.clone(),
                    holder_pid: holder.holder_pid,
                    fencing_token,
                    now_ms: now_millis(),
                };
                let fenced_bindings = ResolutionBindingStore::new(fenced_factory(
                    &factory,
                    layout,
                    holder,
                    fencing_token,
                ));
                let durable_telemetry = telemetry.durable_payload();
                #[cfg(feature = "test-store-resolution-contract")]
                fail_before_rebase_view_cas_for_test()?;
                let published = fenced_bindings
                    .publish_rebased_exact(
                        &publication,
                        &rebased_base_id,
                        &fence,
                        Some(&durable_telemetry),
                        || {
                            exact_publish_heartbeat(
                                coordinator,
                                holder,
                                fencing_token,
                                &publication.request_id,
                            )
                        },
                    )
                    .map_err(|error| format!("resolution_failed: {error}"))?;
                #[cfg(feature = "test-store-resolution-contract")]
                julie_extract_artifact::store::test_hooks::crash_if(
                    "resolution_exact_after_store_commit",
                );
                #[cfg(feature = "test-store-resolution-contract")]
                julie_extract_artifact::store::test_hooks::crash_if(
                    "resolution_rebase_after_store_commit",
                );
                #[cfg(feature = "test-store-resolution-contract")]
                pause_after_exact_publish_for_test()?;
                fenced_bindings
                    .release_pin(
                        &rebased_pin_id,
                        ResolutionPinOwnerKind::Resolve,
                        &request.request_id,
                    )
                    .map_err(|error| format!("resolution_failed: {error}"))?;
                rebased_pin_guard.disarm();
                fenced_bindings
                    .release_pin(&pin_id, ResolutionPinOwnerKind::Resolve, &holder.holder_id)
                    .map_err(|error| format!("resolution_failed: {error}"))?;
                pin_guard.disarm();
                fenced_bindings
                    .cleanup_superseded_deltas(&published.view_id, &store_timestamp(layout, "now")?)
                    .map_err(|error| format!("resolution_failed: {error}"))?;
                append_resolution_terminal(
                    &factory,
                    layout,
                    holder,
                    fencing_token,
                    request,
                    &published,
                    &telemetry,
                )?;
                Ok(())
            },
        )?;
        remove_sqlite_if_exists(&exact_path)?;
        return Ok(());
    }
    with_writer_lease(
        layout,
        coordinator,
        holder,
        deadline,
        |coordinator, fencing_token| {
            heartbeat.ensure_live()?;
            ensure_resolve_claim(layout, request, holder)?;
            let fence = ResolutionPublicationFence {
                claim_owner: holder.holder_id.clone(),
                holder_id: holder.holder_id.clone(),
                holder_pid: holder.holder_pid,
                fencing_token,
                now_ms: now_millis(),
            };
            let fenced_bindings = ResolutionBindingStore::new(fenced_factory(
                &factory,
                layout,
                holder,
                fencing_token,
            ));
            let durable_telemetry = telemetry.durable_payload();
            let published = fenced_bindings
                .publish_exact_with_telemetry(
                    &publication,
                    &fence,
                    &scratch,
                    &gaps,
                    RESOLUTION_WINDOW_SIZE,
                    Some(&durable_telemetry),
                    || {
                        exact_publish_heartbeat(
                            coordinator,
                            holder,
                            fencing_token,
                            &publication.request_id,
                        )
                    },
                )
                .map_err(|error| format!("resolution_failed: {error}"))?;
            #[cfg(feature = "test-store-resolution-contract")]
            julie_extract_artifact::store::test_hooks::crash_if(
                "resolution_exact_after_store_commit",
            );
            #[cfg(feature = "test-store-resolution-contract")]
            pause_after_exact_publish_for_test()?;
            fenced_bindings
                .release_pin(&pin_id, ResolutionPinOwnerKind::Resolve, &holder.holder_id)
                .map_err(|error| format!("resolution_failed: {error}"))?;
            pin_guard.disarm();
            append_resolution_terminal(
                &factory,
                layout,
                holder,
                fencing_token,
                request,
                &published,
                &telemetry,
            )?;
            Ok(())
        },
    )?;
    drop(scratch);
    drop(exact_reader);
    drop(base_reader);
    remove_sqlite_if_exists(&delta_path)?;
    remove_sqlite_if_exists(&exact_path)?;
    Ok(())
}

fn materialize_exact_for_rebase(
    factory: &StoreConnectionFactory,
    identity: &StoreManifestIdentity,
    resolver_output_epoch: i64,
    base_path: &Path,
    delta_path: &Path,
    exact_path: &Path,
    window_size: usize,
) -> Result<(), String> {
    remove_sqlite_if_exists(exact_path)?;
    let connection = factory
        .open_reader()
        .map_err(|error| format!("resolution_failed: {error}"))?;
    let visible_versions = connection
        .prepare(
            "SELECT version_id
             FROM manifest_entries
             WHERE view_id=?1 AND generation=?2
               AND status IN ('indexed','failed_preserved')
             ORDER BY version_id",
        )
        .map_err(|error| format!("resolution_failed: {error}"))?
        .query_map(params![identity.view_id, identity.generation], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| format!("resolution_failed: {error}"))?
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| format!("resolution_failed: {error}"))?;
    let base = ResolutionBaseReader::open(base_path)
        .map_err(|error| format!("resolution_failed: {error}"))?;
    let delta = ResolutionScratchReader::open(delta_path)
        .map_err(|error| format!("resolution_failed: {error}"))?;
    let mut writer = ResolutionBaseWriter::new(
        exact_path,
        identity.manifest_hash.clone(),
        resolver_output_epoch,
    )
    .map_err(|error| format!("resolution_failed: {error}"))?;
    for version_id in &visible_versions {
        writer
            .push_source_version(*version_id)
            .map_err(|error| format!("resolution_failed: {error}"))?;
    }
    let writer = std::cell::RefCell::new(writer);
    apply_base_delta(
        &base,
        &delta,
        window_size,
        |version_id| Ok(visible_versions.contains(&version_id)),
        |row| writer.borrow_mut().push_identifier_resolution(row),
        |row| writer.borrow_mut().push_pending_resolution(row),
    )
    .map_err(|error| format!("resolution_failed: {error}"))?;
    let writer = writer.into_inner();
    let mut target_exists = connection
        .prepare(
            "SELECT EXISTS(
               SELECT 1 FROM symbols AS s
               WHERE s.version_id = ?1 AND s.symbol_id = ?2
                 AND EXISTS (
                   SELECT 1 FROM manifest_entries AS me
                   WHERE me.view_id = ?3 AND me.generation = ?4
                     AND me.status IN ('indexed','failed_preserved')
                     AND me.version_id = s.version_id
                 )
             )",
        )
        .map_err(|error| format!("resolution_failed: {error}"))?;
    let identity = identity.clone();
    writer
        .finish_with_target_lookup(|version_id, symbol_id| {
            target_exists
                .query_row(
                    params![version_id, symbol_id, identity.view_id, identity.generation],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(julie_extract_artifact::store::ResolutionValidationError::Sqlite)
        })
        .map_err(|error| format!("resolution_failed: {error}"))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn prepare_rebased_base(
    layout: &StoreLayout,
    factory: &StoreConnectionFactory,
    coordinator: &mut StoreCoordinator,
    holder: &LeaseHolder,
    request: &CoordinatorRequest,
    heartbeat: &ResolveHeartbeat,
    deadline: i64,
    publication: &ResolutionExactPublish,
    exact_path: &Path,
) -> Result<(String, ResolvePinGuard), String> {
    let pin_id = rebase_pin_id(&publication.request_id);
    let begin = with_writer_lease(
        layout,
        coordinator,
        holder,
        deadline,
        |_coordinator, fencing_token| {
            heartbeat.ensure_live()?;
            ensure_resolve_claim(layout, request, holder)?;
            ResolutionBaseCatalog::new(fenced_factory(factory, layout, holder, fencing_token))
                .begin_build(
                    &publication.manifest_hash,
                    publication.resolver_output_epoch,
                    &publication.request_id,
                    &publication.created_at,
                )
                .map_err(|error| format!("resolution_failed: {error}"))
        },
    )?;
    let catalog = ResolutionBaseCatalog::new(factory.clone());
    let build = match begin {
        ResolutionBaseBegin::Build(build) => Some(build),
        ResolutionBaseBegin::Ready(record) => {
            let pin_guard = open_rebase_pin(
                layout,
                factory,
                coordinator,
                holder,
                request,
                heartbeat,
                deadline,
                publication,
                &record.base_id,
                &pin_id,
            )?;
            return Ok((record.base_id, pin_guard));
        }
        ResolutionBaseBegin::Building(_) => {
            let recovery = with_writer_lease(
                layout,
                coordinator,
                holder,
                deadline,
                |_coordinator, fencing_token| {
                    heartbeat.ensure_live()?;
                    ensure_resolve_claim(layout, request, holder)?;
                    ResolutionBaseCatalog::new(fenced_factory(
                        factory,
                        layout,
                        holder,
                        fencing_token,
                    ))
                    .recover(
                        &publication.manifest_hash,
                        publication.resolver_output_epoch,
                        &publication.request_id,
                        false,
                        &store_timestamp(layout, "now")?,
                    )
                    .map_err(|error| format!("resolution_failed: {error}"))
                },
            )?;
            match recovery {
                ResolutionBaseRecovery::Ready(record) => {
                    let pin_guard = open_rebase_pin(
                        layout,
                        factory,
                        coordinator,
                        holder,
                        request,
                        heartbeat,
                        deadline,
                        publication,
                        &record.base_id,
                        &pin_id,
                    )?;
                    return Ok((record.base_id, pin_guard));
                }
                ResolutionBaseRecovery::Rebuild(build) => Some(build),
                ResolutionBaseRecovery::LiveOwner(_) => {
                    return Err(
                        "resolution_failed: a live base builder owns this identity".to_string()
                    );
                }
            }
        }
    };
    let build = build.expect("rebase build is present");
    let mut pin_guard = open_rebase_pin(
        layout,
        factory,
        coordinator,
        holder,
        request,
        heartbeat,
        deadline,
        publication,
        &build.record.base_id,
        &pin_id,
    )?;
    if let Err(error) = (|| {
        promote_exact_scratch_for_rebase(exact_path, &build)?;
        heartbeat.ensure_current(coordinator, request, holder)?;
        catalog
            .publish_scratch(&build)
            .map_err(|error| format!("resolution_failed: {error}"))?;
        heartbeat.ensure_current(coordinator, request, holder)?;
        Ok::<(), String>(())
    })() {
        if release_rebase_pin_after_error(
            layout,
            factory,
            coordinator,
            holder,
            request,
            heartbeat,
            deadline,
            &pin_id,
        )
        .is_ok()
        {
            pin_guard.disarm();
        }
        return Err(error);
    }
    let base_id = with_writer_lease(
        layout,
        coordinator,
        holder,
        deadline,
        |_coordinator, fencing_token| {
            heartbeat.ensure_live()?;
            ensure_resolve_claim(layout, request, holder)?;
            ResolutionBaseCatalog::new(fenced_factory(factory, layout, holder, fencing_token))
                .mark_ready(&build, &store_timestamp(layout, "now")?)
                .map(|record| record.base_id)
                .map_err(|error| format!("resolution_failed: {error}"))
        },
    )?;
    Ok((base_id, pin_guard))
}

#[allow(clippy::too_many_arguments)]
fn open_rebase_pin(
    layout: &StoreLayout,
    factory: &StoreConnectionFactory,
    coordinator: &mut StoreCoordinator,
    holder: &LeaseHolder,
    request: &CoordinatorRequest,
    heartbeat: &ResolveHeartbeat,
    deadline: i64,
    publication: &ResolutionExactPublish,
    base_id: &str,
    pin_id: &str,
) -> Result<ResolvePinGuard, String> {
    let expires_at = store_timestamp(layout, "+1 hour")?;
    let now = store_timestamp(layout, "now")?;
    with_writer_lease(
        layout,
        coordinator,
        holder,
        deadline,
        |_coordinator, fencing_token| {
            heartbeat.ensure_live()?;
            ensure_resolve_claim(layout, request, holder)?;
            let fenced_factory = fenced_factory(factory, layout, holder, fencing_token);
            ResolutionBindingStore::new(fenced_factory.clone())
                .open_pin_for_base(
                    pin_id,
                    ResolutionPinOwnerKind::Resolve,
                    &publication.request_id,
                    &publication.view_id,
                    publication.manifest_generation,
                    &publication.manifest_hash,
                    base_id,
                    publication.resolver_output_epoch,
                    &expires_at,
                    &now,
                )
                .map_err(|error| format!("resolution_failed: {error}"))?;
            Ok(ResolvePinGuard::armed(
                fenced_factory,
                pin_id.to_string(),
                publication.request_id.clone(),
            ))
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn release_rebase_pin_after_error(
    layout: &StoreLayout,
    factory: &StoreConnectionFactory,
    coordinator: &mut StoreCoordinator,
    holder: &LeaseHolder,
    request: &CoordinatorRequest,
    heartbeat: &ResolveHeartbeat,
    deadline: i64,
    pin_id: &str,
) -> Result<(), String> {
    with_writer_lease(
        layout,
        coordinator,
        holder,
        deadline,
        |_coordinator, fencing_token| {
            heartbeat.ensure_live()?;
            ensure_resolve_claim(layout, request, holder)?;
            ResolutionBindingStore::new(fenced_factory(factory, layout, holder, fencing_token))
                .release_pin(pin_id, ResolutionPinOwnerKind::Resolve, &request.request_id)
                .map(|_| ())
                .map_err(|error| format!("resolution_failed: {error}"))
        },
    )
}

fn rebase_pin_id(request_id: &str) -> String {
    format!("resolve-rebase-{request_id}")
}

fn promote_exact_scratch_for_rebase(
    exact_path: &Path,
    build: &ResolutionBaseBuild,
) -> Result<(), String> {
    fs::rename(exact_path, &build.scratch_path).map_err(|error| {
        format!(
            "resolution_failed: failed to promote exact scratch {} to {}: {error}",
            exact_path.display(),
            build.scratch_path.display()
        )
    })?;
    #[cfg(feature = "test-store-resolution-contract")]
    julie_extract_artifact::store::test_hooks::crash_if("resolution_rebase_after_scratch_promote");
    Ok(())
}

struct ResolvePinGuard {
    factory: StoreConnectionFactory,
    pin_id: String,
    owner_id: String,
    armed: bool,
}

impl ResolvePinGuard {
    fn armed(factory: StoreConnectionFactory, pin_id: String, owner_id: String) -> Self {
        Self {
            factory,
            pin_id,
            owner_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ResolvePinGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.armed = false;
        let _ = ResolutionBindingStore::new(self.factory.clone()).release_pin(
            &self.pin_id,
            ResolutionPinOwnerKind::Resolve,
            &self.owner_id,
        );
    }
}

fn fenced_factory(
    factory: &StoreConnectionFactory,
    layout: &StoreLayout,
    holder: &LeaseHolder,
    fencing_token: i64,
) -> StoreConnectionFactory {
    factory
        .clone()
        .with_generation_fence(GenerationFence::writer(
            layout,
            holder.holder_id.clone(),
            holder.holder_pid,
            fencing_token,
            now_millis(),
        ))
}

/// How often a held writer lease is renewed while its operation runs. Well inside the lease TTL so a
/// scheduling delay costs a renewal, not the lease.
const WRITER_LEASE_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(1_000);

/// Keeps a held writer lease alive for as long as its operation is still running.
///
/// The lease TTL is a liveness backstop for a holder that DIED, not a budget for how long honest work
/// may take. `with_writer_lease` renewed once on acquire and then ran the whole operation with no
/// further renewal, so any step that outran the TTL expired its OWN lease and failed the resolve with
/// `writer lease was lost` / `fence lost`. That is not a rare race: it is certain for a large enough
/// repository, and it reproduces on a busy machine, which is exactly when a resolve is slowest.
///
/// A live holder now renews while it works. A dead one still loses the lease on the TTL, and
/// dead-process takeover is unchanged, so the fencing guarantee is intact.
struct WriterLeaseHeartbeat {
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl WriterLeaseHeartbeat {
    fn start(layout: StoreLayout, holder: LeaseHolder, fencing_token: i64) -> Self {
        let (stop, receiver) = mpsc::channel();
        let coordinator_db = layout.coordinator_db().to_path_buf();
        let worker = thread::spawn(move || {
            loop {
                match receiver.recv_timeout(WRITER_LEASE_HEARTBEAT_INTERVAL) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                match renew_writer_lease_with_retry(&coordinator_db, &holder, fencing_token) {
                    Ok(true) => {}
                    Ok(false) => return,
                    Err(_) => {}
                }
            }
        });
        Self {
            stop,
            worker: Some(worker),
        }
    }

    fn stop(mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for WriterLeaseHeartbeat {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn with_writer_lease<T>(
    layout: &StoreLayout,
    coordinator: &mut StoreCoordinator,
    holder: &LeaseHolder,
    deadline: i64,
    operation: impl FnOnce(&mut StoreCoordinator, i64) -> Result<T, String>,
) -> Result<T, String> {
    let fencing_token = loop {
        match coordinator
            .try_acquire_or_takeover_now(holder.clone())
            .map_err(|error| error.to_string())?
        {
            LeaseDisposition::Acquired { fencing_token } => break fencing_token,
            LeaseDisposition::HeldByOther if now_millis() < deadline => {
                thread::sleep(Duration::from_millis(25));
            }
            LeaseDisposition::HeldByOther => return Err("request_timeout".to_string()),
        }
    };
    // Renew immediately after acquire so the operation starts with a full lease TTL.
    if let Err(error) = exact_publish_heartbeat(coordinator, holder, fencing_token, "writer-lease")
    {
        let _ = coordinator.release_lease(holder, fencing_token);
        return Err(format!("resolution_failed: {error}"));
    }
    let heartbeat = WriterLeaseHeartbeat::start(layout.clone(), holder.clone(), fencing_token);
    let result = operation(coordinator, fencing_token);
    // Stop renewing BEFORE the release so a renewal cannot race it.
    heartbeat.stop();
    let release = coordinator
        .release_lease(holder, fencing_token)
        .map_err(|error| error.to_string());
    match (result, release) {
        (Ok(value), Ok(true)) => Ok(value),
        (Ok(_), Ok(false)) => Err("resolution_failed: writer lease was lost".to_string()),
        (Ok(_), Err(error)) | (Err(error), _) => Err(error),
    }
}

fn exact_publish_heartbeat(
    coordinator: &mut StoreCoordinator,
    holder: &LeaseHolder,
    fencing_token: i64,
    request_id: &str,
) -> Result<(), julie_extract_artifact::store::ResolutionBindingError> {
    use julie_extract_artifact::store::ResolutionBindingError;
    let now = now_millis();
    let alive = coordinator
        .heartbeat_lease(holder, fencing_token, now)
        .map_err(|error| ResolutionBindingError::InvalidPublication {
            detail: error.to_string(),
        })?;
    if !alive {
        return Err(ResolutionBindingError::FenceLost {
            request_id: request_id.to_string(),
        });
    }
    let Some(record) =
        coordinator
            .lease()
            .map_err(|error| ResolutionBindingError::InvalidPublication {
                detail: error.to_string(),
            })?
    else {
        return Err(ResolutionBindingError::FenceLost {
            request_id: request_id.to_string(),
        });
    };
    if record.holder.holder_id != holder.holder_id
        || record.holder.holder_pid != holder.holder_pid
        || record.fencing_token != fencing_token
        || record.expires_at <= now_millis()
    {
        return Err(ResolutionBindingError::FenceLost {
            request_id: request_id.to_string(),
        });
    }
    Ok(())
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
        .try_acquire_or_takeover_now(holder.clone())
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

#[allow(clippy::too_many_arguments)]
fn append_resolution_terminal(
    factory: &StoreConnectionFactory,
    layout: &StoreLayout,
    holder: &LeaseHolder,
    fencing_token: i64,
    request: &CoordinatorRequest,
    binding: &ResolutionViewBinding,
    telemetry: &ResolutionExecutionTelemetry,
) -> Result<(), String> {
    let mut connection = fenced_factory(factory, layout, holder, fencing_token)
        .open_writer()
        .map_err(|error| format!("resolution_failed: {error}"))?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    if StoreLog::committed_in_fact(&transaction, &request.request_id)
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(());
    }
    let delta = transaction
        .query_row(
            "SELECT identifier_replacements,pending_replacements,pending_tombstones,
                    exact_gap_rows,exact_gap_files
             FROM resolution_deltas
             WHERE view_id=?1 AND delta_generation=?2",
            params![binding.view_id, binding.delta_generation],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .map_err(|error| error.to_string())?;
    let result_json = serde_json::json!({
        "base_id": binding.base_id,
        "delta_generation": binding.delta_generation,
        "exact_at_generation": binding.exact_at,
        "exact_gap_files": delta.4,
        "exact_gap_rows": delta.3,
        "gap_lower_bound": delta.3,
        "identifier_replacements": delta.0,
        "manifest_generation": binding.manifest_generation,
        "manifest_hash": binding.manifest_hash,
        "pending_replacements": delta.1,
        "pending_tombstones": delta.2,
        "resolution_state": binding.state.as_str(),
        "resolution_mode": telemetry.resolution_mode,
        "scope_file_count": telemetry.scope_file_count,
        "scope_name_count": telemetry.scope_name_count,
        "scope_row_count": telemetry.scope_row_count,
        "fallback_reason": telemetry.fallback_reason,
        "phase_timings_ms": telemetry.phase_timings_ms,
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

fn resolution_delta_enabled() -> Result<bool, String> {
    match std::env::var("JULIE_STORE_RESOLUTION_DELTA") {
        Ok(value) if value == "on" => Ok(true),
        Ok(value) if value == "off" => Ok(false),
        Ok(value) => Err(format!(
            "resolution_failed: JULIE_STORE_RESOLUTION_DELTA must be 'on' or 'off', found '{value}'"
        )),
        Err(std::env::VarError::NotPresent) => Ok(true),
        Err(std::env::VarError::NotUnicode(_)) => Err(
            "resolution_failed: JULIE_STORE_RESOLUTION_DELTA must be valid UTF-8 'on' or 'off'"
                .to_string(),
        ),
    }
}

fn apply_scope_counts(
    factory: &StoreConnectionFactory,
    identity: &StoreManifestIdentity,
    worklists: &ResolutionWorklists,
    telemetry: &mut ResolutionExecutionTelemetry,
) -> Result<(), String> {
    let connection = factory
        .open_reader()
        .map_err(|error| format!("resolution_failed: {error}"))?;
    telemetry.scope_name_count = u64::try_from(worklists.recheck_names.len())
        .map_err(|_| "resolution_failed: scope name count overflow".to_string())?;
    if worklists.effective_full {
        let file_count = connection
            .query_row(
                "SELECT COUNT(*) FROM manifest_entries
                 WHERE view_id=?1 AND generation=?2
                   AND status IN ('indexed','failed_preserved')",
                params![identity.view_id, identity.generation],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| format!("resolution_failed: {error}"))?;
        telemetry.scope_file_count = u64::try_from(file_count)
            .map_err(|_| "resolution_failed: scope file count invalid".to_string())?;
        for table in ["identifiers", "pending_relationships", "relationships"] {
            let count = connection
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM {table} AS row
                         WHERE EXISTS(
                           SELECT 1 FROM manifest_entries AS entry
                           WHERE entry.view_id=?1 AND entry.generation=?2
                             AND entry.status IN ('indexed','failed_preserved')
                             AND entry.version_id=row.version_id
                         )"
                    ),
                    params![identity.view_id, identity.generation],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|error| format!("resolution_failed: {error}"))?;
            telemetry.scope_row_count = telemetry.scope_row_count.saturating_add(
                u64::try_from(count)
                    .map_err(|_| "resolution_failed: scope row count invalid".to_string())?,
            );
        }
        return Ok(());
    }

    let versions = worklists
        .selected_versions
        .iter()
        .filter_map(|version| match version {
            SemanticVersionId::Store(version_id) => Some(*version_id),
            SemanticVersionId::LegacyFile(_) => None,
        })
        .collect::<Vec<_>>();
    telemetry.scope_file_count = u64::try_from(versions.len())
        .map_err(|_| "resolution_failed: scope file count overflow".to_string())?;
    let mut row_count = 0_u64;
    for chunk in versions.chunks(256) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        for table in ["identifiers", "pending_relationships", "relationships"] {
            let count = connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE version_id IN ({placeholders})"),
                    rusqlite::params_from_iter(chunk),
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|error| format!("resolution_failed: {error}"))?;
            row_count = row_count.saturating_add(
                u64::try_from(count)
                    .map_err(|_| "resolution_failed: scope row count invalid".to_string())?,
            );
        }
    }
    telemetry.scope_row_count = row_count;
    Ok(())
}

fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
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
        report.resolution.resolution_mode =
            result["resolution_mode"].as_str().map(ToOwned::to_owned);
        report.resolution.scope_file_count = result["scope_file_count"].as_u64();
        report.resolution.scope_name_count = result["scope_name_count"].as_u64();
        report.resolution.scope_row_count = result["scope_row_count"].as_u64();
        report.resolution.fallback_reason =
            result["fallback_reason"].as_str().map(ToOwned::to_owned);
        report.resolution.phase_timings_ms = result["phase_timings_ms"].as_object().map(|values| {
            values
                .iter()
                .filter_map(|(name, value)| value.as_u64().map(|value| (name.clone(), value)))
                .collect()
        });
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

fn resolution_rebase_published(layout: &StoreLayout, request_id: &str) -> Result<bool, String> {
    Connection::open(layout.store_db())
        .map_err(|error| error.to_string())?
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM store_log
               WHERE request_id=?1 AND event_kind='resolution_exact_rebased'
             )",
            [request_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
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

fn remove_resolution_scratch_if_exists(exact_path: &std::path::Path) -> Result<(), String> {
    remove_sqlite_if_exists(exact_path)?;
    let mut work_path = exact_path.as_os_str().to_os_string();
    work_path.push(".work");
    let work_path = std::path::PathBuf::from(work_path);
    remove_sqlite_if_exists(&work_path)
}

fn remove_resolution_request_scratch(layout: &StoreLayout, request_id: &str) -> Result<(), String> {
    let exact_path = layout
        .scratch_dir()
        .join(format!("resolve-exact-{request_id}.db"));
    let delta_path = layout
        .scratch_dir()
        .join(format!("resolve-delta-{request_id}.db"));
    remove_resolution_scratch_if_exists(&exact_path)?;
    remove_sqlite_if_exists(&delta_path)
}

fn remove_sqlite_if_exists(path: &std::path::Path) -> Result<(), String> {
    for suffix in ["-wal", "-shm", ""] {
        let path = std::path::PathBuf::from(format!("{}{}", path.display(), suffix));
        remove_if_exists(&path)?;
    }
    Ok(())
}

#[cfg(feature = "test-store-resolution-contract")]
fn pause_after_claim_for_test() -> Result<(), String> {
    pause_on_env_file("JULIE_EXTRACT_STORE_RESOLUTION_PAUSE_FILE", b"claimed")
}

#[cfg(feature = "test-store-resolution-contract")]
fn pause_before_exact_publish_for_test() -> Result<(), String> {
    pause_on_env_file(
        "JULIE_EXTRACT_STORE_RESOLUTION_PAUSE_BEFORE_EXACT_FILE",
        b"exact",
    )
}

#[cfg(feature = "test-store-resolution-contract")]
fn fail_before_exact_finalize_for_test() -> Result<(), String> {
    if std::env::var_os("JULIE_EXTRACT_STORE_RESOLUTION_FAIL_BEFORE_EXACT_FINALIZE").is_some() {
        return Err("resolution_failed: test hook before exact finalization".to_string());
    }
    Ok(())
}

#[cfg(feature = "test-store-resolution-contract")]
fn pause_after_exact_publish_for_test() -> Result<(), String> {
    pause_on_env_file(
        "JULIE_EXTRACT_STORE_RESOLUTION_PAUSE_AFTER_EXACT_FILE",
        b"exact",
    )
}

#[cfg(feature = "test-store-resolution-contract")]
fn fail_before_rebase_view_cas_for_test() -> Result<(), String> {
    if std::env::var("JULIE_EXTRACT_STORE_TEST_FAIL_AT").as_deref()
        == Ok("resolution_rebase_before_view_cas")
    {
        return Err("resolution_failed: test failure before rebase view CAS".to_string());
    }
    Ok(())
}

#[cfg(feature = "test-store-resolution-contract")]
fn pause_on_env_file(env_var: &str, ready_bytes: &[u8]) -> Result<(), String> {
    let Ok(ready_path) = std::env::var(env_var) else {
        return Ok(());
    };
    let ready_path = std::path::PathBuf::from(ready_path);
    fs::write(&ready_path, ready_bytes).map_err(|error| error.to_string())?;
    let resume_path = ready_path.with_extension("resume");
    while !resume_path.exists() {
        thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

struct ResolveHeartbeat {
    stop: mpsc::Sender<()>,
    current: Arc<AtomicBool>,
    lost_reason: Arc<Mutex<Option<String>>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl ResolveHeartbeat {
    /// Starts the claim heartbeat.
    ///
    /// A coordinator ERROR is not evidence that the claim was taken. Every heartbeat opens a fresh
    /// coordinator connection, so a busy database — the normal state under concurrent store work —
    /// surfaces here as an error, and treating the first one as fatal failed honest resolves with
    /// `resolve claim lost` while the claim was still held. Errors are therefore retried.
    ///
    /// They cannot be retried forever: a claim nobody refreshes really does go stale, and another
    /// resolver may take it. So a run of errors that outlasts the staleness window IS a loss. Only
    /// `Ok(false)` — the claim row is no longer owned by this resolver — is an immediate one.
    fn start(layout: StoreLayout, request_id: String, owner_id: String) -> Self {
        let (stop, receiver) = mpsc::channel();
        let current = Arc::new(AtomicBool::new(true));
        let lost_reason: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let worker_current = current.clone();
        let worker_reason = lost_reason.clone();
        let worker = thread::spawn(move || {
            let lose = |reason: String| {
                if let Ok(mut slot) = worker_reason.lock() {
                    *slot = Some(reason);
                }
                worker_current.store(false, Ordering::Release);
            };
            let coordinator = match StoreCoordinator::open(&layout) {
                Ok(coordinator) => coordinator,
                Err(error) => {
                    lose(format!("the coordinator could not be opened: {error}"));
                    return;
                }
            };
            let mut failing_since: Option<i64> = None;
            loop {
                match receiver.recv_timeout(Duration::from_millis(250)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                match coordinator.heartbeat_resolve(&request_id, &owner_id, now_millis()) {
                    Ok(true) => failing_since = None,
                    Ok(false) => {
                        lose("the claim row is no longer owned by this resolver".to_string());
                        return;
                    }
                    Err(error) => {
                        let now = now_millis();
                        let since = *failing_since.get_or_insert(now);
                        if now.saturating_sub(since) >= RESOLVE_CLAIM_STALE_MS {
                            lose(format!(
                                "the claim could not be refreshed for {RESOLVE_CLAIM_STALE_MS}ms: {error}"
                            ));
                            return;
                        }
                    }
                }
            }
        });
        Self {
            stop,
            current,
            lost_reason,
            worker: Some(worker),
        }
    }

    /// The recorded reason the claim was lost, for an error a person can act on.
    fn lost_message(&self) -> String {
        match self.lost_reason.lock() {
            Ok(slot) => match slot.as_deref() {
                Some(reason) => format!("resolution_failed: resolve claim lost — {reason}"),
                None => "resolution_failed: resolve claim lost".to_string(),
            },
            Err(_) => "resolution_failed: resolve claim lost".to_string(),
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
            return Err(self.lost_message());
        }
        Ok(())
    }

    fn ensure_live(&self) -> Result<(), String> {
        if self.current.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(self.lost_message())
        }
    }

    /// Stops the heartbeat and reports whether the claim survived, with the reason if it did not.
    fn stop(mut self) -> Result<(), String> {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if self.current.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(self.lost_message())
        }
    }
}
