use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::store::{
    CapacityProvider, GenerationError, GenerationLifecycle, GenerationPolicy, MaintenanceAction,
    MaintenanceClock, MaintenanceError, MaintenanceExecutor, MaintenanceInspector, MaintenancePlan,
    MaintenanceRun, StoreConnectionError, StoreConnectionFactory, StoreCoordinator, StoreLayout,
    StoreLayoutError, plan_view_retirement,
};

use super::args::{
    StoreMaintainArgs, StoreMaintenanceCommand, StoreMaintenanceInspectArgs,
    StoreMaintenanceMutationArgs, StoreMaintenanceRetireViewArgs,
};
use super::import::{ExistingStoreContext, StoreExecutionOutcome, open_existing_store};
use super::maintenance_report::{
    StoreMaintenanceAction, StoreMaintenanceCommandOutcome, StoreMaintenanceFailureClass,
    StoreMaintenanceMode, StoreMaintenanceReport,
};
use super::report::StoreOutputFormat;

pub(crate) fn run(args: StoreMaintainArgs) -> StoreExecutionOutcome {
    match args.command {
        StoreMaintenanceCommand::Inspect(args) => inspect(args),
        StoreMaintenanceCommand::Gc(args) => plan_mutation(StoreMaintenanceAction::Gc, args),
        StoreMaintenanceCommand::Repair(args) => {
            plan_mutation(StoreMaintenanceAction::Repair, args)
        }
        StoreMaintenanceCommand::Promote(args) => {
            plan_mutation(StoreMaintenanceAction::Promote, args)
        }
        StoreMaintenanceCommand::RetireView(args) => retire_view(args),
        StoreMaintenanceCommand::Cursor(args) => cursor(args),
    }
}

fn retire_view(args: StoreMaintenanceRetireViewArgs) -> StoreExecutionOutcome {
    let action = StoreMaintenanceAction::RetireView;
    let format = output_format(args.json);
    let mode = if args.apply {
        StoreMaintenanceMode::Apply
    } else {
        StoreMaintenanceMode::Plan
    };
    let context = match inspect_context(&args.store, args.family.as_deref(), action, mode) {
        Ok(context) => context,
        Err(report) => return failure(*report, format),
    };
    let planned = match plan_view_retirement(&context.factory, &args.view) {
        Ok(planned) => planned,
        Err(error) => {
            return failure(
                maintenance_error_report(action, &context.plan, mode, &error),
                format,
            );
        }
    };
    let report =
        StoreMaintenanceReport::planned(action, &context.plan).with_view_retirement_plan(&planned);
    if !args.apply {
        return success(report, format);
    }
    pause_after_plan_if_requested();
    let run = maintenance_run();
    let run_id = run.run_id.clone();
    let mut executor =
        match MaintenanceExecutor::acquire(context.factory, run, &context.plan, CliCapacity) {
            Ok(executor) => executor,
            Err(error) => {
                return failure(
                    maintenance_error_report(action, &context.plan, mode, &error),
                    format,
                );
            }
        };
    match executor.retire_view(&context.plan, &args.view) {
        Ok(applied) => success(report.with_view_retirement(run_id, &applied), format),
        Err(error) => failure(
            maintenance_error_report(action, &context.plan, mode, &error),
            format,
        ),
    }
}

fn inspect(args: StoreMaintenanceInspectArgs) -> StoreExecutionOutcome {
    let format = output_format(args.json);
    match inspect_store(
        &args.store,
        args.family.as_deref(),
        StoreMaintenanceAction::Inspect,
    ) {
        Ok(report) => success(report, format),
        Err(report) => failure(*report, format),
    }
}

fn plan_mutation(
    action: StoreMaintenanceAction,
    args: StoreMaintenanceMutationArgs,
) -> StoreExecutionOutcome {
    let format = output_format(args.json);
    let mode = if args.apply {
        StoreMaintenanceMode::Apply
    } else {
        StoreMaintenanceMode::Plan
    };
    match inspect_context(&args.store, args.family.as_deref(), action, mode) {
        Ok(context) if !args.apply => success(
            StoreMaintenanceReport::planned(action, &context.plan),
            format,
        ),
        Ok(context) if action == StoreMaintenanceAction::Gc => {
            pause_after_plan_if_requested();
            apply_gc(context, format)
        }
        Ok(context) if action == StoreMaintenanceAction::Promote => {
            pause_after_plan_if_requested();
            apply_promotion(context, format)
        }
        Ok(context) if action == StoreMaintenanceAction::Repair => {
            pause_after_plan_if_requested();
            apply_repair(context, format)
        }
        Ok(context) => failure(
            StoreMaintenanceReport::failed(
                action,
                StoreMaintenanceMode::Apply,
                context.existing.family_id,
                context.existing.layout.generation_name().to_string(),
                StoreMaintenanceFailureClass::Internal,
                "maintenance_apply_unavailable",
                "maintenance apply is unavailable",
            ),
            format,
        ),
        Err(report) => failure(*report, format),
    }
}

fn apply_repair(context: MaintenanceContext, format: StoreOutputFormat) -> StoreExecutionOutcome {
    let run = maintenance_run();
    let run_id = run.run_id.clone();
    let mut lifecycle = match GenerationLifecycle::acquire(
        context.factory,
        run,
        &context.plan,
        MaintenanceAction::Repair,
        CliCapacity,
    ) {
        Ok(lifecycle) => lifecycle,
        Err(error) => {
            return failure(
                generation_error_report_from_plan(
                    StoreMaintenanceAction::Repair,
                    &context.plan,
                    &error,
                ),
                format,
            );
        }
    };
    match lifecycle.repair(&context.plan, &GenerationPolicy::default()) {
        Ok(applied) => success(
            StoreMaintenanceReport::planned(StoreMaintenanceAction::Repair, &context.plan)
                .with_generation_apply(run_id, &applied),
            format,
        ),
        Err(error) => failure(
            generation_error_report_from_plan(
                StoreMaintenanceAction::Repair,
                &context.plan,
                &error,
            ),
            format,
        ),
    }
}

fn apply_promotion(
    context: MaintenanceContext,
    format: StoreOutputFormat,
) -> StoreExecutionOutcome {
    let run = maintenance_run();
    let run_id = run.run_id.clone();
    let mut lifecycle = match GenerationLifecycle::acquire(
        context.factory,
        run,
        &context.plan,
        MaintenanceAction::Promote,
        CliCapacity,
    ) {
        Ok(lifecycle) => lifecycle,
        Err(error) => {
            return failure(
                generation_error_report_from_plan(
                    StoreMaintenanceAction::Promote,
                    &context.plan,
                    &error,
                ),
                format,
            );
        }
    };
    let applied = lifecycle.promote(&context.plan, &GenerationPolicy::default());
    match applied {
        Ok(applied) => success(
            StoreMaintenanceReport::planned(StoreMaintenanceAction::Promote, &context.plan)
                .with_generation_apply(run_id, &applied),
            format,
        ),
        Err(error) => failure(
            generation_error_report_from_plan(
                StoreMaintenanceAction::Promote,
                &context.plan,
                &error,
            ),
            format,
        ),
    }
}

fn apply_gc(context: MaintenanceContext, format: StoreOutputFormat) -> StoreExecutionOutcome {
    let run = maintenance_run();
    let run_id = run.run_id.clone();
    let mut executor =
        match MaintenanceExecutor::acquire(context.factory, run, &context.plan, CliCapacity) {
            Ok(executor) => executor,
            Err(error) => {
                return failure(
                    maintenance_error_report_from_plan(
                        StoreMaintenanceAction::Gc,
                        &context.plan,
                        &error,
                    ),
                    format,
                );
            }
        };
    match executor.apply(&context.plan) {
        Ok(applied) => success(
            StoreMaintenanceReport::planned(StoreMaintenanceAction::Gc, &context.plan)
                .with_gc_apply(run_id, &applied),
            format,
        ),
        Err(error) => failure(
            maintenance_error_report_from_plan(StoreMaintenanceAction::Gc, &context.plan, &error),
            format,
        ),
    }
}

fn cursor(args: super::args::StoreMaintenanceCursorArgs) -> StoreExecutionOutcome {
    match args.command {
        super::args::StoreMaintenanceCursorCommand::Advance(args) => advance_cursor(args),
        super::args::StoreMaintenanceCursorCommand::Release(args) => release_cursor(args),
    }
}

fn advance_cursor(args: super::args::StoreMaintenanceCursorAdvanceArgs) -> StoreExecutionOutcome {
    let format = output_format(args.json);
    let context = match inspect_context(
        &args.store,
        args.family.as_deref(),
        StoreMaintenanceAction::CursorAdvance,
        if args.apply {
            StoreMaintenanceMode::Apply
        } else {
            StoreMaintenanceMode::Plan
        },
    ) {
        Ok(context) => context,
        Err(report) => return failure(*report, format),
    };
    let planned =
        StoreMaintenanceReport::planned(StoreMaintenanceAction::CursorAdvance, &context.plan)
            .with_cursor(
                &args.consumer,
                Some(args.sequence),
                StoreMaintenanceMode::Plan,
                false,
            );
    if !args.apply {
        return success(planned, format);
    }
    let mut coordinator = match StoreCoordinator::open(&context.existing.layout) {
        Ok(coordinator) => coordinator,
        Err(error) => {
            return failure(
                coordinator_error_report_from_plan(
                    StoreMaintenanceAction::CursorAdvance,
                    &context.plan,
                    &error,
                ),
                format,
            );
        }
    };
    match coordinator.advance_consumer_cursor(
        &args.consumer,
        context.existing.layout.generation_name(),
        args.sequence,
        CliMaintenanceClock.now_ms(),
    ) {
        Ok(cursor) => success(
            planned.with_cursor(
                &cursor.consumer_id,
                Some(cursor.store_log_sequence),
                StoreMaintenanceMode::Apply,
                true,
            ),
            format,
        ),
        Err(error) => failure(
            coordinator_error_report_from_plan(
                StoreMaintenanceAction::CursorAdvance,
                &context.plan,
                &error,
            ),
            format,
        ),
    }
}

fn release_cursor(args: super::args::StoreMaintenanceCursorReleaseArgs) -> StoreExecutionOutcome {
    let format = output_format(args.json);
    let context = match inspect_context(
        &args.store,
        args.family.as_deref(),
        StoreMaintenanceAction::CursorRelease,
        if args.apply {
            StoreMaintenanceMode::Apply
        } else {
            StoreMaintenanceMode::Plan
        },
    ) {
        Ok(context) => context,
        Err(report) => return failure(*report, format),
    };
    let planned =
        StoreMaintenanceReport::planned(StoreMaintenanceAction::CursorRelease, &context.plan)
            .with_cursor(&args.consumer, None, StoreMaintenanceMode::Plan, false);
    if !args.apply {
        return success(planned, format);
    }
    let mut coordinator = match StoreCoordinator::open(&context.existing.layout) {
        Ok(coordinator) => coordinator,
        Err(error) => {
            return failure(
                coordinator_error_report_from_plan(
                    StoreMaintenanceAction::CursorRelease,
                    &context.plan,
                    &error,
                ),
                format,
            );
        }
    };
    match coordinator.release_consumer_cursor(&args.consumer) {
        Ok(removed) => success(
            planned.with_cursor(&args.consumer, None, StoreMaintenanceMode::Apply, removed),
            format,
        ),
        Err(error) => failure(
            coordinator_error_report_from_plan(
                StoreMaintenanceAction::CursorRelease,
                &context.plan,
                &error,
            ),
            format,
        ),
    }
}

fn inspect_store(
    store: &Path,
    requested_family: Option<&str>,
    action: StoreMaintenanceAction,
) -> Result<StoreMaintenanceReport, Box<StoreMaintenanceReport>> {
    inspect_context(store, requested_family, action, StoreMaintenanceMode::Plan)
        .map(|context| StoreMaintenanceReport::planned(action, &context.plan))
}

struct MaintenanceContext {
    existing: ExistingStoreContext,
    factory: StoreConnectionFactory,
    plan: MaintenancePlan,
}

fn inspect_context(
    store: &Path,
    requested_family: Option<&str>,
    action: StoreMaintenanceAction,
    mode: StoreMaintenanceMode,
) -> Result<MaintenanceContext, Box<StoreMaintenanceReport>> {
    if !store.join("CURRENT").exists() && store.exists() {
        match StoreLayout::open(store) {
            Err(StoreLayoutError::CurrentMissing { .. }) if has_named_generation(store) => {
                return Err(Box::new(layout_recovery_report(
                    store,
                    action,
                    mode,
                    requested_family,
                )));
            }
            Err(
                StoreLayoutError::CurrentRecoveryRequired { .. }
                | StoreLayoutError::PartialGenerationRecoveryRequired { .. },
            ) => {
                return Err(Box::new(layout_recovery_report(
                    store,
                    action,
                    mode,
                    requested_family,
                )));
            }
            _ => {}
        }
    }
    let existing = open_existing_store(store, requested_family).map_err(|message| {
        Box::new(StoreMaintenanceReport::failed(
            action,
            mode,
            requested_family.unwrap_or_default().to_string(),
            String::new(),
            classify_failure(&message),
            message.clone(),
            message,
        ))
    })?;
    let factory = StoreConnectionFactory::new(
        existing.layout.clone(),
        &existing.family_id,
        env!("CARGO_PKG_VERSION"),
    );
    let plan = MaintenanceInspector::new(factory.clone(), CliMaintenanceClock, CliCapacity)
        .inspect()
        .map_err(|error| {
            let code = maintenance_error_code(&error);
            Box::new(StoreMaintenanceReport::failed(
                action,
                mode,
                existing.family_id.clone(),
                existing.layout.generation_name().to_string(),
                maintenance_failure_class(&error),
                code,
                error.to_string(),
            ))
        })?;
    Ok(MaintenanceContext {
        existing,
        factory,
        plan,
    })
}

fn has_named_generation(store: &Path) -> bool {
    fs::read_dir(store).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_dir())
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("gen-") && !name.starts_with(".gen-"))
        })
    })
}

fn layout_recovery_report(
    store: &Path,
    action: StoreMaintenanceAction,
    mode: StoreMaintenanceMode,
    requested_family: Option<&str>,
) -> StoreMaintenanceReport {
    let (family_id, source_generation) = recovery_identity(store, requested_family);
    let (class, code, message) =
        if action == StoreMaintenanceAction::Repair && mode == StoreMaintenanceMode::Apply {
            (
                StoreMaintenanceFailureClass::RepairUnavailable,
                "repair_unavailable",
                "no generation can be selected for repair",
            )
        } else {
            (
                StoreMaintenanceFailureClass::RecoveryRequired,
                "store_recovery_required",
                "store recovery is required before maintenance",
            )
        };
    StoreMaintenanceReport::failed(
        action,
        mode,
        family_id,
        source_generation,
        class,
        code,
        message,
    )
}

fn recovery_identity(store: &Path, requested_family: Option<&str>) -> (String, String) {
    let mut generations: Vec<_> = fs::read_dir(store)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            (name.starts_with("gen-") && !name.starts_with(".gen-")).then_some((name, entry.path()))
        })
        .collect();
    generations.sort_by(|left, right| left.0.cmp(&right.0));
    if generations.len() != 1 {
        return (
            requested_family.unwrap_or_default().to_string(),
            String::new(),
        );
    }
    let (generation, path) = &generations[0];
    let family_id = rusqlite::Connection::open_with_flags(
        path.join("store.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .and_then(|connection| {
        connection.query_row(
            "SELECT value FROM store_meta WHERE key='family_id'",
            [],
            |row| row.get::<_, String>(0),
        )
    })
    .unwrap_or_else(|_| requested_family.unwrap_or_default().to_string());
    (family_id, generation.clone())
}

fn maintenance_error_report_from_plan(
    action: StoreMaintenanceAction,
    plan: &MaintenancePlan,
    error: &MaintenanceError,
) -> StoreMaintenanceReport {
    maintenance_error_report(action, plan, StoreMaintenanceMode::Apply, error)
}

fn maintenance_error_report(
    action: StoreMaintenanceAction,
    plan: &MaintenancePlan,
    mode: StoreMaintenanceMode,
    error: &MaintenanceError,
) -> StoreMaintenanceReport {
    StoreMaintenanceReport::planned(action, plan).with_failure(
        mode,
        maintenance_failure_class(error),
        maintenance_error_code(error),
        error.to_string(),
    )
}

fn generation_error_report_from_plan(
    action: StoreMaintenanceAction,
    plan: &MaintenancePlan,
    error: &GenerationError,
) -> StoreMaintenanceReport {
    let code = generation_error_code(error);
    StoreMaintenanceReport::planned(action, plan).with_failure(
        StoreMaintenanceMode::Apply,
        generation_failure_class(error),
        code,
        error.to_string(),
    )
}

fn coordinator_error_report_from_plan(
    action: StoreMaintenanceAction,
    plan: &MaintenancePlan,
    error: &julie_extract_artifact::store::CoordinatorError,
) -> StoreMaintenanceReport {
    let code = coordinator_error_code(error);
    StoreMaintenanceReport::planned(action, plan).with_failure(
        StoreMaintenanceMode::Apply,
        classify_failure(code),
        code,
        error.to_string(),
    )
}

#[cfg(feature = "test-store-contract")]
fn pause_after_plan_if_requested() {
    let Ok(ready) = std::env::var("JULIE_EXTRACT_STORE_TEST_MAINTENANCE_PLAN_READY") else {
        return;
    };
    let Ok(proceed) = std::env::var("JULIE_EXTRACT_STORE_TEST_MAINTENANCE_PLAN_CONTINUE") else {
        return;
    };
    fs::write(&ready, b"ready").expect("maintenance plan marker should be writable");
    for _ in 0..500 {
        if Path::new(&proceed).exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("maintenance plan continuation marker was not created");
}

#[cfg(not(feature = "test-store-contract"))]
fn pause_after_plan_if_requested() {}

fn maintenance_run() -> MaintenanceRun {
    let now_ms = CliMaintenanceClock.now_ms();
    let owner_pid = std::process::id();
    let run_id = format!("maintenance-{owner_pid}-{now_ms}");
    MaintenanceRun::new(run_id.clone(), run_id, owner_pid, now_ms, 60_000)
}

fn success(report: StoreMaintenanceReport, format: StoreOutputFormat) -> StoreExecutionOutcome {
    render(StoreMaintenanceCommandOutcome::success(report), format)
}

fn failure(report: StoreMaintenanceReport, format: StoreOutputFormat) -> StoreExecutionOutcome {
    render(StoreMaintenanceCommandOutcome::failure(report), format)
}

fn render(
    outcome: StoreMaintenanceCommandOutcome,
    format: StoreOutputFormat,
) -> StoreExecutionOutcome {
    let rendered = outcome.render(format);
    let plan = outcome.output_plan(format == StoreOutputFormat::Json);
    StoreExecutionOutcome::rendered(outcome.exit_code(), rendered, plan.stream)
}

fn output_format(json: bool) -> StoreOutputFormat {
    if json {
        StoreOutputFormat::Json
    } else {
        StoreOutputFormat::Human
    }
}

fn classify_failure(code: &str) -> StoreMaintenanceFailureClass {
    match code {
        "maintenance_busy" | "maintenance_fence_lost" => StoreMaintenanceFailureClass::Busy,
        "maintenance_plan_stale" | "maintenance_inspection_raced" => {
            StoreMaintenanceFailureClass::StalePlan
        }
        "generation_destination_exists"
        | "generation_partial_owned"
        | "generation_identity_conflict" => StoreMaintenanceFailureClass::StalePlan,
        "capacity_insufficient" => StoreMaintenanceFailureClass::CapacityInsufficient,
        "store_schema_incompatible"
        | "store_reader_too_old"
        | "store_writer_too_old"
        | "store_family_mismatch"
        | "family_mismatch"
        | "store_epoch_incompatible"
        | "store_catalog_incompatible" => StoreMaintenanceFailureClass::IncompatibleStore,
        "store_recovery_required" | "current_missing" => {
            StoreMaintenanceFailureClass::RecoveryRequired
        }
        "generation_validation_failed" | "integrity_check_failed" => {
            StoreMaintenanceFailureClass::IntegrityFailed
        }
        "repair_unavailable" => StoreMaintenanceFailureClass::RepairUnavailable,
        "invalid_maintenance_policy"
        | "invalid_maintenance_metadata"
        | "generation_invalid_action"
        | "generation_invalid_policy"
        | "generation_invalid_name"
        | "generation_out_of_range" => StoreMaintenanceFailureClass::InvalidArguments,
        _ => StoreMaintenanceFailureClass::Internal,
    }
}

fn maintenance_failure_class(error: &MaintenanceError) -> StoreMaintenanceFailureClass {
    match error {
        MaintenanceError::MaintenanceBusy | MaintenanceError::MaintenanceFenceLost => {
            StoreMaintenanceFailureClass::Busy
        }
        MaintenanceError::StalePlan | MaintenanceError::InspectionRaced { .. } => {
            StoreMaintenanceFailureClass::StalePlan
        }
        MaintenanceError::CapacityInsufficient => {
            StoreMaintenanceFailureClass::CapacityInsufficient
        }
        MaintenanceError::Connection(error) => store_connection_failure_class(error),
        MaintenanceError::UnknownRoot { .. } | MaintenanceError::InvalidMetadata { .. } => {
            StoreMaintenanceFailureClass::IntegrityFailed
        }
        MaintenanceError::InvalidPolicy { .. } | MaintenanceError::ViewNotFound { .. } => {
            StoreMaintenanceFailureClass::InvalidArguments
        }
        MaintenanceError::Coordinator(_)
        | MaintenanceError::Log(_)
        | MaintenanceError::Sqlite(_)
        | MaintenanceError::Io(_)
        | MaintenanceError::Serialization(_) => StoreMaintenanceFailureClass::Internal,
    }
}

fn maintenance_error_code(error: &MaintenanceError) -> &'static str {
    match error {
        MaintenanceError::Connection(error) => store_connection_error_code(error),
        _ => error.code(),
    }
}

fn generation_failure_class(error: &GenerationError) -> StoreMaintenanceFailureClass {
    match error {
        GenerationError::Maintenance(error) => maintenance_failure_class(error),
        GenerationError::Connection(error) => store_connection_failure_class(error),
        GenerationError::Layout(
            StoreLayoutError::CurrentMissing { .. }
            | StoreLayoutError::CurrentRecoveryRequired { .. }
            | StoreLayoutError::PartialGenerationRecoveryRequired { .. },
        ) => StoreMaintenanceFailureClass::RecoveryRequired,
        GenerationError::Validation { .. }
        | GenerationError::InvalidBasePath(_)
        | GenerationError::BaseIdentityMismatch { .. } => {
            StoreMaintenanceFailureClass::IntegrityFailed
        }
        _ => classify_failure(error.code()),
    }
}

fn generation_error_code(error: &GenerationError) -> &'static str {
    match error {
        GenerationError::Connection(error) => store_connection_error_code(error),
        GenerationError::Layout(
            StoreLayoutError::CurrentMissing { .. }
            | StoreLayoutError::CurrentRecoveryRequired { .. }
            | StoreLayoutError::PartialGenerationRecoveryRequired { .. },
        ) => "store_recovery_required",
        _ => error.code(),
    }
}

fn store_connection_failure_class(error: &StoreConnectionError) -> StoreMaintenanceFailureClass {
    match error {
        StoreConnectionError::FamilyMismatch { .. }
        | StoreConnectionError::ReaderVersionTooOld { .. }
        | StoreConnectionError::WriterVersionTooOld { .. }
        | StoreConnectionError::MissingMetadata { .. }
        | StoreConnectionError::InvalidVersion { .. }
        | StoreConnectionError::PragmaMismatch { .. }
        | StoreConnectionError::TextPragmaMismatch { .. }
        | StoreConnectionError::Schema(_) => StoreMaintenanceFailureClass::IncompatibleStore,
        StoreConnectionError::Layout(
            StoreLayoutError::CurrentMissing { .. }
            | StoreLayoutError::CurrentRecoveryRequired { .. }
            | StoreLayoutError::PartialGenerationRecoveryRequired { .. },
        ) => StoreMaintenanceFailureClass::RecoveryRequired,
        StoreConnectionError::CurrentGenerationChanged { .. }
        | StoreConnectionError::GenerationNotServing { .. }
        | StoreConnectionError::MaintenanceInProgress { .. }
        | StoreConnectionError::WriterLeaseLost
        | StoreConnectionError::WriterLeaseUnavailable { .. } => StoreMaintenanceFailureClass::Busy,
        _ => StoreMaintenanceFailureClass::Internal,
    }
}

fn store_connection_error_code(error: &StoreConnectionError) -> &'static str {
    match error {
        StoreConnectionError::ReaderVersionTooOld { .. } => "store_reader_too_old",
        StoreConnectionError::WriterVersionTooOld { .. } => "store_writer_too_old",
        StoreConnectionError::FamilyMismatch { .. } => "store_family_mismatch",
        StoreConnectionError::MissingMetadata { .. }
        | StoreConnectionError::InvalidVersion { .. }
        | StoreConnectionError::PragmaMismatch { .. }
        | StoreConnectionError::TextPragmaMismatch { .. }
        | StoreConnectionError::Schema(_) => "store_schema_incompatible",
        StoreConnectionError::Layout(
            StoreLayoutError::CurrentMissing { .. }
            | StoreLayoutError::CurrentRecoveryRequired { .. }
            | StoreLayoutError::PartialGenerationRecoveryRequired { .. },
        ) => "store_recovery_required",
        _ => "store_connection_error",
    }
}

fn coordinator_error_code(error: &julie_extract_artifact::store::CoordinatorError) -> &'static str {
    use julie_extract_artifact::store::CoordinatorError;
    match error {
        CoordinatorError::InvalidRequest | CoordinatorError::InvalidTime { .. } => {
            "invalid_maintenance_metadata"
        }
        CoordinatorError::CursorRegression { .. } => "consumer_cursor_regression",
        CoordinatorError::CursorAhead { .. } => "consumer_cursor_ahead",
        CoordinatorError::CursorGenerationConflict { .. }
        | CoordinatorError::InvalidGeneration { .. } => "invalid_maintenance_metadata",
        CoordinatorError::WriterVersionTooOld { .. }
        | CoordinatorError::ReaderWriterFloorRequired => "store_writer_too_old",
        CoordinatorError::LeaseUnavailable
        | CoordinatorError::LeaseLost
        | CoordinatorError::ReaderAdmissionBusy => "maintenance_busy",
        CoordinatorError::StoreLog(error) => error.code(),
        CoordinatorError::StoreConnection(error) => store_connection_error_code(error),
        CoordinatorError::IdempotencyConflict { .. }
        | CoordinatorError::RequestIdConflict { .. }
        | CoordinatorError::RequestNotFound { .. }
        | CoordinatorError::CorruptRequest { .. }
        | CoordinatorError::CoordinatorAheadOfStore { .. }
        | CoordinatorError::InvalidVersion { .. }
        | CoordinatorError::MissingLeaseHolder
        | CoordinatorError::ExecutionFailed { .. }
        | CoordinatorError::InvalidPolicy
        | CoordinatorError::QuantumDeadlineExceeded { .. }
        | CoordinatorError::ReaderNotFound
        | CoordinatorError::ReaderOwnerMismatch
        | CoordinatorError::ReaderIdentityUnknown
        | CoordinatorError::ReaderStaleSnapshot
        | CoordinatorError::ReaderOperational
        | CoordinatorError::Sqlite(_) => "maintenance_coordinator_error",
    }
}

#[derive(Clone, Copy)]
struct CliMaintenanceClock;

impl MaintenanceClock for CliMaintenanceClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0)
    }
}

#[derive(Clone, Copy)]
struct CliCapacity;

impl CapacityProvider for CliCapacity {
    fn free_bytes(&self, path: &Path) -> Result<u64, io::Error> {
        filesystem_free_bytes(path)
    }

    fn staged_generation_bytes(&self, path: &Path) -> Result<u64, io::Error> {
        let generation = fs::read_to_string(path.join("CURRENT"))?;
        directory_bytes(&path.join(generation.trim()))
    }
}

pub(crate) fn filesystem_free_bytes(path: &Path) -> Result<u64, io::Error> {
    fs4::available_space(path)
}

fn directory_bytes(path: &Path) -> Result<u64, io::Error> {
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_dir() {
            total = total.saturating_add(directory_bytes(&entry.path())?);
        } else if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}
