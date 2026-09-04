use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use julie_extract_artifact::jsonl::{JSONL_SCHEMA_VERSION, export_jsonl, export_jsonl_to_path};
use julie_extract_artifact::metadata::{ArtifactMetadata, RebindMetadata};
use julie_extract_artifact::model::{
    ArtifactFile, RevisionChangeKind, RevisionInput, WriteMode, WriteOperation,
    WritePhaseDurations, WriteResult,
};
use julie_extract_artifact::reports::{
    RebindReport, ReportCode, ReportDiagnostic, ReportInput, ReportLanguageProfile, ReportMode,
    ReportOperation, ReportProfile, ReportRevision, ReportStatus, RowDomainCounts,
};
use julie_extract_artifact::writer::{
    ArtifactFileSpool, ArtifactSpoolError, ArtifactWriteError, ArtifactWriter,
};
use julie_extractors::{
    ExtractionLevel, capability_snapshot as extractor_capability_snapshot,
    detect_language_for_source,
};
use rayon::prelude::*;
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::args::{
    Cli, Command, DeleteArgs, ExportArgs, InfoArgs, LanguagesArgs, RebindArgs, ScanArgs, UpdateArgs,
};
use crate::artifact_access::{
    ArtifactAccess, ExistingArtifact, artifact_report_from_connection, existing_artifact_for_root,
    file_row_attribution, jsonl_counts, latest_revision_id, load_existing_content_hashes,
    open_artifact, open_artifact_for_info, open_artifact_for_rebind, open_artifact_for_root,
    scan_file_row_attribution, table_totals, write_rebind,
};
use crate::capability_snapshot::{
    artifact_capability_snapshot, current_capability_fingerprints, flags, kind_coverage_json,
    structural_fact_patterns_json,
};
use crate::discovery::{
    DiscoveryExclusions, DiscoveryPolicy, FileSelection, SupportedTarget, UnsupportedReason,
    canonicalize_ignore_files,
};
use crate::extraction::{
    ExtractFileError, SourceSnapshot, extract_artifact_file,
    extract_artifact_file_from_snapshot_at, failed_artifact_file, read_source_snapshot,
    select_extraction_pool, unchanged_artifact_file, unsupported_artifact_file,
};
use crate::limits::{HARD_EXCLUDE_DIRS, HARD_EXCLUDE_SUFFIXES, MAX_SOURCE_FILE_BYTES};
use crate::paths::{
    FileTarget, canonicalize_db_path, canonicalize_progress_file, canonicalize_root,
    canonicalize_spool_dir, canonicalize_update_file, normalize_delete_file,
    reject_progress_file_collision, root_relative_unix,
};
use crate::progress::{Counter, ScanProgress};
use crate::reports::{
    CommandOutcome, PathErrorInput, ReportBuilder, ReportStream, artifact_input, base_report,
    diagnostic, discovery_error_diagnostic, display_path, extract_error_diagnostic,
    extract_error_outcome, outcome, path_error_outcome, path_error_outcome_with_paths,
    slow_file_skipped_diagnostic, spool_error_outcome, write_error_outcome,
    write_error_outcome_with_profile, write_outcome,
};
use crate::spool::{ScanSpool, create_scan_spool, is_spool_artifact_name, reap_unowned_spools};
use crate::watchdog::ParentWatchdog;
use crate::store::import::StoreExecutionOutcome;

pub fn run_from_env() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let exit_code = error.exit_code();
            let _ = error.print();
            return ExitCode::from(exit_code as u8);
        }
    };

    let outcome = run(cli);
    outcome.write();
    ExitCode::from(outcome.exit_code())
}

enum DispatchOutcome {
    Legacy(Box<CommandOutcome>),
    Store(Box<StoreExecutionOutcome>),
}

impl DispatchOutcome {
    fn exit_code(&self) -> u8 {
        match self {
            Self::Legacy(outcome) => outcome.exit_code,
            Self::Store(outcome) => outcome.exit_code(),
        }
    }

    fn write(&self) {
        match self {
            Self::Legacy(outcome) => write_outcome(outcome),
            Self::Store(outcome) => outcome.write(),
        }
    }
}

impl From<CommandOutcome> for DispatchOutcome {
    fn from(outcome: CommandOutcome) -> Self {
        Self::Legacy(Box::new(outcome))
    }
}

fn run(cli: Cli) -> DispatchOutcome {
    match cli.command {
        Command::Store(args) => {
            DispatchOutcome::Store(Box::new(crate::store::dispatch(args)))
        }
        Command::Scan(args) => scan(args).into(),
        Command::Update(args) => update(args).into(),
        Command::Delete(args) => delete(args).into(),
        Command::Info(args) => info(args).into(),
        Command::Export(args) => export(args).into(),
        Command::Languages(args) => languages(args).into(),
        Command::Rebind(args) => rebind(args).into(),
    }
}

/// Run a scan and attach its configuration warnings to whatever report it
/// produced.
///
/// `spool_dir_excluded` and `spool_lock_unavailable` describe how the scan was
/// CONFIGURED, not what it found, so they belong on every exit and not only on
/// the success path. Attached per-exit they were missed by four of them: an
/// operator who excluded `src/` and then hit a write failure got a report that
/// never mentioned the exclusion, fixed the disk, reran, saw a clean `ok`, and
/// never learned — on the run most likely to be read closely. Attaching them
/// here, once, is also what keeps a later early return from silently dropping
/// them again.
fn scan(args: ScanArgs) -> CommandOutcome {
    let mut configuration_warnings = Vec::new();
    scan_collecting_warnings(args, &mut configuration_warnings)
        .with_warnings(configuration_warnings)
}

fn scan_collecting_warnings(
    args: ScanArgs,
    configuration_warnings: &mut Vec<ReportDiagnostic>,
) -> CommandOutcome {
    let scan_started = Instant::now();
    let watchdog = args.parent_pid.map(ParentWatchdog::start);
    let mut profile_phases = BTreeMap::new();
    let mode = if args.force {
        ReportMode::Force
    } else {
        ReportMode::Incremental
    };
    let root = match canonicalize_root(&args.root) {
        Ok(root) => root,
        Err(error) => return path_error_outcome(error, ReportOperation::Scan, mode, args.json),
    };
    let db = match canonicalize_db_path(&args.db) {
        Ok(db) => db,
        Err(error) => return path_error_outcome(error, ReportOperation::Scan, mode, args.json),
    };
    let ignore_files = match canonicalize_ignore_files(&args.ignore_files) {
        Ok(ignore_files) => ignore_files,
        Err(error) => return path_error_outcome(error, ReportOperation::Scan, mode, args.json),
    };
    let spool_dir = match args
        .spool_dir
        .as_deref()
        .map(canonicalize_spool_dir)
        .transpose()
    {
        Ok(spool_dir) => spool_dir,
        Err(error) => return path_error_outcome(error, ReportOperation::Scan, mode, args.json),
    };
    let progress_path = match args
        .progress_file
        .as_deref()
        .map(canonicalize_progress_file)
        .transpose()
    {
        Ok(progress_path) => progress_path,
        Err(error) => return path_error_outcome(error, ReportOperation::Scan, mode, args.json),
    };
    if let Some(progress_path) = progress_path.as_deref()
        && let Err(error) = reject_progress_file_collision(progress_path, &db)
    {
        return path_error_outcome(error, ReportOperation::Scan, mode, args.json);
    }
    let progress = match progress_path
        .as_deref()
        .map(|progress_path| ScanProgress::create_for_artifact(progress_path, &db))
        .transpose()
    {
        Ok(progress) => progress,
        Err(error) => return path_error_outcome(error, ReportOperation::Scan, mode, args.json),
    };
    let controls = ScanControls {
        spool_dir: spool_dir.as_deref(),
        progress: progress.as_ref(),
        watchdog: watchdog.as_ref(),
    };
    configuration_warnings.extend(
        spool_dir
            .as_deref()
            .and_then(|spool_dir| spool_dir_inside_root_warning(spool_dir, &root)),
    );
    if let Some(spool_dir) = controls.spool_dir {
        reap_unowned_spools(spool_dir);
    }
    let input = artifact_input(&db, Some(&root), None, None);
    if let Some(aborted) = controls.parent_exited_outcome(
        args.parent_pid,
        mode,
        &input,
        args.json,
        scan_started,
        &profile_phases,
    ) {
        return aborted;
    }

    let existing_artifact_started = Instant::now();
    controls.enter_phase("existing_artifact");
    let existing_scan_artifact = if db.exists() && !args.force {
        match open_artifact_for_root(&db, args.strict_schema, Some(JSONL_SCHEMA_VERSION), &root) {
            Ok(artifact) => Some(artifact),
            Err(error) => {
                return outcome(
                    base_report(ReportStatus::Failed, ReportOperation::Scan, mode, input)
                        .with_error(error.diagnostic),
                    error.exit_code,
                    args.json,
                    ReportStream::Stdout,
                );
            }
        }
    } else {
        None
    };
    let existing_content_hashes = match existing_scan_artifact
        .as_ref()
        .map(|artifact| load_existing_content_hashes(&artifact.connection))
        .transpose()
    {
        Ok(hashes) => hashes,
        Err(error) => {
            return outcome(
                base_report(ReportStatus::Failed, ReportOperation::Scan, mode, input)
                    .with_error(error.diagnostic),
                error.exit_code,
                args.json,
                ReportStream::Stdout,
            );
        }
    };
    let existing_scan_level = existing_scan_artifact
        .as_ref()
        .filter(|artifact| artifact.has_extraction_history)
        .map(|artifact| artifact.index_level.clone());
    let existing_scan_metadata = existing_scan_artifact.map(|artifact| artifact.write_metadata);
    record_profile_phase(
        &mut profile_phases,
        "existing_artifact",
        existing_artifact_started.elapsed(),
    );

    let discovery_started = Instant::now();
    controls.enter_phase("discovery");
    let exclusions = DiscoveryExclusions {
        progress_path: progress_path.clone(),
        spool_dir: spool_dir.clone(),
    };
    let discovery = match DiscoveryPolicy::build_excluding(&root, &db, exclusions, &ignore_files) {
        Ok(discovery) => discovery,
        Err(error) => return path_error_outcome(error, ReportOperation::Scan, mode, args.json),
    };
    let discovered = discovery.discover_with_progress(controls.progress);
    let preserved_missing_paths = discovered
        .errors
        .iter()
        .map(|error| error.root_relative_path.clone())
        .collect::<Vec<_>>();
    record_profile_phase(
        &mut profile_phases,
        "discovery",
        discovery_started.elapsed(),
    );

    if let Some(aborted) = controls.parent_exited_outcome(
        args.parent_pid,
        mode,
        &input,
        args.json,
        scan_started,
        &profile_phases,
    ) {
        return aborted;
    }

    let force_metadata_started = Instant::now();
    controls.enter_phase("force_metadata");
    let mut force_existing_level = None;
    let force_existing_metadata = if args.force && db.exists() {
        match open_artifact(
            &db,
            args.strict_schema,
            Some(JSONL_SCHEMA_VERSION),
            ArtifactAccess::Write,
        ) {
            Ok(artifact) if artifact.report.root_path == display_path(&root) => {
                if artifact.has_extraction_history {
                    force_existing_level = Some(artifact.index_level.clone());
                }
                Some(artifact.write_metadata)
            }
            // A force scan treats an artifact it cannot reuse as one to rebuild from
            // scratch, but an older schema is the one refusal it must not swallow:
            // the rebuild writes in place whenever the root still matches, which is
            // exactly the case that would stamp the current version onto older DDL.
            Err(error) if error.diagnostic.code == ReportCode::SchemaMigrationRequired => {
                return outcome(
                    base_report(ReportStatus::Failed, ReportOperation::Scan, mode, input)
                        .with_error(error.diagnostic),
                    error.exit_code,
                    args.json,
                    ReportStream::Stdout,
                );
            }
            Ok(_) | Err(_) => None,
        }
    } else {
        None
    };
    record_profile_phase(
        &mut profile_phases,
        "force_metadata",
        force_metadata_started.elapsed(),
    );
    let should_rebuild_db = args.force && db.exists() && force_existing_metadata.is_none();
    let recorded_level = force_existing_level.or(existing_scan_level);
    let requested_level = args.level.map(ExtractionLevel::from);
    let recorded_extraction_level = match recorded_level.as_deref() {
        None => None,
        Some(recorded) => match ExtractionLevel::from_metadata_value(recorded) {
            Some(level) => Some(level),
            None => {
                return outcome(
                    base_report(ReportStatus::Failed, ReportOperation::Scan, mode, input)
                        .with_error(unknown_index_level_diagnostic(&db, recorded)),
                    3,
                    args.json,
                    ReportStream::Stdout,
                );
            }
        },
    };
    let level = match (recorded_extraction_level, requested_level) {
        (Some(recorded), Some(requested)) if requested != recorded => {
            return outcome(
                base_report(ReportStatus::Failed, ReportOperation::Scan, mode, input).with_error(
                    index_level_conflict_diagnostic(&db, recorded.metadata_value(), requested),
                ),
                2,
                args.json,
                ReportStream::Stdout,
            );
        }
        (Some(recorded), _) => recorded,
        (None, requested) => requested.unwrap_or(ExtractionLevel::Full),
    };
    let indexed_at = now_rfc3339();
    let extraction_spool_started = Instant::now();
    controls.enter_phase("extraction_spool");
    let mut extracted = match spool_discovered_files(
        ExtractionRequest {
            root: &root,
            indexed_at,
            existing_content_hashes: existing_content_hashes.as_ref(),
            force: args.force,
            jobs: args.jobs,
            level,
        },
        &discovered.supported_targets,
        &discovered.unsupported_targets,
        controls,
    ) {
        Ok(extracted) => extracted,
        Err(ExtractionSpoolError::Spool(error)) => {
            return spool_error_outcome(error, ReportOperation::Scan, mode, input, args.json);
        }
        Err(ExtractionSpoolError::PoolUnavailable {
            requested_jobs,
            message,
        }) => {
            return outcome(
                base_report(ReportStatus::Failed, ReportOperation::Scan, mode, input).with_error(
                    extraction_pool_unavailable_diagnostic(requested_jobs, &message),
                ),
                1,
                args.json,
                ReportStream::Stdout,
            );
        }
    };
    record_profile_phase(
        &mut profile_phases,
        "extraction_spool",
        extraction_spool_started.elapsed(),
    );
    debug_assert_eq!(extracted.files_spooled, extracted.snapshot_paths.len());
    let profile_languages = extracted.profile.languages.clone();
    configuration_warnings.extend(
        extracted
            .spool
            .ownership_lock_unavailable()
            .then(|| spool_lock_unavailable_warning(spool_dir.as_deref())),
    );

    if let Some(aborted) =
        abort_before_full_rebuild(&db, should_rebuild_db, || match extracted.completion {
            SpoolCompletion::Complete => controls.parent_exited_outcome(
                args.parent_pid,
                mode,
                &input,
                args.json,
                scan_started,
                &profile_phases,
            ),
            SpoolCompletion::ParentExited {
                observed_parent_pid,
            } => Some(parent_exited_abort(
                args.parent_pid,
                observed_parent_pid,
                mode,
                &input,
                args.json,
                scan_started,
                &profile_phases,
            )),
        })
    {
        return aborted;
    }
    let db_existed_before_write = db.exists();

    let metadata = force_existing_metadata
        .or(existing_scan_metadata)
        .map(refreshed_metadata)
        .unwrap_or_else(|| new_artifact_metadata(&root, None));

    let writer_open_started = Instant::now();
    controls.enter_phase("writer_open");
    match ArtifactWriter::open_path(&db, metadata) {
        Ok(mut writer) => {
            record_profile_phase(
                &mut profile_phases,
                "writer_open",
                writer_open_started.elapsed(),
            );
            writer.stage_capability_snapshot(artifact_capability_snapshot());
            writer.stage_index_level(level.metadata_value());
            let artifact_write_started = Instant::now();
            controls.enter_phase("artifact_write");
            match writer.write_scan_spooled_preserving_missing_paths(
                revision_input(
                    WriteOperation::Scan,
                    Some(if args.force {
                        WriteMode::Force
                    } else {
                        WriteMode::Incremental
                    }),
                    &root,
                ),
                &extracted.snapshot_paths,
                &preserved_missing_paths,
                extracted.spool.file_spool_mut(),
            ) {
                Ok(write_result) => {
                    record_profile_phase(
                        &mut profile_phases,
                        "artifact_write",
                        artifact_write_started.elapsed(),
                    );
                    record_write_phase_profile(&mut profile_phases, &write_result.phases);
                    let capability_rows_written = writer.last_capability_rows_written();
                    let connection = writer.connection();
                    let has_source_errors =
                        !extracted.errors.is_empty() || !discovered.errors.is_empty();
                    let totals = table_totals(connection);
                    let artifact = match artifact_report_from_connection(&db, connection) {
                        Ok(artifact) => artifact,
                        Err(error) => {
                            return outcome(
                                base_report(
                                    ReportStatus::Failed,
                                    ReportOperation::Scan,
                                    mode,
                                    input,
                                )
                                .with_error(error.diagnostic)
                                .with_profile(scan_profile(
                                    scan_started,
                                    &profile_phases,
                                    &profile_languages,
                                )),
                                error.exit_code,
                                args.json,
                                ReportStream::Stdout,
                            );
                        }
                    };
                    let status = if has_source_errors {
                        ReportStatus::Partial
                    } else if write_result.revision_id.is_some()
                        || should_rebuild_db
                        || !db_existed_before_write
                        || capability_rows_written.has_rows()
                    {
                        ReportStatus::Ok
                    } else {
                        ReportStatus::NoChange
                    };
                    let mut report = base_report(status, ReportOperation::Scan, mode, input)
                        .with_artifact(artifact)
                        .with_revision(ReportRevision {
                            latest_revision_id: latest_revision_id(connection),
                            created_revision_id: write_result.revision_id,
                        })
                        .with_totals(totals)
                        .with_profile(scan_profile(
                            scan_started,
                            &profile_phases,
                            &profile_languages,
                        ));
                    for conflict in crate::reports::reference_site_conflict_diagnostics(
                        &write_result.reference_site_conflicts,
                        Some(&root),
                    ) {
                        report = report.with_warning(conflict);
                    }
                    report.counts.files_scanned =
                        (discovered.supported_targets.len() + discovered.unsupported_files) as i64;
                    report.counts.files_changed = write_result
                        .files_changed
                        .saturating_sub(write_result.files_deleted)
                        as i64;
                    report.counts.files_unchanged = write_result.files_skipped as i64;
                    report.counts.files_unsupported = discovered.unsupported_files as i64;
                    report.counts.files_deleted = write_result.files_deleted as i64;
                    report.counts.files_failed =
                        (extracted.errors.len() + discovered.errors.len()) as i64;
                    report.counts.rows_written =
                        rows_written_with_capabilities(&capability_rows_written, &write_result);
                    let file_rows = scan_file_row_attribution(connection, args.json);
                    report.counts.file_rows_truncated = file_rows.truncated;
                    report.counts.file_rows = file_rows.rows;
                    report
                        .errors
                        .extend(discovered.errors.iter().map(discovery_error_diagnostic));
                    report
                        .errors
                        .extend(extracted.errors.iter().map(extract_error_diagnostic));
                    report.warnings.extend(
                        discovered
                            .slow_file_skips
                            .iter()
                            .map(slow_file_skipped_diagnostic),
                    );
                    let exit_code = if has_source_errors { 1 } else { 0 };
                    outcome(report, exit_code, args.json, ReportStream::Stdout)
                }
                Err(error) => {
                    record_profile_phase(
                        &mut profile_phases,
                        "artifact_write",
                        artifact_write_started.elapsed(),
                    );
                    write_error_outcome_with_profile(
                        error,
                        ReportOperation::Scan,
                        mode,
                        input,
                        args.json,
                        Some(scan_profile(
                            scan_started,
                            &profile_phases,
                            &profile_languages,
                        )),
                    )
                }
            }
        }
        Err(error) => {
            record_profile_phase(
                &mut profile_phases,
                "writer_open",
                writer_open_started.elapsed(),
            );
            outcome(
                base_report(ReportStatus::Failed, ReportOperation::Scan, mode, input)
                    .with_error(diagnostic(
                        ReportCode::DbOpenFailed,
                        format!("could not create SQLite artifact: {error}"),
                        Some(display_path(&db)),
                        None,
                        true,
                        json!({}),
                    ))
                    .with_profile(scan_profile(
                        scan_started,
                        &profile_phases,
                        &profile_languages,
                    )),
                1,
                args.json,
                ReportStream::Stdout,
            )
        }
    }
}

fn update(args: UpdateArgs) -> CommandOutcome {
    let root = match canonicalize_root(&args.root) {
        Ok(root) => root,
        Err(error) => {
            return path_error_outcome(
                error,
                ReportOperation::Update,
                ReportMode::SingleFile,
                args.json,
            );
        }
    };
    let db = match canonicalize_db_path(&args.db) {
        Ok(db) => db,
        Err(error) => {
            return path_error_outcome(
                error,
                ReportOperation::Update,
                ReportMode::SingleFile,
                args.json,
            );
        }
    };
    let target = match canonicalize_update_file(&root, &args.file) {
        Ok(target) => target,
        Err(error) => {
            return path_error_outcome_with_paths(
                error,
                ReportOperation::Update,
                ReportMode::SingleFile,
                args.json,
                PathErrorInput {
                    db_path: Some(&db),
                    root_path: Some(&root),
                    file_path: Some(&args.file),
                    root_relative_path: None,
                },
            );
        }
    };
    let ignore_files = match canonicalize_ignore_files(&args.ignore_files) {
        Ok(ignore_files) => ignore_files,
        Err(error) => {
            return path_error_outcome_with_paths(
                error,
                ReportOperation::Update,
                ReportMode::SingleFile,
                args.json,
                PathErrorInput {
                    db_path: Some(&db),
                    root_path: Some(&root),
                    file_path: Some(&target.absolute_path),
                    root_relative_path: Some(&target.root_relative_path),
                },
            );
        }
    };
    let input = artifact_input(
        &db,
        Some(&root),
        Some(&target.absolute_path),
        Some(&target.root_relative_path),
    );

    let existing_artifact = match existing_artifact_for_root(
        &db,
        args.strict_schema,
        Some(JSONL_SCHEMA_VERSION),
        &root,
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            return outcome(
                base_report(
                    ReportStatus::Failed,
                    ReportOperation::Update,
                    ReportMode::SingleFile,
                    input,
                )
                .with_error(error.diagnostic),
                error.exit_code,
                args.json,
                ReportStream::Stdout,
            );
        }
    };

    let discovery = match DiscoveryPolicy::build(&root, &db, &ignore_files) {
        Ok(discovery) => discovery,
        Err(error) => {
            return path_error_outcome_with_paths(
                error,
                ReportOperation::Update,
                ReportMode::SingleFile,
                args.json,
                PathErrorInput {
                    db_path: Some(&db),
                    root_path: Some(&root),
                    file_path: Some(&target.absolute_path),
                    root_relative_path: Some(&target.root_relative_path),
                },
            );
        }
    };

    let language = match discovery.select_file(&target) {
        FileSelection::Supported { language } => language,
        FileSelection::Unsupported {
            reason: UnsupportedReason::Oversized,
        } => {
            return cleanup_skipped_update(
                &db,
                &root,
                target,
                existing_artifact,
                UpdateSkipReason::Oversized,
                args.json,
            );
        }
        FileSelection::Unsupported { .. } => {
            return cleanup_skipped_update(
                &db,
                &root,
                target,
                existing_artifact,
                UpdateSkipReason::IgnoredOrUnsupported,
                args.json,
            );
        }
    };

    let update_level = match existing_artifact
        .as_ref()
        .map(|artifact| artifact.index_level.as_str())
    {
        None => ExtractionLevel::Full,
        Some(recorded) => match ExtractionLevel::from_metadata_value(recorded) {
            Some(level) => level,
            None => {
                return outcome(
                    base_report(
                        ReportStatus::Failed,
                        ReportOperation::Update,
                        ReportMode::SingleFile,
                        input,
                    )
                    .with_error(unknown_index_level_diagnostic(&db, recorded)),
                    3,
                    args.json,
                    ReportStream::Stdout,
                );
            }
        },
    };
    let file = match extract_artifact_file(&root, &target, language, now_rfc3339(), update_level) {
        Ok(file) => file,
        Err(error) => {
            return extract_error_outcome(
                error,
                ReportOperation::Update,
                ReportMode::SingleFile,
                input,
                args.json,
            );
        }
    };
    let metadata = existing_artifact
        .map(|artifact| refreshed_metadata(artifact.write_metadata))
        .unwrap_or_else(|| new_artifact_metadata(&root, None));

    match ArtifactWriter::open_path(&db, metadata) {
        Ok(mut writer) => {
            writer.stage_capability_snapshot(artifact_capability_snapshot());
            match writer.write_update(
                revision_input(WriteOperation::Update, Some(WriteMode::SingleFile), &root),
                &file,
            ) {
                Ok(write_result) => {
                    let capability_rows_written = writer.last_capability_rows_written();
                    let connection = writer.connection();
                    let artifact = match artifact_report_from_connection(&db, connection) {
                        Ok(artifact) => artifact,
                        Err(error) => {
                            return outcome(
                                base_report(
                                    ReportStatus::Failed,
                                    ReportOperation::Update,
                                    ReportMode::SingleFile,
                                    input,
                                )
                                .with_error(error.diagnostic),
                                error.exit_code,
                                args.json,
                                ReportStream::Stdout,
                            );
                        }
                    };
                    let status = if write_result.revision_id.is_some()
                        || capability_rows_written.has_rows()
                    {
                        ReportStatus::Ok
                    } else {
                        ReportStatus::NoChange
                    };
                    let mut report = base_report(
                        status,
                        ReportOperation::Update,
                        ReportMode::SingleFile,
                        input,
                    )
                    .with_artifact(artifact)
                    .with_revision(ReportRevision {
                        latest_revision_id: latest_revision_id(connection),
                        created_revision_id: write_result.revision_id,
                    })
                    .with_totals(table_totals(connection));
                    for conflict in crate::reports::reference_site_conflict_diagnostics(
                        &write_result.reference_site_conflicts,
                        Some(&root),
                    ) {
                        report = report.with_warning(conflict);
                    }
                    report.counts.files_scanned = 1;
                    report.counts.files_changed = write_result.files_changed as i64;
                    report.counts.files_unchanged = write_result.files_skipped as i64;
                    report.counts.rows_written =
                        rows_written_with_capabilities(&capability_rows_written, &write_result);
                    outcome(report, 0, args.json, ReportStream::Stdout)
                }
                Err(error) => write_error_outcome(
                    error,
                    ReportOperation::Update,
                    ReportMode::SingleFile,
                    input,
                    args.json,
                ),
            }
        }
        Err(error) => outcome(
            base_report(
                ReportStatus::Failed,
                ReportOperation::Update,
                ReportMode::SingleFile,
                input,
            )
            .with_error(diagnostic(
                ReportCode::DbOpenFailed,
                format!("could not open SQLite artifact for update: {error}"),
                Some(display_path(&db)),
                None,
                true,
                json!({}),
            )),
            1,
            args.json,
            ReportStream::Stdout,
        ),
    }
}

fn delete(args: DeleteArgs) -> CommandOutcome {
    let root = match canonicalize_root(&args.root) {
        Ok(root) => root,
        Err(error) => {
            return path_error_outcome(
                error,
                ReportOperation::Delete,
                ReportMode::SingleFile,
                args.json,
            );
        }
    };
    let db = match canonicalize_db_path(&args.db) {
        Ok(db) => db,
        Err(error) => {
            return path_error_outcome(
                error,
                ReportOperation::Delete,
                ReportMode::SingleFile,
                args.json,
            );
        }
    };
    let target = match normalize_delete_file(&root, &args.file) {
        Ok(target) => target,
        Err(error) => {
            return path_error_outcome_with_paths(
                error,
                ReportOperation::Delete,
                ReportMode::SingleFile,
                args.json,
                PathErrorInput {
                    db_path: Some(&db),
                    root_path: Some(&root),
                    file_path: Some(&args.file),
                    root_relative_path: None,
                },
            );
        }
    };
    let input = artifact_input(
        &db,
        Some(&root),
        Some(&target.absolute_path),
        Some(&target.root_relative_path),
    );

    let existing_artifact = match existing_artifact_for_root(
        &db,
        args.strict_schema,
        Some(JSONL_SCHEMA_VERSION),
        &root,
    ) {
        Ok(artifact) => artifact,
        Err(error) => {
            return outcome(
                base_report(
                    ReportStatus::Failed,
                    ReportOperation::Delete,
                    ReportMode::SingleFile,
                    input,
                )
                .with_error(error.diagnostic),
                error.exit_code,
                args.json,
                ReportStream::Stdout,
            );
        }
    };

    cleanup_delete(&db, &root, target, existing_artifact, args.json)
}

/// Retarget an artifact at a new root: a pure metadata rewrite, atomic, with no
/// copying and no extraction.
///
/// Requesting the root the artifact already records is a success, not an error —
/// a caller that cannot cheaply tell whether the copy it just made needs
/// retargeting should be able to ask unconditionally — but it writes nothing at
/// all, which is what `changed: false` reports.
fn rebind(args: RebindArgs) -> CommandOutcome {
    let root = match canonicalize_root(&args.root) {
        Ok(root) => root,
        Err(error) => {
            return path_error_outcome(
                error,
                ReportOperation::Rebind,
                ReportMode::Metadata,
                args.json,
            );
        }
    };
    let db = match canonicalize_db_path(&args.db) {
        Ok(db) => db,
        Err(error) => {
            return path_error_outcome(
                error,
                ReportOperation::Rebind,
                ReportMode::Metadata,
                args.json,
            );
        }
    };
    let input = artifact_input(&db, Some(&root), None, None);

    let artifact =
        match open_artifact_for_rebind(&db, args.strict_schema, Some(JSONL_SCHEMA_VERSION)) {
            Ok(artifact) => artifact,
            Err(error) => {
                return outcome(
                    base_report(
                        ReportStatus::Failed,
                        ReportOperation::Rebind,
                        ReportMode::Metadata,
                        input,
                    )
                    .with_error(error.diagnostic),
                    error.exit_code,
                    args.json,
                    ReportStream::Stdout,
                );
            }
        };

    let revision = ReportRevision {
        latest_revision_id: latest_revision_id(&artifact.connection),
        created_revision_id: None,
    };
    let mut artifact_report = artifact.report;
    let previous_root = artifact_report.root_path.clone();
    let previous_artifact_id = artifact_report.artifact_id.clone();
    let new_root = display_path(&root);
    drop(artifact.connection);

    if new_root == previous_root {
        return outcome(
            base_report(
                ReportStatus::NoChange,
                ReportOperation::Rebind,
                ReportMode::Metadata,
                input,
            )
            .with_artifact(artifact_report)
            .with_revision(revision)
            .with_rebind(RebindReport {
                previous_root,
                new_root,
                previous_artifact_id: previous_artifact_id.clone(),
                new_artifact_id: previous_artifact_id,
                changed: false,
            }),
            0,
            args.json,
            ReportStream::Stdout,
        );
    }

    let new_artifact_id = match rebound_artifact_id() {
        Ok(artifact_id) => artifact_id,
        Err(error) => {
            return outcome(
                base_report(
                    ReportStatus::Failed,
                    ReportOperation::Rebind,
                    ReportMode::Metadata,
                    input,
                )
                .with_error(diagnostic(
                    ReportCode::InternalError,
                    format!("a new artifact id could not be generated: {error}"),
                    Some(display_path(&db)),
                    None,
                    true,
                    json!({}),
                )),
                1,
                args.json,
                ReportStream::Stdout,
            );
        }
    };
    let rebound_at = now_rfc3339();

    if let Err(error) = write_rebind(
        &db,
        &RebindMetadata {
            previous_root: &previous_root,
            previous_artifact_id: &previous_artifact_id,
            new_root: &new_root,
            new_artifact_id: &new_artifact_id,
            rebound_at: &rebound_at,
        },
    ) {
        return outcome(
            base_report(
                ReportStatus::Failed,
                ReportOperation::Rebind,
                ReportMode::Metadata,
                input,
            )
            .with_error(error.diagnostic),
            error.exit_code,
            args.json,
            ReportStream::Stdout,
        );
    }

    artifact_report.root_path = new_root.clone();
    artifact_report.artifact_id = new_artifact_id.clone();
    outcome(
        base_report(
            ReportStatus::Ok,
            ReportOperation::Rebind,
            ReportMode::Metadata,
            input,
        )
        .with_artifact(artifact_report)
        .with_revision(revision)
        .with_rebind(RebindReport {
            previous_root,
            new_root,
            previous_artifact_id,
            new_artifact_id,
            changed: true,
        }),
        0,
        args.json,
        ReportStream::Stdout,
    )
}

fn info(args: InfoArgs) -> CommandOutcome {
    let input = ReportInput {
        db_path: Some(display_path(&args.db)),
        root_path: None,
        file_path: None,
        root_relative_path: None,
        format: None,
        output_path: None,
    };

    match open_artifact_for_info(&args.db, args.strict_schema, Some(JSONL_SCHEMA_VERSION)) {
        Ok(artifact) => {
            let mut report = base_report(
                ReportStatus::Ok,
                ReportOperation::Info,
                ReportMode::ReadOnly,
                input,
            )
            .with_artifact(artifact.report)
            .with_revision(ReportRevision {
                latest_revision_id: latest_revision_id(&artifact.connection),
                created_revision_id: None,
            })
            .with_totals(table_totals(&artifact.connection));
            let file_rows = file_row_attribution(&artifact.connection, None);
            report.counts.file_rows_truncated = file_rows.truncated;
            report.counts.file_rows = file_rows.rows;
            report.warnings.extend(artifact.warnings);
            outcome(report, 0, args.json, ReportStream::Stdout)
        }
        Err(error) => outcome(
            base_report(
                ReportStatus::Failed,
                ReportOperation::Info,
                ReportMode::ReadOnly,
                input,
            )
            .with_error(error.diagnostic),
            error.exit_code,
            args.json,
            ReportStream::Stdout,
        ),
    }
}

fn export(args: ExportArgs) -> CommandOutcome {
    let input = ReportInput {
        db_path: Some(display_path(&args.db)),
        root_path: None,
        file_path: None,
        root_relative_path: None,
        format: Some(args.format.clone()),
        output_path: Some(display_path(&args.out)),
    };

    if args.format != "jsonl" {
        let report = base_report(
            ReportStatus::Failed,
            ReportOperation::Export,
            ReportMode::Jsonl,
            input,
        )
        .with_error(diagnostic(
            ReportCode::UnsupportedFormat,
            "only JSONL export is supported",
            None,
            None,
            true,
            json!({"requested_format": args.format, "supported_formats": ["jsonl"]}),
        ));
        return outcome(report, 1, args.json, ReportStream::Stdout);
    }

    match open_artifact(
        &args.db,
        args.strict_schema,
        Some(JSONL_SCHEMA_VERSION),
        ArtifactAccess::Read,
    ) {
        Ok(artifact) => {
            let export_result = if args.out == Path::new("-") {
                let stdout = io::stdout();
                let mut lock = stdout.lock();
                export_jsonl(&artifact.connection, &mut lock)
            } else {
                export_jsonl_to_path(&artifact.connection, &args.out)
            };

            match export_result {
                Ok(summary) => {
                    let mut report = base_report(
                        ReportStatus::Ok,
                        ReportOperation::Export,
                        ReportMode::Jsonl,
                        input,
                    )
                    .with_artifact(artifact.report)
                    .with_totals(table_totals(&artifact.connection));
                    report.counts.rows_written = jsonl_counts(&summary.records_by_kind);
                    outcome(
                        report,
                        0,
                        args.json,
                        if args.out == Path::new("-") {
                            ReportStream::Stderr
                        } else {
                            ReportStream::Stdout
                        },
                    )
                }
                Err(error) => {
                    let report_stream = if args.out == Path::new("-") {
                        ReportStream::Stderr
                    } else {
                        ReportStream::Stdout
                    };
                    let report = base_report(
                        ReportStatus::Failed,
                        ReportOperation::Export,
                        ReportMode::Jsonl,
                        input,
                    )
                    .with_error(diagnostic(
                        ReportCode::ExportFailed,
                        format!("JSONL export failed: {error}"),
                        None,
                        None,
                        true,
                        json!({}),
                    ));
                    outcome(report, 1, args.json, report_stream)
                }
            }
        }
        Err(error) => outcome(
            base_report(
                ReportStatus::Failed,
                ReportOperation::Export,
                ReportMode::Jsonl,
                input,
            )
            .with_error(error.diagnostic),
            error.exit_code,
            args.json,
            ReportStream::Stdout,
        ),
    }
}

/// The mechanical file-eligibility limits `scan` and `update` apply before any
/// ignore file is consulted, published so consumers filter with the pinned
/// binary's own values instead of mirroring them.
fn discovery_limits_json() -> Value {
    json!({
        "max_source_file_bytes": MAX_SOURCE_FILE_BYTES,
        "hard_exclude_directories": HARD_EXCLUDE_DIRS,
        "hard_exclude_suffixes": HARD_EXCLUDE_SUFFIXES,
    })
}

fn languages(args: LanguagesArgs) -> CommandOutcome {
    let snapshot = extractor_capability_snapshot();
    let languages = snapshot
        .languages()
        .map(|row| {
            json!({
                "language": row.language,
                "parser_crate": row.parser_crate,
                "extensions": row.extensions,
                "dependency_status": row.dependency_status,
                "target_capabilities": flags(row.target_capabilities),
                "actual_capabilities": flags(row.capabilities),
                "kind_coverage": kind_coverage_json(&row.kind_coverage),
                "fixtures": row.fixtures.len(),
                "capability_gaps": row.capability_gaps.len(),
            })
        })
        .collect::<Vec<_>>();

    let report = base_report(
        ReportStatus::Ok,
        ReportOperation::Languages,
        ReportMode::CapabilitySnapshot,
        ReportInput {
            db_path: None,
            root_path: None,
            file_path: None,
            root_relative_path: None,
            format: None,
            output_path: None,
        },
    )
    .with_languages(json!({
        "total": languages.len(),
        "languages": languages,
        "discovery_limits": discovery_limits_json(),
    }))
    .with_structural_fact_patterns(structural_fact_patterns_json());
    outcome(report, 0, args.json, ReportStream::Stdout)
}

/// Opt-in process-lifecycle controls for one scan. Every field is `None` unless
/// the matching flag was passed, so an absent flag costs a branch and nothing
/// else — no file, no thread, no syscall.
#[derive(Clone, Copy, Default)]
struct ScanControls<'a> {
    spool_dir: Option<&'a Path>,
    progress: Option<&'a ScanProgress>,
    watchdog: Option<&'a ParentWatchdog>,
}

impl ScanControls<'_> {
    fn enter_phase(&self, phase: &'static str) {
        if let Some(progress) = self.progress {
            progress.enter_phase(phase);
        }
    }

    fn advance(&self, counter: Counter, by: u64) {
        if let Some(progress) = self.progress {
            progress.advance(counter, by);
        }
    }

    /// Why the extraction loop must stop before the next chunk, or `None`. The
    /// break condition and the reason recorded in the return value come from
    /// this one expression, so a caller can never disagree with the loop about
    /// whether the spool it is holding is complete.
    fn stop_requested(&self) -> Option<SpoolCompletion> {
        let watchdog = self.watchdog?;
        watchdog
            .parent_exited()
            .then(|| SpoolCompletion::ParentExited {
                observed_parent_pid: watchdog.observed_parent_pid(),
            })
    }

    fn parent_exited_outcome(
        &self,
        expected_parent_pid: Option<u32>,
        mode: ReportMode,
        input: &ReportInput,
        json_report: bool,
        scan_started: Instant,
        profile_phases: &BTreeMap<String, u64>,
    ) -> Option<CommandOutcome> {
        let watchdog = self.watchdog?;
        if !watchdog.parent_exited() {
            return None;
        }
        Some(parent_exited_abort(
            expected_parent_pid,
            watchdog.observed_parent_pid(),
            mode,
            input,
            json_report,
            scan_started,
            profile_phases,
        ))
    }
}

fn parent_exited_abort(
    expected_parent_pid: Option<u32>,
    observed_parent_pid: Option<u32>,
    mode: ReportMode,
    input: &ReportInput,
    json_report: bool,
    scan_started: Instant,
    profile_phases: &BTreeMap<String, u64>,
) -> CommandOutcome {
    outcome(
        base_report(
            ReportStatus::Failed,
            ReportOperation::Scan,
            mode,
            input.clone(),
        )
        .with_error(diagnostic(
            ReportCode::ParentExited,
            "parent process exited; scan aborted before writing the artifact",
            None,
            None,
            true,
            json!({
                "expected_parent_pid": expected_parent_pid,
                "observed_parent_pid": observed_parent_pid,
            }),
        ))
        .with_profile(scan_profile(scan_started, profile_phases, &BTreeMap::new())),
        1,
        json_report,
        ReportStream::Stdout,
    )
}

/// Warn when `--spool-dir` resolved to a directory inside `--root` that holds
/// something the scan would otherwise have read.
///
/// The exclusion itself is correct — a surviving `.jsonl` spool would otherwise
/// be extracted as source — and the directory is created when missing, so no
/// existence check can catch a caller who typed the wrong variable. Without this
/// warning `--spool-dir $ROOT/src` exits `ok` with zero diagnostics and an
/// artifact silently missing every symbol under `src/`, and an incremental
/// rescan then drops the previously indexed rows for that subtree as missing.
///
/// The hazard is a spool directory that SWALLOWS content, so a dedicated scratch
/// directory such as `$ROOT/.spool` or `$ROOT/.miller/spool` — the layout the
/// flag is meant to be used with — must stay silent. Warning on placement alone
/// would put a permanent unactionable warning on every scan the named consumer
/// runs, which is how a warning channel stops being read.
///
/// A spool directory that IS the root is not warned about either: only
/// spool-shaped file names are skipped there, so no source is lost.
fn spool_dir_inside_root_warning(spool_dir: &Path, root: &Path) -> Option<ReportDiagnostic> {
    if spool_dir == root || !spool_dir.starts_with(root) {
        return None;
    }
    if !holds_non_spool_entries(spool_dir) {
        return None;
    }
    let root_relative_path = root_relative_unix(root, spool_dir).ok();
    Some(diagnostic(
        ReportCode::SpoolDirExcluded,
        format!(
            "spool directory is inside the source root; it and everything under it \
             are excluded from this scan: {}",
            display_path(spool_dir)
        ),
        Some(display_path(spool_dir)),
        root_relative_path,
        true,
        json!({ "root_path": display_path(root) }),
    ))
}

/// Whether a spool directory holds anything other than spool files and their
/// sentinels — that is, whether excluding it costs the scan any content.
///
/// One non-recursive read is enough signal: an entry this module did not create
/// is content the walk will no longer see, and the directory it names is content
/// too. An unreadable directory returns `false`: a missing signal is not evidence
/// of a hazard, and probing for a warning must never fail the scan.
///
/// Filesystem metadata a scratch directory collects on its own does not count:
/// the operator never put content there, and warning on it would restore the
/// permanent unactionable warning this predicate exists to avoid. The set is
/// deliberately tiny — hidden entries at large DO count, because discovery does
/// not skip them and a dot-directory can hold real source.
fn holds_non_spool_entries(spool_dir: &Path) -> bool {
    const INERT_METADATA: [&str; 2] = [".DS_Store", "Thumbs.db"];

    let Ok(entries) = std::fs::read_dir(spool_dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_none_or(|name| !INERT_METADATA.contains(&name) && !is_spool_artifact_name(name))
    })
}

/// Warn when the spool directory could not carry an ownership lock.
///
/// Falling back to an unreapable spool name is the right trade — failing the
/// scan on an `ENOLCK` scratch mount swaps a leak for an outage — but silently
/// falling back leaves an operator who adopted `--spool-dir` to stop a leak with
/// no way to learn the protection is inert.
fn spool_lock_unavailable_warning(spool_dir: Option<&Path>) -> ReportDiagnostic {
    diagnostic(
        ReportCode::SpoolLockUnavailable,
        "spool directory could not carry an ownership lock; this scan's spool is \
         removed only by this process, and a later scan can never reclaim it if \
         this one is killed",
        spool_dir.map(display_path),
        None,
        true,
        json!({}),
    )
}

/// Whether an extraction pass reached the end of its target list, and if not,
/// why it stopped.
///
/// A partial spool is shaped exactly like a complete one, and promoting it as a
/// complete scan deletes every file after the stop point from the artifact. The
/// reason travels in the return value rather than being re-derived by the
/// caller, so a second stop condition added later is a compile error at every
/// caller instead of silent data loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpoolCompletion {
    Complete,
    ParentExited { observed_parent_pid: Option<u32> },
}

/// What one spooling extraction pass extracts, and how. Kept as a struct because
/// the alternative is an eight-argument call whose meaning depends on position.
struct ExtractionRequest<'a> {
    root: &'a Path,
    indexed_at: String,
    existing_content_hashes: Option<&'a BTreeMap<String, String>>,
    force: bool,
    jobs: usize,
    level: ExtractionLevel,
}

/// Why a spooling extraction pass failed outright. Per-file extraction failures
/// travel in [`SpooledExtractedFiles::errors`] instead; these fail the scan.
#[derive(Debug)]
enum ExtractionSpoolError {
    Spool(ArtifactSpoolError),
    /// No thread pool carrying the 16 MiB stack reservation could be built, even
    /// single-threaded. Running the chunk on rayon's global pool instead would
    /// reintroduce the stack-overflow abort the reservation exists to prevent.
    PoolUnavailable {
        requested_jobs: usize,
        message: String,
    },
}

impl From<ArtifactSpoolError> for ExtractionSpoolError {
    fn from(error: ArtifactSpoolError) -> Self {
        Self::Spool(error)
    }
}

fn spool_discovered_files(
    request: ExtractionRequest<'_>,
    targets: &[SupportedTarget],
    unsupported_targets: &[FileTarget],
    controls: ScanControls<'_>,
) -> Result<SpooledExtractedFiles, ExtractionSpoolError> {
    let level = request.level;
    let indexed_at = request.indexed_at.clone();
    let mut spooled = extract_supported_files_to_spool(
        request,
        targets,
        controls,
        move |root, target, language, indexed_at, snapshot| {
            extract_artifact_file_from_snapshot_at(
                root, target, language, indexed_at, snapshot, level,
            )
        },
    )?;
    spool_unsupported_files(&mut spooled, unsupported_targets, &indexed_at)?;
    Ok(spooled)
}

/// Spool the files the walk dropped as unsupported so the scan's change journal
/// accounts for them. They carry a content hash and nothing else: the writer
/// journals one `unsupported` change per content change, and a consumer reading
/// the journal never has to guess whether the scan saw the path.
fn spool_unsupported_files(
    spooled: &mut SpooledExtractedFiles,
    targets: &[FileTarget],
    indexed_at: &str,
) -> Result<(), ExtractionSpoolError> {
    for target in targets {
        match unsupported_artifact_file(target, indexed_at.to_string()) {
            Ok(file) => {
                spooled.spool.file_spool_mut().push(&file)?;
                spooled.snapshot_paths.push(file.path);
            }
            Err(error) => spooled.errors.push(error),
        }
    }
    spooled.files_spooled = spooled.spool.len();
    Ok(())
}

#[cfg(test)]
#[derive(Debug, Default, Clone, PartialEq)]
struct ExtractedFiles {
    files: Vec<ArtifactFile>,
    errors: Vec<ExtractFileError>,
}

#[cfg(test)]
impl ExtractedFiles {
    #[cfg(test)]
    fn unwrap(self) -> Vec<ArtifactFile> {
        assert!(
            self.errors.is_empty(),
            "expected extraction to succeed without per-file errors: {:?}",
            self.errors
        );
        self.files
    }
}

struct SpooledExtractedFiles {
    /// Owns the spool file and its removal. The guard is created with the spool
    /// rather than with this struct so that a spool write failure — which returns
    /// before this struct exists — still removes the file.
    spool: ScanSpool,
    snapshot_paths: Vec<String>,
    files_spooled: usize,
    errors: Vec<ExtractFileError>,
    profile: ScanExtractionProfile,
    completion: SpoolCompletion,
}

impl SpooledExtractedFiles {
    #[cfg(test)]
    fn unwrap(mut self) -> Vec<ArtifactFile> {
        assert!(
            self.errors.is_empty(),
            "expected extraction to succeed without per-file errors: {:?}",
            self.errors
        );
        self.spool.file_spool_mut().finish().unwrap();
        self.spool
            .file_spool()
            .iter()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }
}

type SupportedFileTarget = SupportedTarget;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ScanExtractionProfile {
    languages: BTreeMap<String, ReportLanguageProfile>,
}

impl ScanExtractionProfile {
    fn language_mut(&mut self, language: &str) -> &mut ReportLanguageProfile {
        self.languages.entry(language.to_string()).or_default()
    }
}

fn scan_profile(
    started: Instant,
    phases: &BTreeMap<String, u64>,
    languages: &BTreeMap<String, ReportLanguageProfile>,
) -> ReportProfile {
    ReportProfile {
        total_duration_ms: duration_ms(started.elapsed()),
        phases: phases.clone(),
        languages: languages.clone(),
    }
}

fn record_profile_phase(phases: &mut BTreeMap<String, u64>, phase: &str, duration: Duration) {
    add_duration_ms(phases.entry(phase.to_string()).or_insert(0), duration);
}

/// Additive `artifact_write_*` sub-phase keys. The segments partition the write,
/// so they sum to `artifact_write` up to clock-read overhead.
fn record_write_phase_profile(
    phases: &mut BTreeMap<String, u64>,
    write_phases: &WritePhaseDurations,
) {
    record_profile_phase(phases, "artifact_write_plan", write_phases.plan);
    record_profile_phase(
        phases,
        "artifact_write_file_symbol_insert",
        write_phases.file_symbol_insert,
    );
    record_profile_phase(phases, "artifact_write_child_rows", write_phases.child_rows);
    record_profile_phase(
        phases,
        "artifact_write_index_build",
        write_phases.index_build,
    );
    record_profile_phase(
        phases,
        "artifact_write_foreign_key_check",
        write_phases.foreign_key_check,
    );
    record_profile_phase(phases, "artifact_write_commit", write_phases.commit);
    record_profile_phase(
        phases,
        "artifact_write_wal_checkpoint",
        write_phases.wal_checkpoint,
    );
}

fn add_duration_ms(total: &mut u64, duration: Duration) {
    *total = total.saturating_add(duration_ms(duration));
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn push_profiled_spool(
    spool: &mut ArtifactFileSpool,
    profile: &mut ScanExtractionProfile,
    language: &str,
    file: &ArtifactFile,
) -> Result<(), ArtifactSpoolError> {
    let started = Instant::now();
    spool.push(file)?;
    add_duration_ms(
        &mut profile.language_mut(language).spool_write_duration_ms,
        started.elapsed(),
    );
    Ok(())
}

#[cfg(test)]
fn extract_supported_files(
    root: &Path,
    targets: &[SupportedFileTarget],
    indexed_at: String,
    existing_content_hashes: Option<&BTreeMap<String, String>>,
    force: bool,
    mut extract: impl FnMut(
        &Path,
        &FileTarget,
        String,
        String,
        SourceSnapshot,
    ) -> Result<ArtifactFile, ExtractFileError>,
) -> ExtractedFiles {
    let mut files = Vec::with_capacity(targets.len());
    let mut errors = Vec::new();
    for supported in targets {
        let snapshot = match read_source_snapshot(&supported.target) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                files.push(failed_artifact_file(
                    &supported.target,
                    supported.language.clone(),
                    indexed_at.clone(),
                    &error,
                ));
                errors.push(error);
                continue;
            }
        };
        if !force
            && existing_content_hashes
                .and_then(|hashes| hashes.get(&supported.target.root_relative_path))
                .is_some_and(|existing_hash| existing_hash == &snapshot.content_hash)
        {
            files.push(unchanged_artifact_file(
                &supported.target,
                supported.language.clone(),
                indexed_at.clone(),
                &snapshot,
            ));
            continue;
        }

        match extract(
            root,
            &supported.target,
            supported.language.clone(),
            indexed_at.clone(),
            snapshot.clone(),
        ) {
            Ok(file) => files.push(file),
            Err(error) => {
                files.push(failed_artifact_file(
                    &supported.target,
                    supported.language.clone(),
                    indexed_at.clone(),
                    &error,
                ));
                errors.push(error);
            }
        }
    }
    ExtractedFiles { files, errors }
}

/// Maximum number of files whose extracted rows are held in memory at once before
/// being drained to the on-disk spool. Bounds peak RSS during parallel extraction
/// while keeping enough work in flight to saturate the worker pool.
const EXTRACT_SPOOL_CHUNK_SIZE: usize = 512;

#[derive(Clone, Copy)]
enum FileOutcomeKind {
    ReadFailed,
    Unchanged,
    Changed,
    ExtractFailed,
}

/// Self-contained result of extracting one file, computed off the main thread.
/// Carries everything needed to update the profile, spool, and error list so the
/// parallel phase touches no shared mutable state.
struct FileOutcome {
    snapshot_path: String,
    language: String,
    file: ArtifactFile,
    kind: FileOutcomeKind,
    read_duration: Duration,
    extract_duration: Duration,
    bytes: i64,
    error: Option<ExtractFileError>,
}

fn compute_file_outcome(
    root: &Path,
    supported: &SupportedFileTarget,
    indexed_at: &str,
    existing_content_hashes: Option<&BTreeMap<String, String>>,
    force: bool,
    extract: &(
         impl Fn(
        &Path,
        &FileTarget,
        String,
        String,
        SourceSnapshot,
    ) -> Result<ArtifactFile, ExtractFileError>
         + Sync
     ),
) -> FileOutcome {
    let language = supported.language.clone();
    let snapshot_path = supported.target.root_relative_path.clone();

    let read_started = Instant::now();
    let snapshot = match read_source_snapshot(&supported.target) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let read_duration = read_started.elapsed();
            let file = failed_artifact_file(
                &supported.target,
                supported.language.clone(),
                indexed_at.to_string(),
                &error,
            );
            return FileOutcome {
                snapshot_path,
                language,
                file,
                kind: FileOutcomeKind::ReadFailed,
                read_duration,
                extract_duration: Duration::ZERO,
                bytes: error.content_bytes.unwrap_or(0),
                error: Some(error),
            };
        }
    };
    let read_duration = read_started.elapsed();
    let bytes = snapshot.content_bytes;
    let language =
        detect_language_for_source(&supported.target.root_relative_path, &snapshot.content)
            .unwrap_or(supported.language.as_str())
            .to_string();

    if !force
        && existing_content_hashes
            .and_then(|hashes| hashes.get(&supported.target.root_relative_path))
            .is_some_and(|existing_hash| existing_hash == &snapshot.content_hash)
    {
        let file = unchanged_artifact_file(
            &supported.target,
            language.clone(),
            indexed_at.to_string(),
            &snapshot,
        );
        return FileOutcome {
            snapshot_path,
            language,
            file,
            kind: FileOutcomeKind::Unchanged,
            read_duration,
            extract_duration: Duration::ZERO,
            bytes,
            error: None,
        };
    }

    let extract_started = Instant::now();
    match extract(
        root,
        &supported.target,
        language.clone(),
        indexed_at.to_string(),
        snapshot,
    ) {
        Ok(file) => {
            let extract_duration = extract_started.elapsed();
            FileOutcome {
                snapshot_path,
                language,
                file,
                kind: FileOutcomeKind::Changed,
                read_duration,
                extract_duration,
                bytes,
                error: None,
            }
        }
        Err(error) => {
            let extract_duration = extract_started.elapsed();
            let file = failed_artifact_file(
                &supported.target,
                language.clone(),
                indexed_at.to_string(),
                &error,
            );
            FileOutcome {
                snapshot_path,
                language,
                file,
                kind: FileOutcomeKind::ExtractFailed,
                read_duration,
                extract_duration,
                bytes,
                error: Some(error),
            }
        }
    }
}

fn unknown_index_level_diagnostic(db: &Path, recorded: &str) -> ReportDiagnostic {
    diagnostic(
        ReportCode::SchemaIncompatible,
        format!(
            "artifact records index_level '{recorded}', which this julie-extract does not \
             recognize; a newer julie-extract likely built it — upgrade this binary or rebuild \
             into a fresh artifact"
        ),
        Some(display_path(db)),
        None,
        false,
        json!({ "artifact_index_level": recorded }),
    )
}

fn index_level_conflict_diagnostic(
    db: &Path,
    recorded: &str,
    requested: ExtractionLevel,
) -> ReportDiagnostic {
    diagnostic(
        ReportCode::UsageError,
        format!(
            "index level is fixed when an artifact is first built: this artifact records \
             '{recorded}' but --level {} was requested; rebuild into a fresh artifact to \
             change level",
            requested.metadata_value()
        ),
        Some(display_path(db)),
        None,
        false,
        json!({
            "artifact_index_level": recorded,
            "requested_index_level": requested.metadata_value(),
        }),
    )
}

fn extraction_pool_unavailable_diagnostic(
    requested_jobs: usize,
    message: &str,
) -> ReportDiagnostic {
    diagnostic(
        ReportCode::InternalError,
        format!("extraction thread pool could not be built, even single-threaded: {message}"),
        None,
        None,
        false,
        json!({"requested_jobs": requested_jobs}),
    )
}

fn extract_supported_files_to_spool(
    request: ExtractionRequest<'_>,
    targets: &[SupportedFileTarget],
    controls: ScanControls<'_>,
    extract: impl Fn(
        &Path,
        &FileTarget,
        String,
        String,
        SourceSnapshot,
    ) -> Result<ArtifactFile, ExtractFileError>
    + Sync,
) -> Result<SpooledExtractedFiles, ExtractionSpoolError> {
    let mut spool = create_scan_spool(controls.spool_dir)?;
    let mut snapshot_paths = Vec::with_capacity(targets.len());
    let mut errors = Vec::new();
    let mut profile = ScanExtractionProfile::default();
    let mut completion = SpoolCompletion::Complete;

    // `num_threads(0)` lets rayon pick from available parallelism. The stack
    // reservation is virtual and committed lazily.
    let pool = select_extraction_pool(request.jobs, |threads| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .stack_size(16 * 1024 * 1024)
            .build()
    })
    .map_err(|error| ExtractionSpoolError::PoolUnavailable {
        requested_jobs: request.jobs,
        message: error.to_string(),
    })?;

    for chunk in targets.chunks(EXTRACT_SPOOL_CHUNK_SIZE) {
        // Cooperative abort point for the parent watchdog. Stopping between chunks
        // keeps every `Drop` intact, which is what removes the spool file.
        if let Some(stopped) = controls.stop_requested() {
            completion = stopped;
            break;
        }
        // Extract every file in the chunk in parallel. `collect` into a Vec preserves
        // chunk order, so the serial drain below stays byte-identical to a sequential scan.
        let map_chunk = || {
            chunk
                .par_iter()
                .map(|supported| {
                    let outcome = compute_file_outcome(
                        request.root,
                        supported,
                        &request.indexed_at,
                        request.existing_content_hashes,
                        request.force,
                        &extract,
                    );
                    controls.advance(Counter::Extracted, 1);
                    outcome
                })
                .collect::<Vec<FileOutcome>>()
        };
        let outcomes = pool.install(map_chunk);

        // Serial drain in target order: this owns all shared mutable state (spool,
        // profile, errors) so output ordering and profile counts match a sequential scan.
        for outcome in outcomes {
            snapshot_paths.push(outcome.snapshot_path);
            {
                let language_profile = profile.language_mut(&outcome.language);
                language_profile.files += 1;
                add_duration_ms(
                    &mut language_profile.read_duration_ms,
                    outcome.read_duration,
                );
                match outcome.kind {
                    FileOutcomeKind::ReadFailed => {
                        language_profile.bytes += outcome.bytes;
                        language_profile.failed_files += 1;
                    }
                    FileOutcomeKind::Unchanged => {
                        language_profile.bytes += outcome.bytes;
                        language_profile.unchanged_files += 1;
                    }
                    FileOutcomeKind::Changed => {
                        language_profile.bytes += outcome.bytes;
                        language_profile.changed_files += 1;
                        add_duration_ms(
                            &mut language_profile.extract_duration_ms,
                            outcome.extract_duration,
                        );
                    }
                    FileOutcomeKind::ExtractFailed => {
                        language_profile.bytes += outcome.bytes;
                        language_profile.failed_files += 1;
                        add_duration_ms(
                            &mut language_profile.extract_duration_ms,
                            outcome.extract_duration,
                        );
                    }
                }
            }
            push_profiled_spool(
                spool.file_spool_mut(),
                &mut profile,
                &outcome.language,
                &outcome.file,
            )?;
            if let Some(error) = outcome.error {
                errors.push(error);
            }
        }
        controls.advance(Counter::Spooled, chunk.len() as u64);
    }

    let files_spooled = spool.len();
    Ok(SpooledExtractedFiles {
        spool,
        snapshot_paths,
        files_spooled,
        errors,
        profile,
        completion,
    })
}

/// Why `update` refused to extract a file it was pointed at. Both variants
/// converge the artifact the same way — the path's rows are removed — and differ
/// only in the diagnostic they report. An oversized file keeps the same
/// `slow_file_skipped` warning `scan` emits, and `scan` removes its rows too, so
/// a file that grows past [`crate::limits::MAX_SOURCE_FILE_BYTES`] never keeps
/// serving stale symbols.
#[derive(Clone, Copy)]
enum UpdateSkipReason {
    IgnoredOrUnsupported,
    Oversized,
}

fn update_skip_diagnostic(
    reason: UpdateSkipReason,
    rows_removed: bool,
    absolute_path: &Path,
    root_relative_path: String,
) -> ReportDiagnostic {
    let (code, message) = match reason {
        UpdateSkipReason::IgnoredOrUnsupported => (
            ReportCode::UnsupportedFile,
            if rows_removed {
                "file is ignored or unsupported; stale artifact rows were removed".to_string()
            } else {
                "file is ignored or unsupported and no artifact rows exist".to_string()
            },
        ),
        UpdateSkipReason::Oversized => (
            ReportCode::SlowFileSkipped,
            crate::limits::slow_file_skip_message(),
        ),
    };
    diagnostic(
        code,
        message,
        Some(display_path(absolute_path)),
        Some(root_relative_path),
        true,
        json!({}),
    )
}

fn cleanup_skipped_update(
    db: &Path,
    root: &Path,
    target: FileTarget,
    existing_artifact: Option<ExistingArtifact>,
    reason: UpdateSkipReason,
    json_report: bool,
) -> CommandOutcome {
    let input = artifact_input(
        db,
        Some(root),
        Some(&target.absolute_path),
        Some(&target.root_relative_path),
    );
    if existing_artifact.is_none() {
        let mut report = base_report(
            ReportStatus::Unsupported,
            ReportOperation::Update,
            ReportMode::SingleFile,
            input,
        )
        .with_warning(update_skip_diagnostic(
            reason,
            false,
            &target.absolute_path,
            target.root_relative_path,
        ));
        report.counts.files_scanned = 1;
        report.counts.files_unsupported = 1;
        return outcome(report, 0, json_report, ReportStream::Stdout);
    }

    let root_relative_path = target.root_relative_path.clone();
    match delete_artifact_rows(
        db,
        root,
        &root_relative_path,
        existing_artifact,
        WriteOperation::Update,
        RevisionChangeKind::Unsupported,
    ) {
        Ok((writer, write_result, capability_rows_written)) => {
            let connection = writer.connection();
            let artifact = match artifact_report_from_connection(db, connection) {
                Ok(artifact) => artifact,
                Err(error) => {
                    return outcome(
                        base_report(
                            ReportStatus::Failed,
                            ReportOperation::Update,
                            ReportMode::SingleFile,
                            input,
                        )
                        .with_error(error.diagnostic),
                        error.exit_code,
                        json_report,
                        ReportStream::Stdout,
                    );
                }
            };
            let mut report = base_report(
                ReportStatus::Unsupported,
                ReportOperation::Update,
                ReportMode::SingleFile,
                input,
            )
            .with_artifact(artifact)
            .with_revision(ReportRevision {
                latest_revision_id: latest_revision_id(connection),
                created_revision_id: write_result.revision_id,
            })
            .with_totals(table_totals(connection))
            .with_warning(update_skip_diagnostic(
                reason,
                write_result.files_changed > 0,
                &target.absolute_path,
                root_relative_path,
            ));
            report.counts.files_scanned = 1;
            report.counts.files_unsupported = 1;
            report.counts.files_deleted = write_result.files_changed as i64;
            report.counts.rows_written =
                rows_written_with_capabilities(&capability_rows_written, &write_result);
            outcome(report, 0, json_report, ReportStream::Stdout)
        }
        Err(error) => write_error_outcome(
            error,
            ReportOperation::Update,
            ReportMode::SingleFile,
            input,
            json_report,
        ),
    }
}

fn cleanup_delete(
    db: &Path,
    root: &Path,
    target: FileTarget,
    existing_artifact: Option<ExistingArtifact>,
    json_report: bool,
) -> CommandOutcome {
    let input = artifact_input(
        db,
        Some(root),
        Some(&target.absolute_path),
        Some(&target.root_relative_path),
    );
    if existing_artifact.is_none() {
        let report = base_report(
            ReportStatus::NotFound,
            ReportOperation::Delete,
            ReportMode::SingleFile,
            input,
        );
        return outcome(report, 0, json_report, ReportStream::Stdout);
    }

    match delete_artifact_rows(
        db,
        root,
        &target.root_relative_path,
        existing_artifact,
        WriteOperation::Delete,
        RevisionChangeKind::Deleted,
    ) {
        Ok((writer, write_result, capability_rows_written)) => {
            let connection = writer.connection();
            let artifact = match artifact_report_from_connection(db, connection) {
                Ok(artifact) => artifact,
                Err(error) => {
                    return outcome(
                        base_report(
                            ReportStatus::Failed,
                            ReportOperation::Delete,
                            ReportMode::SingleFile,
                            input,
                        )
                        .with_error(error.diagnostic),
                        error.exit_code,
                        json_report,
                        ReportStream::Stdout,
                    );
                }
            };
            let status = if write_result.files_changed == 0 {
                ReportStatus::NotFound
            } else {
                ReportStatus::Ok
            };
            let mut report = base_report(
                status,
                ReportOperation::Delete,
                ReportMode::SingleFile,
                input,
            )
            .with_artifact(artifact)
            .with_revision(ReportRevision {
                latest_revision_id: latest_revision_id(connection),
                created_revision_id: write_result.revision_id,
            })
            .with_totals(table_totals(connection));
            report.counts.files_deleted = write_result.files_changed as i64;
            report.counts.rows_written =
                rows_written_with_capabilities(&capability_rows_written, &write_result);
            outcome(report, 0, json_report, ReportStream::Stdout)
        }
        Err(error) => write_error_outcome(
            error,
            ReportOperation::Delete,
            ReportMode::SingleFile,
            input,
            json_report,
        ),
    }
}

type RowRemovalResult = (ArtifactWriter, WriteResult, RowDomainCounts);

fn delete_artifact_rows(
    db: &Path,
    root: &Path,
    root_relative_path: &str,
    existing_artifact: Option<ExistingArtifact>,
    operation: WriteOperation,
    change_kind: RevisionChangeKind,
) -> Result<RowRemovalResult, ArtifactWriteError> {
    let metadata = existing_artifact
        .map(|artifact| refreshed_metadata(artifact.write_metadata))
        .unwrap_or_else(|| new_artifact_metadata(root, None));
    let mut writer = ArtifactWriter::open_path(db, metadata)?;
    writer.stage_capability_snapshot(artifact_capability_snapshot());
    let revision = revision_input(operation, Some(WriteMode::SingleFile), root);
    let result = match change_kind {
        RevisionChangeKind::Unsupported => {
            writer.remove_unsupported_file(revision, root_relative_path)?
        }
        RevisionChangeKind::Deleted => writer.delete_file(revision, root_relative_path)?,
        RevisionChangeKind::Inserted | RevisionChangeKind::Updated => {
            unreachable!("row removal does not support inserted/updated change kinds")
        }
    };
    let capability_rows_written = writer.last_capability_rows_written();
    Ok((writer, result, capability_rows_written))
}

fn rows_written_with_capabilities(
    capability_rows_written: &RowDomainCounts,
    write_result: &WriteResult,
) -> RowDomainCounts {
    let mut rows_written = RowDomainCounts::from(&write_result.rows_written);
    rows_written.add_counts(capability_rows_written);
    rows_written
}

fn new_artifact_metadata(root: &Path, artifact_id: Option<String>) -> ArtifactMetadata {
    let now = now_rfc3339();
    let (parser_inventory_fingerprint, capability_snapshot_fingerprint) =
        current_capability_fingerprints();
    ArtifactMetadata {
        artifact_id: artifact_id.unwrap_or_else(generated_artifact_id),
        root_path: display_path(root),
        binary_version: env!("CARGO_PKG_VERSION").to_string(),
        hash_algorithm: "blake3".to_string(),
        parser_inventory_fingerprint,
        capability_snapshot_fingerprint,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn refreshed_metadata(mut metadata: ArtifactMetadata) -> ArtifactMetadata {
    let (parser_inventory_fingerprint, capability_snapshot_fingerprint) =
        current_capability_fingerprints();
    metadata.binary_version = env!("CARGO_PKG_VERSION").to_string();
    metadata.parser_inventory_fingerprint = parser_inventory_fingerprint;
    metadata.capability_snapshot_fingerprint = capability_snapshot_fingerprint;
    metadata.updated_at = now_rfc3339();
    metadata
}

fn revision_input(
    operation: WriteOperation,
    mode: Option<WriteMode>,
    root: &Path,
) -> RevisionInput {
    let started_at = now_rfc3339();
    RevisionInput {
        operation,
        mode,
        started_at: started_at.clone(),
        completed_at: started_at,
        binary_version: env!("CARGO_PKG_VERSION").to_string(),
        input_root: Some(display_path(root)),
    }
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// Mint the identity a rebound artifact carries from now on.
///
/// Random rather than clock-derived: two worktrees rebound from the same copy in
/// the same nanosecond must not collide, and consumers key cache invalidation on
/// `artifact_id` changing.
fn rebound_artifact_id() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)?;
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("artifact-{hex}"))
}

fn generated_artifact_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("artifact-{nanos}")
}

/// The scan's last cooperative abort point, ordered ahead of its first
/// destructive step.
///
/// A full rebuild unlinks the live artifact before the writer runs, and past the
/// writer the spool must stay on disk until it has been read back, so this is the
/// only place both can be decided. Deciding the abort second would let a scan
/// delete the artifact and then report `parent_exited` — which
/// `docs/contracts/reports.md` documents as leaving the artifact untouched.
fn abort_before_full_rebuild(
    db: &Path,
    should_rebuild_db: bool,
    aborted: impl FnOnce() -> Option<CommandOutcome>,
) -> Option<CommandOutcome> {
    let aborted = aborted();
    if aborted.is_none() && should_rebuild_db {
        remove_artifact_files(db);
    }
    aborted
}

fn remove_artifact_files(db: &Path) {
    for path in [
        db.to_path_buf(),
        Path::new(&format!("{}-wal", db.display())).to_path_buf(),
        Path::new(&format!("{}-shm", db.display())).to_path_buf(),
    ] {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use crate::extraction::extract_artifact_file_from_snapshot;

    use julie_extract_artifact::model::{
        ArtifactCapabilityFlags, ArtifactLanguageCapabilityFixtureRow,
        ArtifactLanguageCapabilityGapRow, ArtifactLanguageCapabilityRow,
        ArtifactParserInventoryRow, ArtifactSymbol, CapabilityGapStatus, FileStatus,
    };
    use tempfile::TempDir;

    use crate::capability_snapshot::{
        capability_snapshot_fingerprint, parser_inventory_fingerprint,
    };
    use crate::spool::{owned_spool_file_name, sentinel_file_name, unowned_spool_file_name};

    use super::*;

    #[test]
    fn incremental_scan_reuses_existing_hash_without_parser_work() {
        let fixture = ScanFixture::new();
        let unchanged = fixture.write("src/unchanged.rs", "pub fn unchanged() {}\n");
        let changed = fixture.write("src/changed.rs", "pub fn changed() {}\n");
        let unchanged_snapshot = read_source_snapshot(&unchanged).unwrap();
        let mut existing_hashes = BTreeMap::new();
        existing_hashes.insert(
            unchanged.root_relative_path.clone(),
            unchanged_snapshot.content_hash.clone(),
        );
        existing_hashes.insert(
            changed.root_relative_path.clone(),
            "blake3:stale".to_string(),
        );
        let extracted_paths = RefCell::new(Vec::new());

        let files = extract_supported_files(
            fixture.root(),
            &[
                SupportedFileTarget::new(unchanged.clone(), "rust"),
                SupportedFileTarget::new(changed.clone(), "rust"),
            ],
            "2026-06-01T00:00:00Z".to_string(),
            Some(&existing_hashes),
            false,
            |_, target, language, indexed_at, snapshot| {
                extracted_paths
                    .borrow_mut()
                    .push(target.root_relative_path.clone());
                Ok(extracted_artifact_file(
                    target, language, indexed_at, snapshot,
                ))
            },
        )
        .unwrap();

        assert_eq!(extracted_paths.into_inner(), vec!["src/changed.rs"]);
        assert_eq!(
            files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/unchanged.rs", "src/changed.rs"]
        );
        assert!(
            files
                .iter()
                .find(|file| file.path == "src/unchanged.rs")
                .unwrap()
                .symbols
                .is_empty(),
            "unchanged files must be represented in the snapshot without parser rows"
        );
        assert_eq!(
            files
                .iter()
                .find(|file| file.path == "src/changed.rs")
                .unwrap()
                .symbols
                .len(),
            1,
            "changed files must still use the extraction callback"
        );
    }

    #[test]
    fn incremental_scan_can_spool_supported_files_without_parser_work_for_unchanged_files() {
        let fixture = ScanFixture::new();
        let unchanged = fixture.write("src/unchanged.rs", "pub fn unchanged() {}\n");
        let changed = fixture.write("src/changed.rs", "pub fn changed() {}\n");
        let unchanged_snapshot = read_source_snapshot(&unchanged).unwrap();
        let mut existing_hashes = BTreeMap::new();
        existing_hashes.insert(
            unchanged.root_relative_path.clone(),
            unchanged_snapshot.content_hash.clone(),
        );
        existing_hashes.insert(
            changed.root_relative_path.clone(),
            "blake3:stale".to_string(),
        );
        let extracted_paths = std::sync::Mutex::new(Vec::new());

        let extracted = extract_supported_files_to_spool(
            ExtractionRequest {
                root: fixture.root(),
                indexed_at: "2026-06-01T00:00:00Z".to_string(),
                existing_content_hashes: Some(&existing_hashes),
                force: false,
                jobs: 1,
                level: ExtractionLevel::Full,
            },
            &[
                SupportedFileTarget::new(unchanged.clone(), "rust"),
                SupportedFileTarget::new(changed.clone(), "rust"),
            ],
            ScanControls::default(),
            |_, target, language, indexed_at, snapshot| {
                extracted_paths
                    .lock()
                    .unwrap()
                    .push(target.root_relative_path.clone());
                Ok(extracted_artifact_file(
                    target, language, indexed_at, snapshot,
                ))
            },
        )
        .unwrap();

        assert_eq!(
            extracted_paths.into_inner().unwrap(),
            vec!["src/changed.rs"]
        );
        assert_eq!(
            extracted.snapshot_paths,
            vec!["src/unchanged.rs".to_string(), "src/changed.rs".to_string()]
        );
        assert_eq!(extracted.files_spooled, 2);
        let files = extracted.unwrap();
        assert_eq!(
            files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/unchanged.rs", "src/changed.rs"]
        );
        assert!(
            files
                .iter()
                .find(|file| file.path == "src/unchanged.rs")
                .unwrap()
                .symbols
                .is_empty(),
            "unchanged files must be represented in the snapshot without parser rows"
        );
        assert_eq!(
            files
                .iter()
                .find(|file| file.path == "src/changed.rs")
                .unwrap()
                .symbols
                .len(),
            1,
            "changed files must still use the extraction callback"
        );
    }

    #[test]
    fn extraction_runs_supported_files_concurrently_when_jobs_allow() {
        let fixture = ScanFixture::new();
        let mut targets = Vec::new();
        for i in 0..8 {
            let file = fixture.write(&format!("src/f{i}.rs"), &format!("pub fn f{i}() {{}}\n"));
            targets.push(SupportedFileTarget::new(file, "rust"));
        }

        let in_flight = std::sync::atomic::AtomicUsize::new(0);
        let max_in_flight = std::sync::atomic::AtomicUsize::new(0);

        let extracted = extract_supported_files_to_spool(
            request(fixture.root(), 4),
            &targets,
            ScanControls::default(),
            |_, target, language, indexed_at, snapshot| {
                let current = in_flight.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                max_in_flight.fetch_max(current, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(std::time::Duration::from_millis(50));
                in_flight.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                Ok(extracted_artifact_file(
                    target, language, indexed_at, snapshot,
                ))
            },
        )
        .unwrap();

        assert_eq!(extracted.snapshot_paths.len(), 8);
        let observed = max_in_flight.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            observed >= 2,
            "expected overlapping extraction with jobs=4, observed max concurrency {observed}"
        );
    }

    #[test]
    fn parallel_extraction_matches_single_threaded_reference() {
        let fixture = ScanFixture::new();
        let mut targets = Vec::new();
        for i in 0..40 {
            let file = fixture.write(
                &format!("src/module_{i:02}.rs"),
                &format!("pub struct S{i};\npub fn f{i}(x: i32) -> i32 {{ x + {i} }}\n"),
            );
            targets.push(SupportedFileTarget::new(file, "rust"));
        }

        let reference = extract_supported_files_to_spool(
            request(fixture.root(), 1),
            &targets,
            ScanControls::default(),
            extract_artifact_file_from_snapshot,
        )
        .unwrap()
        .unwrap();

        let parallel = extract_supported_files_to_spool(
            request(fixture.root(), 8),
            &targets,
            ScanControls::default(),
            extract_artifact_file_from_snapshot,
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            parallel, reference,
            "parallel extraction must produce byte-identical artifacts to the single-threaded path"
        );
        assert_eq!(
            parallel
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>(),
            targets
                .iter()
                .map(|target| target.target.root_relative_path.clone())
                .collect::<Vec<_>>(),
            "spooled files must follow input target order regardless of worker scheduling"
        );
    }

    #[test]
    fn select_extraction_pool_uses_the_requested_pool_when_it_builds() {
        let mut attempts = Vec::new();
        let pool: Result<usize, String> = select_extraction_pool(4, |threads| {
            attempts.push(threads);
            Ok(threads)
        });

        assert_eq!(pool, Ok(4));
        assert_eq!(attempts, vec![4]);
    }

    #[test]
    fn select_extraction_pool_retries_single_threaded_when_the_requested_pool_fails() {
        let mut attempts = Vec::new();
        let pool = select_extraction_pool(4, |threads| {
            attempts.push(threads);
            if threads == 4 {
                Err("resource exhausted".to_string())
            } else {
                Ok(threads)
            }
        });

        assert_eq!(pool, Ok(1));
        assert_eq!(attempts, vec![4, 1]);
    }

    #[test]
    fn select_extraction_pool_retries_single_threaded_when_auto_detection_fails() {
        let mut attempts = Vec::new();
        let pool = select_extraction_pool(0, |threads| {
            attempts.push(threads);
            if threads == 0 {
                Err("resource exhausted".to_string())
            } else {
                Ok(threads)
            }
        });

        assert_eq!(pool, Ok(1));
        assert_eq!(attempts, vec![0, 1]);
    }

    #[test]
    fn select_extraction_pool_fails_when_the_single_threaded_retry_also_fails() {
        let mut attempts = Vec::new();
        let pool: Result<usize, String> = select_extraction_pool(4, |threads| {
            attempts.push(threads);
            Err(format!("failed at {threads}"))
        });

        assert_eq!(pool, Err("failed at 1".to_string()));
        assert_eq!(attempts, vec![4, 1]);
    }

    #[test]
    fn select_extraction_pool_does_not_retry_a_single_threaded_request() {
        let mut attempts = Vec::new();
        let pool: Result<usize, String> = select_extraction_pool(1, |threads| {
            attempts.push(threads);
            Err("resource exhausted".to_string())
        });

        assert_eq!(pool, Err("resource exhausted".to_string()));
        assert_eq!(attempts, vec![1]);
    }

    #[test]
    fn extraction_pool_unavailable_diagnostic_is_fatal_and_names_the_requested_jobs() {
        let diagnostic = extraction_pool_unavailable_diagnostic(4, "resource exhausted");

        assert_eq!(diagnostic.code, ReportCode::InternalError);
        assert!(!diagnostic.recoverable);
        assert!(diagnostic.message.contains("resource exhausted"));
        assert_eq!(diagnostic.details["requested_jobs"], 4);
    }

    #[test]
    fn parallel_spool_orders_failed_unchanged_and_changed_by_target() {
        let fixture = ScanFixture::new();
        let changed = fixture.write("src/a_changed.rs", "pub fn a() {}\n");
        let unchanged = fixture.write("src/b_unchanged.rs", "pub fn b() {}\n");
        let bad_path = fixture.root().join("src/c_bad.rs");
        std::fs::write(&bad_path, [0xff, 0xfe, 0x00]).unwrap();
        let bad = FileTarget {
            absolute_path: bad_path,
            root_relative_path: "src/c_bad.rs".to_string(),
        };

        let unchanged_snapshot = read_source_snapshot(&unchanged).unwrap();
        let mut existing_hashes = BTreeMap::new();
        existing_hashes.insert(
            unchanged.root_relative_path.clone(),
            unchanged_snapshot.content_hash.clone(),
        );

        let targets = vec![
            SupportedFileTarget::new(changed.clone(), "rust"),
            SupportedFileTarget::new(unchanged.clone(), "rust"),
            SupportedFileTarget::new(bad.clone(), "rust"),
        ];

        let mut extracted = extract_supported_files_to_spool(
            ExtractionRequest {
                root: fixture.root(),
                indexed_at: "2026-06-01T00:00:00Z".to_string(),
                existing_content_hashes: Some(&existing_hashes),
                force: false,
                jobs: 8,
                level: ExtractionLevel::Full,
            },
            &targets,
            ScanControls::default(),
            extract_artifact_file_from_snapshot,
        )
        .unwrap();

        assert_eq!(
            extracted.snapshot_paths,
            vec![
                "src/a_changed.rs".to_string(),
                "src/b_unchanged.rs".to_string(),
                "src/c_bad.rs".to_string(),
            ],
            "snapshot paths must stay in input target order"
        );
        assert_eq!(
            extracted.errors.len(),
            1,
            "only the malformed file should error"
        );
        assert_eq!(extracted.errors[0].root_relative_path, "src/c_bad.rs");

        extracted.spool.file_spool_mut().finish().unwrap();
        let files = extracted
            .spool
            .file_spool()
            .iter()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            files
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>(),
            vec![
                "src/a_changed.rs".to_string(),
                "src/b_unchanged.rs".to_string(),
                "src/c_bad.rs".to_string(),
            ],
            "spooled files must stay in input target order"
        );
        let changed_file = files.iter().find(|f| f.path == "src/a_changed.rs").unwrap();
        assert!(
            !changed_file.symbols.is_empty(),
            "changed files must be extracted"
        );
        let unchanged_file = files
            .iter()
            .find(|f| f.path == "src/b_unchanged.rs")
            .unwrap();
        assert!(
            unchanged_file.symbols.is_empty(),
            "unchanged files must skip the parser"
        );
        let failed_file = files.iter().find(|f| f.path == "src/c_bad.rs").unwrap();
        assert_eq!(failed_file.status, FileStatus::FailedPreserved);
    }

    #[test]
    fn force_scan_ignores_existing_hashes_and_extracts_all_supported_files() {
        let fixture = ScanFixture::new();
        let left = fixture.write("src/left.rs", "pub fn left() {}\n");
        let right = fixture.write("src/right.rs", "pub fn right() {}\n");
        let mut existing_hashes = BTreeMap::new();
        existing_hashes.insert(
            left.root_relative_path.clone(),
            read_source_snapshot(&left).unwrap().content_hash,
        );
        existing_hashes.insert(
            right.root_relative_path.clone(),
            read_source_snapshot(&right).unwrap().content_hash,
        );
        let extracted_paths = RefCell::new(Vec::new());

        let files = extract_supported_files(
            fixture.root(),
            &[
                SupportedFileTarget::new(left.clone(), "rust"),
                SupportedFileTarget::new(right.clone(), "rust"),
            ],
            "2026-06-01T00:00:00Z".to_string(),
            Some(&existing_hashes),
            true,
            |_, target, language, indexed_at, snapshot| {
                extracted_paths
                    .borrow_mut()
                    .push(target.root_relative_path.clone());
                Ok(extracted_artifact_file(
                    target, language, indexed_at, snapshot,
                ))
            },
        )
        .unwrap();

        assert_eq!(
            extracted_paths.into_inner(),
            vec!["src/left.rs", "src/right.rs"]
        );
        assert_eq!(
            files
                .iter()
                .map(|file| file.symbols.len())
                .collect::<Vec<_>>(),
            vec![1, 1]
        );
    }

    #[test]
    fn metadata_fingerprints_change_when_snapshot_rows_change() {
        let mut parser_inventory = vec![ArtifactParserInventoryRow {
            language: "rust".to_string(),
            parser_package: "tree-sitter-rust".to_string(),
            parser_version: Some("1.0.0".to_string()),
            grammar_version: Some("2.0.0".to_string()),
            source: Some("test".to_string()),
            metadata: Some(json!({"dependency_status": "available"})),
        }];
        let parser = parser_inventory_fingerprint(&parser_inventory);
        parser_inventory[0].grammar_version = Some("2.0.1".to_string());
        let changed_parser = parser_inventory_fingerprint(&parser_inventory);

        assert_sha256(&parser);
        assert_sha256(&changed_parser);
        assert_ne!(parser, changed_parser);

        let mut languages = vec![ArtifactLanguageCapabilityRow {
            language: "rust".to_string(),
            parser_package: "tree-sitter-rust".to_string(),
            extensions: vec!["rs".to_string()],
            dependency_status: "available".to_string(),
            target_capabilities: capability_flags(true),
            actual_capabilities: capability_flags(true),
            kind_coverage: json!({"symbols": {"supported": ["function"]}}),
            fixtures: vec![ArtifactLanguageCapabilityFixtureRow {
                fixture_name: "basic".to_string(),
                source_path: "fixtures/rust/basic.rs".to_string(),
                expected_path: "fixtures/rust/basic.json".to_string(),
            }],
            gaps: vec![ArtifactLanguageCapabilityGapRow {
                gap_id: "rust:types".to_string(),
                capability: "types".to_string(),
                status: CapabilityGapStatus::Open,
                reason: "test gap".to_string(),
                required_closure: "task".to_string(),
                evidence: json!({"source": "test"}),
            }],
        }];
        let capabilities = capability_snapshot_fingerprint(&languages);
        languages[0].actual_capabilities.types = false;
        let changed_capabilities = capability_snapshot_fingerprint(&languages);

        assert_sha256(&capabilities);
        assert_sha256(&changed_capabilities);
        assert_ne!(capabilities, changed_capabilities);
    }

    #[test]
    fn an_aborting_scan_never_unlinks_the_artifact_it_reports_as_untouched() {
        let temp = TempDir::new().unwrap();
        let db = temp.path().join("artifact.sqlite");
        std::fs::write(&db, b"unopenable artifact").unwrap();
        std::fs::write(temp.path().join("artifact.sqlite-wal"), b"wal").unwrap();

        let aborted = abort_before_full_rebuild(&db, true, || Some(parent_exited_outcome()));

        assert!(aborted.is_some());
        assert!(
            db.exists(),
            "parent_exited is documented as leaving the artifact untouched"
        );
        assert!(temp.path().join("artifact.sqlite-wal").exists());
    }

    #[test]
    fn a_full_rebuild_that_is_not_aborting_clears_the_artifact_and_its_sidecars() {
        let temp = TempDir::new().unwrap();
        let db = temp.path().join("artifact.sqlite");
        std::fs::write(&db, b"unopenable artifact").unwrap();
        std::fs::write(temp.path().join("artifact.sqlite-wal"), b"wal").unwrap();

        let aborted = abort_before_full_rebuild(&db, true, || None);

        assert!(aborted.is_none());
        assert!(!db.exists());
        assert!(!temp.path().join("artifact.sqlite-wal").exists());
    }

    fn parent_exited_outcome() -> CommandOutcome {
        let watchdog = ParentWatchdog::tripped(1);
        let controls = ScanControls {
            watchdog: Some(&watchdog),
            ..ScanControls::default()
        };
        controls
            .parent_exited_outcome(
                Some(2),
                ReportMode::Force,
                &artifact_input(Path::new("artifact.sqlite"), None, None, None),
                true,
                Instant::now(),
                &BTreeMap::new(),
            )
            .unwrap()
    }

    #[test]
    fn a_tripped_parent_watchdog_stops_extraction_before_the_first_chunk() {
        let fixture = ScanFixture::new();
        let targets = (0..4)
            .map(|index| {
                SupportedFileTarget::new(
                    fixture.write(
                        &format!("src/f{index}.rs"),
                        &format!("pub fn f{index}() {{}}\n"),
                    ),
                    "rust",
                )
            })
            .collect::<Vec<_>>();
        let watchdog = ParentWatchdog::tripped(1);
        let controls = ScanControls {
            watchdog: Some(&watchdog),
            ..ScanControls::default()
        };

        let extracted = extract_supported_files_to_spool(
            request(fixture.root(), 1),
            &targets,
            controls,
            extract_artifact_file_from_snapshot,
        )
        .unwrap();

        assert_eq!(extracted.files_spooled, 0);
        assert!(extracted.snapshot_paths.is_empty());
        assert!(extracted.errors.is_empty());
        assert_eq!(
            extracted.completion,
            SpoolCompletion::ParentExited {
                observed_parent_pid: Some(1)
            },
            "a partial spool must say so in the return value, not leave the caller to re-derive it"
        );
    }

    #[test]
    fn a_finished_extraction_pass_reports_a_complete_spool() {
        let fixture = ScanFixture::new();
        let target =
            SupportedFileTarget::new(fixture.write("src/only.rs", "pub fn only() {}\n"), "rust");

        let extracted = extract_supported_files_to_spool(
            request(fixture.root(), 1),
            &[target],
            ScanControls::default(),
            extract_artifact_file_from_snapshot,
        )
        .unwrap();

        assert_eq!(extracted.completion, SpoolCompletion::Complete);
        assert_eq!(extracted.files_spooled, 1);
    }

    #[test]
    fn a_spool_dir_that_swallows_source_is_warned_about_and_one_outside_is_not() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("repo");
        let source = root.join("src");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("lib.rs"), "pub fn lib() {}\n").unwrap();

        let inside = spool_dir_inside_root_warning(&source, &root).unwrap();
        assert_eq!(inside.code, ReportCode::SpoolDirExcluded);
        assert_eq!(inside.root_relative_path.as_deref(), Some("src"));
        assert_eq!(inside.path.as_deref(), Some(display_path(&source).as_str()));

        assert!(
            spool_dir_inside_root_warning(&temp.path().join("spools"), &root).is_none(),
            "a spool dir outside the root excludes nothing"
        );
        assert!(
            spool_dir_inside_root_warning(&root, &root).is_none(),
            "a spool dir that is the root only skips spool-shaped names"
        );
    }

    #[test]
    fn a_dedicated_scratch_spool_dir_inside_the_root_never_warns() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("repo");
        let scratch = root.join(".miller").join("spool");
        std::fs::create_dir_all(&scratch).unwrap();

        assert!(
            spool_dir_inside_root_warning(&scratch, &root).is_none(),
            "an empty scratch directory excludes nothing"
        );

        for name in [
            owned_spool_file_name(11, 1_754_000_000_000_000_000),
            sentinel_file_name(11, 1_754_000_000_000_000_000),
            unowned_spool_file_name(22, 1_754_000_000_000_000_001),
        ] {
            std::fs::write(scratch.join(name), b"{}\n").unwrap();
        }
        assert!(
            spool_dir_inside_root_warning(&scratch, &root).is_none(),
            "leftover spools are the flag doing its job, not content the scan lost"
        );

        std::fs::write(scratch.join(".DS_Store"), b"\0\0").unwrap();
        assert!(
            spool_dir_inside_root_warning(&scratch, &root).is_none(),
            "metadata the filesystem left behind is not content the operator put there"
        );

        std::fs::create_dir(scratch.join(".hidden")).unwrap();
        assert!(
            spool_dir_inside_root_warning(&scratch, &root).is_some(),
            "discovery does not skip dot-directories, so neither may this probe"
        );
        std::fs::remove_dir(scratch.join(".hidden")).unwrap();

        std::fs::write(scratch.join("notes.md"), b"# real content\n").unwrap();
        assert!(
            spool_dir_inside_root_warning(&scratch, &root).is_some(),
            "one entry the scan would have read is the hazard the warning names"
        );
    }

    #[test]
    fn an_unreadable_spool_dir_inside_the_root_does_not_warn() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();

        assert!(
            spool_dir_inside_root_warning(&root.join("absent"), &root).is_none(),
            "a missing signal is not evidence that content was excluded"
        );
    }

    #[test]
    fn a_scan_that_fails_before_it_writes_still_carries_the_spool_dir_warning() {
        let fixture = ScanFixture::new();
        fixture.write("src/a.rs", "pub fn a() {}\n");
        let output = TempDir::new().unwrap();
        let db = output.path().join("artifact.sqlite");
        std::fs::create_dir(&db).unwrap();

        let outcome = scan(ScanArgs {
            root: fixture.root().to_path_buf(),
            db,
            force: true,
            ignore_files: Vec::new(),
            strict_schema: false,
            json: true,
            jobs: 1,
            spool_dir: Some(fixture.root().join("src")),
            progress_file: None,
            parent_pid: None,
            level: None,
        });

        assert_eq!(outcome.exit_code, 1, "the artifact could not be opened");
        assert!(
            outcome
                .warning_codes()
                .contains(&ReportCode::SpoolDirExcluded),
            "a failing run is the one an operator reads closely: {:?}",
            outcome.warning_codes()
        );
    }

    #[test]
    fn an_unlockable_spool_dir_warns_that_leak_protection_is_inert() {
        let warning = spool_lock_unavailable_warning(Some(Path::new("/mnt/scratch/spools")));

        assert_eq!(warning.code, ReportCode::SpoolLockUnavailable);
        assert_eq!(warning.path.as_deref(), Some("/mnt/scratch/spools"));
        assert!(warning.recoverable);
    }

    #[test]
    fn scan_text_mode_runs_no_group_by_file_id_query() {
        let _guard = crate::artifact_access::SCAN_ATTRIBUTION_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let fixture = ScanFixture::new();
        fixture.write("src/a.rs", "pub fn a() {}\n");
        fixture.write("src/b.rs", "pub fn b() {}\n");
        let db = fixture.root().join("artifact.sqlite");

        crate::artifact_access::ATTRIBUTION_CALL_COUNT
            .store(0, std::sync::atomic::Ordering::SeqCst);
        crate::artifact_access::ATTRIBUTION_GROUP_BY_QUERY_COUNT
            .store(0, std::sync::atomic::Ordering::SeqCst);

        let outcome = scan(ScanArgs {
            root: fixture.root().to_path_buf(),
            db: db.clone(),
            force: true,
            ignore_files: Vec::new(),
            strict_schema: false,
            json: false,
            jobs: 1,
            spool_dir: None,
            progress_file: None,
            parent_pid: None,
            level: None,
        });

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(
            crate::artifact_access::ATTRIBUTION_CALL_COUNT
                .load(std::sync::atomic::Ordering::SeqCst),
            0,
            "text scan must not call file_row_attribution"
        );
        assert_eq!(
            crate::artifact_access::ATTRIBUTION_GROUP_BY_QUERY_COUNT
                .load(std::sync::atomic::Ordering::SeqCst),
            0,
            "text scan must execute 0 GROUP BY file_id queries"
        );
    }

    #[test]
    fn scan_json_mode_executes_group_by_file_id_queries() {
        let _guard = crate::artifact_access::SCAN_ATTRIBUTION_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let fixture = ScanFixture::new();
        fixture.write("src/a.rs", "pub fn a() {}\n");
        fixture.write("src/b.rs", "pub fn b() {}\n");
        let db = fixture.root().join("artifact.sqlite");

        crate::artifact_access::ATTRIBUTION_CALL_COUNT
            .store(0, std::sync::atomic::Ordering::SeqCst);
        crate::artifact_access::ATTRIBUTION_GROUP_BY_QUERY_COUNT
            .store(0, std::sync::atomic::Ordering::SeqCst);

        let outcome = scan(ScanArgs {
            root: fixture.root().to_path_buf(),
            db: db.clone(),
            force: true,
            ignore_files: Vec::new(),
            strict_schema: false,
            json: true,
            jobs: 1,
            spool_dir: None,
            progress_file: None,
            parent_pid: None,
            level: None,
        });

        assert_eq!(outcome.exit_code, 0);
        assert_eq!(
            crate::artifact_access::ATTRIBUTION_CALL_COUNT
                .load(std::sync::atomic::Ordering::SeqCst),
            1,
            "json scan must call file_row_attribution"
        );
        assert_eq!(
            crate::artifact_access::ATTRIBUTION_GROUP_BY_QUERY_COUNT
                .load(std::sync::atomic::Ordering::SeqCst),
            14,
            "json scan must execute all 14 GROUP BY file_id queries"
        );
    }

    #[test]
    fn extraction_reports_advancing_counters_to_the_progress_file() {
        let fixture = ScanFixture::new();
        let progress_path = fixture.root().join("scan.progress");
        let progress = ScanProgress::create_with_interval(&progress_path, Duration::ZERO).unwrap();
        let targets = (0..4)
            .map(|index| {
                SupportedFileTarget::new(
                    fixture.write(
                        &format!("src/f{index}.rs"),
                        &format!("pub fn f{index}() {{}}\n"),
                    ),
                    "rust",
                )
            })
            .collect::<Vec<_>>();
        let controls = ScanControls {
            progress: Some(&progress),
            ..ScanControls::default()
        };

        extract_supported_files_to_spool(
            request(fixture.root(), 1),
            &targets,
            controls,
            extract_artifact_file_from_snapshot,
        )
        .unwrap();

        let records = std::fs::read_to_string(&progress_path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert!(!records.is_empty());
        let extracted = records
            .iter()
            .map(|record| record["files_extracted"].as_u64().unwrap())
            .collect::<Vec<_>>();
        assert!(extracted.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(
            records.last().unwrap()["files_spooled"].as_u64(),
            Some(targets.len() as u64)
        );
    }

    #[test]
    fn extraction_without_controls_writes_no_progress_records() {
        let fixture = ScanFixture::new();
        let target =
            SupportedFileTarget::new(fixture.write("src/only.rs", "pub fn only() {}\n"), "rust");

        extract_supported_files_to_spool(
            request(fixture.root(), 1),
            &[target],
            ScanControls::default(),
            extract_artifact_file_from_snapshot,
        )
        .unwrap();

        let stray = std::fs::read_dir(fixture.root())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "src")
            .collect::<Vec<_>>();
        assert!(stray.is_empty(), "no side files should appear: {stray:?}");
    }

    fn request(root: &Path, jobs: usize) -> ExtractionRequest<'_> {
        ExtractionRequest {
            root,
            indexed_at: "2026-06-01T00:00:00Z".to_string(),
            existing_content_hashes: None,
            force: false,
            jobs,
            level: ExtractionLevel::Full,
        }
    }

    struct ScanFixture {
        temp: TempDir,
    }

    impl ScanFixture {
        fn new() -> Self {
            Self {
                temp: TempDir::new().unwrap(),
            }
        }

        fn root(&self) -> &Path {
            self.temp.path()
        }

        fn write(&self, relative_path: &str, contents: &str) -> FileTarget {
            let absolute_path = self.root().join(relative_path);
            std::fs::create_dir_all(absolute_path.parent().unwrap()).unwrap();
            std::fs::write(&absolute_path, contents).unwrap();
            FileTarget {
                absolute_path,
                root_relative_path: relative_path.to_string(),
            }
        }
    }

    fn extracted_artifact_file(
        target: &FileTarget,
        language: String,
        indexed_at: String,
        snapshot: SourceSnapshot,
    ) -> ArtifactFile {
        ArtifactFile {
            file_id: format!("extracted-{}", target.root_relative_path),
            path: target.root_relative_path.clone(),
            language,
            content_hash: snapshot.content_hash,
            content_bytes: snapshot.content_bytes,
            line_count: snapshot.line_count,
            indexed_at,
            status: FileStatus::Indexed,
            metadata_json: None,
            symbols: vec![ArtifactSymbol {
                symbol_id: format!("symbol-{}", target.root_relative_path),
                name: "extracted".to_string(),
                ..ArtifactSymbol::default()
            }],
            symbol_annotations: Vec::new(),
            identifiers: Vec::new(),
            relationships: Vec::new(),
            pending_relationships: Vec::new(),
            type_facts: Vec::new(),
            type_argument_usages: Vec::new(),
            type_arguments: Vec::new(),
            literals: Vec::new(),
            source_regions: Vec::new(),
            structural_facts: Vec::new(),
            complexity_metrics: Vec::new(),
            parse_diagnostics: Vec::new(),
        }
    }

    fn capability_flags(value: bool) -> ArtifactCapabilityFlags {
        ArtifactCapabilityFlags {
            symbols: value,
            relationships: value,
            pending_relationships: value,
            identifiers: value,
            types: value,
        }
    }

    fn assert_sha256(value: &str) {
        assert!(value.starts_with("sha256:"));
        assert_eq!(value.len(), "sha256:".len() + 64);
    }
}
