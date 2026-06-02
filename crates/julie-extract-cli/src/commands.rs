use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use julie_extract_artifact::jsonl::{JSONL_RECORD_KINDS, JSONL_SCHEMA_VERSION, export_jsonl};
use julie_extract_artifact::metadata::{ArtifactMetadata, read_metadata};
use julie_extract_artifact::model::{
    ArtifactCapabilityFlags, ArtifactCapabilitySnapshot, ArtifactFile,
    ArtifactLanguageCapabilityFixtureRow, ArtifactLanguageCapabilityGapRow,
    ArtifactLanguageCapabilityRow, ArtifactParserInventoryRow, RevisionInput, WriteMode,
    WriteOperation, WriteResult,
};
use julie_extract_artifact::reports::{
    ArtifactReport, Report, ReportCode, ReportCounts, ReportDiagnostic, ReportInput, ReportMode,
    ReportOperation, ReportRevision, ReportStatus, RowDomainCounts, ToolReport,
};
use julie_extract_artifact::schema::{EXTRACT_CONTRACT_VERSION, SQLITE_SCHEMA_VERSION};
use julie_extract_artifact::writer::{
    ArtifactFileSpool, ArtifactSpoolError, ArtifactWriteError, ArtifactWriter,
};
use julie_extractors::{CapabilityFlags, KindCoverage, capability_snapshot};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::args::{
    Cli, Command, DeleteArgs, ExportArgs, InfoArgs, LanguagesArgs, ScanArgs, UpdateArgs,
};
use crate::discovery::{DiscoveryPolicy, FileSelection, canonicalize_ignore_files};
use crate::extraction::{
    ExtractFileError, ExtractFileErrorKind, SourceSnapshot, extract_artifact_file,
    extract_artifact_file_from_snapshot, failed_artifact_file, read_source_snapshot,
    unchanged_artifact_file,
};
use crate::paths::{
    FileTarget, PathPolicyError, canonicalize_db_path, canonicalize_root, canonicalize_update_file,
    normalize_delete_file,
};

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

    let discovery = match DiscoveryPolicy::build(&root, &db, &ignore_files) {
        Ok(discovery) => discovery,
        Err(error) => return path_error_outcome(error, ReportOperation::Scan, mode, args.json),
    };
    let discovered = discovery.discover();

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
    let should_rebuild_db = args.force && db.exists() && force_existing_metadata.is_none();
    let indexed_at = now_rfc3339();
    let mut extracted = match spool_discovered_files(
        &root,
        &discovery,
        &discovered.supported_files,
        indexed_at,
        existing_content_hashes.as_ref(),
        args.force,
    ) {
        Ok(extracted) => extracted,
        Err(error) => {
            return spool_error_outcome(error, ReportOperation::Scan, mode, input, args.json);
        }
    };
    debug_assert_eq!(extracted.files_spooled, extracted.snapshot_paths.len());

    if should_rebuild_db {
        remove_artifact_files(&db);
    }
    let db_existed_before_write = db.exists();

    let metadata = force_existing_metadata
        .or(existing_scan_metadata)
        .map(refreshed_metadata)
        .unwrap_or_else(|| new_artifact_metadata(&root, None));

    match ArtifactWriter::open_path(&db, metadata) {
        Ok(mut writer) => {
            let capability_rows_written = match sync_capability_snapshot(&mut writer) {
                Ok(counts) => counts,
                Err(error) => {
                    return write_error_outcome(
                        error,
                        ReportOperation::Scan,
                        mode,
                        input,
                        args.json,
                    );
                }
            };
            match writer.write_scan_spooled(
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
                &mut extracted.spool,
            ) {
                Ok(write_result) => {
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
                                .with_error(error.diagnostic),
                                error.exit_code,
                                args.json,
                                ReportStream::Stdout,
                            );
                        }
                    };
                    let status = if !extracted.errors.is_empty() {
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
                        .with_totals(table_totals(connection));
                    report.counts.files_scanned =
                        (discovered.supported_files.len() + discovered.unsupported_files) as i64;
                    report.counts.files_changed = write_result
                        .files_changed
                        .saturating_sub(write_result.files_deleted)
                        as i64;
                    report.counts.files_unchanged = write_result.files_skipped as i64;
                    report.counts.files_unsupported = discovered.unsupported_files as i64;
                    report.counts.files_deleted = write_result.files_deleted as i64;
                    report.counts.files_failed = extracted.errors.len() as i64;
                    report.counts.rows_written =
                        rows_written_with_capabilities(&capability_rows_written, &write_result);
                    report
                        .errors
                        .extend(extracted.errors.iter().map(extract_error_diagnostic));
                    let exit_code = if extracted.errors.is_empty() { 0 } else { 1 };
                    outcome(report, exit_code, args.json, ReportStream::Stdout)
                }
                Err(error) => {
                    write_error_outcome(error, ReportOperation::Scan, mode, input, args.json)
                }
            }
        }
        Err(error) => outcome(
            base_report(ReportStatus::Failed, ReportOperation::Scan, mode, input).with_error(
                diagnostic(
                    ReportCode::DbOpenFailed,
                    format!("could not create SQLite artifact: {error}"),
                    Some(display_path(&db)),
                    None,
                    true,
                    json!({}),
                ),
            ),
            1,
            args.json,
            ReportStream::Stdout,
        ),
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
            let capability_rows_written = match sync_capability_snapshot(&mut writer) {
                Ok(counts) => counts,
                Err(error) => {
                    return write_error_outcome(
                        error,
                        ReportOperation::Update,
                        ReportMode::SingleFile,
                        input,
                        args.json,
                    );
                }
            };
            match writer.write_update(
                revision_input(WriteOperation::Update, Some(WriteMode::SingleFile), &root),
                &file,
            ) {
                Ok(write_result) => {
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

    match open_artifact(&args.db, args.strict_schema, Some(JSONL_SCHEMA_VERSION)) {
        Ok(artifact) => {
            let report = base_report(
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
                let file = std::fs::File::create(&args.out).map_err(Into::into);
                file.and_then(|mut file| export_jsonl(&artifact.connection, &mut file))
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
                    outcome(report, 1, args.json, ReportStream::Stdout)
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

fn extract_supported_files_to_spool(
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
) -> Result<SpooledExtractedFiles, ArtifactSpoolError> {
    let mut spool = create_scan_spool()?;
    let mut snapshot_paths = Vec::with_capacity(targets.len());
    let mut errors = Vec::new();
    for supported in targets {
        snapshot_paths.push(supported.target.root_relative_path.clone());
        let snapshot = match read_source_snapshot(&supported.target) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let file = failed_artifact_file(
                    &supported.target,
                    supported.language.clone(),
                    indexed_at.clone(),
                    &error,
                );
                spool.push(&file)?;
                errors.push(error);
                continue;
            }
        };
        if !force
            && existing_content_hashes
                .and_then(|hashes| hashes.get(&supported.target.root_relative_path))
                .is_some_and(|existing_hash| existing_hash == &snapshot.content_hash)
        {
            let file = unchanged_artifact_file(
                &supported.target,
                supported.language.clone(),
                indexed_at.clone(),
                &snapshot,
            );
            spool.push(&file)?;
            continue;
        }

        match extract(
            root,
            &supported.target,
            supported.language.clone(),
            indexed_at.clone(),
            snapshot.clone(),
        ) {
            Ok(file) => spool.push(&file)?,
            Err(error) => {
                let file = failed_artifact_file(
                    &supported.target,
                    supported.language.clone(),
                    indexed_at.clone(),
                    &error,
                );
                spool.push(&file)?;
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
    let root_relative_path = target.root_relative_path.clone();
    match delete_artifact_rows(
        db,
        root,
        &root_relative_path,
        existing_artifact,
        WriteOperation::Delete,
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
                "file is ignored or unsupported; stale artifact rows were removed",
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
    match delete_artifact_rows(
        db,
        root,
        &target.root_relative_path,
        existing_artifact,
        WriteOperation::Delete,
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
) -> Result<(ArtifactWriter, WriteResult, RowDomainCounts), ArtifactWriteError> {
    let metadata = existing_artifact
        .map(|artifact| refreshed_metadata(artifact.write_metadata))
        .unwrap_or_else(|| new_artifact_metadata(root, None));
    let mut writer = ArtifactWriter::open_path(db, metadata)?;
    let capability_rows_written = sync_capability_snapshot(&mut writer)?;
    let result = writer.delete_file(
        revision_input(operation, Some(WriteMode::SingleFile), root),
        root_relative_path,
    )?;
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
    outcome(
        base_report(ReportStatus::Failed, operation, mode, input).with_error(diagnostic(
            report_code,
            message,
            None,
            None,
            false,
            details,
        )),
        code,
        json_report,
        ReportStream::Stdout,
    )
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
            "parse_diagnostic" => counts.parse_diagnostics = count,
            _ => {}
        }
    }
    counts
}

fn sync_capability_snapshot(
    writer: &mut ArtifactWriter,
) -> Result<RowDomainCounts, ArtifactWriteError> {
    writer.sync_capability_snapshot(&artifact_capability_snapshot())
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
        .map(|row| ArtifactParserInventoryRow {
            language: row.language.clone(),
            parser_package: row.parser_package.clone(),
            parser_version: None,
            grammar_version: None,
            source: Some("capability_snapshot".to_string()),
            metadata: Some(json!({
                "dependency_status": row.dependency_status,
            })),
        })
        .collect();

    ArtifactCapabilitySnapshot {
        parser_inventory,
        languages,
    }
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
        let extracted_paths = RefCell::new(Vec::new());

        let extracted = extract_supported_files_to_spool(
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
