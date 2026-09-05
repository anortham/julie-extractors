use std::ffi::{OsStr, OsString};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::error::ErrorKind;
use julie_extract_artifact::store::{
    CoordinatorError, MaintenanceError, MaintenanceExecutor, MaintenanceRun, ReaderAcquireRequest,
    ReaderRegistration, ReaderReleaseRequest, ReaderRenewRequest, ReaderReportFacts,
    StoreConnectionError, StoreConnectionFactory, StoreCoordinator,
};
use serde::Serialize;

use super::args::{
    StoreReaderAcquireArgs, StoreReaderArgs, StoreReaderCommand, StoreReaderReleaseArgs,
    StoreReaderRenewArgs,
};
use super::import::{StoreExecutionOutcome, open_existing_store};
use super::report::{
    STORE_EXIT_INCOMPATIBLE, STORE_EXIT_OPERATIONAL_FAILURE, STORE_EXIT_SUCCESS, STORE_EXIT_USAGE,
    StoreOutputStream,
};

const READER_REPORT_SCHEMA_VERSION: u32 = 1;

pub(crate) fn run(args: StoreReaderArgs) -> StoreExecutionOutcome {
    match args.command {
        StoreReaderCommand::Acquire(args) => acquire(args),
        StoreReaderCommand::Renew(args) => renew(args),
        StoreReaderCommand::Release(args) => release(args),
    }
}

pub(crate) fn parse_failure(
    raw_args: &[OsString],
    error: &clap::Error,
) -> Option<StoreExecutionOutcome> {
    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) || raw_args.get(1).map(OsString::as_os_str) != Some(OsStr::new("store"))
        || raw_args.get(2).map(OsString::as_os_str) != Some(OsStr::new("reader"))
    {
        return None;
    }
    let operation = match raw_args.get(3).map(OsString::as_os_str) {
        Some(value) if value == OsStr::new("acquire") => "reader_acquire",
        Some(value) if value == OsStr::new("renew") => "reader_renew",
        Some(value) if value == OsStr::new("release") => "reader_release",
        _ => "reader",
    };
    let json = raw_args
        .iter()
        .skip(3)
        .any(|value| value == OsStr::new("--json"));
    Some(render_failure(
        operation,
        ReaderFailure::invalid_arguments(),
        json,
    ))
}

fn acquire(args: StoreReaderAcquireArgs) -> StoreExecutionOutcome {
    let operation = "reader_acquire";
    let request = ReaderAcquireRequest::new(
        &args.family,
        &args.view,
        &args.generation,
        &args.owner,
        args.owner_pid.get(),
        &args.nonce,
        args.lease_ms,
    );
    let existing = match open_existing_store(&args.store, Some(&args.family)) {
        Ok(existing) => existing,
        Err(code) => return render_failure(operation, classify_open_error(&code), args.json),
    };
    let factory = StoreConnectionFactory::new(
        existing.layout.clone(),
        &existing.family_id,
        env!("CARGO_PKG_VERSION"),
    );
    let registration = match acquire_registration(&existing.layout, factory, &request) {
        Ok(registration) => registration,
        Err(failure) => return render_failure(operation, failure, args.json),
    };
    render_registration(operation, "acquired", &args.nonce, &registration, args.json)
}

fn acquire_registration(
    layout: &julie_extract_artifact::store::StoreLayout,
    factory: StoreConnectionFactory,
    request: &ReaderAcquireRequest,
) -> Result<ReaderRegistration, ReaderFailure> {
    let mut coordinator = StoreCoordinator::open(layout).map_err(classify_coordinator_error)?;
    match coordinator.acquire_reader(request) {
        Ok(result) => Ok(result.into_registration()),
        Err(CoordinatorError::ReaderWriterFloorRequired) => {
            drop(coordinator);
            MaintenanceExecutor::activate_reader_writer_floor(factory, maintenance_run())
                .map_err(classify_maintenance_error)?;
            let mut coordinator =
                StoreCoordinator::open(layout).map_err(classify_coordinator_error)?;
            coordinator
                .acquire_reader(request)
                .map(|result| result.into_registration())
                .map_err(classify_coordinator_error)
        }
        Err(error) => Err(classify_coordinator_error(error)),
    }
}

fn renew(args: StoreReaderRenewArgs) -> StoreExecutionOutcome {
    let operation = "reader_renew";
    let request = ReaderRenewRequest::new(
        &args.family,
        &args.pin,
        &args.nonce,
        args.owner_pid.get(),
        args.lease_ms,
    );
    let existing = match open_existing_store(&args.store, Some(&args.family)) {
        Ok(existing) => existing,
        Err(code) => return render_failure(operation, classify_open_error(&code), args.json),
    };
    let mut coordinator = match StoreCoordinator::open(&existing.layout) {
        Ok(coordinator) => coordinator,
        Err(error) => {
            return render_failure(operation, classify_coordinator_error(error), args.json);
        }
    };
    match coordinator.renew_reader(&request) {
        Ok(registration) => {
            render_registration(operation, "renewed", &args.nonce, &registration, args.json)
        }
        Err(error) => render_failure(operation, classify_coordinator_error(error), args.json),
    }
}

fn release(args: StoreReaderReleaseArgs) -> StoreExecutionOutcome {
    let operation = "reader_release";
    let request = ReaderReleaseRequest::new(&args.family, &args.pin, &args.nonce);
    let existing = match open_existing_store(&args.store, Some(&args.family)) {
        Ok(existing) => existing,
        Err(code) => return render_failure(operation, classify_open_error(&code), args.json),
    };
    let mut coordinator = match StoreCoordinator::open(&existing.layout) {
        Ok(coordinator) => coordinator,
        Err(error) => {
            return render_failure(operation, classify_coordinator_error(error), args.json);
        }
    };
    match coordinator.release_reader(&request) {
        Ok(released) => {
            let report = ReaderReport::released(args.family, args.pin, released);
            render_success(report, args.json)
        }
        Err(error) => render_failure(operation, classify_coordinator_error(error), args.json),
    }
}

fn render_registration(
    operation: &'static str,
    state: &'static str,
    presented_nonce: &str,
    registration: &ReaderRegistration,
    json: bool,
) -> StoreExecutionOutcome {
    let facts = ReaderReportFacts::from_registration(registration, None);
    render_success(
        ReaderReport::registration(operation, state, presented_nonce, &facts),
        json,
    )
}

fn render_success(report: ReaderReport, json: bool) -> StoreExecutionOutcome {
    let rendered = if json {
        report.render_json()
    } else {
        report.render_human()
    };
    StoreExecutionOutcome::rendered(STORE_EXIT_SUCCESS, rendered, StoreOutputStream::Stdout)
}

fn render_failure(
    operation: &'static str,
    failure: ReaderFailure,
    json: bool,
) -> StoreExecutionOutcome {
    let report = ReaderReport::failure(operation, failure.class, failure.error);
    let rendered = if json {
        report.render_json()
    } else {
        report.render_human()
    };
    let stream = if json {
        StoreOutputStream::Stdout
    } else {
        StoreOutputStream::Stderr
    };
    StoreExecutionOutcome::rendered(failure.exit_code, rendered, stream)
}

fn maintenance_run() -> MaintenanceRun {
    let now = now_ms();
    let pid = std::process::id();
    let run_id = format!("reader-floor-{pid}-{now}");
    MaintenanceRun::new(run_id.clone(), run_id, pid, now, 60_000)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn classify_open_error(code: &str) -> ReaderFailure {
    if code == "family_mismatch" {
        ReaderFailure::incompatible_store()
    } else {
        ReaderFailure::operational()
    }
}

fn classify_coordinator_error(error: CoordinatorError) -> ReaderFailure {
    match error {
        CoordinatorError::InvalidRequest
        | CoordinatorError::InvalidTime { .. }
        | CoordinatorError::InvalidGeneration { .. }
        | CoordinatorError::InvalidPolicy => ReaderFailure::invalid_arguments(),
        CoordinatorError::ReaderNotFound => ReaderFailure::reader_not_found(),
        CoordinatorError::ReaderOwnerMismatch => ReaderFailure::reader_owner_mismatch(),
        CoordinatorError::ReaderIdentityUnknown => ReaderFailure::reader_identity_unknown(),
        CoordinatorError::ReaderStaleSnapshot => ReaderFailure::stale_snapshot(),
        CoordinatorError::ReaderAdmissionBusy
        | CoordinatorError::LeaseUnavailable
        | CoordinatorError::LeaseLost => ReaderFailure::busy(),
        CoordinatorError::WriterVersionTooOld { .. } | CoordinatorError::InvalidVersion { .. } => {
            ReaderFailure::incompatible_store()
        }
        CoordinatorError::StoreConnection(error) => classify_connection_error(error),
        _ => ReaderFailure::operational(),
    }
}

fn classify_connection_error(error: StoreConnectionError) -> ReaderFailure {
    match error {
        StoreConnectionError::FamilyMismatch { .. }
        | StoreConnectionError::ReaderVersionTooOld { .. }
        | StoreConnectionError::WriterVersionTooOld { .. }
        | StoreConnectionError::MissingMetadata { .. }
        | StoreConnectionError::InvalidVersion { .. }
        | StoreConnectionError::PragmaMismatch { .. }
        | StoreConnectionError::TextPragmaMismatch { .. }
        | StoreConnectionError::Schema(_) => ReaderFailure::incompatible_store(),
        StoreConnectionError::CurrentGenerationChanged { .. }
        | StoreConnectionError::GenerationNotServing { .. }
        | StoreConnectionError::MaintenanceInProgress { .. }
        | StoreConnectionError::WriterLeaseLost
        | StoreConnectionError::WriterLeaseUnavailable { .. } => ReaderFailure::busy(),
        _ => ReaderFailure::operational(),
    }
}

fn classify_maintenance_error(error: MaintenanceError) -> ReaderFailure {
    match error {
        MaintenanceError::MaintenanceBusy | MaintenanceError::MaintenanceFenceLost => {
            ReaderFailure::busy()
        }
        MaintenanceError::StalePlan | MaintenanceError::InspectionRaced { .. } => {
            ReaderFailure::stale_snapshot()
        }
        MaintenanceError::CapacityInsufficient => ReaderFailure::capacity_insufficient(),
        MaintenanceError::Connection(error) => classify_connection_error(error),
        MaintenanceError::Coordinator(error) => classify_coordinator_error(error),
        MaintenanceError::UnknownRoot { .. } | MaintenanceError::InvalidMetadata { .. } => {
            ReaderFailure::incompatible_store()
        }
        MaintenanceError::InvalidPolicy { .. } | MaintenanceError::ViewNotFound { .. } => {
            ReaderFailure::invalid_arguments()
        }
        _ => ReaderFailure::operational(),
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReaderFailureClass {
    Busy,
    StaleSnapshot,
    InvalidArguments,
    IncompatibleStore,
    ReaderNotFound,
    ReaderOwnerMismatch,
    ReaderIdentityUnknown,
    CapacityInsufficient,
    Operational,
}

impl ReaderFailureClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::StaleSnapshot => "stale_snapshot",
            Self::InvalidArguments => "invalid_arguments",
            Self::IncompatibleStore => "incompatible_store",
            Self::ReaderNotFound => "reader_not_found",
            Self::ReaderOwnerMismatch => "reader_owner_mismatch",
            Self::ReaderIdentityUnknown => "reader_identity_unknown",
            Self::CapacityInsufficient => "capacity_insufficient",
            Self::Operational => "operational",
        }
    }
}

#[derive(Clone, Copy)]
struct ReaderFailure {
    class: ReaderFailureClass,
    error: &'static str,
    exit_code: u8,
}

impl ReaderFailure {
    fn busy() -> Self {
        Self::operational_class(ReaderFailureClass::Busy, "reader operation is busy")
    }

    fn stale_snapshot() -> Self {
        Self::operational_class(
            ReaderFailureClass::StaleSnapshot,
            "reader snapshot is stale",
        )
    }

    fn invalid_arguments() -> Self {
        Self {
            class: ReaderFailureClass::InvalidArguments,
            error: "reader arguments are invalid",
            exit_code: STORE_EXIT_USAGE,
        }
    }

    fn incompatible_store() -> Self {
        Self {
            class: ReaderFailureClass::IncompatibleStore,
            error: "store is incompatible with reader operations",
            exit_code: STORE_EXIT_INCOMPATIBLE,
        }
    }

    fn reader_not_found() -> Self {
        Self::operational_class(
            ReaderFailureClass::ReaderNotFound,
            "reader registration was not found",
        )
    }

    fn reader_owner_mismatch() -> Self {
        Self::operational_class(
            ReaderFailureClass::ReaderOwnerMismatch,
            "reader authentication failed",
        )
    }

    fn reader_identity_unknown() -> Self {
        Self::operational_class(
            ReaderFailureClass::ReaderIdentityUnknown,
            "reader process identity could not be verified",
        )
    }

    fn capacity_insufficient() -> Self {
        Self::operational_class(
            ReaderFailureClass::CapacityInsufficient,
            "reader floor activation lacks capacity",
        )
    }

    fn operational() -> Self {
        Self::operational_class(ReaderFailureClass::Operational, "reader operation failed")
    }

    fn operational_class(class: ReaderFailureClass, error: &'static str) -> Self {
        Self {
            class,
            error,
            exit_code: STORE_EXIT_OPERATIONAL_FAILURE,
        }
    }
}

#[derive(Serialize)]
struct ReaderReport {
    report_schema_version: u32,
    operation: &'static str,
    state: &'static str,
    family_id: Option<String>,
    view_id: Option<String>,
    pin_id: Option<String>,
    generation_name: Option<String>,
    manifest_generation: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_nonce: Option<String>,
    owner_pid: Option<u32>,
    store_instance_id: Option<String>,
    manifest_hash: Option<String>,
    extraction_identity_epoch: Option<i64>,
    served_store_log_sequence: Option<i64>,
    min_retained_store_log_sequence: Option<i64>,
    snapshot_fingerprint: Option<String>,
    protected_manifest_count: Option<usize>,
    expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    released: Option<bool>,
    warning: Option<String>,
    failure_class: Option<ReaderFailureClass>,
    error: Option<&'static str>,
}

impl ReaderReport {
    fn registration(
        operation: &'static str,
        state: &'static str,
        presented_nonce: &str,
        facts: &ReaderReportFacts,
    ) -> Self {
        let snapshot = facts.snapshot();
        Self {
            report_schema_version: READER_REPORT_SCHEMA_VERSION,
            operation,
            state,
            family_id: Some(snapshot.family_id().to_string()),
            view_id: Some(snapshot.view_id().to_string()),
            pin_id: Some(facts.pin_id().to_string()),
            generation_name: Some(snapshot.generation_name().to_string()),
            manifest_generation: Some(snapshot.manifest_generation()),
            owner_nonce: Some(presented_nonce.to_string()),
            owner_pid: Some(facts.owner_pid()),
            store_instance_id: Some(snapshot.store_instance_id().to_string()),
            manifest_hash: Some(snapshot.manifest_hash().to_string()),
            extraction_identity_epoch: Some(snapshot.extraction_identity_epoch()),
            served_store_log_sequence: Some(snapshot.served_store_log_sequence()),
            min_retained_store_log_sequence: Some(snapshot.min_retained_store_log_sequence()),
            snapshot_fingerprint: Some(snapshot.snapshot_fingerprint().to_string()),
            protected_manifest_count: Some(facts.protected_manifest_count()),
            expires_at: Some(facts.expires_at()),
            released: None,
            warning: facts.warning().map(str::to_string),
            failure_class: None,
            error: None,
        }
    }

    fn released(family_id: String, pin_id: String, released: bool) -> Self {
        Self {
            report_schema_version: READER_REPORT_SCHEMA_VERSION,
            operation: "reader_release",
            state: "released",
            family_id: Some(family_id),
            view_id: None,
            pin_id: Some(pin_id),
            generation_name: None,
            manifest_generation: None,
            owner_nonce: None,
            owner_pid: None,
            store_instance_id: None,
            manifest_hash: None,
            extraction_identity_epoch: None,
            served_store_log_sequence: None,
            min_retained_store_log_sequence: None,
            snapshot_fingerprint: None,
            protected_manifest_count: None,
            expires_at: None,
            released: Some(released),
            warning: None,
            failure_class: None,
            error: None,
        }
    }

    fn failure(
        operation: &'static str,
        failure_class: ReaderFailureClass,
        error: &'static str,
    ) -> Self {
        Self {
            report_schema_version: READER_REPORT_SCHEMA_VERSION,
            operation,
            state: "refused",
            family_id: None,
            view_id: None,
            pin_id: None,
            generation_name: None,
            manifest_generation: None,
            owner_nonce: None,
            owner_pid: None,
            store_instance_id: None,
            manifest_hash: None,
            extraction_identity_epoch: None,
            served_store_log_sequence: None,
            min_retained_store_log_sequence: None,
            snapshot_fingerprint: None,
            protected_manifest_count: None,
            expires_at: None,
            released: None,
            warning: None,
            failure_class: Some(failure_class),
            error: Some(error),
        }
    }

    fn render_json(&self) -> String {
        let mut rendered = serde_json::to_string(self).expect("reader report serializes");
        rendered.push('\n');
        rendered
    }

    fn render_human(&self) -> String {
        match self.state {
            "acquired" | "renewed" => format!(
                "{} family={} view={} generation={} pin={} expires_at={}\n",
                self.state,
                self.family_id.as_deref().unwrap_or_default(),
                self.view_id.as_deref().unwrap_or_default(),
                self.generation_name.as_deref().unwrap_or_default(),
                self.pin_id.as_deref().unwrap_or_default(),
                self.expires_at.unwrap_or_default(),
            ),
            "released" => format!(
                "released family={} pin={} released={}\n",
                self.family_id.as_deref().unwrap_or_default(),
                self.pin_id.as_deref().unwrap_or_default(),
                self.released.unwrap_or(false),
            ),
            _ => format!(
                "refused operation={} failure={} error={}\n",
                self.operation,
                self.failure_class
                    .map(ReaderFailureClass::as_str)
                    .unwrap_or("operational"),
                self.error
                    .unwrap_or("reader operation failed")
                    .replace(' ', "_"),
            ),
        }
    }
}
