use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use julie_extract_artifact::jsonl::{JSONL_SCHEMA_VERSION, export_jsonl, export_jsonl_to_path};
use julie_extract_artifact::metadata::ArtifactMetadata;
use julie_extract_artifact::model::{
    ArtifactFile, RevisionChangeKind, RevisionInput, WriteMode, WriteOperation, WriteResult,
};
use julie_extract_artifact::reports::{
    ReportCode, ReportInput, ReportLanguageProfile, ReportMode, ReportOperation, ReportProfile,
    ReportRevision, ReportStatus, RowDomainCounts,
};
use julie_extract_artifact::writer::{
    ArtifactFileSpool, ArtifactSpoolError, ArtifactWriteError, ArtifactWriter,
};
use julie_extractors::{
    capability_snapshot as extractor_capability_snapshot, detect_language_for_source,
};
use rayon::prelude::*;
use serde_json::json;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::args::{
    Cli, Command, DeleteArgs, ExportArgs, InfoArgs, LanguagesArgs, ScanArgs, UpdateArgs,
};
use crate::artifact_access::{
    ExistingArtifact, artifact_report_from_connection, existing_artifact_for_root,
    file_row_attribution, jsonl_counts, latest_revision_id, load_existing_content_hashes,
    open_artifact, open_artifact_for_info, open_artifact_for_root, table_totals,
};
use crate::capability_snapshot::{
    artifact_capability_snapshot, current_capability_fingerprints, flags, kind_coverage_json,
    structural_fact_patterns_json,
};
use crate::discovery::{
    DiscoveryPolicy, FileSelection, UnsupportedReason, canonicalize_ignore_files,
};
use crate::extraction::{
    ExtractFileError, SourceSnapshot, extract_artifact_file, extract_artifact_file_from_snapshot,
    failed_artifact_file, read_source_snapshot, unchanged_artifact_file,
};
use crate::paths::{
    FileTarget, canonicalize_db_path, canonicalize_root, canonicalize_update_file,
    normalize_delete_file,
};
use crate::reports::{
    CommandOutcome, PathErrorInput, ReportBuilder, ReportStream, artifact_input, base_report,
    diagnostic, discovery_error_diagnostic, display_path, extract_error_diagnostic,
    extract_error_outcome, outcome, path_error_outcome, path_error_outcome_with_paths,
    slow_file_skipped_diagnostic, spool_error_outcome, write_error_outcome,
    write_error_outcome_with_profile, write_outcome,
};

const SCAN_REPORT_FILE_ROW_LIMIT: usize = 20;

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
    write_outcome(&outcome);
    ExitCode::from(outcome.exit_code)
}

fn run(cli: Cli) -> CommandOutcome {
    match cli.command {
        Command::Scan(args) => scan(args),
        Command::Update(args) => update(args),
        Command::Delete(args) => delete(args),
        Command::Info(args) => info(args),
        Command::Export(args) => export(args),
        Command::Languages(args) => languages(args),
    }
}

fn scan(args: ScanArgs) -> CommandOutcome {
    let scan_started = Instant::now();
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
    let input = artifact_input(&db, Some(&root), None, None);

    let existing_artifact_started = Instant::now();
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
    let existing_scan_metadata = existing_scan_artifact.map(|artifact| artifact.write_metadata);
    record_profile_phase(
        &mut profile_phases,
        "existing_artifact",
        existing_artifact_started.elapsed(),
    );

    let discovery_started = Instant::now();
    let discovery = match DiscoveryPolicy::build(&root, &db, &ignore_files) {
        Ok(discovery) => discovery,
        Err(error) => return path_error_outcome(error, ReportOperation::Scan, mode, args.json),
    };
    let discovered = discovery.discover();
    let preserved_missing_paths = discovered
        .errors
        .iter()
        .chain(discovered.slow_file_skips.iter())
        .map(|error| error.root_relative_path.clone())
        .collect::<Vec<_>>();
    record_profile_phase(
        &mut profile_phases,
        "discovery",
        discovery_started.elapsed(),
    );

    let force_metadata_started = Instant::now();
    let force_existing_metadata = if args.force && db.exists() {
        match open_artifact(&db, args.strict_schema, Some(JSONL_SCHEMA_VERSION)) {
            Ok(artifact) if artifact.report.root_path == display_path(&root) => {
                Some(artifact.write_metadata)
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
    let indexed_at = now_rfc3339();
    let extraction_spool_started = Instant::now();
    let mut extracted = match spool_discovered_files(
        &root,
        &discovery,
        &discovered.supported_files,
        indexed_at,
        existing_content_hashes.as_ref(),
        args.force,
        args.jobs,
    ) {
        Ok(extracted) => extracted,
        Err(error) => {
            return spool_error_outcome(error, ReportOperation::Scan, mode, input, args.json);
        }
    };
    record_profile_phase(
        &mut profile_phases,
        "extraction_spool",
        extraction_spool_started.elapsed(),
    );
    debug_assert_eq!(extracted.files_spooled, extracted.snapshot_paths.len());
    let profile_languages = extracted.profile.languages.clone();

    if should_rebuild_db {
        remove_artifact_files(&db);
    }
    let db_existed_before_write = db.exists();

    let metadata = force_existing_metadata
        .or(existing_scan_metadata)
        .map(refreshed_metadata)
        .unwrap_or_else(|| new_artifact_metadata(&root, None));

    let writer_open_started = Instant::now();
    match ArtifactWriter::open_path(&db, metadata) {
        Ok(mut writer) => {
            record_profile_phase(
                &mut profile_phases,
                "writer_open",
                writer_open_started.elapsed(),
            );
            writer.stage_capability_snapshot(artifact_capability_snapshot());
            let artifact_write_started = Instant::now();
            let mut resolution_report: Option<crate::resolution::ResolutionReport> = None;
            match writer.write_scan_spooled_preserving_missing_paths_with_resolution(
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
                &mut extracted.spool,
                |tx, scope| {
                    let (counts, report) = crate::resolution::resolve_workspace(tx, scope)?;
                    resolution_report = Some(report);
                    Ok(counts)
                },
            ) {
                Ok(write_result) => {
                    record_profile_phase(
                        &mut profile_phases,
                        "artifact_write",
                        artifact_write_started.elapsed(),
                    );
                    let capability_rows_written = writer.last_capability_rows_written();
                    let connection = writer.connection();
                    // Persist the durable `reference_resolution_*` metadata before
                    // reading totals so the scan report reflects it.
                    crate::resolution::finalize_resolution_metadata(
                        connection,
                        &write_result,
                        resolution_report.as_ref(),
                    );
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
                    let has_errors = !extracted.errors.is_empty() || !discovered.errors.is_empty();
                    let status = if has_errors {
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
                        .with_totals(table_totals(connection))
                        .with_profile(scan_profile(
                            scan_started,
                            &profile_phases,
                            &profile_languages,
                        ));
                    if let Some(section) = crate::reports::resolution_report_section(
                        resolution_report.as_ref(),
                        &write_result,
                    ) {
                        report = report.with_languages(section);
                    }
                    if let Some(message) = &write_result.resolution.failed {
                        report = report.with_warning(diagnostic(
                            ReportCode::ResolutionFailed,
                            format!(
                                "reference resolution failed; affected rows left unresolved: {message}"
                            ),
                            None,
                            None,
                            true,
                            json!({}),
                        ));
                    }
                    report.counts.files_scanned =
                        (discovered.supported_files.len() + discovered.unsupported_files) as i64;
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
                    let file_rows =
                        file_row_attribution(connection, Some(SCAN_REPORT_FILE_ROW_LIMIT));
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
                    let exit_code = if has_errors { 1 } else { 0 };
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
            return skip_oversized_update(&db, &root, target, args.json);
        }
        FileSelection::Unsupported { .. } => {
            return cleanup_unsupported_update(&db, &root, target, existing_artifact, args.json);
        }
    };

    let file = match extract_artifact_file(&root, &target, language, now_rfc3339()) {
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
            let mut resolution_report: Option<crate::resolution::ResolutionReport> = None;
            match writer.write_update_with_resolution(
                revision_input(WriteOperation::Update, Some(WriteMode::SingleFile), &root),
                &file,
                |tx, scope| {
                    let (counts, report) = crate::resolution::resolve_workspace(tx, scope)?;
                    resolution_report = Some(report);
                    Ok(counts)
                },
            ) {
                Ok(write_result) => {
                    let capability_rows_written = writer.last_capability_rows_written();
                    let connection = writer.connection();
                    crate::resolution::finalize_resolution_metadata(
                        connection,
                        &write_result,
                        resolution_report.as_ref(),
                    );
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
                    if let Some(section) = crate::reports::resolution_report_section(
                        resolution_report.as_ref(),
                        &write_result,
                    ) {
                        report = report.with_languages(section);
                    }
                    if let Some(message) = &write_result.resolution.failed {
                        report = report.with_warning(diagnostic(
                            ReportCode::ResolutionFailed,
                            format!(
                                "reference resolution failed; affected rows left unresolved: {message}"
                            ),
                            None,
                            None,
                            true,
                            json!({}),
                        ));
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

    match open_artifact(&args.db, args.strict_schema, Some(JSONL_SCHEMA_VERSION)) {
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
    }))
    .with_structural_fact_patterns(structural_fact_patterns_json());
    outcome(report, 0, args.json, ReportStream::Stdout)
}

fn spool_discovered_files(
    root: &Path,
    discovery: &DiscoveryPolicy,
    targets: &[FileTarget],
    indexed_at: String,
    existing_content_hashes: Option<&BTreeMap<String, String>>,
    force: bool,
    jobs: usize,
) -> Result<SpooledExtractedFiles, ArtifactSpoolError> {
    let mut supported_targets = Vec::with_capacity(targets.len());
    for target in targets {
        if let FileSelection::Supported { language } = discovery.select_file(target) {
            supported_targets.push(SupportedFileTarget::new(target.clone(), language));
        }
    }
    extract_supported_files_to_spool(
        root,
        &supported_targets,
        indexed_at,
        existing_content_hashes,
        force,
        jobs,
        extract_artifact_file_from_snapshot,
    )
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
    spool: ArtifactFileSpool,
    snapshot_paths: Vec<String>,
    files_spooled: usize,
    errors: Vec<ExtractFileError>,
    profile: ScanExtractionProfile,
}

impl SpooledExtractedFiles {
    #[cfg(test)]
    fn unwrap(mut self) -> Vec<ArtifactFile> {
        assert!(
            self.errors.is_empty(),
            "expected extraction to succeed without per-file errors: {:?}",
            self.errors
        );
        self.spool.finish().unwrap();
        self.spool
            .iter()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }
}

impl Drop for SpooledExtractedFiles {
    fn drop(&mut self) {
        let _ = self.spool.finish();
        let _ = std::fs::remove_file(self.spool.path());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SupportedFileTarget {
    target: FileTarget,
    language: String,
}

impl SupportedFileTarget {
    fn new(target: FileTarget, language: impl Into<String>) -> Self {
        Self {
            target,
            language: language.into(),
        }
    }
}

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

fn extract_supported_files_to_spool(
    root: &Path,
    targets: &[SupportedFileTarget],
    indexed_at: String,
    existing_content_hashes: Option<&BTreeMap<String, String>>,
    force: bool,
    jobs: usize,
    extract: impl Fn(
        &Path,
        &FileTarget,
        String,
        String,
        SourceSnapshot,
    ) -> Result<ArtifactFile, ExtractFileError>
    + Sync,
) -> Result<SpooledExtractedFiles, ArtifactSpoolError> {
    let mut spool = create_scan_spool()?;
    let mut snapshot_paths = Vec::with_capacity(targets.len());
    let mut errors = Vec::new();
    let mut profile = ScanExtractionProfile::default();

    // `num_threads(0)` lets rayon pick from available parallelism. If the pool
    // cannot be built we fall back to rayon's global pool rather than failing the scan.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .ok();

    for chunk in targets.chunks(EXTRACT_SPOOL_CHUNK_SIZE) {
        // Extract every file in the chunk in parallel. `collect` into a Vec preserves
        // chunk order, so the serial drain below stays byte-identical to a sequential scan.
        let map_chunk = || {
            chunk
                .par_iter()
                .map(|supported| {
                    compute_file_outcome(
                        root,
                        supported,
                        &indexed_at,
                        existing_content_hashes,
                        force,
                        &extract,
                    )
                })
                .collect::<Vec<FileOutcome>>()
        };
        let outcomes = match &pool {
            Some(pool) => pool.install(map_chunk),
            None => map_chunk(),
        };

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
            push_profiled_spool(&mut spool, &mut profile, &outcome.language, &outcome.file)?;
            if let Some(error) = outcome.error {
                errors.push(error);
            }
        }
    }

    let files_spooled = spool.len();
    Ok(SpooledExtractedFiles {
        spool,
        snapshot_paths,
        files_spooled,
        errors,
        profile,
    })
}

fn create_scan_spool() -> Result<ArtifactFileSpool, ArtifactSpoolError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "julie-extract-scan-spool-{}-{nanos}.jsonl",
        std::process::id()
    ));
    ArtifactFileSpool::create(path)
}

/// Handle `update` on an otherwise-supported source file that exceeds
/// [`crate::limits::MAX_SOURCE_FILE_BYTES`]. The file is skipped exactly as a
/// full scan skips it: a typed `slow_file_skipped` warning is emitted and the
/// file's existing artifact rows are left untouched. Deleting rows is reserved
/// for genuinely unsupported or ignored files, so this path never opens a writer.
fn skip_oversized_update(
    db: &Path,
    root: &Path,
    target: FileTarget,
    json_report: bool,
) -> CommandOutcome {
    let input = artifact_input(
        db,
        Some(root),
        Some(&target.absolute_path),
        Some(&target.root_relative_path),
    );
    let mut report = base_report(
        ReportStatus::NoChange,
        ReportOperation::Update,
        ReportMode::SingleFile,
        input,
    )
    .with_warning(diagnostic(
        ReportCode::SlowFileSkipped,
        crate::limits::slow_file_skip_message(),
        Some(display_path(&target.absolute_path)),
        Some(target.root_relative_path),
        true,
        json!({}),
    ));
    report.counts.files_scanned = 1;
    outcome(report, 0, json_report, ReportStream::Stdout)
}

fn cleanup_unsupported_update(
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
        let mut report = base_report(
            ReportStatus::Unsupported,
            ReportOperation::Update,
            ReportMode::SingleFile,
            input,
        )
        .with_warning(diagnostic(
            ReportCode::UnsupportedFile,
            "file is ignored or unsupported and no artifact rows exist",
            Some(display_path(&target.absolute_path)),
            Some(target.root_relative_path),
            true,
            json!({}),
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
        Ok((writer, write_result, capability_rows_written, resolution_report)) => {
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
            .with_warning(diagnostic(
                ReportCode::UnsupportedFile,
                if write_result.files_changed > 0 {
                    "file is ignored or unsupported; stale artifact rows were removed"
                } else {
                    "file is ignored or unsupported and no artifact rows exist"
                },
                Some(display_path(&target.absolute_path)),
                Some(root_relative_path),
                true,
                json!({}),
            ));
            if let Some(section) =
                crate::reports::resolution_report_section(resolution_report.as_ref(), &write_result)
            {
                report = report.with_languages(section);
            }
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
        Ok((writer, write_result, capability_rows_written, resolution_report)) => {
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
            if let Some(section) =
                crate::reports::resolution_report_section(resolution_report.as_ref(), &write_result)
            {
                report = report.with_languages(section);
            }
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

type RowRemovalResult = (
    ArtifactWriter,
    WriteResult,
    RowDomainCounts,
    Option<crate::resolution::ResolutionReport>,
);

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
    let mut resolution_report: Option<crate::resolution::ResolutionReport> = None;
    let result = match change_kind {
        RevisionChangeKind::Unsupported => writer.remove_unsupported_file_with_resolution(
            revision,
            root_relative_path,
            |tx, scope| {
                let (counts, report) = crate::resolution::resolve_workspace(tx, scope)?;
                resolution_report = Some(report);
                Ok(counts)
            },
        )?,
        RevisionChangeKind::Deleted => {
            writer.delete_file_with_resolution(revision, root_relative_path, |tx, scope| {
                let (counts, report) = crate::resolution::resolve_workspace(tx, scope)?;
                resolution_report = Some(report);
                Ok(counts)
            })?
        }
        RevisionChangeKind::Inserted | RevisionChangeKind::Updated => {
            unreachable!("row removal does not support inserted/updated change kinds")
        }
    };
    let capability_rows_written = writer.last_capability_rows_written();
    crate::resolution::finalize_resolution_metadata(
        writer.connection(),
        &result,
        resolution_report.as_ref(),
    );
    Ok((writer, result, capability_rows_written, resolution_report))
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

fn generated_artifact_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("artifact-{nanos}")
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

    use julie_extract_artifact::model::{
        ArtifactCapabilityFlags, ArtifactLanguageCapabilityFixtureRow,
        ArtifactLanguageCapabilityGapRow, ArtifactLanguageCapabilityRow,
        ArtifactParserInventoryRow, ArtifactSymbol, FileStatus,
    };
    use tempfile::TempDir;

    use crate::capability_snapshot::{
        capability_snapshot_fingerprint, parser_inventory_fingerprint,
    };

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
            fixture.root(),
            &[
                SupportedFileTarget::new(unchanged.clone(), "rust"),
                SupportedFileTarget::new(changed.clone(), "rust"),
            ],
            "2026-06-01T00:00:00Z".to_string(),
            Some(&existing_hashes),
            false,
            1,
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
            fixture.root(),
            &targets,
            "2026-06-01T00:00:00Z".to_string(),
            None,
            false,
            4,
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
            fixture.root(),
            &targets,
            "2026-06-01T00:00:00Z".to_string(),
            None,
            false,
            1,
            extract_artifact_file_from_snapshot,
        )
        .unwrap()
        .unwrap();

        let parallel = extract_supported_files_to_spool(
            fixture.root(),
            &targets,
            "2026-06-01T00:00:00Z".to_string(),
            None,
            false,
            8,
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
            fixture.root(),
            &targets,
            "2026-06-01T00:00:00Z".to_string(),
            Some(&existing_hashes),
            false,
            8,
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

        extracted.spool.finish().unwrap();
        let files = extracted
            .spool
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
                status: "open".to_string(),
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
