use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use julie_extract_artifact::jsonl::{
    JSONL_RECORD_KINDS, JSONL_SCHEMA_VERSION, export_jsonl, export_jsonl_to_path,
};
use julie_extract_artifact::metadata::{ArtifactMetadata, REQUIRED_METADATA_KEYS, read_metadata};
use julie_extract_artifact::model::{
    ArtifactCapabilityFlags, ArtifactCapabilitySnapshot, ArtifactFile,
    ArtifactLanguageCapabilityFixtureRow, ArtifactLanguageCapabilityGapRow,
    ArtifactLanguageCapabilityRow, ArtifactParserInventoryRow, RevisionChangeKind, RevisionInput,
    WriteMode, WriteOperation, WriteResult,
};
use julie_extract_artifact::reports::{
    ArtifactReport, Report, ReportCode, ReportCounts, ReportDiagnostic, ReportInput,
    ReportLanguageProfile, ReportMode, ReportOperation, ReportProfile, ReportRevision,
    ReportStatus, RowDomainCounts, ToolReport,
};
use julie_extract_artifact::schema::{EXTRACT_CONTRACT_VERSION, SQLITE_SCHEMA_VERSION};
use julie_extract_artifact::writer::{
    ArtifactFileSpool, ArtifactSpoolError, ArtifactWriteError, ArtifactWriter,
};
use julie_extractors::{
    CapabilityFlags, KindCoverage, capability_snapshot, detect_language_for_source,
};
use rayon::prelude::*;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::args::{
    Cli, Command, DeleteArgs, ExportArgs, InfoArgs, LanguagesArgs, ScanArgs, UpdateArgs,
};
use crate::discovery::{DiscoveryError, DiscoveryPolicy, FileSelection, canonicalize_ignore_files};
use crate::extraction::{
    ExtractFileError, ExtractFileErrorKind, SourceSnapshot, extract_artifact_file,
    extract_artifact_file_from_snapshot, failed_artifact_file, read_source_snapshot,
    unchanged_artifact_file,
};
use crate::paths::{
    FileTarget, PathPolicyError, canonicalize_db_path, canonicalize_root, canonicalize_update_file,
    normalize_delete_file,
};

const CARGO_LOCK: &str = include_str!("../../../Cargo.lock");

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

struct CommandOutcome {
    report: Report,
    exit_code: u8,
    json: bool,
    report_stream: ReportStream,
}

#[derive(Clone, Copy)]
enum ReportStream {
    Stdout,
    Stderr,
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
                &mut extracted.spool,
            ) {
                Ok(write_result) => {
                    record_profile_phase(
                        &mut profile_phases,
                        "artifact_write",
                        artifact_write_started.elapsed(),
                    );
                    let capability_rows_written = writer.last_capability_rows_written();
                    let connection = writer.connection();
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
                    report
                        .errors
                        .extend(discovered.errors.iter().map(discovery_error_diagnostic));
                    report
                        .errors
                        .extend(extracted.errors.iter().map(extract_error_diagnostic));
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
    let snapshot = capability_snapshot();
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
    }));
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

fn extract_error_outcome(
    error: ExtractFileError,
    operation: ReportOperation,
    mode: ReportMode,
    input: ReportInput,
    json_report: bool,
) -> CommandOutcome {
    outcome(
        base_report(ReportStatus::Failed, operation, mode, input)
            .with_error(extract_error_diagnostic(&error)),
        1,
        json_report,
        ReportStream::Stdout,
    )
}

fn extract_error_diagnostic(error: &ExtractFileError) -> ReportDiagnostic {
    let code = match error.kind {
        ExtractFileErrorKind::Read => ReportCode::ReadFailed,
        ExtractFileErrorKind::Extract => ReportCode::ParseFailed,
        ExtractFileErrorKind::Serialize => ReportCode::InternalError,
    };
    diagnostic(
        code,
        error.message.clone(),
        Some(error.path.clone()),
        Some(error.root_relative_path.clone()),
        true,
        json!({}),
    )
}

fn discovery_error_diagnostic(error: &DiscoveryError) -> ReportDiagnostic {
    diagnostic(
        ReportCode::ReadFailed,
        error.message.clone(),
        Some(error.path.clone()),
        Some(error.root_relative_path.clone()),
        true,
        json!({}),
    )
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

fn delete_artifact_rows(
    db: &Path,
    root: &Path,
    root_relative_path: &str,
    existing_artifact: Option<ExistingArtifact>,
    operation: WriteOperation,
    change_kind: RevisionChangeKind,
) -> Result<(ArtifactWriter, WriteResult, RowDomainCounts), ArtifactWriteError> {
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

fn artifact_report_from_connection(
    db: &Path,
    connection: &Connection,
) -> Result<ArtifactReport, CommandError> {
    let metadata = read_metadata(connection).map_err(|error| {
        command_error(
            3,
            ReportCode::SchemaIncompatible,
            format!("artifact metadata could not be read: {error}"),
            Some(display_path(db)),
            None,
            false,
            json!({}),
        )
    })?;
    artifact_report(db, &metadata, Some(JSONL_SCHEMA_VERSION))
}

fn write_error_outcome(
    error: ArtifactWriteError,
    operation: ReportOperation,
    mode: ReportMode,
    input: ReportInput,
    json_report: bool,
) -> CommandOutcome {
    write_error_outcome_with_profile(error, operation, mode, input, json_report, None)
}

fn write_error_outcome_with_profile(
    error: ArtifactWriteError,
    operation: ReportOperation,
    mode: ReportMode,
    input: ReportInput,
    json_report: bool,
    profile: Option<ReportProfile>,
) -> CommandOutcome {
    let (code, report_code, message, details) = match error {
        ArtifactWriteError::Sqlite(error) => (
            1,
            ReportCode::DbWriteFailed,
            format!("SQLite artifact write failed: {error}"),
            json!({}),
        ),
        ArtifactWriteError::Spool(error) => (
            1,
            ReportCode::InternalError,
            format!("artifact file spool failed: {error}"),
            json!({}),
        ),
        ArtifactWriteError::DataLossGuard {
            path,
            existing_symbols,
            reason,
        } => (
            1,
            ReportCode::DataLossGuard,
            format!("data-loss guard preserved existing rows for {path}"),
            json!({"path": path, "existing_symbols": existing_symbols, "reason": reason}),
        ),
        ArtifactWriteError::SnapshotMissingSpooledPath { path } => (
            1,
            ReportCode::InternalError,
            format!("artifact file spool path was missing from scan snapshot: {path}"),
            json!({"path": path}),
        ),
    };
    let mut report = base_report(ReportStatus::Failed, operation, mode, input)
        .with_error(diagnostic(report_code, message, None, None, false, details));
    report.profile = profile;
    outcome(report, code, json_report, ReportStream::Stdout)
}

fn spool_error_outcome(
    error: ArtifactSpoolError,
    operation: ReportOperation,
    mode: ReportMode,
    input: ReportInput,
    json_report: bool,
) -> CommandOutcome {
    outcome(
        base_report(ReportStatus::Failed, operation, mode, input).with_error(diagnostic(
            ReportCode::InternalError,
            format!("artifact file spool failed: {error}"),
            None,
            None,
            false,
            json!({}),
        )),
        1,
        json_report,
        ReportStream::Stdout,
    )
}

struct OpenArtifact {
    connection: Connection,
    report: ArtifactReport,
    write_metadata: ArtifactMetadata,
}

struct OpenInfoArtifact {
    connection: Connection,
    report: ArtifactReport,
    warnings: Vec<ReportDiagnostic>,
}

struct ExistingArtifact {
    write_metadata: ArtifactMetadata,
}

struct CommandError {
    diagnostic: ReportDiagnostic,
    exit_code: u8,
}

fn open_artifact(
    db_path: &Path,
    strict_schema: bool,
    jsonl_schema_version: Option<i64>,
) -> Result<OpenArtifact, CommandError> {
    let connection = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| {
            command_error(
                1,
                ReportCode::DbOpenFailed,
                format!("could not open SQLite artifact: {error}"),
                Some(display_path(db_path)),
                None,
                true,
                json!({}),
            )
        })?;
    let metadata = read_metadata(&connection).map_err(|error| {
        command_error(
            3,
            ReportCode::SchemaIncompatible,
            format!("artifact metadata could not be read: {error}"),
            Some(display_path(db_path)),
            None,
            false,
            json!({}),
        )
    })?;
    check_versions(&metadata, strict_schema)?;
    let write_metadata = artifact_metadata_from_rows(&metadata)?;
    let report = artifact_report(db_path, &metadata, jsonl_schema_version)?;
    Ok(OpenArtifact {
        connection,
        report,
        write_metadata,
    })
}

fn open_artifact_for_info(
    db_path: &Path,
    strict_schema: bool,
    jsonl_schema_version: Option<i64>,
) -> Result<OpenInfoArtifact, CommandError> {
    let connection = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| {
            command_error(
                1,
                ReportCode::DbOpenFailed,
                format!("could not open SQLite artifact: {error}"),
                Some(display_path(db_path)),
                None,
                true,
                json!({}),
            )
        })?;
    let metadata = read_metadata(&connection).map_err(|error| {
        command_error(
            3,
            ReportCode::SchemaIncompatible,
            format!("artifact metadata could not be read: {error}"),
            Some(display_path(db_path)),
            None,
            false,
            json!({}),
        )
    })?;
    check_versions(&metadata, strict_schema)?;
    let report = artifact_report(db_path, &metadata, jsonl_schema_version)?;
    let warnings = missing_metadata_warnings(&metadata);
    Ok(OpenInfoArtifact {
        connection,
        report,
        warnings,
    })
}

fn load_existing_content_hashes(
    connection: &Connection,
) -> Result<BTreeMap<String, String>, CommandError> {
    let mut statement = connection
        .prepare("SELECT path, content_hash FROM files")
        .map_err(|error| {
            command_error(
                3,
                ReportCode::SchemaIncompatible,
                format!("existing file hashes could not be prepared: {error}"),
                None,
                None,
                false,
                json!({}),
            )
        })?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| {
            command_error(
                3,
                ReportCode::SchemaIncompatible,
                format!("existing file hashes could not be read: {error}"),
                None,
                None,
                false,
                json!({}),
            )
        })?;

    let mut hashes = BTreeMap::new();
    for row in rows {
        let (path, content_hash) = row.map_err(|error| {
            command_error(
                3,
                ReportCode::SchemaIncompatible,
                format!("existing file hash row could not be read: {error}"),
                None,
                None,
                false,
                json!({}),
            )
        })?;
        hashes.insert(path, content_hash);
    }
    Ok(hashes)
}

fn existing_artifact_for_root(
    db_path: &Path,
    strict_schema: bool,
    jsonl_schema_version: Option<i64>,
    root: &Path,
) -> Result<Option<ExistingArtifact>, CommandError> {
    if !db_path.exists() {
        return Ok(None);
    }

    let artifact = open_artifact(db_path, strict_schema, jsonl_schema_version)?;
    if artifact.report.root_path != display_path(root) {
        return Err(command_error(
            3,
            ReportCode::RootMismatch,
            "artifact root does not match requested root",
            Some(display_path(db_path)),
            None,
            false,
            json!({
                "artifact_root": artifact.report.root_path,
                "requested_root": display_path(root),
            }),
        ));
    }

    Ok(Some(ExistingArtifact {
        write_metadata: artifact.write_metadata,
    }))
}

fn open_artifact_for_root(
    db_path: &Path,
    strict_schema: bool,
    jsonl_schema_version: Option<i64>,
    root: &Path,
) -> Result<OpenArtifact, CommandError> {
    let artifact = open_artifact(db_path, strict_schema, jsonl_schema_version)?;
    if artifact.report.root_path != display_path(root) {
        return Err(command_error(
            3,
            ReportCode::RootMismatch,
            "artifact root does not match requested root",
            Some(display_path(db_path)),
            None,
            false,
            json!({
                "artifact_root": artifact.report.root_path,
                "requested_root": display_path(root),
            }),
        ));
    }
    Ok(artifact)
}

fn check_versions(
    metadata: &BTreeMap<String, String>,
    strict_schema: bool,
) -> Result<(), CommandError> {
    let sqlite_schema_version = metadata_i64(metadata, "sqlite_schema_version")?;
    let schema_version = metadata_i64(metadata, "schema_version")?;
    let extract_contract_version = metadata_i64(metadata, "extract_contract_version")?;

    if sqlite_schema_version > SQLITE_SCHEMA_VERSION || schema_version > SQLITE_SCHEMA_VERSION {
        return Err(command_error(
            3,
            ReportCode::SchemaIncompatible,
            "artifact schema version is newer than this binary supports",
            None,
            None,
            false,
            json!({
                "supported_sqlite_schema_version": SQLITE_SCHEMA_VERSION,
                "artifact_sqlite_schema_version": sqlite_schema_version,
                "artifact_schema_version": schema_version,
            }),
        ));
    }
    if strict_schema
        && (sqlite_schema_version != SQLITE_SCHEMA_VERSION
            || schema_version != SQLITE_SCHEMA_VERSION)
    {
        return Err(command_error(
            3,
            ReportCode::SchemaMigrationRequired,
            "artifact schema migration is required",
            None,
            None,
            true,
            json!({
                "required_sqlite_schema_version": SQLITE_SCHEMA_VERSION,
                "artifact_sqlite_schema_version": sqlite_schema_version,
                "artifact_schema_version": schema_version,
            }),
        ));
    }
    if extract_contract_version != EXTRACT_CONTRACT_VERSION {
        return Err(command_error(
            3,
            ReportCode::ContractIncompatible,
            "artifact extraction contract version is incompatible",
            None,
            None,
            false,
            json!({
                "supported_extract_contract_version": EXTRACT_CONTRACT_VERSION,
                "artifact_extract_contract_version": extract_contract_version,
            }),
        ));
    }
    Ok(())
}

fn artifact_report(
    db_path: &Path,
    metadata: &BTreeMap<String, String>,
    jsonl_schema_version: Option<i64>,
) -> Result<ArtifactReport, CommandError> {
    Ok(ArtifactReport {
        db_path: display_path(db_path),
        root_path: metadata_string(metadata, "root_path")?,
        artifact_id: metadata_string(metadata, "artifact_id")?,
        schema_version: metadata_i64(metadata, "schema_version")?,
        extract_contract_version: metadata_i64(metadata, "extract_contract_version")?,
        sqlite_schema_version: metadata_i64(metadata, "sqlite_schema_version")?,
        jsonl_schema_version,
        hash_algorithm: metadata_string(metadata, "hash_algorithm")?,
        parser_inventory_fingerprint: metadata_string(metadata, "parser_inventory_fingerprint")?,
        capability_snapshot_fingerprint: metadata_string(
            metadata,
            "capability_snapshot_fingerprint",
        )?,
    })
}

fn artifact_metadata_from_rows(
    metadata: &BTreeMap<String, String>,
) -> Result<ArtifactMetadata, CommandError> {
    Ok(ArtifactMetadata {
        artifact_id: metadata_string(metadata, "artifact_id")?,
        root_path: metadata_string(metadata, "root_path")?,
        binary_version: metadata_string(metadata, "binary_version")?,
        hash_algorithm: metadata_string(metadata, "hash_algorithm")?,
        parser_inventory_fingerprint: metadata_string(metadata, "parser_inventory_fingerprint")?,
        capability_snapshot_fingerprint: metadata_string(
            metadata,
            "capability_snapshot_fingerprint",
        )?,
        created_at: metadata_string(metadata, "created_at")?,
        updated_at: metadata_string(metadata, "updated_at")?,
    })
}

fn missing_metadata_warnings(metadata: &BTreeMap<String, String>) -> Vec<ReportDiagnostic> {
    REQUIRED_METADATA_KEYS
        .iter()
        .filter(|key| !metadata.contains_key(**key))
        .map(|key| {
            diagnostic(
                ReportCode::MetadataMissing,
                format!("artifact is missing metadata key {key}"),
                None,
                None,
                true,
                json!({"missing_key": key}),
            )
        })
        .collect()
}

fn metadata_string(
    metadata: &BTreeMap<String, String>,
    key: &'static str,
) -> Result<String, CommandError> {
    metadata.get(key).cloned().ok_or_else(|| {
        command_error(
            3,
            ReportCode::SchemaIncompatible,
            format!("artifact is missing metadata key {key}"),
            None,
            None,
            false,
            json!({"missing_key": key}),
        )
    })
}

fn metadata_i64(
    metadata: &BTreeMap<String, String>,
    key: &'static str,
) -> Result<i64, CommandError> {
    let value = metadata_string(metadata, key)?;
    value.parse::<i64>().map_err(|error| {
        command_error(
            3,
            ReportCode::SchemaIncompatible,
            format!("artifact metadata key {key} is not an integer: {error}"),
            None,
            None,
            false,
            json!({"key": key, "value": value}),
        )
    })
}

fn table_totals(connection: &Connection) -> RowDomainCounts {
    RowDomainCounts {
        artifact_metadata: table_count(connection, "artifact_metadata"),
        parser_inventory: table_count(connection, "parser_inventory"),
        language_capabilities: table_count(connection, "language_capabilities"),
        language_capability_fixtures: table_count(connection, "language_capability_fixtures"),
        language_capability_gaps: table_count(connection, "language_capability_gaps"),
        extraction_revisions: table_count(connection, "extraction_revisions"),
        revision_file_changes: table_count(connection, "revision_file_changes"),
        files: table_count(connection, "files"),
        symbols: table_count(connection, "symbols"),
        symbol_annotations: table_count(connection, "symbol_annotations"),
        identifiers: table_count(connection, "identifiers"),
        relationships: table_count(connection, "relationships"),
        pending_relationships: table_count(connection, "pending_relationships"),
        type_facts: table_count(connection, "type_facts"),
        type_argument_usages: table_count(connection, "type_argument_usages"),
        type_arguments: table_count(connection, "type_arguments"),
        literals: table_count(connection, "literals"),
        source_regions: table_count(connection, "source_regions"),
        structural_facts: table_count(connection, "structural_facts"),
        parse_diagnostics: table_count(connection, "parse_diagnostics"),
    }
}

fn table_count(connection: &Connection, table: &str) -> i64 {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    connection
        .query_row(&sql, [], |row| row.get(0))
        .unwrap_or(0)
}

fn latest_revision_id(connection: &Connection) -> Option<i64> {
    connection
        .query_row(
            "SELECT MAX(revision_id) FROM extraction_revisions",
            [],
            |row| row.get(0),
        )
        .unwrap_or(None)
}

fn jsonl_counts(records_by_kind: &BTreeMap<&'static str, usize>) -> RowDomainCounts {
    let mut counts = RowDomainCounts::default();
    for kind in JSONL_RECORD_KINDS {
        let count = records_by_kind.get(kind).copied().unwrap_or(0) as i64;
        match *kind {
            "artifact" => counts.artifact_metadata = count,
            "parser_inventory" => counts.parser_inventory = count,
            "language_capability" => counts.language_capabilities = count,
            "language_capability_fixture" => counts.language_capability_fixtures = count,
            "language_capability_gap" => counts.language_capability_gaps = count,
            "revision" => counts.extraction_revisions = count,
            "revision_file_change" => counts.revision_file_changes = count,
            "file" => counts.files = count,
            "symbol" => counts.symbols = count,
            "symbol_annotation" => counts.symbol_annotations = count,
            "identifier" => counts.identifiers = count,
            "relationship" => counts.relationships = count,
            "pending_relationship" => counts.pending_relationships = count,
            "type_fact" => counts.type_facts = count,
            "type_argument_usage" => counts.type_argument_usages = count,
            "type_argument" => counts.type_arguments = count,
            "literal" => counts.literals = count,
            "source_region" => counts.source_regions = count,
            "structural_fact" => counts.structural_facts = count,
            "parse_diagnostic" => counts.parse_diagnostics = count,
            _ => {}
        }
    }
    counts
}

fn current_capability_fingerprints() -> (String, String) {
    let snapshot = artifact_capability_snapshot();
    (
        parser_inventory_fingerprint(&snapshot.parser_inventory),
        capability_snapshot_fingerprint(&snapshot.languages),
    )
}

fn artifact_capability_snapshot() -> ArtifactCapabilitySnapshot {
    let snapshot = capability_snapshot();
    let lock_packages = cargo_lock_packages();
    let languages = snapshot
        .languages()
        .map(|row| ArtifactLanguageCapabilityRow {
            language: row.language.clone(),
            parser_package: row.parser_crate.clone(),
            extensions: row.extensions.clone(),
            dependency_status: row.dependency_status.clone(),
            target_capabilities: artifact_flags(row.target_capabilities),
            actual_capabilities: artifact_flags(row.capabilities),
            kind_coverage: json!({
                "symbols": kind_coverage_domain(&row.kind_coverage.symbols),
                "relationships": kind_coverage_domain(&row.kind_coverage.relationships),
                "identifiers": kind_coverage_domain(&row.kind_coverage.identifiers),
                "body_spans": kind_coverage_domain(&row.kind_coverage.body_spans),
            }),
            fixtures: row
                .fixtures
                .iter()
                .map(|fixture| ArtifactLanguageCapabilityFixtureRow {
                    fixture_name: fixture.name.clone(),
                    source_path: fixture.source.clone(),
                    expected_path: fixture.expected.clone(),
                })
                .collect(),
            gaps: row
                .capability_gaps
                .iter()
                .map(|gap| ArtifactLanguageCapabilityGapRow {
                    gap_id: format!("{}:{}", row.language, gap.capability),
                    capability: gap.capability.clone(),
                    status: gap.status.clone(),
                    reason: gap.reason.clone(),
                    required_closure: gap.required_closure.clone(),
                    evidence: gap.evidence.clone(),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    let parser_inventory = languages
        .iter()
        .map(|row| {
            let lock_package = lock_packages.get(&row.parser_package);
            let parser_version = lock_package.map(|package| package.version.clone());
            ArtifactParserInventoryRow {
                language: row.language.clone(),
                parser_package: row.parser_package.clone(),
                parser_version: parser_version.clone(),
                grammar_version: parser_version,
                source: lock_package
                    .and_then(|package| package.source.clone())
                    .or_else(|| Some("cargo_lock".to_string())),
                metadata: Some(json!({
                    "dependency_status": row.dependency_status,
                    "cargo_lock_source": lock_package.and_then(|package| package.source.as_ref()),
                })),
            }
        })
        .collect();

    ArtifactCapabilitySnapshot {
        parser_inventory,
        languages,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CargoLockPackage {
    version: String,
    source: Option<String>,
}

fn cargo_lock_packages() -> BTreeMap<String, CargoLockPackage> {
    #[derive(Default)]
    struct PartialPackage {
        name: Option<String>,
        version: Option<String>,
        source: Option<String>,
    }

    fn push_package(packages: &mut BTreeMap<String, CargoLockPackage>, package: PartialPackage) {
        let (Some(name), Some(version)) = (package.name, package.version) else {
            return;
        };
        packages.insert(
            name,
            CargoLockPackage {
                version,
                source: package.source,
            },
        );
    }

    let mut packages = BTreeMap::new();
    let mut current: Option<PartialPackage> = None;

    for line in CARGO_LOCK.lines() {
        if line == "[[package]]" {
            if let Some(package) = current.take() {
                push_package(&mut packages, package);
            }
            current = Some(PartialPackage::default());
            continue;
        }

        let Some(package) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once(" = ") else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key {
            "name" => package.name = Some(value),
            "version" => package.version = Some(value),
            "source" => package.source = Some(value),
            _ => {}
        }
    }

    if let Some(package) = current {
        push_package(&mut packages, package);
    }

    packages
}

fn parser_inventory_fingerprint(rows: &[ArtifactParserInventoryRow]) -> String {
    let mut canonical_rows = rows
        .iter()
        .map(|row| {
            (
                row.language.clone(),
                row.parser_package.clone(),
                json!({
                    "language": row.language,
                    "parser_package": row.parser_package,
                    "parser_version": row.parser_version,
                    "grammar_version": row.grammar_version,
                    "source": row.source,
                    "metadata": row.metadata,
                }),
            )
        })
        .collect::<Vec<_>>();
    canonical_rows.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    fingerprint_json(&json!({
        "domain": "parser_inventory",
        "version": 1,
        "rows": canonical_rows
            .into_iter()
            .map(|(_, _, value)| value)
            .collect::<Vec<_>>(),
    }))
}

fn capability_snapshot_fingerprint(rows: &[ArtifactLanguageCapabilityRow]) -> String {
    let mut canonical_rows = rows
        .iter()
        .map(|row| {
            let mut extensions = row.extensions.clone();
            extensions.sort();
            let mut fixtures = row
                .fixtures
                .iter()
                .map(|fixture| {
                    (
                        fixture.fixture_name.clone(),
                        json!({
                            "fixture_name": fixture.fixture_name,
                            "source_path": fixture.source_path,
                            "expected_path": fixture.expected_path,
                        }),
                    )
                })
                .collect::<Vec<_>>();
            fixtures.sort_by(|left, right| left.0.cmp(&right.0));
            let mut gaps = row
                .gaps
                .iter()
                .map(|gap| {
                    (
                        gap.gap_id.clone(),
                        json!({
                            "gap_id": gap.gap_id,
                            "capability": gap.capability,
                            "status": gap.status,
                            "reason": gap.reason,
                            "required_closure": gap.required_closure,
                            "evidence": gap.evidence,
                        }),
                    )
                })
                .collect::<Vec<_>>();
            gaps.sort_by(|left, right| left.0.cmp(&right.0));
            (
                row.language.clone(),
                json!({
                    "language": row.language,
                    "parser_package": row.parser_package,
                    "extensions": extensions,
                    "dependency_status": row.dependency_status,
                    "target_capabilities": capability_flags_json(row.target_capabilities),
                    "actual_capabilities": capability_flags_json(row.actual_capabilities),
                    "kind_coverage": row.kind_coverage,
                    "fixtures": fixtures
                        .into_iter()
                        .map(|(_, value)| value)
                        .collect::<Vec<_>>(),
                    "gaps": gaps
                        .into_iter()
                        .map(|(_, value)| value)
                        .collect::<Vec<_>>(),
                }),
            )
        })
        .collect::<Vec<_>>();
    canonical_rows.sort_by(|left, right| left.0.cmp(&right.0));
    fingerprint_json(&json!({
        "domain": "capability_snapshot",
        "version": 1,
        "rows": canonical_rows
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>(),
    }))
}

fn capability_flags_json(flags: ArtifactCapabilityFlags) -> Value {
    json!({
        "symbols": flags.symbols,
        "relationships": flags.relationships,
        "pending_relationships": flags.pending_relationships,
        "identifiers": flags.identifiers,
        "types": flags.types,
    })
}

fn fingerprint_json(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).expect("capability fingerprint input must serialize");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

fn artifact_flags(flags: CapabilityFlags) -> ArtifactCapabilityFlags {
    ArtifactCapabilityFlags {
        symbols: flags.symbols,
        relationships: flags.relationships,
        pending_relationships: flags.pending_relationships,
        identifiers: flags.identifiers,
        types: flags.types,
    }
}

fn kind_coverage_domain(domain: &KindCoverage) -> Value {
    json!({
        "supported": domain.supported,
        "not_applicable": domain.not_applicable,
        "open_gaps": domain.open_gaps.iter().map(|gap| {
            json!({
                "kind": gap.kind,
                "reason": gap.reason,
                "required_closure": gap.required_closure,
                "planned_closure_task": gap.planned_closure_task,
            })
        }).collect::<Vec<_>>(),
    })
}

fn rows_written_with_capabilities(
    capability_rows_written: &RowDomainCounts,
    write_result: &WriteResult,
) -> RowDomainCounts {
    let mut rows_written = RowDomainCounts::from(&write_result.rows_written);
    rows_written.add_counts(capability_rows_written);
    rows_written
}

fn flags(flags: CapabilityFlags) -> Value {
    json!({
        "symbols": flags.symbols,
        "relationships": flags.relationships,
        "pending_relationships": flags.pending_relationships,
        "identifiers": flags.identifiers,
        "types": flags.types,
    })
}

fn base_report(
    status: ReportStatus,
    operation: ReportOperation,
    mode: ReportMode,
    input: ReportInput,
) -> Report {
    Report {
        status,
        operation,
        mode,
        input,
        artifact: None,
        tool: ToolReport {
            binary_name: "julie-extract".to_string(),
            binary_version: env!("CARGO_PKG_VERSION").to_string(),
        },
        revision: None,
        counts: ReportCounts::default(),
        profile: None,
        errors: Vec::new(),
        warnings: Vec::new(),
        languages: None,
    }
}

fn artifact_input(
    db_path: &Path,
    root_path: Option<&Path>,
    file_path: Option<&Path>,
    root_relative_path: Option<&str>,
) -> ReportInput {
    ReportInput {
        db_path: Some(display_path(db_path)),
        root_path: root_path.map(display_path),
        file_path: file_path.map(display_path),
        root_relative_path: root_relative_path.map(ToOwned::to_owned),
        format: None,
        output_path: None,
    }
}

fn path_error_outcome(
    error: PathPolicyError,
    operation: ReportOperation,
    mode: ReportMode,
    json_report: bool,
) -> CommandOutcome {
    path_error_outcome_with_paths(
        error,
        operation,
        mode,
        json_report,
        PathErrorInput::default(),
    )
}

#[derive(Default)]
struct PathErrorInput<'a> {
    db_path: Option<&'a Path>,
    root_path: Option<&'a Path>,
    file_path: Option<&'a Path>,
    root_relative_path: Option<&'a str>,
}

fn path_error_outcome_with_paths(
    error: PathPolicyError,
    operation: ReportOperation,
    mode: ReportMode,
    json_report: bool,
    input_paths: PathErrorInput<'_>,
) -> CommandOutcome {
    let diagnostic = path_error_diagnostic(error);
    let input = ReportInput {
        db_path: input_paths.db_path.map(display_path),
        root_path: input_paths.root_path.map(display_path),
        file_path: input_paths.file_path.map(display_path),
        root_relative_path: input_paths
            .root_relative_path
            .map(ToOwned::to_owned)
            .or_else(|| diagnostic.root_relative_path.clone()),
        format: None,
        output_path: None,
    };
    outcome(
        base_report(ReportStatus::Failed, operation, mode, input).with_error(diagnostic),
        1,
        json_report,
        ReportStream::Stdout,
    )
}

fn path_error_diagnostic(error: PathPolicyError) -> ReportDiagnostic {
    match error {
        PathPolicyError::InvalidPath { path, message } => diagnostic(
            ReportCode::InvalidPath,
            message,
            Some(path),
            None,
            true,
            json!({}),
        ),
        PathPolicyError::FileOutsideRoot { path, root_path } => diagnostic(
            ReportCode::FileOutsideRoot,
            "file is outside the requested root",
            Some(path),
            None,
            true,
            json!({"root_path": root_path}),
        ),
        PathPolicyError::FileNotFound {
            path,
            root_relative_path,
        } => diagnostic(
            ReportCode::FileNotFound,
            "update target file does not exist",
            Some(path),
            root_relative_path,
            true,
            json!({"hint": "use delete for removed source files"}),
        ),
    }
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

trait ReportBuilder {
    fn with_artifact(self, artifact: ArtifactReport) -> Self;
    fn with_revision(self, revision: ReportRevision) -> Self;
    fn with_totals(self, totals: RowDomainCounts) -> Self;
    fn with_profile(self, profile: ReportProfile) -> Self;
    fn with_error(self, error: ReportDiagnostic) -> Self;
    fn with_warning(self, warning: ReportDiagnostic) -> Self;
    fn with_languages(self, languages: Value) -> Self;
}

impl ReportBuilder for Report {
    fn with_artifact(mut self, artifact: ArtifactReport) -> Self {
        self.artifact = Some(artifact);
        self
    }

    fn with_revision(mut self, revision: ReportRevision) -> Self {
        self.revision = Some(revision);
        self
    }

    fn with_totals(mut self, totals: RowDomainCounts) -> Self {
        self.counts.totals = totals;
        self
    }

    fn with_profile(mut self, profile: ReportProfile) -> Self {
        self.profile = Some(profile);
        self
    }

    fn with_error(mut self, error: ReportDiagnostic) -> Self {
        self.errors.push(error);
        self
    }

    fn with_warning(mut self, warning: ReportDiagnostic) -> Self {
        self.warnings.push(warning);
        self
    }

    fn with_languages(mut self, languages: Value) -> Self {
        self.languages = Some(languages);
        self
    }
}

fn outcome(
    report: Report,
    exit_code: u8,
    json: bool,
    report_stream: ReportStream,
) -> CommandOutcome {
    CommandOutcome {
        report,
        exit_code,
        json,
        report_stream,
    }
}

fn command_error(
    exit_code: u8,
    code: ReportCode,
    message: impl Into<String>,
    path: Option<String>,
    root_relative_path: Option<String>,
    recoverable: bool,
    details: Value,
) -> CommandError {
    CommandError {
        diagnostic: diagnostic(
            code,
            message,
            path,
            root_relative_path,
            recoverable,
            details,
        ),
        exit_code,
    }
}

fn diagnostic(
    code: ReportCode,
    message: impl Into<String>,
    path: Option<String>,
    root_relative_path: Option<String>,
    recoverable: bool,
    details: Value,
) -> ReportDiagnostic {
    ReportDiagnostic {
        code,
        message: message.into(),
        path,
        root_relative_path,
        recoverable,
        details,
    }
}

fn write_outcome(outcome: &CommandOutcome) {
    if outcome.json {
        match outcome.report_stream {
            ReportStream::Stdout => write_json(io::stdout().lock(), &outcome.report),
            ReportStream::Stderr => write_json(io::stderr().lock(), &outcome.report),
        }
        return;
    }

    if outcome.exit_code == 0 {
        let _ = writeln!(io::stdout(), "{}", human_status(&outcome.report));
    } else {
        let _ = writeln!(io::stderr(), "{}", human_status(&outcome.report));
    }
}

fn write_json(mut writer: impl Write, report: &Report) {
    let _ = serde_json::to_writer(&mut writer, report);
    let _ = writeln!(writer);
}

fn human_status(report: &Report) -> &'static str {
    match report.status {
        ReportStatus::Ok => "ok",
        ReportStatus::NoChange => "no_change",
        ReportStatus::Unsupported => "unsupported",
        ReportStatus::NotFound => "not_found",
        ReportStatus::Partial => "partial",
        ReportStatus::Failed => "failed",
    }
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
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
        std::fs::write(&bad_path, [0xff, 0xfe, 0x00, 0x9f]).unwrap();
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
            "only the unreadable file should error"
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
