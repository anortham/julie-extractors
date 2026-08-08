use std::io::{self, Write};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::store::{
    CoordinatorPolicy, CoordinatorRequest, LeaseHolder, RequestKind, RequestState,
    StoreCoordinator, StoreLayout,
};

use super::args::{StoreArgs, StoreCommand, StoreImportArgs, StoreLevelArg};
use super::executor::{DiscoveredImportFile, ImportRequestPayload, StoreRequestExecutor};
use super::report::{
    StoreCommandOutcome, StoreCoordinatorDisposition, StoreFailureClass, StoreLevelCompletion,
    StoreManifestDisposition, StoreOutputFormat, StoreOutputStream, StoreReport, StoreRequestState,
    StoreRequestedLevel,
};

pub struct StoreExecutionOutcome {
    outcome: StoreCommandOutcome,
    format: StoreOutputFormat,
}

impl StoreExecutionOutcome {
    pub fn exit_code(&self) -> u8 {
        self.outcome.exit_code()
    }

    pub fn write(&self) {
        let rendered = self.outcome.render(self.format);
        match self
            .outcome
            .output_plan(self.format == StoreOutputFormat::Json)
            .stream
        {
            StoreOutputStream::Stdout => {
                let _ = io::stdout().lock().write_all(rendered.as_bytes());
            }
            StoreOutputStream::Stderr => {
                let _ = io::stderr().lock().write_all(rendered.as_bytes());
            }
        }
    }
}

pub fn dispatch(args: StoreArgs) -> StoreExecutionOutcome {
    match args.command {
        StoreCommand::Import(args) => run_import(args),
    }
}

fn run_import(args: StoreImportArgs) -> StoreExecutionOutcome {
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
        Ok(report) => StoreExecutionOutcome {
            outcome: StoreCommandOutcome::queued(report),
            format,
        },
        Err(message) => {
            let report = base_report(
                &args,
                &request_id,
                &idempotency_key,
                StoreRequestState::Failed,
            )
            .with_failure(classify_failure(&message), message);
            StoreExecutionOutcome {
                outcome: StoreCommandOutcome::failed(report),
                format,
            }
        }
    }
}

fn execute_import(
    args: &StoreImportArgs,
    request_id: &str,
    idempotency_key: &str,
) -> Result<StoreReport, String> {
    let root = args
        .root
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let root_text = root.to_string_lossy().into_owned();
    let layout = StoreLayout::create(&args.store, &args.family, env!("CARGO_PKG_VERSION"))
        .map_err(|error| error.to_string())?;
    let now = now_millis();
    let payload = serde_json::to_string(&ImportRequestPayload {
        family_id: args.family.clone(),
        root: root_text.clone(),
        view_id: args.view.clone(),
    })
    .expect("import payload is serializable");
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
    coordinator
        .enqueue(request)
        .map_err(|error| error.to_string())?;
    let progress = args
        .scan
        .progress_file
        .as_deref()
        .map(|path| crate::progress::ScanProgress::create_for_artifact(path, layout.store_db()))
        .transpose()
        .map_err(|error| format!("{error:?}"))?
        .map(Arc::new);
    if let Some(progress) = progress.as_deref() {
        progress.enter_phase("discovery");
    }
    let discovery =
        crate::discovery::DiscoveryPolicy::build(&root, layout.store_db(), &args.scan.ignore_files)
            .map_err(|error| format!("{error:?}"))?
            .discover_with_progress(progress.as_deref());
    if let Some(error) = discovery.errors.first() {
        return Err(error.message.clone());
    }
    let files = discovery
        .supported_files
        .into_iter()
        .map(|target| {
            let (content_hash, content_bytes) =
                crate::extraction::read_source_identity(&target).map_err(|error| error.message)?;
            Ok(DiscoveredImportFile {
                target,
                content_hash,
                content_bytes,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if let Some(progress) = progress.as_deref() {
        progress.enter_phase("store_import");
    }
    let watchdog = args
        .scan
        .parent_pid
        .map(crate::watchdog::ParentWatchdog::start);
    let mut executor = StoreRequestExecutor::new(
        root,
        files,
        args.scan.spool_dir.clone(),
        args.level == StoreLevelArg::Full,
        progress,
        watchdog,
    );
    let policy = CoordinatorPolicy {
        own_request_id: Some(request_id.to_string()),
        ..CoordinatorPolicy::default()
    };
    coordinator
        .drain(&mut executor, &policy)
        .map_err(|error| error.to_string())?;
    if let Some(progress) = executor.progress() {
        progress.enter_phase("complete");
    }
    let request = coordinator
        .request(request_id)
        .map_err(|error| error.to_string())?;
    if request.state != RequestState::Committed && request.state != RequestState::Acknowledged {
        return Err(request
            .error_json
            .unwrap_or_else(|| "store_import_failed".to_string()));
    }
    let result: serde_json::Value = serde_json::from_str(
        request
            .result_json
            .as_deref()
            .ok_or("missing_import_result")?,
    )
    .map_err(|error| error.to_string())?;
    let mut report = base_report(
        args,
        request_id,
        idempotency_key,
        StoreRequestState::Committed,
    );
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
    Ok(report)
}

fn classify_failure(message: &str) -> StoreFailureClass {
    if message.contains("l1_projection_mismatch") {
        StoreFailureClass::L1ProjectionMismatch
    } else if message.contains("changed_between_waves") {
        StoreFailureClass::ChangedBetweenWaves
    } else if message.contains("root")
        && (message.contains("mismatch") || message.contains("does not match"))
    {
        StoreFailureClass::ViewRootMismatch
    } else if message.contains("lease") {
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

fn mint_request_id() -> String {
    format!("request-{}-{}", std::process::id(), now_millis())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        })
}

#[derive(Debug)]
struct ImportClock;

impl julie_extract_artifact::store::UnixMillisClock for ImportClock {
    fn now_ms(&self) -> i64 {
        now_millis()
    }
}

#[derive(Debug)]
struct ImportPidLiveness;

impl julie_extract_artifact::store::PidLiveness for ImportPidLiveness {
    fn status(&self, pid: u32) -> julie_extract_artifact::store::PidStatus {
        crate::watchdog::process_status(pid)
    }
}
