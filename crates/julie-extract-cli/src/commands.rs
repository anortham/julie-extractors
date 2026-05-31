use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use julie_extract_artifact::jsonl::{JSONL_RECORD_KINDS, JSONL_SCHEMA_VERSION, export_jsonl};
use julie_extract_artifact::metadata::read_metadata;
use julie_extract_artifact::reports::{
    ArtifactReport, Report, ReportCode, ReportCounts, ReportDiagnostic, ReportInput, ReportMode,
    ReportOperation, ReportRevision, ReportStatus, RowDomainCounts, ToolReport,
};
use julie_extract_artifact::schema::{EXTRACT_CONTRACT_VERSION, SQLITE_SCHEMA_VERSION};
use julie_extractors::{CapabilityFlags, capability_snapshot};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

use crate::args::{
    Cli, Command, DeleteArgs, ExportArgs, InfoArgs, LanguagesArgs, ScanArgs, UpdateArgs,
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
    let report = base_report(
        ReportStatus::Failed,
        ReportOperation::Scan,
        if args.force {
            ReportMode::Force
        } else {
            ReportMode::Incremental
        },
        ReportInput {
            db_path: Some(display_path(&args.db)),
            root_path: Some(display_path(&args.root)),
            file_path: None,
            root_relative_path: None,
            format: None,
            output_path: None,
        },
    )
    .with_error(diagnostic(
        ReportCode::InternalError,
        "scan requires source discovery",
        None,
        None,
        false,
        json!({"command": "scan"}),
    ));
    outcome(report, 1, args.json, ReportStream::Stdout)
}

fn update(args: UpdateArgs) -> CommandOutcome {
    let report = base_report(
        ReportStatus::Failed,
        ReportOperation::Update,
        ReportMode::SingleFile,
        ReportInput {
            db_path: Some(display_path(&args.db)),
            root_path: Some(display_path(&args.root)),
            file_path: Some(display_path(&args.file)),
            root_relative_path: None,
            format: None,
            output_path: None,
        },
    )
    .with_error(diagnostic(
        ReportCode::InternalError,
        "update requires source discovery",
        Some(display_path(&args.file)),
        None,
        false,
        json!({"command": "update"}),
    ));
    outcome(report, 1, args.json, ReportStream::Stdout)
}

fn delete(args: DeleteArgs) -> CommandOutcome {
    let report = base_report(
        ReportStatus::Failed,
        ReportOperation::Delete,
        ReportMode::SingleFile,
        ReportInput {
            db_path: Some(display_path(&args.db)),
            root_path: Some(display_path(&args.root)),
            file_path: Some(display_path(&args.file)),
            root_relative_path: None,
            format: None,
            output_path: None,
        },
    )
    .with_error(diagnostic(
        ReportCode::InternalError,
        "delete requires source discovery",
        Some(display_path(&args.file)),
        None,
        false,
        json!({"command": "delete"}),
    ));
    outcome(report, 1, args.json, ReportStream::Stdout)
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

struct OpenArtifact {
    connection: Connection,
    report: ArtifactReport,
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
    let report = artifact_report(db_path, &metadata, jsonl_schema_version)?;
    Ok(OpenArtifact { connection, report })
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

trait ReportBuilder {
    fn with_artifact(self, artifact: ArtifactReport) -> Self;
    fn with_revision(self, revision: ReportRevision) -> Self;
    fn with_totals(self, totals: RowDomainCounts) -> Self;
    fn with_error(self, error: ReportDiagnostic) -> Self;
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
